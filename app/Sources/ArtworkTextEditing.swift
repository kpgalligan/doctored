import AppKit
import CoreText

// Artwork additions land at the slot `position` names at BOTH depths and select it (12E-2b); the
// typed path no longer appends a top root. Generated artwork ignores canvas selection clipping.
// Parent job controls retain cancellation across fresh child stages, report monotonic
// progress/current scratch and recheck captured source/target/selection before one central
// publication. Limits and qualification live in `designs/phase-8e-artwork-status.md`.

/// Color-font callbacks ignore foreground alpha. The editor uses the same
/// continuous run opacity as native assembly, applied around the intrinsic run.
final class ArtworkEditingLayoutManager: NSLayoutManager {
    /// The colour a run is SHOWN in (#547 phase 6b: a Grayscale document's gray of it); the
    /// storage, and so the payload, keep the authored colour. Colour glyphs keep their own.
    var displayInk: ((NSColor) -> NSColor)?

    override init() { super.init() }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("ArtworkEditingLayoutManager does not support NSCoder") }

    override func showCGGlyphs(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>,
                              count: Int, font: NSFont, textMatrix: CGAffineTransform,
                              attributes: [NSAttributedString.Key: Any], in context: CGContext) {
        let foreground = attributes[.foregroundColor] as? NSColor
        let opacity = foreground?.alphaComponent ?? 1
        let colorGlyphs = CTFontGetSymbolicTraits(font as CTFont).contains(.traitColorGlyphs)
        if !colorGlyphs, let displayInk, let foreground {
            let ink = displayInk(foreground)
            var shown = attributes
            shown[.foregroundColor] = ink
            context.saveGState()
            context.setFillColor(ink.cgColor)
            super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
                textMatrix: textMatrix, attributes: shown, in: context)
            context.restoreGState()
            return
        }
        guard colorGlyphs, opacity < 1 else {
            super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
                textMatrix: textMatrix, attributes: attributes, in: context)
            return
        }
        var opaque = attributes
        opaque[.foregroundColor] = NSColor.white
        context.saveGState()
        context.setAlpha(opacity)
        context.beginTransparencyLayer(auxiliaryInfo: nil)
        super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
            textMatrix: textMatrix, attributes: opaque, in: context)
        context.endTransparencyLayer()
        context.restoreGState()
    }
}

enum ArtworkTextEditing {
    static func makeView(frame: CGRect, displayInk: ((NSColor) -> NSColor)? = nil) -> CanvasTextView {
        let storage = NSTextStorage()
        let manager = ArtworkEditingLayoutManager()
        manager.displayInk = displayInk
        let container = NSTextContainer(size: frame.size)
        manager.addTextContainer(container); storage.addLayoutManager(manager)
        return CanvasTextView(frame: frame, textContainer: container)
    }

    static func restyle(_ view: NSTextView, style: TextStyle, ink: NSColor) {
        // Compared as linear numbers, unclamped, so an above-white change
        // counts; validated, so two unconvertible colours compare
        // equal (nil) rather than as NaN.
        let previous = ArtworkColor.rgba((view as? CanvasTextView)?.artworkToolColor ?? ink,
                                         in: .linearSRGB)
        (view as? CanvasTextView)?.artworkToolColor = ink
        let next = ArtworkColor.rgba(ink, in: .linearSRGB)
        let attributes = style.attributes(color: ink)
        var typography = attributes; typography.removeValue(forKey: .foregroundColor)
        view.defaultParagraphStyle = TextStyle.paragraphStyle(alignment: style.alignment, leading: style.leading)
        view.typingAttributes = attributes; view.insertionPointColor = ink
        guard let storage = view.textStorage, storage.length > 0 else { return }
        let range = NSRange(location: 0, length: storage.length)
        // Remove optional typography left by a previous style, retaining colors.
        for key in [NSAttributedString.Key.kern, .baselineOffset, .underlineStyle, .strikethroughStyle] {
            storage.removeAttribute(key, range: range)
        }
        storage.addAttributes(typography, range: range)
        if previous != next, view.selectedRange().length > 0 {
            storage.addAttribute(.foregroundColor, value: ink, range: view.selectedRange())
        }
    }

    /// The session's payload, in the ink of its base colour's space (a
    /// reopened 32-bit layer's linear sRGB colour keeps `.linearSRGB`; any
    /// other colour is sRGB), which the commit converts to the document's.
    /// Its caller is frozen, so the ink is not a parameter.
    static func payload(view: NSTextView, style: TextStyle, ink: NSColor, width: Double) -> TextLayerPayload {
        let baseColor = (view as? CanvasTextView)?.artworkBaseColor ?? ink
        let artworkInk = ArtworkInk.of(baseColor)
        var result = TextLayerPayload(string: view.string, style: style, color: baseColor,
                                      box: .width(width), ink: artworkInk)
        guard let storage = view.textStorage, let base = result.artwork?.rgba else { return result }
        var runs: [ArtworkColorRun] = []
        storage.enumerateAttribute(.foregroundColor, in: NSRange(location: 0, length: storage.length)) { value, range, _ in
            guard let color = value as? NSColor else { return }
            let rgba = ArtworkColor.authored(color, in: artworkInk)
            if rgba != base { runs.append(ArtworkColorRun(location: range.location, length: range.length, rgba: rgba)) }
        }
        result.artwork?.runs = runs
        return result
    }
}

extension ImageCanvasView {
    func restoreArtworkText(_ payload: TextLayerPayload) {
        guard let view = activeTextView, let storage = view.textStorage else { return }
        let selection = view.selectedRange()
        view.artworkBaseColor = payload.nsColor
        view.artworkToolColor = payload.nsColor
        storage.setAttributedString(payload.attributedString(style: payload.style))
        view.setSelectedRange(selection)
        resizeActiveTextSession()
    }
}
