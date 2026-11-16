import AppKit

// Warp Text (designs/warp-text.md, WT-1..WT-7): a text layer whose glyph outlines and decorations
// go through one of nine envelopes that bend the text block's box onto itself. The description
// is `TextLayerPayload.warp` (version 8, TextLayer.swift); TextKit still lays the block out
// straight, and the artwork capture sends every outline through the core's envelope map
// (ArtworkTextWarp.swift). This file holds the value, the payload's setter and the raster rect.
// Layer ▸ Warp Text… is EditorViewController+WarpText.swift; the MCP argument is
// AgentServer+TextWarp.swift.

/// A text layer's warp: an envelope style and its three numbers, each in −1…1. JSON keys
/// `style`, `bend`, `horizontal`, `vertical`, all four always written.
struct TextWarp: Codable, Equatable {
    /// The styles, in the Style popup's order (the names are Doctored's own; the formulas are
    /// core `outline_envelope.rs`'s module doc).
    static let styles = ["arch", "upper", "lower", "bulge", "squeeze", "inflate", "flag", "rise", "arc"]
    /// The popup's names, in `styles`' order.
    static let titles = ["Arch", "Upper", "Lower", "Bulge", "Squeeze", "Inflate", "Flag", "Rise", "Arc"]
    /// How far, in source px, the raster rect is grown around the mapped rect: the outlines'
    /// flattening (0.05 raster px) and antialiasing stay inside it.
    static let margin: CGFloat = 1
    /// The refusal for a glyph with no outline to bend: a colour or bitmap glyph (emoji).
    static let outlineGlyphsRefusal = "warped text needs outline glyphs: remove the colour or "
        + "bitmap glyphs (emoji), or remove the warp"

    var style: String
    var bend: Double
    var horizontal: Double
    var vertical: Double

    /// A known style and three finite numbers in −1…1.
    var isValid: Bool {
        Self.styles.contains(style)
            && [bend, horizontal, vertical].allSatisfy { $0.isFinite && (-1...1).contains($0) }
    }

    /// All three numbers 0: no warp, which is never stored (WT-1).
    var isIdentity: Bool { bend == 0 && horizontal == 0 && vertical == 0 }

    /// The envelope over the left-aligned layout rect `box`, as the core takes it.
    func envelope(box: CGRect) -> OutlineEnvelope {
        OutlineEnvelope(style: style, rect: box, bend: bend, horizontal: horizontal,
                        vertical: vertical)
    }
}

extension TextLayerPayload {
    /// The payload with `value` as its warp: nil or all zero takes it off. The ink outset is
    /// cleared either way (the next render measures its own, on the unwarped layout:
    /// TextInkRect.swift), and a payload with no style first takes one from its sRGB hex
    /// colour (`withArtwork`), since a version-8 payload always carries `artwork`.
    func withWarp(_ value: TextWarp?) -> TextLayerPayload {
        var result = self
        result.inkOutset = nil
        guard let value, !value.isIdentity else {
            result.warp = nil
            return result
        }
        result.warp = value
        return result.withArtwork()
    }
}

extension TextLayer {
    /// WT-4, the source rect of a warped layer: the core's bounds of the envelope over the
    /// layout rect, taken of the straight padded rect grown by the stored ink outset — so
    /// every point the straight rule would cover lands inside — grown by `TextWarp.margin`.
    /// A pure function of the payload and its layout, as the straight rule is. MAIN THREAD
    /// (TextKit); nil without a warp, for a failed layout or a refused envelope.
    static func warpSourceRect(_ payload: TextLayerPayload) -> CGRect? {
        guard payload.warp != nil, let layout = layout(payload) else { return nil }
        return warpSourceRect(payload, layoutRect: layout.rect)
    }

    /// `warpSourceRect` with the layout rect already measured.
    static func warpSourceRect(_ payload: TextLayerPayload, layoutRect: CGRect) -> CGRect? {
        guard let warp = payload.warp else { return nil }
        let straight = sourceRect(layoutRect: layoutRect, size: payload.size, outset: payload.inkOutset)
        guard let mapped = try? RasterPathCore.envelopeBounds(
            warp.envelope(box: layoutRect), of: straight)
        else { return nil }
        return mapped.insetBy(dx: -TextWarp.margin, dy: -TextWarp.margin)
    }
}
