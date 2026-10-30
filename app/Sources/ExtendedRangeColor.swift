import AppKit

/// Above-white colour at 32 bits: the eyedropper's float
/// sample as the foreground, and the Intensity (stops) split the Color
/// Picker and the Artwork Colors sheet show for an above-white colour.
///
/// Photoshop 26.10.0, measured (`scripts/photoshop-probe/hdr_colour.py`,
/// receipt `doctored-fixtures/photoshop/hdr-colour-317-608/take2`, row B1):
/// an `RGBFloatColor` (4, 0.5, 0.25) shape fill renders as exactly those
/// linear numbers; set as the foreground it reads back unscaled under
/// `redFloat`/`greenFloat`/`blueFloat`, and a Fill with it writes 4.0; a
/// 32 → 16/8 conversion clips each channel at 1 before encoding (N-clip).
/// The split of a colour into a base and an intensity is Doctored's own
/// (`split`): the brightest channel becomes 1 and the rest is the intensity
/// in stops.
enum ExtendedRangeColor {
    /// The largest RGB number a 32-bit float document stores; the master is
    /// core `float_document::FLOAT_RANGE_MAX`.
    static let maximum: Double = 65504
    /// The Intensity field's range in stops (2^16 = 65536 just covers
    /// `maximum` from a base at 1).
    static let intensityRange: ClosedRange<Double> = -20...16
    /// Whether the foreground holds an above-white sample (take2, FH-float:
    /// `FrgC` read back (4, 0.5, 0.25) unscaled).
    static let foregroundHoldsAboveWhite = true

    /// Extended linear sRGB: linear light in sRGB primaries, components
    /// beyond [0, 1] kept. The named space always resolves on macOS 15.
    static var space: NSColorSpace {
        CGColorSpace(name: CGColorSpace.extendedLinearSRGB)
            .flatMap(NSColorSpace.init(cgColorSpace:)) ?? .extendedSRGB
    }

    /// `rgb` (alpha, when present, passes through) as a base whose
    /// brightest channel is 1 and the intensity in stops that restores it,
    /// when that channel is above 1; otherwise `rgb` itself at intensity 0.
    static func split(_ rgb: [Double]) -> (base: [Double], intensity: Double) {
        guard let top = rgb.prefix(3).max(), top.isFinite, top > 1 else { return (rgb, 0) }
        let base = rgb.enumerated().map { $0.offset < 3 ? $0.element / top : $0.element }
        return (base, log2(top))
    }

    /// `base`'s RGB × 2^`intensity` (alpha passes through); nil unless every
    /// result is finite and the RGB is in `0...maximum`.
    static func combined(_ base: [Double], intensity: Double) -> [Double]? {
        guard intensity.isFinite else { return nil }
        let scale = pow(2, intensity)
        let values = base.enumerated().map { $0.offset < 3 ? $0.element * scale : $0.element }
        guard values.allSatisfy(\.isFinite),
              values.prefix(3).allSatisfy({ (0...maximum).contains($0) }) else { return nil }
        return values
    }

    /// `color` read in `space` and split; the base comes back as an sRGB
    /// colour. A colour that cannot be read in `space` is its own base.
    static func split(_ color: NSColor) -> (base: NSColor, intensity: Double) {
        guard let linear = color.usingColorSpace(space) else { return (color, 0) }
        let values = [linear.redComponent, linear.greenComponent, linear.blueComponent,
                      linear.alphaComponent].map(Double.init)
        let (base, intensity) = split(values)
        guard intensity != 0 else { return (color.usingColorSpace(.sRGB) ?? color, 0) }
        let built = NSColor(colorSpace: space, components: base.map { CGFloat($0) }, count: 4)
        return (built.usingColorSpace(.sRGB) ?? built, intensity)
    }

