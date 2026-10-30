import AppKit

// **Phase 12E-3** adds `knockout` and `blendInteriorAsGroup` beside `fillOpacity` and `blendIf`:
// both are decoded, both are encoded ONLY when they are not at their default (the core's own
// conditional-emission rule, mirrored so a style without them serializes to exactly the JSON it
// always did and the sheet's byte-stable "unchanged" compare keeps working), and `knockout !=
// .none` makes a style non-identity while the lone flag does NOT.
//
// **Phase 12E-3b** makes Merge Down and Merge Layers EXACT — they capture the real backdrop — and
// replaces 12E-3's blanket knockout merge note with a RESIDUAL one:
// `LayerStyle.knockoutMergeResidualNote` and `RasterDocument.knockoutMergeResidualNote(merging:)`
// fire only where NO backdrop can be captured (R1 an isolated ancestor stops the mode with a
// visible sibling beneath the set, R2 a merged pass-through nest, R3 a lowest root clipped to a
// base outside the set), and `RasterDocument.isIsolatedGroup` is a named host MIRROR of the core's
// `doc_group::is_isolated` — deliberately conservative (it ignores `baseless`), so the note may
// fire on an exact merge but is never missing from an inexact one. It is asked BEFORE the edit by
// both `merge_down` branches in `AgentServer+Groups.swift`, with the same sentence in that tool's
// catalog text; `merge_visible` carries NEITHER, because it bakes the document's own projection.
// **Phase 12E-3c** adds **R4** — a root knockout that reaches the document's root where
// `RasterDocument.hasQualifyingBackground()` (a named host MIRROR of the core's
// `style_knockout::background_entry` / `precision_style_knockout::background_record`) answers false
// and a visible entry sits below the lowest root's subtree — and extends the note constant and the
// `merge_down` catalog sentence with its clause. ONE case is now deliberately SILENT and recorded
// in `designs/phase-12e-layer-ops-status.md`: a present backdrop that is not opaque under the
// shape. `export_psd`'s catalog text names the `knko`/`infx` records the export writes.

/// The typed Swift view of a layer style — the effect stack plus the
/// blending options beyond opacity and mode — mirroring the core's JSON
/// contract key for key (core/src/style.rs is the schema; this file only
/// reads the core's canonical JSON back into values the dialog can bind to
/// and writes them out again). The core canonicalizes, clamps, quantizes and
/// enforces the identity rule; nothing here re-derives any of that.
///
/// The value types are `Codable` with the snake-case key strategies, so
/// each Swift property name IS its JSON key (`useGlobalLight` ↔
/// `use_global_light`). The effect structs decode with Swift's synthesized
/// initializers, which need every key present — always true of the core's
/// canonical output, the only JSON `decode` ever sees.

/// The document's shared light direction, read by every effect with
/// `use_global_light` on. Degrees, the PSD convention: 0 = light from the
/// right, 90 = from the top; altitude 0…90.
struct GlobalLight: Equatable {
    var angle: Double = 120
    var altitude: Double = 30
}

/// The nine effects. The raw value is the JSON `type`; the declaration
/// order is the Layer Style sheet's checklist order (top to bottom) and the
/// Layer > Layer Style submenu's — NOT the render order, which the core owns.
enum LayerStyleEffectKind: String, CaseIterable {
    case bevelEmboss = "bevel_emboss"
    case stroke
    case innerShadow = "inner_shadow"
    case innerGlow = "inner_glow"
    case satin
    case colorOverlay = "color_overlay"
    case gradientOverlay = "gradient_overlay"
    case patternOverlay = "pattern_overlay"
    case outerGlow = "outer_glow"
    case dropShadow = "drop_shadow"

    var title: String {
        switch self {
        case .bevelEmboss: return "Bevel & Emboss"
        case .stroke: return "Stroke"
        case .innerShadow: return "Inner Shadow"
        case .innerGlow: return "Inner Glow"
        case .satin: return "Satin"
        case .colorOverlay: return "Color Overlay"
        case .gradientOverlay: return "Gradient Overlay"
        case .patternOverlay: return "Pattern Overlay"
        case .outerGlow: return "Outer Glow"
        case .dropShadow: return "Drop Shadow"
        }
    }

    /// Whether a style may hold several instances of this kind: the
    /// five kinds the dialog gives a "+" button, the core's
    /// `EffectKind::repeats`.
    var repeats: Bool {
        switch self {
        case .dropShadow, .innerShadow, .stroke, .colorOverlay, .gradientOverlay: return true
        case .bevelEmboss, .innerGlow, .satin, .outerGlow, .patternOverlay: return false
        }
    }
}

/// The style JSON's blend names ↔ `DrBlendMode`: the 27 layer blend modes
/// as snake_case names, ONE table in `DrBlendMode.allBlendModes` order.
/// `linear_dodge` is `DR_BLEND_ADDITION` ("Linear Dodge (Add)"); every
/// other name is the display name lowercased with underscores.
enum LayerStyleBlend {
    static let names: [(DrBlendMode, String)] = [
        (DR_BLEND_NORMAL, "normal"),
        (DR_BLEND_DISSOLVE, "dissolve"),
        (DR_BLEND_DARKEN, "darken"),
        (DR_BLEND_MULTIPLY, "multiply"),
        (DR_BLEND_COLOR_BURN, "color_burn"),
        (DR_BLEND_LINEAR_BURN, "linear_burn"),
        (DR_BLEND_DARKER_COLOR, "darker_color"),
        (DR_BLEND_LIGHTEN, "lighten"),
        (DR_BLEND_SCREEN, "screen"),
        (DR_BLEND_COLOR_DODGE, "color_dodge"),
        (DR_BLEND_ADDITION, "linear_dodge"),
        (DR_BLEND_LIGHTER_COLOR, "lighter_color"),
        (DR_BLEND_OVERLAY, "overlay"),
        (DR_BLEND_SOFT_LIGHT, "soft_light"),
        (DR_BLEND_HARD_LIGHT, "hard_light"),
        (DR_BLEND_VIVID_LIGHT, "vivid_light"),
        (DR_BLEND_LINEAR_LIGHT, "linear_light"),
        (DR_BLEND_PIN_LIGHT, "pin_light"),
        (DR_BLEND_HARD_MIX, "hard_mix"),
        (DR_BLEND_DIFFERENCE, "difference"),
        (DR_BLEND_EXCLUSION, "exclusion"),
        (DR_BLEND_SUBTRACT, "subtract"),
        (DR_BLEND_DIVIDE, "divide"),
        (DR_BLEND_HUE, "hue"),
        (DR_BLEND_SATURATION, "saturation"),
        (DR_BLEND_COLOR, "color"),
        (DR_BLEND_LUMINOSITY, "luminosity"),
    ]

