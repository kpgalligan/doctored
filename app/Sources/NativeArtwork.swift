import AppKit

// Retained v3 text/shape descriptions carry normalized continuous sRGB ink and whole-character
// UTF-16 runs.
//
// The ink rule (description version 4): a style's `ink` says what its numbers ARE.
// Absent means sRGB-encoded (every v3 description, its bytes unchanged); `linear_srgb` means
// linear light in sRGB primaries and is written only in a 32-bit float document, where Photoshop
// stores type and shape colour as linear numbers (measured, take2: H-raw). A document reads and
// writes its own ink (`RasterDocument.artworkInk`): `layerDescription`/`textPayload`/
// `shapePayload` convert on read, `nativeArtwork` and `NativeArtwork.drawing` convert before they
// render or publish, so a description pasted into or narrowed out of 32 bits is rewritten to look
// the same (`ArtworkInk.converted`). In memory a style's `ink` holds only nil or `.linearSRGB`.
//
// Above white (measured in Photoshop 26.10.0: `scripts/photoshop-probe/hdr_colour.py`, take2,
// row B1): linear numbers admit RGB in 0...65504 (`ExtendedRangeColor.maximum`), alpha in [0, 1];
// sRGB numbers stay in [0, 1]. Narrowing out of 32 bits clips each channel at 1 before the sRGB
// curve (N-clip), as the core's clip tone map writes the narrowed pixels.
//
// Intrinsic color glyphs retain declared RGBA8 sRGB assets and separate continuous opacity;
// intrinsic monochrome bitmap coverage retains A8 with independent ink. Never promote platform RGB
// in Swift or label intrinsic byte detail as native. Profile changes and affine/document geometry
// regenerate artwork, preserving independent masks; raw core edits must invalidate stale
// descriptions. Native v20 stores typed artwork alongside existing mixed retained sources. Saved
// raster display never requires rerendering or fonts.

/// Geometry and intrinsic glyph assets remain separate until Rust assembly.
/// Paths use output coordinates, row zero at the top. Intrinsic samples are
/// straight RGBA8 sRGB; authored opacity is applied continuously in the core.
enum NativeArtworkPart {
    case path(CGPath, rgba: [Double])
    case intrinsic(pixels: [UInt8], opacity: Double)
    case coverage(pixels: [UInt8], rgba: [Double])
    /// A fill layer's one colour over the whole drawing. No plane.
    /// `linear` is the colour as linear sRGB components beyond sRGB
    /// (an imported Photoshop `SoCo`), which the core renders in its place;
    /// never in CMYK.
    case fill(rgba: [Double], linear: [Double]?)
    /// A fill layer's gradient over the whole drawing: the style-gradient
    /// object and one `[r, g, b]` per stop, in the stops' order. No plane.
    /// `linear` is each stop's colour as linear sRGB
    /// components (an imported Photoshop `GdFl` whose stops leave sRGB), in
    /// the same order; never in CMYK. `box` is an aligned gradient
    /// fill's `[x, y, w, h]` in drawing pixels (`RasterDocument.fillAlignBox`),
    /// serialized as the component's `"box"`; nil spans the whole drawing.
    case gradient([String: Any], colors: [[Double]], linear: [[Double]]?, box: [Int]?)
    /// A pattern fill layer's tile over the whole drawing: the
    /// `pattern` component's `fill` object (`FillLayer.patternComponent`).
    /// No plane, and no colour conversion: the tile's codes are document
    /// values.
    case pattern([String: Any])
}

struct NativeArtworkDrawing {
    let width: Int
    let height: Int
    let offsetX: Int
    let offsetY: Int
    let parts: [NativeArtworkPart]
}

struct ArtworkColorRun: Codable, Equatable {
    var location: Int
    var length: Int
    var rgba: [Double]
}

