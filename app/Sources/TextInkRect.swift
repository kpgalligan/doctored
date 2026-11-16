import AppKit
import CoreText

// A straight text layer's raster covers its glyphs' ink (designs/text-ink-rect.md, TI-1..TI-8).
// The padded layout rect (TextLayer.sourceRect) cuts a glyph whose ink reaches past the padding —
// Zapfino's swashes — so a rendered payload stores how far its ink reaches past that rect,
// `ink_outset` (payload version 7, TextLayer.swift), and its rect grows by exactly that. The
// rect stays a pure function of the payload: anchor recovery reads the stored numbers and never
// measures ink. Only `nativeArtwork` writes the field, through `RasterDocument.inkFitted`, on
// every text render; a saved layer without it keeps its bytes and anchor until an edit
// re-renders it.

/// How far a payload's glyph ink reaches past its padded layout rect, per side, in whole SOURCE
/// px (TI-1, TI-3): the payload's `ink_outset`, `[left, top, right, bottom]`, stored only when a
/// side is positive.
struct TextInkOutset: Equatable {
    /// The most one side may hold: far past any glyph's reach at the 1000 pt size cap.
    static let maxOutset = 100_000
    let left: Int, top: Int, right: Int, bottom: Int

    /// Exactly four whole numbers, each in 0...maxOutset, not all zero; nil otherwise.
    init?(array: [Int]) {
        guard array.count == 4, array.allSatisfy({ (0...Self.maxOutset).contains($0) }),
              array.contains(where: { $0 > 0 }) else { return nil }
        left = array[0]
        top = array[1]
        right = array[2]
        bottom = array[3]
    }

    /// `[left, top, right, bottom]`, the JSON form.
    var array: [Int] { [left, top, right, bottom] }

    /// `rect` (source space, y down) grown by the four sides.
    func growing(_ rect: CGRect) -> CGRect {
        CGRect(x: rect.minX - CGFloat(left), y: rect.minY - CGFloat(top),
               width: rect.width + CGFloat(left + right),
               height: rect.height + CGFloat(top + bottom))
    }
}

extension TextLayerPayload {
    /// The payload with `outset` stored (nil clears it). A payload carrying one is version 7,
    /// which always carries `artwork`, so one with no style first takes one (`withArtwork`).
    func withInkOutset(_ outset: TextInkOutset?) -> TextLayerPayload {
        var result = self
        result.inkOutset = outset
        return outset == nil ? result : result.withArtwork()
    }

    /// The payload with the outset its glyphs' ink needs, measured from scratch
    /// (`TextLayer.inkOutset`): none on a path, for a failed layout, or when the ink fits.
    /// MAIN THREAD (TextKit).
    func fittedToInk() -> TextLayerPayload {
        withInkOutset(TextLayer.inkOutset(withInkOutset(nil)))
    }

    /// The payload with a style: itself when it has one, else one taken from its sRGB hex
    /// colour, as versions 6 and 7 require.
    func withArtwork() -> TextLayerPayload {
        guard artwork == nil else { return self }
        var result = self
        result.artwork = ArtworkTextStyle(rgba: ArtworkColor.authored(nsColor), in: .srgb)
        return result
    }
}

extension LayerDescription {
    /// A text description refitted to its glyphs' ink (`TextLayerPayload.fittedToInk`); a shape
    /// or fill unchanged. MAIN THREAD (TextKit).
    func fittedToInk() -> LayerDescription {
        guard case let .text(payload) = self else { return self }
        return .text(payload.fittedToInk())
    }

    /// A text description with no ink outset; a shape or fill unchanged.
    func withoutInkOutset() -> LayerDescription {
        guard case let .text(payload) = self else { return self }
        return .text(payload.withInkOutset(nil))
    }
}