    /// The JSON names alone, in menu order — the catalog's enum.
    static let allNames: [String] = names.map { $0.1 }

    static func name(for mode: DrBlendMode) -> String {
        names.first { $0.0 == mode }?.1 ?? "normal"
    }

    static func mode(named name: String) -> DrBlendMode? {
        names.first { $0.1 == name }?.0
    }
}

/// Blending Options > Knockout: whether the layer's own shape punches a hole
/// through a backdrop instead of compositing onto it, and how far down that
/// backdrop is taken. The core owns every rule (core/src/style_knockout.rs);
/// the raw value IS the JSON string.
///
/// No UI control sets this yet — Phase 12E-3 ships the compositor and the
/// format, and 12E-3b the Blending Options pane — but the value must survive
/// a round trip through this type all the same, because the Layer Style
/// sheet's Apply writes back the WHOLE style and would otherwise silently
/// drop a knockout an agent had set.
enum Knockout: String, Codable {
    case none
    case shallow
    case deep
}

/// Colours cross the style JSON as "#rrggbb" — six digits, lowercase, no
/// alpha (every effect has its own opacity). NOT `TextLayer.hex`, which
/// writes eight.
enum LayerStyleColor {
    static func hex(_ color: NSColor) -> String {
        let c = ColorProfile.converted(color, into: .sRGB) ?? .black
        let byte: (CGFloat) -> Int = { Int((min(max($0, 0), 1) * 255).rounded()) }
        return String(
            format: "#%02x%02x%02x",
            byte(c.redComponent), byte(c.greenComponent), byte(c.blueComponent))
    }

    /// Parses "#rrggbb" (and, leniently, "#rrggbbaa" — the core accepts it
    /// and ignores the alpha); nil on anything else.
    static func color(fromHex string: String) -> NSColor? {
        TextLayer.color(fromHex: string)
    }

    /// How far, in byte codes, a colour's sRGB numbers may sit from the byte
    /// grid and still be that byte — the core's `HEX_TOLERANCE`.
    private static let hexTolerance = 5e-3

    /// A picked colour as a slot's value: its hex and, when it is not
    /// an sRGB byte (off the grid, or outside sRGB's gamut — a P3 pick, a
    /// 16-bit pick), its linear-light sRGB components from
    /// `CGColorSpace.extendedLinearSRGB`.
    static func slot(_ color: NSColor) -> StyleSlotColor {
        let byteHex = hex(color)
        guard let extended = space(CGColorSpace.extendedSRGB),
            let linearSpace = space(CGColorSpace.extendedLinearSRGB),
            let numbers = color.usingColorSpace(extended),
            let linear = color.usingColorSpace(linearSpace)
        else { return StyleSlotColor(hex: byteHex, linear: nil) }
        let components = [numbers.redComponent, numbers.greenComponent, numbers.blueComponent]
        let onGrid = components.allSatisfy { c in
            let code = Double(c) * 255
            return c >= 0 && c <= 1 && abs(code - code.rounded()) <= hexTolerance
        }
        if onGrid { return StyleSlotColor(hex: byteHex, linear: nil) }
        let values = [linear.redComponent, linear.greenComponent, linear.blueComponent]
        return StyleSlotColor(hex: byteHex, linear: values.map { Double($0) })
    }

    /// A slot's colour for its swatch: the wide colour in
    /// `extendedLinearSRGB` when it has one, else the hex.
    static func color(hex: String, linear: [Double]?) -> NSColor {
        if let linear, linear.count == 3, let space = space(CGColorSpace.extendedLinearSRGB) {
            let components = linear.map { CGFloat($0) } + [1]
            return NSColor(colorSpace: space, components: components, count: 4)
        }
        return color(fromHex: hex) ?? .black
    }

    private static func space(_ name: CFString) -> NSColorSpace? {
        CGColorSpace(name: name).flatMap { NSColorSpace(cgColorSpace: $0) }
    }
}

/// One solid colour slot as the sheet edits it: the "#rrggbb" the
/// style JSON's `color` carries and, beyond the byte grid, the wide colour
/// its `linear` carries (nil for an sRGB byte). The core replaces `hex` with
/// the wide colour's nearest byte, so the two never disagree.
struct StyleSlotColor: Equatable {
    var hex: String
    var linear: [Double]?
}

// MARK: - Shared sub-objects

struct GradientStop: Codable, Equatable {
    var position: Double
    var color: String
    var opacity: Double
    /// A CMYK document's straight ink [c, m, y, k] 0…1 (#547): authoritative
    /// there, `color` its sRGB; absent elsewhere.
    var ink: [Double]? = nil
    /// The colour midpoint toward the next stop as a fraction
    /// 0.05…0.95: where the colour is half way. nil means 0.5 and writes no
    /// key, as the core writes none at 0.5; the UI and MCP store nil at 0.5.
    var midpoint: Double? = nil
    /// The opacity midpoint toward the next stop, as `midpoint`.
    var opacityMidpoint: Double? = nil

    /// The midpoint range, 5…95 % (the core's
    /// `gradient_field::MIDPOINT_MIN…MIDPOINT_MAX`).
    static let midpointRange = 0.05...0.95

    /// This stop with both midpoints cleared: what a midpoint-only edit
    /// leaves unchanged (`FillLayerPayload.gradient`'s didSet).
    var withoutMidpoints: GradientStop {
        var stop = self
        stop.midpoint = nil
        stop.opacityMidpoint = nil
        return stop
    }
}

/// Shared by `stroke` (gradient fill) and `gradient_overlay`.
struct GradientFill: Codable, Equatable {
    var stops: [GradientStop] = [
        GradientStop(position: 0, color: "#000000", opacity: 1),
        GradientStop(position: 1, color: "#ffffff", opacity: 1),
    ]
    var style = "linear"
    var angle = 90.0
    var scale = 1.0
    var reverse = false
    var alignWithLayer = true
    /// Dither, on a stroke's and a gradient overlay's
    /// gradient. Optional so a value without it — every fill layer's — writes
    /// no key; the core writes it only when true.
    var dither: Bool? = nil
    /// Smoothness as a fraction 0…1: the linear and Classic ramps
    /// blended by it (1, 100 %, the Classic ramp).
    /// nil means 1 and writes no key, as the core writes none at 1; the UI and
    /// MCP store nil at 1.
    var smoothness: Double? = nil

    static let styles = ["linear", "radial", "angle", "reflected", "diamond"]
    /// The Stroke's styles: the five plus Shape Burst, which only the
    /// Stroke offers. Fill layers and the gradient map keep `styles`.
    static let strokeStyles = styles + ["shape_burst"]
}