/// What a description's colour numbers are. `.srgb` is never
/// stored in a style (`stored`); the payload's computed `ink` reads it back.
/// `.cmyk` (#547) is a CMYK document's: five numbers `[c, m, y, k, alpha]`,
/// each 0…1, in the document's own profile (CmykArtwork.swift). Its carrier
/// `NSColor` is built in `genericCMYK` with those five components — a
/// container for the numbers, never a colour conversion.
enum ArtworkInk: String, Codable {
    case srgb
    case linearSRGB = "linear_srgb"
    case cmyk

    /// The colour space an `NSColor` holding these numbers is built in.
    var colorSpace: NSColorSpace? {
        switch self {
        case .srgb: return .sRGB
        case .linearSRGB:
            // Extended, so an above-white number survives.
            return CGColorSpace(name: CGColorSpace.extendedLinearSRGB)
                .flatMap(NSColorSpace.init(cgColorSpace:))
        case .cmyk: return .genericCMYK
        }
    }

    /// The value a style stores: nil for `.srgb` (Decision 2's invariant).
    var stored: ArtworkInk? { self == .srgb ? nil : self }

    /// `.linearSRGB` for a colour built in the linear or extended linear
    /// sRGB space, `.srgb` for every other colour (which converts by
    /// appearance).
    static func of(_ color: NSColor) -> ArtworkInk {
        guard color.type == .componentBased,
              let name = color.colorSpace.cgColorSpace?.name,
              name == CGColorSpace.linearSRGB || name == CGColorSpace.extendedLinearSRGB
        else { return .srgb }
        return .linearSRGB
    }

    /// `rgba` re-coded from one ink to the other so it looks the same: the
    /// IEC 61966-2-1 sRGB curve on RGB (decode into linear, encode out of
    /// it), alpha untouched; the identity when the inks agree. Linear to
    /// sRGB first clamps each RGB to [0, 1] (N-clip), as the core's
    /// clip tone map does on the narrowed pixels, so a carried description
    /// agrees with them. The host's one copy of that curve; the master is
    /// `core/src/icc_builtin.rs`'s `srgb_transfer`.
    /// Ink is never re-coded here: when exactly one side is `.cmyk` the
    /// numbers come back unchanged, and the profile conversion is
    /// `RasterDocument.cmykArtwork`'s (CmykArtwork.swift).
    static func converted(_ rgba: [Double], from: ArtworkInk, to: ArtworkInk) -> [Double] {
        guard from != to, rgba.count == 4, from != .cmyk, to != .cmyk else { return rgba }
        let curve: (Double) -> Double
        switch to {
        case .linearSRGB:
            curve = { $0 <= 0.04045 ? $0 / 12.92 : pow(($0 + 0.055) / 1.055, 2.4) }
        case .srgb, .cmyk:
            curve = { value in
                let v = min(max(value, 0), 1)
                return v <= 0.0031308 ? v * 12.92 : 1.055 * pow(v, 1 / 2.4) - 0.055
            }
        }
        return [curve(rgba[0]), curve(rgba[1]), curve(rgba[2]), rgba[3]]
    }
}

struct ArtworkTextStyle: Codable, Equatable {
    var rgba: [Double]
    var runs: [ArtworkColorRun] = []
    var evaluation = "outline_area_v1"
    var intrinsicGlyphs = "platform_byte_assets_v1"
    /// nil = sRGB-encoded numbers; holds only nil or `.linearSRGB`.
    var ink: ArtworkInk?

    func isValid(for string: String) -> Bool {
        let numbers = ink ?? .srgb
        guard ArtworkColor.valid(rgba, in: numbers), runs.count <= 64, ink != .srgb,
              evaluation == "outline_area_v1", intrinsicGlyphs == "platform_byte_assets_v1" else { return false }
        let text = string as NSString
        var end = 0
        for run in runs {
            guard run.location >= end, run.length > 0, run.location <= text.length,
                  run.length <= text.length - run.location,
                  ArtworkColor.valid(run.rgba, in: numbers) else { return false }
            let range = NSRange(location: run.location, length: run.length)
            guard text.rangeOfComposedCharacterSequences(for: range) == range else { return false }
            end = run.location + run.length
        }
        return true
    }
}