extension TextLayer {
    /// TI-3, the measure: over the stack that DRAWS `payload` (`drawnStack`, the aligned one),
    /// the union of each shown glyph's `CTFontGetBoundingRectsForGlyphs` box at its layout
    /// position — the box `ArtworkTextCapture.glyphRect` maps — grown 1 px; per side, how far
    /// that reaches past the padded rect, rounded up. Whatever outset the payload carries is
    /// ignored: the padded rect is the unmarked one. nil on a path (TP-6 covers it: TI-5), for a
    /// failed layout, or when no side overflows. MAIN THREAD (TextKit).
    static func inkOutset(_ payload: TextLayerPayload) -> TextInkOutset? {
        guard payload.onPath == nil, let layout = measure(payload) else { return nil }
        let padded = sourceRect(layoutRect: layout.rect, size: payload.size)
        let stack = drawnStack(payload, layout: layout, color: nil, layoutManager: nil)
        var ink = CGRect.null
        for shown in shownGlyphs(of: stack) {
            var glyph = shown.glyph
            let bounds = CTFontGetBoundingRectsForGlyphs(shown.font, .default, &glyph, nil, 1)
            guard !bounds.isEmpty, bounds.minX.isFinite, bounds.minY.isFinite,
                  bounds.width.isFinite, bounds.height.isFinite else { continue }
            // Glyph space is y up from the baseline; source space is y down.
            ink = ink.union(CGRect(x: shown.origin.x + bounds.minX, y: shown.origin.y - bounds.maxY,
                                   width: bounds.width, height: bounds.height))
        }
        guard !ink.isNull else { return nil }
        ink = ink.insetBy(dx: -1, dy: -1)
        let reach = { (excess: CGFloat) -> Int in
            guard excess > 0, excess.isFinite else { return 0 }
            return Int(min(excess, CGFloat(TextInkOutset.maxOutset)).rounded(.up))
        }
        return TextInkOutset(array: [
            reach(padded.minX - ink.minX), reach(padded.minY - ink.minY),
            reach(ink.maxX - padded.maxX), reach(ink.maxY - padded.maxY),
        ])
    }

    /// Each shown glyph of a laid-out stack: the glyph, the face its run was typeset in (the
    /// storage's `.font` at the glyph's character, so a fallback face counts) and its origin on
    /// the baseline in the layout (the line fragment's origin plus `location(forGlyphAt:)`).
    /// Null and control glyphs (a ligature's absorbed characters, a line break) have no ink and
    /// are left out. The one glyph walk: the ink measure and text on a path's advances read it.
    static func shownGlyphs(of stack: TypesetStack)
        -> [(glyph: CGGlyph, font: CTFont, origin: CGPoint)]
    {
        let manager = stack.manager
        var result: [(glyph: CGGlyph, font: CTFont, origin: CGPoint)] = []
        withExtendedLifetime(stack.storage) {
            for index in 0..<manager.numberOfGlyphs {
                let property = manager.propertyForGlyph(at: index)
                guard !property.contains(.null), !property.contains(.controlCharacter) else { continue }
                let character = manager.characterIndexForGlyph(at: index)
                guard character < stack.storage.length,
                      let font = stack.storage.attribute(.font, at: character, effectiveRange: nil)
                        as? NSFont
                else { continue }
                let fragment = manager.lineFragmentRect(forGlyphAt: index, effectiveRange: nil)
                let location = manager.location(forGlyphAt: index)
                let origin = CGPoint(x: fragment.minX + location.x, y: fragment.minY + location.y)
                guard origin.x.isFinite, origin.y.isFinite else { continue }
                result.append((manager.cgGlyph(at: index), font as CTFont, origin))
            }
        }
        return result
    }
}

extension RasterDocument {
    /// `description` as a render of it must be published: `fittedToInk()`, except
    /// `withoutInkOutset()` when `replacing` names a layer whose stored description is text
    /// WITHOUT `ink_outset` and whose alpha a transparency lock freezes (TI-8: a frozen alpha
    /// channel cannot follow a raster that grows, so the layer is marked at its first re-render
    /// after the lock is cleared). A shape or fill is returned unchanged. MAIN THREAD (TextKit).
    func inkFitted(_ description: LayerDescription, replacing layer: Int?) -> LayerDescription {
        guard case .text = description else { return description }
        if let layer, case let .text(stored)? = LayerDescription.decode(layerMeta(layer)),
           stored.inkOutset == nil, transparencyLockOwner(layer) != nil {
            return description.withoutInkOutset()
        }
        return description.fittedToInk()
    }
}