/// One channel's Blend If ranges: the two split-slider ramps
/// on `channel`, an entry of `BlendIf.extra`.
struct BlendIfRange: Codable, Equatable {
    var channel: String
    var thisLayer: [Int]
    var underlying: [Int]

    /// Both ramps full: the range weights every value 1.
    var isIdentity: Bool {
        thisLayer == BlendIf.fullRamp && underlying == BlendIf.fullRamp
    }
}

/// Blending Options > Blend If: two split-slider ramps, `[lo0, lo1, hi0,
/// hi1]` over 0…255 with `lo0 <= lo1 <= hi0 <= hi1`, on this layer's and
/// the underlying composite's channel — the PRIMARY range — plus `extra`,
/// one more range per other channel, whose weights the core
/// multiplies. The core's canonical form (`BlendIf::from_ranges`): identity
/// ranges dropped, the rest in `channelOrder`, the first the primary.
struct BlendIf: Codable, Equatable {
    static let fullRamp = [0, 0, 255, 255]
    static let channels = ["gray", "red", "green", "blue"]
    static let titles = ["Gray", "Red", "Green", "Blue"]
    /// A CMYK document's channels (#547): Gray and the four plates.
    static let cmykChannels = ["gray", "cyan", "magenta", "yellow", "black"]
    static let cmykTitles = ["Gray", "Cyan", "Magenta", "Yellow", "Black"]
    /// A Lab Color document's channels (#547 Lab phase 2f): Lightness, a and b, stored as red,
    /// green and blue (`lab_style::BLEND_IF_CHANNELS`); Gray refuses.
    static let labChannels = ["red", "green", "blue"]
    static let labTitles = ["Lightness", "a", "b"]
    /// The core's `BlendIfChannel` order, which sorts a canonical value's ranges.
    static let channelOrder = ["gray", "red", "green", "blue", "cyan", "magenta", "yellow", "black"]

    var channel = "gray"
    var thisLayer = BlendIf.fullRamp
    var underlying = BlendIf.fullRamp
    /// The ranges beside the primary; nil (no key) for a single channel.
    var extra: [BlendIfRange]? = nil

    /// Full weight everywhere — every range identity, the core's
    /// `BlendIf::is_identity` (a full ramp weights every channel the same).
    var isIdentity: Bool {
        ranges.allSatisfy(\.isIdentity)
    }

    /// The primary range, then the extras.
    var ranges: [BlendIfRange] {
        [BlendIfRange(channel: channel, thisLayer: thisLayer, underlying: underlying)]
            + (extra ?? [])
    }

    /// `channel`'s range, full ramps when the value holds none for it.
    func range(for channel: String) -> BlendIfRange {
        ranges.first { $0.channel == channel }
            ?? BlendIfRange(channel: channel, thisLayer: BlendIf.fullRamp, underlying: BlendIf.fullRamp)
    }

    /// This value with `range` replacing (or joining) its channel's range, in
    /// the core's canonical form: identity ranges dropped, the rest sorted by
    /// `channelOrder`, the first the primary; when none remains, the primary
    /// as written alone. `extra` is nil when empty.
    func setting(_ range: BlendIfRange) -> BlendIf {
        var all = ranges.filter { $0.channel != range.channel }
        if ranges.contains(where: { $0.channel == range.channel }) {
            let at = ranges.firstIndex { $0.channel == range.channel } ?? all.count
            all.insert(range, at: min(at, all.count))
        } else {
            all.append(range)
        }
        let order = { (r: BlendIfRange) in BlendIf.channelOrder.firstIndex(of: r.channel) ?? .max }
        let kept = all.filter { !$0.isIdentity }.sorted { order($0) < order($1) }
        guard let first = kept.first ?? all.first else { return BlendIf() }
        let rest = Array(kept.dropFirst())
        return BlendIf(
            channel: first.channel, thisLayer: first.thisLayer, underlying: first.underlying,
            extra: rest.isEmpty ? nil : rest)
    }
}

// MARK: - Effects (defaults are the core's; see core/src/style.rs)

struct DropShadowEffect: Codable, Equatable {
    var type = LayerStyleEffectKind.dropShadow.rawValue
    var enabled = true
    var blend = "multiply"
    var color = "#000000"
    /// The slot's CMYK ink (#547, `GradientStop.ink`).
    var ink: [Double]? = nil
    /// The slot's wide colour (`StyleSlotColor`): linear-light sRGB
    /// components, extended range; nil writes no key.
    var linear: [Double]? = nil
    var opacity = 0.75
    var angle = 120.0
    var useGlobalLight = true
    var distance = 5.0
    var spread = 0.0
    var size = 5.0
    /// "Layer Knocks Out Drop Shadow": the shadow is not drawn
    /// under the layer's own shape (visible only at fill opacity below 1).
    var layerKnocksOut = true
    /// The Contour picker (`LayerStyleContour.swift`): nil is Linear
    /// and writes no key, like `antiAliased` nil (off).
    var contour: ContourValue? = nil
    var antiAliased: Bool? = nil
    /// Noise (0...1 = 0-100 %): nil is 0 and
    /// writes no key.
    var noise: Double? = nil
}

struct InnerShadowEffect: Codable, Equatable {
    var type = LayerStyleEffectKind.innerShadow.rawValue
    var enabled = true
    var blend = "multiply"
    var color = "#000000"
    /// The slot's CMYK ink (#547, `GradientStop.ink`).
    var ink: [Double]? = nil
    /// The slot's wide colour (`StyleSlotColor`): linear-light sRGB
    /// components, extended range; nil writes no key.
    var linear: [Double]? = nil
    var opacity = 0.75
    var angle = 120.0
    var useGlobalLight = true
    var distance = 5.0
    var choke = 0.0
    var size = 5.0
    /// The Contour picker (`LayerStyleContour.swift`): nil is Linear
    /// and writes no key, like `antiAliased` nil (off).
    var contour: ContourValue? = nil
    var antiAliased: Bool? = nil
    /// Noise (0...1 = 0-100 %): nil is 0 and
    /// writes no key.
    var noise: Double? = nil
}

