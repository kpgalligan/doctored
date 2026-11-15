import AppKit

// Layer Style > Pattern Overlay: the value types the sheet and the copy/record paths
// use. The core owns the schema (core/src/style_pattern.rs): a pattern is one of four frozen
// presets or a custom tile, a LINKED overlay is anchored by the core's setter where the layer was
// MADE (its offset less its moves since, Photoshop's fxrp; the tile then follows the layer), and
// `anchor` is echoed back.

/// A pattern: `{"preset": id}` or a custom tile `{"name", "width", "height", "rgba"}` (standard
/// base64 of straight RGBA8), the custom tile optionally with `native`, the samples its RGBA8
/// raster cannot hold (`PatternNative`). Exactly one form is meaningful; nil fields are not
/// written.
struct PatternRef: Codable, Equatable {
    var preset: String? = nil
    var name: String? = nil
    var width: Int? = nil
    var height: Int? = nil
    var rgba: String? = nil
    var native: PatternNative? = nil

    /// Whether this is a custom tile (it carries pixels).
    var isCustom: Bool { preset == nil && rgba != nil }

    /// This pattern without its native samples: what a reader of the RGBA8 raster alone takes
    /// (Mode ▸ Bitmap's Custom Pattern, whose options are bounded).
    var withoutNative: PatternRef {
        var out = self
        out.native = nil
        return out
    }
}

/// A custom tile's native samples (core/src/pattern_samples.rs, `designs/pattern-tiles.md`):
/// `kind` "rgb16", "rgb32" or "cmyk16", `data` the standard base64 of the samples'
/// little-endian components. A document that stores that kind of number reads them in place of
/// the RGBA8 raster, so a pattern defined from a CMYK, 16-bit or 32-bit document fills back
/// exactly; the core validates them.
struct PatternNative: Codable, Equatable {
    var kind: String
    var data: String

    static let kinds: Set<String> = ["rgb16", "rgb32", "cmyk16"]

    var json: [String: Any] { ["kind": kind, "data": data] }

    /// `object` as samples when it is `{"kind", "data"}` with a known kind and a string; nil
    /// otherwise.
    static func from(_ object: Any?) -> PatternNative? {
        guard let object = object as? [String: Any], object.count == 2,
              let kind = object["kind"] as? String, kinds.contains(kind),
              let data = object["data"] as? String else { return nil }
        return PatternNative(kind: kind, data: data)
    }
}

/// The pattern fill a Pattern Overlay and a Stroke of Fill Type Pattern share
/// (core/src/style_pattern_fill.rs): the overlay holds these keys flat, a stroke in its
/// `pattern_fill` object. `pattern` nil is the core's default (checkerboard); `anchor` nil asks
/// the core to anchor a linked fill where the layer was made.
struct PatternFill: Codable, Equatable {
    var pattern: PatternRef? = nil
    /// The Angle in degrees, normalized by the core into -180..<180.
    var angle = 0.0
    /// 0.01…10 (1…1000 %).
    var scale = 1.0
    var linkWithLayer = true
    /// The tile's offset in canvas px.
    var phase = [0.0, 0.0]
    var anchor: [Int]? = nil
}

/// The `pattern_overlay` effect, key for key (snake_case through the coders). `pattern` nil is
/// the core's default (checkerboard); `anchor` nil asks the core to anchor a linked overlay where
/// the layer was made.
struct PatternOverlayEffect: Codable, Equatable {
    var type = LayerStyleEffectKind.patternOverlay.rawValue
    var enabled = true
    var blend = "normal"
    var opacity = 1.0
    var pattern: PatternRef? = nil
    /// The Angle in degrees, normalized by the core into -180..<180; turns the tile about
    /// its origin.
    var angle = 0.0
    /// 0.01…10 (1…1000 %).
    var scale = 1.0
    var linkWithLayer = true
    /// The tile's offset in canvas px.
    var phase = [0.0, 0.0]
    var anchor: [Int]? = nil

    /// The six fill fields as one `PatternFill`, so the overlay's pane and a Pattern stroke's
    /// share one set of rows (`PatternFillRows`); the stored fields stay flat.
    var fill: PatternFill {
        get {
            PatternFill(
                pattern: pattern, angle: angle, scale: scale, linkWithLayer: linkWithLayer,
                phase: phase, anchor: anchor)
        }
        set {
            pattern = newValue.pattern
            angle = newValue.angle
            scale = newValue.scale
            linkWithLayer = newValue.linkWithLayer
            phase = newValue.phase
            anchor = newValue.anchor
        }
    }
}

/// A Bevel & Emboss texture (core/src/style_texture.rs): the pattern fill's
/// placement read as HEIGHT, with Depth (1.0 = +100 %, the core clamps to -10…10) and Invert.
/// The texture has no Angle, so `fill` reads it as 0 and drops a set one.
/// `pattern` nil is the core's default (checkerboard); `anchor` nil asks the core to anchor a
/// linked texture where the layer was made.
struct BevelTexture: Codable, Equatable {
    var pattern: PatternRef? = nil
    /// 0.01…10 (1…1000 %).
    var scale = 1.0
    var linkWithLayer = true
    /// The tile's offset in canvas px.
    var phase = [0.0, 0.0]
    var anchor: [Int]? = nil
    var depth = 1.0
    var invert = false

    /// The placement fields as one `PatternFill` (angle 0), so the texture shares the overlay's
    /// rows (`PatternFillRows` without its Angle row).
    var fill: PatternFill {
        get {
            PatternFill(
                pattern: pattern, angle: 0, scale: scale, linkWithLayer: linkWithLayer,
                phase: phase, anchor: anchor)
        }
        set {
            pattern = newValue.pattern
            scale = newValue.scale
            linkWithLayer = newValue.linkWithLayer
            phase = newValue.phase
            anchor = newValue.anchor
        }
    }
}

/// One preset as the sheet's popup shows it, with its tile (`width` × `height`, `rgba` the
/// standard base64 of its straight RGBA8 bytes) for the paths that need the pixels: Edit ▸ Fill…'s
/// 8-bit Pattern contents.
struct PatternPreset {
    let id: String
    let name: String
    let image: NSImage
    let width: Int
    let height: Int
    let rgba: String
}

extension LayerStyle {
    /// This style with every pattern fill's anchor dropped (each overlay's, each stroke's
    /// `patternFill` and the bevel's `texture`): a style moved to ANOTHER layer (Copy/Paste Layer
    /// Style, a recorded Action step) re-anchors where that layer was made: a pasted pattern
    /// starts at the canvas origin of the layer it lands on.
    func droppingPatternAnchors() -> LayerStyle {
        var out = self
        for i in out.patternOverlays.indices { out.patternOverlays[i].anchor = nil }
        for i in out.strokes.indices { out.strokes[i].patternFill?.anchor = nil }
        out.bevelEmboss?.texture?.anchor = nil
        return out
    }
}
