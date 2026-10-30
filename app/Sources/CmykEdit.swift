import AppKit

// Editing a CMYK document in ink (#1 phase 2): the ink value `CmykInk` (the Color Picker's
// C/M/Y/K fields, the stored foreground/background ink in `SharedToolState`, MCP's `ink`), the
// `CmykPaint` request builders every paint command site branches to (Brush, Eraser, Fill, Gradient,
// Paint Bucket: `cmyk_paint` in the core, ink in place of colour, in the CMYK modes; the bucket's All
// Layers is `cmykBucketFilledSamplingComposite`) and `CmykTone`, the Levels/Curves namespace
// `CmykToneTools.swift` extends. The gate is `CmykDocument.swift`; the
// conversions are `DoctoredCore+Cmyk.swift`'s `dr_cmyk_color` wrappers, the one colour engine.

/// One CMYK ink, each component a percent 0…100 (100 = full ink), as the Color Picker and the
/// Info panel show it and MCP's `ink` passes it.
struct CmykInk: Codable, Equatable {
    var c: Double
    var m: Double
    var y: Double
    var k: Double

    /// The core's unit form, 0…1 each, clamped.
    var unit: [Double] { [c, m, y, k].map { min(max($0, 0), 100) / 100 } }

    /// Total ink, percent (0…400), the Info panel's "Total ink".
    var total: Double { [c, m, y, k].map { min(max($0, 0), 100) }.reduce(0, +) }

    /// Four percents; nil unless every one is a finite number in 0…100.
    init?(percent values: [Double]) {
        guard values.count == 4, values.allSatisfy({ $0.isFinite && (0...100).contains($0) }) else {
            return nil
        }
        self.init(c: values[0], m: values[1], y: values[2], k: values[3])
    }

    init(c: Double, m: Double, y: Double, k: Double) {
        self.c = c
        self.m = m
        self.y = y
        self.k = k
    }

    /// The four percents, rounded to the picker's whole percent.
    var rounded: [Int] { [c, m, y, k].map { Int($0.rounded()) } }
}

/// The paint requests of a CMYK document (`cmyk_paint`'s `CmykPaintEdit`): `ink` is straight
/// `[c, m, y, k, alpha]` in 0…1, in any mode of `NativeLayerBlending.cmykModes` (#1's retouch
/// batch: the mode composites on `1 − ink`, `cmyk_composite::over_mode`).
enum CmykPaint {
    /// The refusal of a paint, fill, stroke or clone `mode` a CMYK document does not offer (Pass
    /// Through is a group's), in the core's words; nil for a mode it paints in.
    static func modeRefusal(_ mode: DrBlendMode) -> String? {
        NativeLayerBlending.cmykModes.contains(mode) ? nil
            : "The \(DrBlendMode.displayName(for: mode)) blend mode is unavailable in a CMYK document."
    }

    /// The ink a colour paints with in `document`: the `stored` ink (a picked or eyedropped CMYK
    /// value, kept exactly) when there is one, else the colour converted through the document's
    /// profile (`dr_cmyk_color`); the colour's alpha is the fifth component.
    static func ink(_ color: NSColor, stored: CmykInk?, in document: RasterPrecisionDocument) throws -> [Double] {
        let srgb = color.usingColorSpace(.sRGB) ?? NSColor.black
        let alpha = Double(min(max(srgb.alphaComponent, 0), 1))
        if let stored { return stored.unit + [alpha] }
        let rgb = [srgb.redComponent, srgb.greenComponent, srgb.blueComponent]
            .map { Double(min(max($0, 0), 1)) }
        return try RasterPrecisionDocument.cmykInk(fromSRGB: rgb, document: document).unit + [alpha]
    }

    /// The captured request (`PrecisionPixelCommand`) of one CMYK paint op; refuses an RGB colour
    /// key, which the core would refuse too.
    static func command(_ request: [String: Any]) throws -> PrecisionPixelCommand {
        for key in ["color", "color_space", "start", "end"] where request[key] != nil {
            throw DoctoredCoreError(message: "A CMYK paint request carries ink, not \(key).")
        }
        return try PrecisionPixelCommand(request)
    }

    /// A gradient's stops in ink: each stop's position and its colour's ink (`ink(_:stored:in:)`
    /// with no stored value but at the two ends, which take the foreground and background inks).
    /// `reversed` is the ramp's Reverse (`GradientRamp` put the background's colour first), so
    /// each end takes the ink of the swatch its colour came from. `midpoints` (parallel to
    /// `ramp`) adds each stop's midpoint keys that are not 0.5.
    static func stops(_ ramp: [(position: Double, color: NSColor)], foreground: CmykInk?,
                      background: CmykInk?, reversed: Bool = false,
                      midpoints: [GradientRamp.StopMidpoint]? = nil,
                      in document: RasterPrecisionDocument) throws -> [[String: Any]] {
        let (first, last) = reversed ? (background, foreground) : (foreground, background)
        return try ramp.enumerated().map { index, stop in
            let stored = index == 0 ? first : index == ramp.count - 1 ? last : nil
            var entry: [String: Any] = [
                "position": stop.position, "ink": try ink(stop.color, stored: stored, in: document),
            ]
            if let midpoints, index < midpoints.count {
                entry.merge(midpoints[index].keys) { _, new in new }
            }
            return entry
        }
    }
}

/// Levels and Curves on ink (#1 phases 2 and 5): the histogram, Auto, the eyedroppers and
/// presets on ink, and the scope a dialog reads, are `CmykToneTools.swift`'s extension.
enum CmykTone {}

extension RasterPrecisionDocument {
    /// The Paint Bucket's All Layers on a CMYK document (#1), `bucketFilledSamplingComposite`'s
    /// twin: the composite's `magic_wand` region over its INK (`cmyk_select`, the same predicate
    /// the single-layer bucket floods with), then a CMYK `paint_blend` of `ink` (straight
    /// `[c, m, y, k, alpha]`, 0…1) through `mode` on `layer` over that region and the selection
    /// plane. nil when
    /// the seed is outside the selection or nothing changes.
    func cmykBucketFilledSamplingComposite(layer: Int, x: Int, y: Int, tolerance: Int, contiguous: Bool,
                                           ink: [Double], opacity: Double,
                                           mode: DrBlendMode = DR_BLEND_NORMAL, selection: CanvasSelection?,
                                           width: Int, height: Int) throws -> RasterPrecisionDocument? {
        guard x >= 0, y >= 0, x < width, y < height else {
            throw DoctoredCoreError(message: "The fill's seed point is outside the canvas.")
        }
        let selected = selection?.maskSamples16()
        if let selected, selected.count != width * height || selected[y * width + x] == 0 { return nil }
        let region = try NativeSelectionWork(
            source: self, width: width, height: height,
            operation: ["kind": "magic_wand", "x": x, "y": y, "tolerance": tolerance,
                        "contiguous": contiguous],
            scope: .composite).run(title: "Fill")
        var planes = [try PrecisionScalarBuffer(samples: region, width: width, height: height)]
        if let selected {
            planes.append(try PrecisionScalarBuffer(
                samples: selected, width: width, height: height,
                provenance: selection?.hasNativeCoverage == true ? .nativeU16 : .promotedU8))
        }
        let command = try CmykPaint.command(["op": "paint_blend", "layer": layer, "ink": ink,
                                             "opacity": opacity, "blend_mode": Int(mode.rawValue),
                                             "selection": selected != nil])
        return try PrecisionPixelEdit(self, command: command, input: .joining(planes)).run(title: "Fill")
    }
}