struct OuterGlowEffect: Codable, Equatable {
    var type = LayerStyleEffectKind.outerGlow.rawValue
    var enabled = true
    var blend = "screen"
    var color = "#ffffbe"
    /// The slot's CMYK ink (#547, `GradientStop.ink`).
    var ink: [Double]? = nil
    /// The slot's wide colour (`StyleSlotColor`): linear-light sRGB
    /// components, extended range; nil writes no key.
    var linear: [Double]? = nil
    var opacity = 0.75
    var spread = 0.0
    var size = 5.0
    /// The Contour picker (`LayerStyleContour.swift`): nil is Linear
    /// and writes no key, like `antiAliased` nil (off), and `range`
    /// nil (0.5, 50 %).
    var contour: ContourValue? = nil
    var antiAliased: Bool? = nil
    var range: Double? = nil
    /// Noise (0...1 = 0-100 %): nil is 0 and
    /// writes no key.
    var noise: Double? = nil
    /// The glow's Fill (`GlowFillRows.swift`): nil is "color" and
    /// writes no key; `gradient` holds only its stops (the core resets every
    /// other key), nil writes none; `jitter` 0...1 (0-100 %),
    /// nil is 0 and writes no key.
    var fillType: String? = nil
    var gradient: GradientFill? = nil
    var jitter: Double? = nil

    static let fillTypes = ["color", "gradient"]
}

struct InnerGlowEffect: Codable, Equatable {
    static let sources = ["edge", "center"]
    static let fillTypes = OuterGlowEffect.fillTypes

    var type = LayerStyleEffectKind.innerGlow.rawValue
    var enabled = true
    var blend = "screen"
    var color = "#ffffbe"
    /// The slot's CMYK ink (#547, `GradientStop.ink`).
    var ink: [Double]? = nil
    /// The slot's wide colour (`StyleSlotColor`): linear-light sRGB
    /// components, extended range; nil writes no key.
    var linear: [Double]? = nil
    var opacity = 0.75
    var choke = 0.0
    var size = 5.0
    var source = "edge"
    /// The Contour picker (`LayerStyleContour.swift`): nil is Linear
    /// and writes no key, like `antiAliased` nil (off), and `range`
    /// nil (0.5, 50 %).
    var contour: ContourValue? = nil
    var antiAliased: Bool? = nil
    var range: Double? = nil
    /// Noise (0...1 = 0-100 %): nil is 0 and
    /// writes no key.
    var noise: Double? = nil
    /// See `OuterGlowEffect.fillType`.
    var fillType: String? = nil
    var gradient: GradientFill? = nil
    var jitter: Double? = nil
}

struct StrokeEffect: Codable, Equatable {
    static let positions = ["outside", "inside", "center"]
    static let fillTypes = ["color", "gradient", "pattern"]

    var type = LayerStyleEffectKind.stroke.rawValue
    var enabled = true
    var blend = "normal"
    var opacity = 1.0
    var size = 3.0
    var position = "outside"
    var fillType = "color"
    var color = "#000000"
    /// The slot's CMYK ink (#547, `GradientStop.ink`).
    var ink: [Double]? = nil
    /// The slot's wide colour (`StyleSlotColor`): linear-light sRGB
    /// components, extended range; nil writes no key.
    var linear: [Double]? = nil
    var gradient = GradientFill()
    /// Fill Type Pattern's fill (`PatternFill`): nil writes no `pattern_fill`, so
    /// a Color or Gradient stroke's JSON is unchanged.
    var patternFill: PatternFill? = nil
}

struct ColorOverlayEffect: Codable, Equatable {
    var type = LayerStyleEffectKind.colorOverlay.rawValue
    var enabled = true
    var blend = "normal"
    var color = "#ff0000"
    /// The slot's CMYK ink (#547, `GradientStop.ink`).
    var ink: [Double]? = nil
    /// The slot's wide colour (`StyleSlotColor`): linear-light sRGB
    /// components, extended range; nil writes no key.
    var linear: [Double]? = nil
    var opacity = 1.0
}

struct GradientOverlayEffect: Codable, Equatable {
    var type = LayerStyleEffectKind.gradientOverlay.rawValue
    var enabled = true
    var blend = "normal"
    var opacity = 1.0
    var gradient = GradientFill()
}

struct BevelEmbossEffect: Codable, Equatable {
    static let styles = ["outer_bevel", "inner_bevel", "emboss", "pillow_emboss"]
    static let directions = ["up", "down"]
    static let techniques = ["smooth", "chisel_hard", "chisel_soft"]

    var type = LayerStyleEffectKind.bevelEmboss.rawValue
    var enabled = true
    var style = "inner_bevel"
    /// The Technique (#578): nil is Smooth and writes no key.
    var technique: String? = nil
    /// 1.0 is 100 %; the core clamps to 0.01…10.
    var depth = 1.0
    var direction = "up"
    var size = 5.0
    var soften = 0.0
    var angle = 120.0
    var useGlobalLight = true
    var altitude = 30.0
    var highlightBlend = "screen"
    var highlightColor = "#ffffff"
    /// The highlight side's CMYK ink (#547, `GradientStop.ink`).
    var highlightInk: [Double]? = nil
    /// The highlight side's wide colour (`StyleSlotColor`).
    var highlightLinear: [Double]? = nil
    var highlightOpacity = 0.75
    var shadowBlend = "multiply"
    var shadowColor = "#000000"
    /// The shadow side's CMYK ink (#547, `GradientStop.ink`).
    var shadowInk: [Double]? = nil
    /// The shadow side's wide colour (`StyleSlotColor`).
    var shadowLinear: [Double]? = nil
    var shadowOpacity = 0.75
    /// The Gloss Contour (`LayerStyleContour.swift`) and the Contour
    /// sub-panel: nil is Linear / off / 0.5 (Range 50 %) and
    /// writes no key.
    var glossContour: ContourValue? = nil
    var glossAntiAliased: Bool? = nil
    var contourEnabled: Bool? = nil
    var contour: ContourValue? = nil
    var contourAntiAliased: Bool? = nil
    var contourRange: Double? = nil
    /// The Texture sub-panel (`PatternOverlay.swift`): its
    /// checkbox (nil is off and writes no key) and the texture, nil until
    /// edited (the core keeps a switched-off texture for the sheet).
    var textureEnabled: Bool? = nil
    var texture: BevelTexture? = nil
}

struct SatinEffect: Codable, Equatable {
    var type = LayerStyleEffectKind.satin.rawValue
    var enabled = true
    var blend = "multiply"
    var color = "#000000"
    /// The slot's CMYK ink (#547, `GradientStop.ink`).
    var ink: [Double]? = nil
    /// The slot's wide colour (`StyleSlotColor`): linear-light sRGB
    /// components, extended range; nil writes no key.
    var linear: [Double]? = nil
    var opacity = 0.5
    var angle = 19.0
    var distance = 11.0
    var size = 14.0
    var invert = true
    /// The Contour picker (`LayerStyleContour.swift`): nil is Linear
    /// and writes no key, like `antiAliased` nil (off).
    var contour: ContourValue? = nil
    var antiAliased: Bool? = nil
}