extension ArtworkTextStyle {
    /// A style whose numbers are in `ink`, stored per the invariant.
    init(rgba: [Double], runs: [ArtworkColorRun] = [], in ink: ArtworkInk) {
        self.init(rgba: rgba, runs: runs, ink: ink.stored)
    }

    /// This style's numbers re-coded into `target` (`ArtworkInk.converted`);
    /// unchanged when exactly one side is `.cmyk`.
    func inInk(_ target: ArtworkInk) -> ArtworkTextStyle {
        let source = ink ?? .srgb
        guard source != target, source != .cmyk, target != .cmyk else { return self }
        var result = self
        result.rgba = ArtworkInk.converted(rgba, from: source, to: target)
        result.runs = runs.map { run in
            var run = run
            run.rgba = ArtworkInk.converted(run.rgba, from: source, to: target)
            return run
        }
        result.ink = target.stored
        return result
    }
}

struct ArtworkShapeStyle: Codable, Equatable {
    var fill: [Double]?
    var stroke: [Double]?
    var evaluation = "outline_area_v1"
    /// nil = sRGB-encoded numbers; holds only nil or `.linearSRGB`.
    var ink: ArtworkInk?
    var isValid: Bool {
        let numbers = ink ?? .srgb
        return evaluation == "outline_area_v1"
            && fill.map { ArtworkColor.valid($0, in: numbers) } != false
            && stroke.map { ArtworkColor.valid($0, in: numbers) } != false && ink != .srgb
    }
}

extension ArtworkShapeStyle {
    /// A style whose numbers are in `ink`, stored per the invariant.
    init(fill: [Double]?, stroke: [Double]?, in ink: ArtworkInk) {
        self.init(fill: fill, stroke: stroke, ink: ink.stored)
    }

    /// This style's numbers re-coded into `target` (`ArtworkInk.converted`);
    /// unchanged when exactly one side is `.cmyk`.
    func inInk(_ target: ArtworkInk) -> ArtworkShapeStyle {
        let source = ink ?? .srgb
        guard source != target, source != .cmyk, target != .cmyk else { return self }
        var result = self
        result.fill = fill.map { ArtworkInk.converted($0, from: source, to: target) }
        result.stroke = stroke.map { ArtworkInk.converted($0, from: source, to: target) }
        result.ink = target.stored
        return result
    }
}

enum ArtworkColor {
    /// Four finite numbers valid in `ink`: sRGB numbers in [0, 1]; linear
    /// numbers with RGB in `0...ExtendedRangeColor.maximum` (above white)
    /// and alpha in [0, 1]. Ink is FIVE numbers, each in [0, 1].
    static func valid(_ rgba: [Double], in ink: ArtworkInk) -> Bool {
        if ink == .cmyk {
            return rgba.count == 5 && rgba.allSatisfy { $0.isFinite && (0...1).contains($0) }
        }
        guard rgba.count == 4, rgba.allSatisfy(\.isFinite) else { return false }
        switch ink {
        case .srgb: return rgba.allSatisfy { (0...1).contains($0) }
        case .linearSRGB:
            return rgba.prefix(3).allSatisfy { (0...ExtendedRangeColor.maximum).contains($0) }
                && (0...1).contains(rgba[3])
        case .cmyk: return false
        }
    }

    /// A description colour's alpha: its LAST number in every ink (the
    /// fourth of `[r, g, b, a]`, the fifth of `[c, m, y, k, a]`).
    static func alpha(_ numbers: [Double]) -> Double { numbers.last ?? 1 }
    /// Preserve unsupported authored values so validation refuses them; a new
    /// color must never silently acquire the legacy byte-color path.
    static func authored(_ color: NSColor) -> [Double] { authored(color, in: .srgb) }
    static func rgba(_ color: NSColor) -> [Double]? { rgba(color, in: .srgb) }
    static func color(_ rgba: [Double]) -> NSColor? { color(rgba, in: .srgb) }

