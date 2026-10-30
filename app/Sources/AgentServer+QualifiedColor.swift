import AppKit

/// PROFILE-QUALIFIED COLOUR (Phase 12A).
///
/// A hex string carries no colour space, so every hex over MCP has always
/// meant one authored sRGB colour converted once into the document's space.
/// That is exactly right for "paint me a red" and exactly wrong for "sample
/// this wide-gamut pixel and paint it back": no sRGB hex can name a colour
/// outside the sRGB gamut (`ColorProfile.paintBytes` says so, and reports it).
///
/// `color_space` is the sibling argument that names the hex's space:
///
/// - absent or `"srgb"` — TODAY'S MEANING, unchanged: an authored sRGB hex,
///   converted once into the document's space.
/// - `"document"` — the hex IS the document's own bytes, used verbatim. This
///   is what `sample_color`'s `hex` reports and what a wide-gamut recording
///   carries (`ActionArgs.documentHex`), so sample → paint is exact.
/// - `"display_p3"` — the hex is a Display P3 colour, converted once into the
///   document's space. Never recorded; it exists so a caller holding P3
///   numbers need not convert them by hand.
///
/// Numeric `rgba` stays a 16-bit-only spelling (`precisionInk`); this is the
/// byte surface's answer, and the two agree on the vocabulary. At 16 bits
/// a `"document"` hex means the same thing: each byte v is the
/// document's own component v/255, the exact code v·257 (`documentHexInk`).
///
/// Which tools take the argument at all is `DocumentPrecision.colorSpaceGate`'s
/// one answer; every other tool refuses it by name at every depth.
///
/// Text and shape LAYER colours (`add_text_layer`, `add_shape_layer` and
/// their edits) are continuous sRGB numbers, not hex, so they never take
/// `color_space`. Instead `sample_color` reports the
/// sampled pixel as `srgb_rgba` (`SRGBReading`, extended sRGB, never
/// clipped) with `srgb_in_gamut`; passing those numbers as `rgba`,
/// `fill_rgba` or `stroke_rgba` reuses the sampled colour. A pixel outside
/// sRGB reads outside 0…1, and the artwork validator refuses it there.
enum ColorSpaceArgument: String {
    case srgb, document
    case displayP3 = "display_p3"

    static let refusal = "color_space must be srgb, document or display_p3."
}

/// A document-space sample as continuous extended-sRGB numbers.
struct SRGBReading {
    static let epsilon = 1e-6
    static let outOfGamutNote =
        "srgb_rgba is outside 0…1, so a text or shape colour cannot hold this pixel either."

    /// 4 values; RGB clamped to 0...1 when in gamut, the raw extended values
    /// otherwise; alpha passed through.
    let rgba: [Double]
    /// Every RGB component within [-epsilon, 1+epsilon].
    let inGamut: Bool

    /// nil when `space` cannot convert to `.extendedSRGB` (e.g. a LUT profile
    /// with no usable tables) or converts through the core
    /// (`ColorProfile.convertsThroughCore`, a LUT-mode profile), which
    /// has no extended form. `.extendedSRGB`, never `.sRGB`, which would clip.
    init?(documentComponents: [Double], in space: NSColorSpace) {
        guard documentComponents.count == 4, !ColorProfile.convertsThroughCore(space)
        else { return nil }
        let components = documentComponents.map { CGFloat($0) }
        guard let converted = NSColor(colorSpace: space, components: components, count: 4)
            .usingColorSpace(.extendedSRGB) else { return nil }
        let rgb = [converted.redComponent, converted.greenComponent, converted.blueComponent]
            .map { Double($0) }
        guard rgb.allSatisfy({ $0.isFinite }) else { return nil }
        let inside = rgb.allSatisfy { $0 >= -Self.epsilon && $0 <= 1 + Self.epsilon }
        inGamut = inside
        rgba = (inside ? rgb.map { min(max($0, 0), 1) } : rgb) + [documentComponents[3]]
    }

    var fields: [String: Any] { ["srgb_rgba": rgba, "srgb_in_gamut": inGamut] }
}