// MARK: - The style

/// One layer's style. `Codable` by hand at the top level because the
/// effects live in the JSON as an unkeyed `"effects"` array of typed
/// objects — read by peeking each element's `type`, written in checklist
/// order (the core re-sorts into render order); the structs themselves
/// use the synthesized coders.
///
/// The five kinds that `repeats` hold up to `maxInstances` instances
/// each, in RENDER order — index 0 draws lowest, a later instance on top
/// (the core's rule; the JSON writes them in this order and the core's sort
/// is stable). The other five hold at most one.
struct LayerStyle: Codable, Equatable {
    /// The most instances one repeating kind may hold (the core's
    /// `MAX_INSTANCES`).
    static let maxInstances = 10

    var version = 1
    /// Opacity of the PIXELS only, never of the effects.
    var fillOpacity = 1.0
    var blendIf: BlendIf? = nil
    /// Blending Options > Knockout (`Knockout`); no control sets it yet.
    var knockout: Knockout = .none
    /// Blending Options > "Blend Interior Effects as Group": the layer's own
    /// blend mode applies to Inner Glow, Satin and the three Overlays. Renders
    /// nothing on its own, which is why `isIdentity` ignores it — the core's
    /// rule, mirrored exactly.
    var blendInteriorAsGroup = false
    var dropShadows: [DropShadowEffect] = []
    var innerShadows: [InnerShadowEffect] = []
    var outerGlow: OuterGlowEffect? = nil
    var innerGlow: InnerGlowEffect? = nil
    var strokes: [StrokeEffect] = []
    var colorOverlays: [ColorOverlayEffect] = []
    var gradientOverlays: [GradientOverlayEffect] = []
    /// At most one (`repeats` is false); an array so the sheet's instance binding serves it.
    var patternOverlays: [PatternOverlayEffect] = []
    var bevelEmboss: BevelEmbossEffect? = nil
    var satin: SatinEffect? = nil

    init() {}

    /// This style with every colour slot — each effect's colour and ink, the
    /// bevel's two sides, every gradient stop — rewritten by `f` (#547: a
    /// CMYK document's sheet maps slots to inks through `CmykShownInks`).
    func mappingColourSlots(_ f: (inout String, inout [Double]?) -> Void) -> LayerStyle {
        var out = self
        func stops(_ fill: inout GradientFill) {
            for i in fill.stops.indices {
                var stop = fill.stops[i]
                f(&stop.color, &stop.ink)
                fill.stops[i] = stop
            }
        }
        // Every instance of a repeating kind.
        for i in out.dropShadows.indices {
            var e = out.dropShadows[i]
            f(&e.color, &e.ink)
            out.dropShadows[i] = e
        }
        for i in out.innerShadows.indices {
            var e = out.innerShadows[i]
            f(&e.color, &e.ink)
            out.innerShadows[i] = e
        }
        if var e = out.outerGlow { f(&e.color, &e.ink); out.outerGlow = e }
        if var e = out.innerGlow { f(&e.color, &e.ink); out.innerGlow = e }
        for i in out.strokes.indices {
            var e = out.strokes[i]
            f(&e.color, &e.ink)
            stops(&e.gradient)
            out.strokes[i] = e
        }
        for i in out.colorOverlays.indices {
            var e = out.colorOverlays[i]
            f(&e.color, &e.ink)
            out.colorOverlays[i] = e
        }
        for i in out.gradientOverlays.indices { stops(&out.gradientOverlays[i].gradient) }
        if var e = out.bevelEmboss {
            f(&e.highlightColor, &e.highlightInk)
            f(&e.shadowColor, &e.shadowInk)
            out.bevelEmboss = e
        }
        if var e = out.satin { f(&e.color, &e.ink); out.satin = e }
        return out
    }

    // MARK: Effects by kind

    /// Present — enabled or not (a disabled effect keeps its settings).
    func hasEffect(_ kind: LayerStyleEffectKind) -> Bool {
        instanceCount(kind) > 0
    }

    /// Whether ANY instance of `kind` is enabled.
    func effectEnabled(_ kind: LayerStyleEffectKind) -> Bool {
        enabledFlags(kind).contains(true)
    }

    /// How many instances of `kind` the style holds, enabled or not.
    func instanceCount(_ kind: LayerStyleEffectKind) -> Int {
        enabledFlags(kind).count
    }

    /// Whether instance `instance` (render order) of `kind` is enabled;
    /// false for one that does not exist.
    func effectEnabled(_ kind: LayerStyleEffectKind, instance: Int) -> Bool {
        let flags = enabledFlags(kind)
        return flags.indices.contains(instance) && flags[instance]
    }

    /// Turns EVERY instance of an effect on or off; enabling one that was
    /// never set creates it with the core's defaults, and disabling keeps it
    /// (its settings survive a round trip through the checklist).
    mutating func setEffectEnabled(_ kind: LayerStyleEffectKind, _ on: Bool) {
        let count = max(instanceCount(kind), 1)
        for instance in 0..<count {
            setEffectEnabled(kind, instance: instance, on)
        }
    }

    /// Turns one instance on or off; instance 0 of a kind with none is
    /// created with the core's defaults. Any other missing instance is
    /// ignored.
    mutating func setEffectEnabled(_ kind: LayerStyleEffectKind, instance: Int, _ on: Bool) {
        func set<E>(_ list: inout [E], _ make: () -> E, _ flag: WritableKeyPath<E, Bool>) {
            if list.isEmpty, instance == 0 { list.append(make()) }
            guard list.indices.contains(instance) else { return }
            list[instance][keyPath: flag] = on
        }
        func single<E>(_ value: inout E?, _ make: () -> E, _ flag: WritableKeyPath<E, Bool>) {
            guard instance == 0 else { return }
            var fx = value ?? make()
            fx[keyPath: flag] = on
            value = fx
        }
        switch kind {
        case .dropShadow: set(&dropShadows, { DropShadowEffect() }, \.enabled)
        case .innerShadow: set(&innerShadows, { InnerShadowEffect() }, \.enabled)
        case .outerGlow: single(&outerGlow, { OuterGlowEffect() }, \.enabled)
        case .innerGlow: single(&innerGlow, { InnerGlowEffect() }, \.enabled)
        case .stroke: set(&strokes, { StrokeEffect() }, \.enabled)
        case .colorOverlay: set(&colorOverlays, { ColorOverlayEffect() }, \.enabled)
        case .gradientOverlay: set(&gradientOverlays, { GradientOverlayEffect() }, \.enabled)
        case .patternOverlay: set(&patternOverlays, { PatternOverlayEffect() }, \.enabled)
        case .bevelEmboss: single(&bevelEmboss, { BevelEmbossEffect() }, \.enabled)
        case .satin: single(&satin, { SatinEffect() }, \.enabled)
        }
    }