    /// `color`'s components in `ink`'s space — by appearance when it was
    /// built in another — unvalidated (NaN when it cannot convert).
    static func authored(_ color: NSColor, in ink: ArtworkInk) -> [Double] {
        // Ink is read off a CMYK-model colour's own five components (the
        // `.cmyk` carrier); any other colour is not ink (#547).
        if ink == .cmyk {
            guard color.type == .componentBased, color.colorSpace.colorSpaceModel == .cmyk,
                  color.numberOfComponents == 5 else { return Array(repeating: .nan, count: 5) }
            var components = [CGFloat](repeating: 0, count: 5)
            color.getComponents(&components)
            return components.map(Double.init)
        }
        // A colour already built in the linear space is read as it is, so
        // its numbers never pass through a ColorSync identity transform.
        let native = ink == .linearSRGB && ArtworkInk.of(color) == .linearSRGB
        // A colour built in another space (a Display P3 panel colour, say)
        // converts through the clamped linear space, so an out-of-gamut or
        // rounding-error component clips instead of going negative; only a
        // colour built in linear light carries an above-white number.
        let target = ink == .linearSRGB
            ? CGColorSpace(name: CGColorSpace.linearSRGB).flatMap(NSColorSpace.init(cgColorSpace:))
            : ink.colorSpace
        guard let space = native ? ink.colorSpace : target,
              let color = native ? color : ColorProfile.converted(color, into: space) else {
            return [.nan, .nan, .nan, .nan]
        }
        return [Double(color.redComponent), Double(color.greenComponent),
                Double(color.blueComponent), Double(color.alphaComponent)]
    }
    static func rgba(_ color: NSColor, in ink: ArtworkInk) -> [Double]? {
        let values = authored(color, in: ink)
        return valid(values, in: ink) ? values : nil
    }
    /// The colour whose numbers in `ink` are `rgba`, built in `ink`'s space.
    static func color(_ rgba: [Double], in ink: ArtworkInk) -> NSColor? {
        guard valid(rgba, in: ink) else { return nil }
        switch ink {
        case .srgb:
            return NSColor(srgbRed: rgba[0], green: rgba[1], blue: rgba[2], alpha: rgba[3])
        case .linearSRGB, .cmyk:
            guard let space = ink.colorSpace else { return nil }
            let components = rgba.map { CGFloat($0) }
            return NSColor(colorSpace: space, components: components, count: components.count)
        }
    }
}

extension TextLayerPayload {
    func attributedString(style: TextStyle, override: NSColor? = nil) -> NSAttributedString {
        let result = NSMutableAttributedString(string: string,
            attributes: style.attributes(color: override ?? nsColor))
        if override == nil, let artwork, artwork.isValid(for: string) {
            for run in artwork.runs {
                if let color = ArtworkColor.color(run.rgba, in: ink) {
                    result.addAttribute(.foregroundColor, value: color,
                        range: NSRange(location: run.location, length: run.length))
                }
            }
        }
        return result
    }
}

extension LayerDescription {
    /// The description with its colour numbers re-coded into `ink`
    /// (`TextLayerPayload.inInk` / `ShapeLayerPayload.inInk`); a fill's
    /// stay authored sRGB at every depth
    /// (FillLayer.swift: `FillLayer.drawing` re-codes them as it draws).
    func inInk(_ ink: ArtworkInk) -> LayerDescription {
        switch self {
        case let .text(payload): return .text(payload.inInk(ink))
        case let .shape(payload): return .shape(payload.inInk(ink))
        case .fill: return self
        }
    }
}

extension RasterDocument {
    /// The ink this document's descriptions are read and written in: ink in
    /// a CMYK document (#547), linear light at 32 bits,
    /// sRGB-encoded otherwise.
    var artworkInk: ArtworkInk { isCmyk ? .cmyk : isFloat ? .linearSRGB : .srgb }
}
