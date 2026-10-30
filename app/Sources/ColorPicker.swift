import AppKit

// The Color Picker is `ColorPairSlot`, `Hexcone` (the one hexcone, which the Info panel's
// `PixelReadout` reads too) and `PickerColor` (sRGB codes are the colour, HSB the field position,
// Lab through `RasterLab` — the core's D50 Lab on the built-in sRGB profile, `dr_srgb_to_lab` /
// `dr_lab_to_srgb`); the SB field, hue strip and new/current swatch drawn in `ColorProfile.sRGB`;
// and the sheet, whose field actions are no-ops on an unchanged value and whose OK commits only a
// changed colour, so an untouched eyedropped colour stays byte-exact.

/// The Color Picker's model: which swatch of the pair it edits,
/// the hexcone both the picker and the Info panel read HSB through, and the
/// picker's one colour value.
///
/// The colour rule (`ColorProfile.swift`'s header): what the picker hands
/// back is an AUTHORED sRGB colour. Its Lab numbers are the core's D50 Lab
/// through the built-in sRGB profile (`RasterLab`), the same pipeline the
/// Info panel reads an sRGB document through.

/// The rail swatch a picker edits.
enum ColorPairSlot {
    case foreground
    case background
}

/// The standard hexcone. Display arithmetic only, which is why it lives on
/// this side rather than in the core: the Info panel's HSB row
/// (`PixelReadout`) and the picker's HSB fields both read it.
enum Hexcone {
    /// Hue in degrees 0..<360, saturation and brightness as fractions 0…1,
    /// of encoded RGB components in 0…1.
    static func hsb(r red: Double, g green: Double, b blue: Double)
        -> (h: Double, s: Double, b: Double)
    {
        let high = max(red, green, blue)
        let low = min(red, green, blue)
        let delta = high - low
        var hue = 0.0
        if delta > 0 {
            if high == red {
                hue = 60 * ((green - blue) / delta).truncatingRemainder(dividingBy: 6)
            } else if high == green {
                hue = 60 * ((blue - red) / delta + 2)
            } else {
                hue = 60 * ((red - green) / delta + 4)
            }
        }
        if hue < 0 { hue += 360 }
        return (h: hue, s: high == 0 ? 0 : delta / high, b: high)
    }

    /// `hsb`'s inverse: encoded RGB components in 0…1. Any hue is taken
    /// modulo 360; saturation and brightness are clamped into 0…1.
    static func rgb(h: Double, s: Double, b: Double) -> (r: Double, g: Double, b: Double) {
        let hue = (h.truncatingRemainder(dividingBy: 360) + 360).truncatingRemainder(dividingBy: 360)
        let saturation = min(max(s, 0), 1)
        let brightness = min(max(b, 0), 1)
        let chroma = brightness * saturation
        let sector = hue / 60
        let second = chroma * (1 - abs(sector.truncatingRemainder(dividingBy: 2) - 1))
        let floor = brightness - chroma
        let (red, green, blue): (Double, Double, Double)
        switch Int(sector) {
        case 0: (red, green, blue) = (chroma, second, 0)
        case 1: (red, green, blue) = (second, chroma, 0)
        case 2: (red, green, blue) = (0, chroma, second)
        case 3: (red, green, blue) = (0, second, chroma)
        case 4: (red, green, blue) = (second, 0, chroma)
        default: (red, green, blue) = (chroma, 0, second)
        }
        return (r: red + floor, g: green + floor, b: blue + floor)
    }
}

/// The picker's colour. The sRGB 8-bit CODES are the colour; hue,
/// saturation and brightness are the FIELD POSITION, kept separately so a
/// grey keeps the hue a user dragged to and an HSB edit is never re-derived
/// from rounded codes. Alpha is carried through untouched — the picker never
/// edits it (System Colors… does).
struct PickerColor: Equatable {
    private(set) var red: Int
    private(set) var green: Int
    private(set) var blue: Int
    private(set) var hue: Double
    private(set) var saturation: Double
    private(set) var brightness: Double
    let alpha: CGFloat
    /// True only right after a Lab entry the sRGB gamut could not hold, so
    /// the codes are its clamp.
    private(set) var outOfGamut = false