    /// The dialog's "+": a copy of instance `instance` inserted right above it
    /// (at `instance + 1`, so it draws on top); returns the copy's index. nil
    /// for a kind that does not repeat, a missing instance, or a kind at
    /// `maxInstances`.
    mutating func duplicateInstance(_ kind: LayerStyleEffectKind, _ instance: Int) -> Int? {
        guard kind.repeats, instanceCount(kind) < Self.maxInstances else { return nil }
        func copy<E>(_ list: inout [E]) -> Int? {
            guard list.indices.contains(instance) else { return nil }
            let copy = list[instance]
            list.insert(copy, at: instance + 1)
            return instance + 1
        }
        switch kind {
        case .dropShadow: return copy(&dropShadows)
        case .innerShadow: return copy(&innerShadows)
        case .stroke: return copy(&strokes)
        case .colorOverlay: return copy(&colorOverlays)
        case .gradientOverlay: return copy(&gradientOverlays)
        case .outerGlow, .innerGlow, .bevelEmboss, .satin, .patternOverlay: return nil
        }
    }

    /// Removes one instance (a single kind's only one clears it); a missing
    /// instance is ignored.
    mutating func removeInstance(_ kind: LayerStyleEffectKind, _ instance: Int) {
        func drop<E>(_ list: inout [E]) {
            if list.indices.contains(instance) { list.remove(at: instance) }
        }
        switch kind {
        case .dropShadow: drop(&dropShadows)
        case .innerShadow: drop(&innerShadows)
        case .stroke: drop(&strokes)
        case .colorOverlay: drop(&colorOverlays)
        case .gradientOverlay: drop(&gradientOverlays)
        case .patternOverlay: drop(&patternOverlays)
        case .outerGlow: if instance == 0 { outerGlow = nil }
        case .innerGlow: if instance == 0 { innerGlow = nil }
        case .bevelEmboss: if instance == 0 { bevelEmboss = nil }
        case .satin: if instance == 0 { satin = nil }
        }
    }

    /// Moves one instance within its kind to `to` (render order: a higher
    /// index draws above). Out-of-range indices are ignored.
    mutating func moveInstance(_ kind: LayerStyleEffectKind, from: Int, to: Int) {
        func move<E>(_ list: inout [E]) {
            guard list.indices.contains(from), list.indices.contains(to), from != to else { return }
            let item = list.remove(at: from)
            list.insert(item, at: to)
        }
        switch kind {
        case .dropShadow: move(&dropShadows)
        case .innerShadow: move(&innerShadows)
        case .stroke: move(&strokes)
        case .colorOverlay: move(&colorOverlays)
        case .gradientOverlay: move(&gradientOverlays)
        case .outerGlow, .innerGlow, .bevelEmboss, .satin, .patternOverlay: return
        }
    }

    /// The enabled effects in checklist order.
    var enabledKinds: [LayerStyleEffectKind] {
        LayerStyleEffectKind.allCases.filter { effectEnabled($0) }
    }

    /// Renders nothing: fill opacity 1, no (or a full-weight) Blend If,
    /// knockout `none`, no enabled effect — the core's rule, mirrored ONLY
    /// for the sheet's Apply/Reset comparisons and the paste guard; the core
    /// enforces it (an identity style is never stored: setting one clears).
    /// `blendInteriorAsGroup` is deliberately absent, as it is in the core:
    /// without a grouped effect it changes no pixel.
    var isIdentity: Bool {
        fillOpacity >= 1 && (blendIf?.isIdentity ?? true) && knockout == .none
            && enabledKinds.isEmpty
    }

    /// Each instance's enabled flag, render order; empty when absent.
    private func enabledFlags(_ kind: LayerStyleEffectKind) -> [Bool] {
        switch kind {
        case .dropShadow: return dropShadows.map(\.enabled)
        case .innerShadow: return innerShadows.map(\.enabled)
        case .outerGlow: return outerGlow.map { [$0.enabled] } ?? []
        case .innerGlow: return innerGlow.map { [$0.enabled] } ?? []
        case .stroke: return strokes.map(\.enabled)
        case .colorOverlay: return colorOverlays.map(\.enabled)
        case .gradientOverlay: return gradientOverlays.map(\.enabled)
        case .patternOverlay: return patternOverlays.map(\.enabled)
        case .bevelEmboss: return bevelEmboss.map { [$0.enabled] } ?? []
        case .satin: return satin.map { [$0.enabled] } ?? []
        }
    }

    // MARK: JSON

    private enum CodingKeys: String, CodingKey {
        case version, fillOpacity, blendIf, knockout, blendInteriorAsGroup, effects
    }

    /// Reads one element's `type` without consuming it from the real
    /// container (decoding from a COPY of an unkeyed container advances only
    /// the copy).
    private struct TypeProbe: Decodable {
        let type: String
    }

