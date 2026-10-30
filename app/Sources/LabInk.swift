import AppKit

// The Lab Color document's foreground and background ink (#547 Lab phase 2e), CMYK's `CmykInk`
// pattern: the Eyedropper on a Lab document keeps the L*a*b* it picked exactly
// (`SharedToolState.foregroundLab`), so a colour outside sRGB paints as picked rather than through
// its clipped sRGB swatch. The swatch shows `LabPaint.display` (the core's Lab → sRGB, clipped); the
// ink is cleared whenever the swatch's colour is set any other way (`SharedToolState`'s `didSet`).
// The host converts no Lab colour itself: the ink reaches the core as the twin's own unit codes,
// which `ColorProfile.converted` passes through untouched, and the core quantizes them
// (`precision_paint::prepare_colors`).

/// One picked L*a*b* colour (D50): L* 0…100, a* and b* −128…127.
struct LabInk: Codable, Equatable {
    var l: Double
    var a: Double
    var b: Double

    /// `LabDocument.lab` of the first three normalized codes.
    init(normalized: [Double]) {
        let value = LabDocument.lab(normalized)
        l = value.l
        a = value.a
        b = value.b
    }

    /// The twin's unit codes (`lab_document`'s ICC v4 encoding), each clamped to 0…1.
    var unit: [Double] {
        [l / 100, (a + 128) / 255, (b + 128) / 255].map { min(max($0, 0), 1) }
    }
}

enum LabPaint {
    /// The colour a Lab `doc` paints for a swatch: its stored ink as the twin's codes
    /// (`doc.nsColorSpace`), alpha the swatch's; otherwise `color` unchanged.
    static func color(_ color: NSColor, stored: LabInk?, in doc: RasterDocument?) -> NSColor {
        guard let doc, doc.isLab, let stored else { return color }
        let alpha = Double(color.usingType(.componentBased)?.alphaComponent ?? 1)
        let components = (stored.unit + [alpha]).map { CGFloat($0) }
        return NSColor(colorSpace: doc.nsColorSpace, components: components, count: 4)
    }

    /// The swatch colour of `ink`: the sRGB of `RasterLab.srgb(lab:)`, clipped.
    static func display(_ ink: LabInk) -> NSColor {
        let rgb = RasterLab.srgb(lab: [ink.l, ink.a, ink.b])?.rgb ?? [0, 0, 0]
        return NSColor(srgbRed: CGFloat(rgb[0]), green: CGFloat(rgb[1]), blue: CGFloat(rgb[2]),
                       alpha: 1)
    }
}