    /// `base` with its RGB × 2^`intensity` in `space`; `base` unchanged at
    /// intensity 0, nil when `combined` refuses the numbers.
    static func combined(_ base: NSColor, intensity: Double) -> NSColor? {
        guard intensity != 0 else { return base }
        guard let linear = base.usingColorSpace(space),
              let values = combined([linear.redComponent, linear.greenComponent,
                                     linear.blueComponent, linear.alphaComponent].map(Double.init),
                                    intensity: intensity) else { return nil }
        return NSColor(colorSpace: space, components: values.map { CGFloat($0) }, count: 4)
    }

    /// The eyedropper's colour for a 32-bit float sample: the block mean,
    /// linear light in the document's primaries, carried into `space` with
    /// RGB clamped to `[0, maximum]` (`[0, 1]` when the foreground holds no
    /// above-white colour) and alpha to [0, 1]. nil when it cannot convert.
    static func eyedropped(_ sample: FloatReadout, in document: RasterDocument) -> NSColor? {
        guard sample.mean.count == 4 else { return nil }
        let source = CGColorSpaceCreateExtendedLinearized(document.drawingSpace)
            .flatMap(NSColorSpace.init(cgColorSpace:)) ?? space
        let sampled = NSColor(colorSpace: source, components: sample.mean.map { CGFloat($0) },
                              count: 4)
        guard let linear = sampled.usingColorSpace(space) else { return nil }
        let top = foregroundHoldsAboveWhite ? maximum : 1
        let clamp: (CGFloat, Double) -> CGFloat = { value, upper in
            value.isFinite ? CGFloat(min(max(Double(value), 0), upper)) : 0
        }
        let components = [clamp(linear.redComponent, top), clamp(linear.greenComponent, top),
                          clamp(linear.blueComponent, top), clamp(linear.alphaComponent, 1)]
        return NSColor(colorSpace: space, components: components, count: 4)
    }

    /// The document's own primaries as linear light with components beyond
    /// [0, 1] kept: what a float sample is expressed in (`eyedropped`'s
    /// source space).
    private static func linearSpace(of document: RasterDocument) -> NSColorSpace {
        CGColorSpaceCreateExtendedLinearized(document.drawingSpace)
            .flatMap(NSColorSpace.init(cgColorSpace:)) ?? space
    }

    /// `color` as above-white Fill / Stroke ink: linear light in the
    /// document's primaries, RGB clamped to `0...maximum` and alpha to 0…1.
    /// nil unless some RGB is above 1 + 1e-6 (an in-range colour keeps the
    /// document route), or when it cannot convert. `linearNumbers`, kept only
    /// when above white: the clamp to `0...maximum` moves no value across 1.
    static func aboveWhite(_ color: NSColor, in document: RasterDocument) -> [Double]? {
        guard let values = linearNumbers(color, in: document),
              values.prefix(3).contains(where: { $0 > 1 + 1e-6 }) else { return nil }
        return values
    }

    /// `color` as linear light in the document's primaries, RGB clamped to
    /// `0...maximum` and alpha to 0…1; nil when it cannot convert. The
    /// Gradient's endpoints at float (`FloatPaint.linearEndpoints`):
    /// unlike `aboveWhite` it answers for an in-range colour too, so both
    /// ends of a ramp are expressed the same way.
    static func linearNumbers(_ color: NSColor, in document: RasterDocument) -> [Double]? {
        guard let linear = color.usingColorSpace(linearSpace(of: document)) else { return nil }
        let values = [linear.redComponent, linear.greenComponent, linear.blueComponent,
                      linear.alphaComponent].map(Double.init)
        guard values.allSatisfy(\.isFinite) else { return nil }
        return values.enumerated().map { min(max($0.element, 0), $0.offset < 3 ? maximum : 1) }
    }

    /// `rgba` — linear light in the document's primaries, as `aboveWhite`
    /// answers and MCP's `rgba32` passes — as a colour in that space; nil
    /// unless it is four finite numbers.
    static func linearColor(_ rgba: [Double], in document: RasterDocument) -> NSColor? {
        guard rgba.count == 4, rgba.allSatisfy(\.isFinite) else { return nil }
        return NSColor(colorSpace: linearSpace(of: document),
                       components: rgba.map { CGFloat($0) }, count: 4)
    }
}