    /// Consumes one element of any object shape — how an unknown effect
    /// type (a newer core's) is skipped rather than refused.
    private struct SkippedEffect: Decodable {}

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        version = try c.decodeIfPresent(Int.self, forKey: .version) ?? 1
        fillOpacity = try c.decodeIfPresent(Double.self, forKey: .fillOpacity) ?? 1
        blendIf = try c.decodeIfPresent(BlendIf.self, forKey: .blendIf)
        // Both take their default when absent, which is how the core writes
        // them: each key is emitted ONLY when it is not at its default.
        knockout = try c.decodeIfPresent(Knockout.self, forKey: .knockout) ?? .none
        blendInteriorAsGroup =
            try c.decodeIfPresent(Bool.self, forKey: .blendInteriorAsGroup) ?? false
        guard c.contains(.effects) else { return }
        var effects = try c.nestedUnkeyedContainer(forKey: .effects)
        while !effects.isAtEnd {
            let probe = effects
            let type = try LayerStyle.peekType(probe)
            switch LayerStyleEffectKind(rawValue: type) {
            // A repeating kind's instances arrive in render order.
            case .dropShadow: dropShadows.append(try effects.decode(DropShadowEffect.self))
            case .innerShadow: innerShadows.append(try effects.decode(InnerShadowEffect.self))
            case .outerGlow: outerGlow = try effects.decode(OuterGlowEffect.self)
            case .innerGlow: innerGlow = try effects.decode(InnerGlowEffect.self)
            case .stroke: strokes.append(try effects.decode(StrokeEffect.self))
            case .colorOverlay: colorOverlays.append(try effects.decode(ColorOverlayEffect.self))
            case .gradientOverlay:
                gradientOverlays.append(try effects.decode(GradientOverlayEffect.self))
            case .patternOverlay:
                patternOverlays.append(try effects.decode(PatternOverlayEffect.self))
            case .bevelEmboss: bevelEmboss = try effects.decode(BevelEmbossEffect.self)
            case .satin: satin = try effects.decode(SatinEffect.self)
            case nil: _ = try effects.decode(SkippedEffect.self)
            }
        }
    }

    private static func peekType(_ container: UnkeyedDecodingContainer) throws -> String {
        var copy = container
        return try copy.decode(TypeProbe.self).type
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(version, forKey: .version)
        try c.encode(fillOpacity, forKey: .fillOpacity)
        // An explicit null, the canonical spelling of "no Blend If".
        try c.encode(blendIf, forKey: .blendIf)
        // The two knockout keys are written ONLY when they are not at their
        // defaults — the core's own rule, mirrored so a style with neither
        // encodes to exactly the JSON it did before they existed and the
        // sheet's byte-stable "unchanged" compare keeps working.
        if knockout != .none { try c.encode(knockout, forKey: .knockout) }
        if blendInteriorAsGroup { try c.encode(true, forKey: .blendInteriorAsGroup) }
        var effects = c.nestedUnkeyedContainer(forKey: .effects)
        for kind in LayerStyleEffectKind.allCases {
            switch kind {
            case .dropShadow: for fx in dropShadows { try effects.encode(fx) }
            case .innerShadow: for fx in innerShadows { try effects.encode(fx) }
            case .outerGlow: if let fx = outerGlow { try effects.encode(fx) }
            case .innerGlow: if let fx = innerGlow { try effects.encode(fx) }
            case .stroke: for fx in strokes { try effects.encode(fx) }
            case .colorOverlay: for fx in colorOverlays { try effects.encode(fx) }
            case .gradientOverlay: for fx in gradientOverlays { try effects.encode(fx) }
            case .patternOverlay: for fx in patternOverlays { try effects.encode(fx) }
            case .bevelEmboss: if let fx = bevelEmboss { try effects.encode(fx) }
            case .satin: if let fx = satin { try effects.encode(fx) }
            }
        }
    }

    /// The style as JSON the core accepts (snake_case keys, sorted for a
    /// byte-stable no-op compare); nil only for a non-finite number, which
    /// no control produces.
    func json() -> String? {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        encoder.keyEncodingStrategy = .convertToSnakeCase
        guard let data = try? encoder.encode(self) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// The core's canonical JSON back into a value; nil on anything
    /// malformed (never throws to callers — the dialog then opens on a
    /// fresh style).
    static func decode(_ json: String) -> LayerStyle? {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let data = json.data(using: .utf8) else { return nil }
        return try? decoder.decode(LayerStyle.self, from: data)
    }
}

// MARK: - The knockout merge RESIDUAL disclosure (Phase 12E-3b, decision xi)

extension LayerStyle {
    /// What is LEFT of D7's disclosure once Merge Down and Merge Layers
    /// evaluate knockout against the REAL backdrop (Phase 12E-3b, contract g).
    ///
    /// A merged raster cannot un-draw what lies between, so exactness holds
    /// wherever `B0` is present and opaque under the shape. Where `B0` is
    /// ABSENT BY STRUCTURE the hole still bakes as transparency — inside an
    /// isolated group, inside a merged pass-through group, clipped to a
    /// base outside the merge, or punching to the ROOT of a document with no
    /// qualifying Background layer (R4, added by 12E-3c with
    /// `hasQualifyingBackground`) — and that is what this says, at the POINT
    /// OF ACTION, the way the PSD export's loss sentences already do.
    ///
    /// The sentence is a single constant because the MCP reply and the tool
    /// catalog must not be able to word it two ways.
    static let knockoutMergeResidualNote =
        "A merged layer used knockout where no backdrop could be captured (inside an isolated "
        + "group, inside a merged pass-through group, clipped to a base outside the merge, or "
        + "punching to the root of a document with no qualifying Background layer). "
        + "This merge baked the hole as TRANSPARENCY, so the merged layer now shows whatever lies "
        + "beneath it rather than the knockout's backdrop, and the picture can change. "
        + "flatten_image, stamp_visible and merge_visible bake knockout exactly."
}

extension RasterDocument {
    /// A host MIRROR of the core's `style_knockout::background_entry` /
    /// `precision_style_knockout::background_record` — D5's structural
    /// Background rule, named as such: entry 0 is at depth 0, is a raster and
    /// not an adjustment, visible, Normal, opacity ≥ 1, has no ENABLED mask,
    /// carries no style (at 16 bits that row also covers Blend If), sits at
    /// (0, 0), is exactly canvas-sized, and entry 1 (if any) is unclipped.
    ///
    /// Doctored has no Background layer KIND, so this is the only Background
    /// either depth knows (D13). It is what R4 below asks before it claims a
    /// root knockout had nothing to punch through to.
    func hasQualifyingBackground() -> Bool {
        guard let entry = layerInfo(0) else { return false }
        guard entry.depth == 0, entry.kind == DR_LAYER_RASTER, !layerIsAdjustment(0) else {
            return false
        }
        guard entry.visible, entry.blendMode == DR_BLEND_NORMAL, entry.opacity >= 1.0 else {
            return false
        }
        if layerHasMask(0), layerMaskEnabled(0) { return false }
        if layerHasStyle(0) { return false }
        guard entry.offsetX == 0, entry.offsetY == 0 else { return false }
        guard entry.width == width, entry.height == height else { return false }
        // A clipped entry 1 makes entry 0 a clip BASE, whose composite is not
        // finished when its pixel pass is.
        if layerInfo(1) != nil, layerClipped(1) { return false }
        return true
    }

    /// A host MIRROR of the core's `doc_group::is_isolated`, named as such:
    /// a GROUP whose blend mode is not Pass Through, or which carries a style,
    /// or which is clipped, or which is a clipping base with visible clipped
    /// layers. Deliberately CONSERVATIVE — it ignores the core's `baseless`
    /// term, so a clipped bottom-of-level group reads isolated here where the
    /// compositor treats it as pass-through. The consequence is a note that
    /// may fire on an exact merge, never one that is missing from an inexact
    /// one.
    func isIsolatedGroup(_ idx: Int) -> Bool {
        guard layerIsGroup(idx) else { return false }
        if layerInfo(idx)?.blendMode != DR_BLEND_PASS_THROUGH { return true }
        if layerHasStyle(idx) { return true }
        if layerClipped(idx) { return true }
        return isClipBaseWithVisibleMembers(idx)
    }