extension AgentServer {
    /// The one `color_space` validator: absent is `.srgb`; an unknown name
    /// refuses with `ColorSpaceArgument.refusal`.
    func colorSpaceArgument(_ a: [String: Any]) throws -> ColorSpaceArgument {
        guard let named = stringArg(a, "color_space") else { return .srgb }
        guard let space = ColorSpaceArgument(rawValue: named) else {
            throw ToolError(message: ColorSpaceArgument.refusal)
        }
        return space
    }

    /// A `"document"` hex at 16 bits: the hex's four bytes, alpha included,
    /// as the document's own normalized components (v/255, so the core's
    /// u16 code is exactly v·257). `parseColor` built the colour from hex/255
    /// in sRGB, so reading its sRGB bytes back recovers the literal hex.
    func documentHexInk(_ a: [String: Any], _ key: String,
                        fallback: NSColor) throws -> PrecisionInk {
        let authored = try parseColor(a, key, fallback: fallback)
        guard let bytes = ColorProfile.bytes(authored, in: .sRGB), bytes.count == 4 else {
            throw ToolError(message: "Could not convert the color")
        }
        let v = bytes.map { Double($0) / 255 }
        return try PrecisionInk(red: v[0], green: v[1], blue: v[2], alpha: v[3])
    }

    /// The colour a paint/fill/gradient argument names, QUANTIZED to the
    /// document's own bytes and expressed in the document's space.
    ///
    /// Quantizing here is what makes a byte canvas commit and its replay
    /// byte-identical: both rasterize from the same document-space colour
    /// through `SoftBrush.rasterizeDabs` (`ColorProfile.quantized`).
    func qualifiedColor(
        _ a: [String: Any], _ key: String, in document: ImageDocument, fallback: NSColor
    ) throws -> NSColor {
        let resolved = try resolvedColor(a, key, in: document, fallback: fallback)
        guard let quantized = ColorProfile.quantized(resolved, in: document.nsColorSpace) else {
            throw ToolError(message: "Could not convert the color")
        }
        return quantized
    }

    /// The colour a COVERAGE target (mask, Quick Mask, channel, plane)
    /// reduces to gray: `qualifiedColor`, except on a LUT-mode document
    /// (`ColorProfile.convertsThroughCore`), where it is the colour
    /// before quantizing, so an authored white reaches the mask as 255
    /// (`coverageRGBA`) rather than as the 235 the LUT encodes it to.
    func coverageColor(
        _ a: [String: Any], _ key: String, in document: ImageDocument, fallback: NSColor
    ) throws -> NSColor {
        guard ColorProfile.convertsThroughCore(document.nsColorSpace) else {
            return try qualifiedColor(a, key, in: document, fallback: fallback)
        }
        return try resolvedColor(a, key, in: document, fallback: fallback)
    }

    /// `qualifiedColor` before it is quantized: the authored colour in its
    /// own space (sRGB or Display P3), or a `document` hex as the document's
    /// own components.
    private func resolvedColor(
        _ a: [String: Any], _ key: String, in document: ImageDocument, fallback: NSColor
    ) throws -> NSColor {
        let authored = try parseColor(a, key, fallback: fallback)
        let space = document.nsColorSpace
        let resolved: NSColor
        switch try colorSpaceArgument(a) {
        case .srgb:
            resolved = authored
        case .document:
            // The hex's four bytes, reinterpreted as the DOCUMENT's own
            // components. `parseColor` built the colour from hex/255 in sRGB,
            // so reading its sRGB bytes back recovers the literal hex exactly.
            resolved = NSColor(
                colorSpace: space, components: try components(authored, in: .sRGB), count: 4)
        case .displayP3:
            resolved = NSColor(
                colorSpace: .displayP3, components: try components(authored, in: .sRGB),
                count: 4)
        }
        return resolved
    }

    private func components(_ color: NSColor, in space: NSColorSpace) throws -> [CGFloat] {
        guard let bytes = ColorProfile.bytes(color, in: space) else {
            throw ToolError(message: "Could not convert the color")
        }
        return bytes.map { CGFloat($0) / 255 }
    }
}