    /// `color` read as sRGB (black when it has no sRGB form), rounded to
    /// codes the way `TextLayer.hex` rounds them.
    init(_ color: NSColor) {
        let srgb = color.usingColorSpace(.sRGB)
        let code: (CGFloat) -> Int = { Int((min(max($0, 0), 1) * 255).rounded()) }
        red = srgb.map { code($0.redComponent) } ?? 0
        green = srgb.map { code($0.greenComponent) } ?? 0
        blue = srgb.map { code($0.blueComponent) } ?? 0
        alpha = srgb?.alphaComponent ?? 1
        let derived = Hexcone.hsb(
            r: Double(red) / 255, g: Double(green) / 255, b: Double(blue) / 255)
        hue = derived.h
        saturation = derived.s
        brightness = derived.b
    }

    /// The field position as given; the codes are its rounded hexcone.
    mutating func setHSB(h: Double, s: Double, b: Double) {
        hue = h
        saturation = s
        brightness = b
        let rgb = Hexcone.rgb(h: h, s: s, b: b)
        red = Self.code(rgb.r)
        green = Self.code(rgb.g)
        blue = Self.code(rgb.b)
        outOfGamut = false
    }

    /// Codes clamped into 0…255; the field position re-derived from them.
    mutating func setRGB(r: Int, g: Int, b: Int) {
        adopt(r: r.clamped(0, 255), g: g.clamped(0, 255), b: b.clamped(0, 255))
        outOfGamut = false
    }

    /// L clamped into 0…100 and a/b into −128…127, converted by the core;
    /// the codes are the nearest sRGB colour, and `outOfGamut` says whether
    /// that was a clamp. A Lab the core cannot convert changes nothing.
    mutating func setLab(l: Double, a: Double, b: Double) {
        let lab = [min(max(l, 0), 100), min(max(a, -128), 127), min(max(b, -128), 127)]
        guard let converted = RasterLab.srgb(lab: lab), converted.rgb.count == 3 else { return }
        adopt(r: Self.code(converted.rgb[0]), g: Self.code(converted.rgb[1]),
              b: Self.code(converted.rgb[2]))
        outOfGamut = !converted.inGamut
    }

    /// `RRGGBB`, with or without "#"; false (nothing changed) for anything
    /// else, an 8-digit value included — alpha is not the picker's.
    mutating func setHex(_ text: String) -> Bool {
        var digits = text.trimmingCharacters(in: .whitespaces)
        if digits.hasPrefix("#") { digits.removeFirst() }
        guard digits.count == 6,
              let parsed = TextLayer.color(fromHex: digits)?.usingColorSpace(.sRGB)
        else { return false }
        let code: (CGFloat) -> Int = { Int((min(max($0, 0), 1) * 255).rounded()) }
        adopt(r: code(parsed.redComponent), g: code(parsed.greenComponent),
              b: code(parsed.blueComponent))
        outOfGamut = false
        return true
    }

    /// The codes' D50 Lab through the built-in sRGB profile; nil when the
    /// core cannot model it.
    var lab: (l: Double, a: Double, b: Double)? {
        guard let lab = RasterLab.lab(
            srgb: [Double(red) / 255, Double(green) / 255, Double(blue) / 255]),
            lab.count == 3
        else { return nil }
        return (l: lab[0], a: lab[1], b: lab[2])
    }

    var hex: String { String(format: "%02X%02X%02X", red, green, blue) }

    var nsColor: NSColor {
        NSColor(srgbRed: CGFloat(red) / 255, green: CGFloat(green) / 255,
                 blue: CGFloat(blue) / 255, alpha: alpha)
    }

    /// New codes and the field position derived from them — keeping the
    /// previous hue where the hexcone has none (no saturation or no
    /// brightness) and the previous saturation where it has none (black).
    private mutating func adopt(r: Int, g: Int, b: Int) {
        red = r
        green = g
        blue = b
        let derived = Hexcone.hsb(r: Double(r) / 255, g: Double(g) / 255, b: Double(b) / 255)
        if derived.s > 0 && derived.b > 0 { hue = derived.h }
        if derived.b > 0 { saturation = derived.s }
        brightness = derived.b
    }

    private static func code(_ component: Double) -> Int {
        Int((min(max(component, 0), 1) * 255).rounded())
    }
}