    /// [`LayerStyle.knockoutMergeResidualNote`] when some VISIBLE entry in the
    /// merge set's subtrees uses knockout AND no backdrop can be captured for
    /// it, else nil. `roots` are the entries the merge names, lowest first;
    /// each one's whole SUBTREE is scanned, because a group merges its
    /// descendants with it.
    ///
    /// The three shapes (decision xi of 12E-3b):
    ///
    /// * **R1** — the entry is a ROOT of the set, a visible sibling lies below
    ///   the set's lowest root in its level, and the backdrop that entry wants
    ///   is stopped by an isolated group (Deep: any ancestor group of that
    ///   level; Shallow: the level's parent group).
    /// * **R2** — the entry is NOT a root, and every group from its member
    ///   root down to its parent is a PASS-THROUGH group, so the merge
    ///   rasterizes the whole nest with a transparent backdrop.
    /// * **R3** — the lowest root is CLIPPED and an unclipped sibling lies
    ///   below it in its level, so its base is outside the merge and the merge
    ///   runs with no backdrop at all.
    /// * **R4** (12E-3c) — the entry is a ROOT whose knockout REACHES the
    ///   document's root (Deep, or Shallow with no parent group), R1's
    ///   isolation test does not stop it, the document has no qualifying
    ///   Background (`hasQualifyingBackground`), and some visible entry
    ///   outside the set sits below the lowest root's subtree. The knockout
    ///   punches to transparency, so the merged raster shows that entry.
    ///
    /// Callers must ask BEFORE the edit: a merge drops the styles it bakes, so
    /// afterwards there is nothing left to find. The only callers are
    /// `merge_down`'s two branches — Merge Visible renders the document's own
    /// projection and is exact.
    ///
    /// ONE case is deliberately SILENT, and it is recorded in
    /// `designs/phase-12e-layer-ops-status.md`: a backdrop that is present but
    /// not opaque under the shape (pixel-dependent; the catalog sentence names
    /// it). The root-with-no-Background case is R4 since 12E-3c, now that the
    /// host evaluates D5 itself.
    func knockoutMergeResidualNote(merging roots: [Int]) -> String? {
        guard let lowest = roots.min() else { return nil }
        let tree = layerTree

        // R3 — the lowest root is clipped to a base the merge does not hold.
        if layerClipped(lowest),
           tree.siblings(of: lowest).contains(where: { $0 < lowest && !layerClipped($0) }) {
            if setUsesKnockout(roots, tree) { return LayerStyle.knockoutMergeResidualNote }
        }

        // Whether a visible sibling lies BELOW the set's lowest root in its
        // own level — the thing a transparent hole would newly reveal.
        let siblingBeneath = tree.siblings(of: lowest).contains { idx in
            idx < lowest && layerInfo(idx)?.visible == true && !roots.contains(idx)
        }
        // R4's own "something would show through": a visible entry OUTSIDE
        // the set, below the lowest root's whole subtree. A bottom-first flat
        // forest keeps a subtree contiguous, so everything under its floor is
        // outside the set by construction.
        let floor = tree.subtree(of: lowest).min() ?? lowest
        let visibleBelowSet = (0..<floor).contains { layerInfo($0)?.visible == true }

        var seen = Set<Int>()
        for root in roots {
            for idx in tree.subtree(of: root) where seen.insert(idx).inserted {
                guard layerInfo(idx)?.visible == true else { continue }
                guard let style = layerStylePayload(idx), style.knockout != .none else { continue }
                if idx == root {
                    let stopped: Bool
                    switch style.knockout {
                    case .deep: stopped = tree.ancestors(of: idx).contains { isIsolatedGroup($0) }
                    case .shallow: stopped = tree.parent(of: idx).map { isIsolatedGroup($0) } ?? false
                    case .none: stopped = false
                    }
                    // R1.
                    if siblingBeneath, stopped { return LayerStyle.knockoutMergeResidualNote }
                    // R4 — nothing stopped it, so it punches to the ROOT; a
                    // document with no qualifying Background has nothing there.
                    let reachesRoot = style.knockout == .deep || tree.parent(of: idx) == nil
                    if !stopped, reachesRoot, !hasQualifyingBackground(), visibleBelowSet {
                        return LayerStyle.knockoutMergeResidualNote
                    }
                } else {
                    // R2 — every group from the member root down to this
                    // entry's parent is a pass-through group. The walk tests
                    // EVERY group on the way up, the merged root INCLUDED:
                    // a knockout child of a merged ISOLATED group is exact by
                    // decision (v), so it must not carry the note, and the
                    // root is that child's parent in the shortest such shape.
                    var passThrough = true
                    var current = idx
                    while let up = tree.parent(of: current) {
                        if isIsolatedGroup(up) { passThrough = false; break }
                        if up == root { break }
                        current = up
                    }
                    if passThrough { return LayerStyle.knockoutMergeResidualNote }
                }
            }
        }
        return nil
    }

    /// Whether some visible entry in the set's subtrees uses knockout at all.
    private func setUsesKnockout(_ roots: [Int], _ tree: LayerTree) -> Bool {
        var seen = Set<Int>()
        for root in roots {
            for idx in tree.subtree(of: root) where seen.insert(idx).inserted {
                guard layerInfo(idx)?.visible == true else { continue }
                if let style = layerStylePayload(idx), style.knockout != .none { return true }
            }
        }
        return false
    }
}

// MARK: - Reading and writing the style on a document layer

extension RasterDocument {
    /// Layer `idx`'s style as a typed value, or nil when it has none.
    func layerStylePayload(_ idx: Int) -> LayerStyle? {
        layerStyle(idx).flatMap(LayerStyle.decode)
    }

    /// Attaches `style` to layer `idx`, or CLEARS it when nil. An identity
    /// style is passed as nil — the core would clear it anyway (its one
    /// rule), and passing nil keeps the "unchanged" refusal exact for an
    /// unstyled layer. nil for an unchanged value or a bad index; throws
    /// with the core's message for a style the core refuses.
    func withLayerStylePayload(_ idx: Int, _ style: LayerStyle?) throws -> RasterDocument? {
        guard let style = style, !style.isIdentity else { return try withLayerStyle(idx, nil) }
        guard let json = style.json() else { return nil }
        return try withLayerStyle(idx, json)
    }

    var globalLight: GlobalLight {
        GlobalLight(angle: globalLightAngle, altitude: globalLightAltitude)
    }

    func withGlobalLight(_ light: GlobalLight) throws -> RasterDocument? {
        try withGlobalLight(angle: light.angle, altitude: light.altitude)
    }
}
