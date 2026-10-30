import Foundation

/// The Art History Brush's Style, ten of them: the length, curl and
/// looseness of the strokes one click lays (`core/src/art_history_stroke.rs`
/// holds the table). The raw value is the wire spelling — MCP
/// `art_history_brush_stroke`'s `style`, an Action's recorded `style` and the
/// core request's `"style"`.
enum ArtHistoryStyle: String, Codable, CaseIterable {
    case tightShort = "tight_short"
    case tightMedium = "tight_medium"
    case tightLong = "tight_long"
    case looseMedium = "loose_medium"
    case looseLong = "loose_long"
    case dab = "dab"
    case tightCurl = "tight_curl"
    case tightCurlLong = "tight_curl_long"
    case looseCurl = "loose_curl"
    case looseCurlLong = "loose_curl_long"

    /// The menu's wording, in the menu's order.
    var displayName: String {
        switch self {
        case .tightShort: return "Tight Short"
        case .tightMedium: return "Tight Medium"
        case .tightLong: return "Tight Long"
        case .looseMedium: return "Loose Medium"
        case .looseLong: return "Loose Long"
        case .dab: return "Dab"
        case .tightCurl: return "Tight Curl"
        case .tightCurlLong: return "Tight Curl Long"
        case .looseCurl: return "Loose Curl"
        case .looseCurlLong: return "Loose Curl Long"
        }
    }
}

/// One Art History Brush stroke's options as the core reads them (the
/// request of `dr_doc_art_history_brush_layer` and its typed twin): the Style,
/// the Area in px (0–500), the Tolerance in percent (0–100), the Opacity
/// (0–1), the Mode (`modes`, seven for this tool) and the seed the
/// strokes are drawn from. The options bar's values come from
/// `PaintToolOptions` (`artStyle`, `artArea`, `artTolerance`, `blendIndex`,
/// `opacity`); MCP and an Action's replay pass them as arguments.
struct ArtHistoryOptions {
    var style: ArtHistoryStyle
    var area: Double
    var tolerance: Double
    var opacity: Double
    var mode: DrBlendMode
    var seed: UInt32

    /// The tool's Mode list, in its order.
    static let modes: [DrBlendMode] = [
        DR_BLEND_NORMAL, DR_BLEND_DARKEN, DR_BLEND_LIGHTEN, DR_BLEND_HUE,
        DR_BLEND_SATURATION, DR_BLEND_COLOR, DR_BLEND_LUMINOSITY,
    ]
    static let defaultArea: Double = 50
    static let modeRefusal =
        "blend_mode must be Normal, Darken, Lighten, Hue, Saturation, Color or Luminosity"
    static let styleRefusal = "style must be "
        + ArtHistoryStyle.allCases.map(\.rawValue).dropLast().joined(separator: ", ")
        + " or loose_curl_long"

    /// A mode's menu name (`DrBlendMode.allBlendModes`).
    static func name(of mode: DrBlendMode) -> String {
        DrBlendMode.allBlendModes.first { $0.0 == mode }?.1 ?? "Normal"
    }

    /// The options bar's stroke: `paint`'s Style, Area, Tolerance, Opacity
    /// (percent) and Mode (`blendIndex`, Normal unless one of `modes` and, on a Grayscale
    /// `document`, one of its modes — #547 phase 2e — or on a Lab Color one one of
    /// `NativeLayerBlending.labModes` — #547 Lab 2f — as the Gray and Lab popups show it). The
    /// byte callers pass no document.
    init(paint: PaintToolOptions, seed: UInt32, document: RasterDocument? = nil) {
        let stored = DrBlendMode(rawValue: UInt32(max(paint.blendIndex, 0)))
        var mode = Self.modes.contains(stored) ? stored : DR_BLEND_NORMAL
        if let document, document.isGray,
           !NativeLayerBlending.grayModes(for: document).contains(mode) {
            mode = DR_BLEND_NORMAL
        }
        if let document, document.isLab, !NativeLayerBlending.labModes.contains(mode) {
            mode = DR_BLEND_NORMAL
        }
        self.init(
            style: paint.artStyle ?? .tightShort,
            area: min(max(paint.artArea ?? Self.defaultArea, 0), 500),
            tolerance: min(max(paint.artTolerance ?? 0, 0), 100),
            opacity: min(max(paint.opacity, 0), 100) / 100,
            mode: mode,
            seed: seed)
    }

    init(style: ArtHistoryStyle, area: Double, tolerance: Double, opacity: Double,
         mode: DrBlendMode, seed: UInt32) {
        self.style = style
        self.area = area
        self.tolerance = tolerance
        self.opacity = opacity
        self.mode = mode
        self.seed = seed
    }

    /// The core request (`dr_doc_art_history_brush_layer`'s `request`).
    var request: String {
        "{\"version\":1,\"style\":\"\(style.rawValue)\",\"area\":\(area),"
            + "\"tolerance\":\(tolerance),\"opacity\":\(opacity),"
            + "\"blend_mode\":\(mode.rawValue),\"seed\":\(seed)}"
    }

    /// The reply's `applied` and an Action's recorded arguments: the
    /// options in MCP's own units (Opacity in percent, Mode by name).
    var arguments: [String: Any] {
        ["style": style.rawValue, "area": area, "tolerance": tolerance,
         "opacity": opacity * 100, "blend_mode": Self.name(of: mode), "seed": Int(seed)]
    }

    /// An MCP or Action argument dictionary's options: `style` (default
    /// tight_short), `area` px 0–500 (50), `tolerance` % 0–100 (0), `opacity`
    /// 0–100 (100), `blend_mode` by name (Normal) and `seed` (0). Every
    /// value outside its range or vocabulary is refused by name.
    static func argument(_ a: [String: Any]) throws -> ArtHistoryOptions {
        func number(_ key: String) -> Double? {
            (a[key] as? NSNumber)?.doubleValue ?? (a[key] as? String).flatMap(Double.init)
        }
        func refuse(_ message: String) -> AgentServer.ToolError {
            AgentServer.ToolError(message: message)
        }
        var style = ArtHistoryStyle.tightShort
        if let raw = a["style"] {
            guard let name = raw as? String, let named = ArtHistoryStyle(rawValue: name) else {
                throw refuse(styleRefusal)
            }
            style = named
        }
        let area = number("area") ?? defaultArea
        guard area.isFinite, (0...500).contains(area) else {
            throw refuse("area must be 0 to 500 px")
        }
        let tolerance = number("tolerance") ?? 0
        guard tolerance.isFinite, (0...100).contains(tolerance) else {
            throw refuse("tolerance must be 0 to 100")
        }
        let opacity = number("opacity") ?? 100
        guard opacity.isFinite, (0...100).contains(opacity) else {
            throw refuse("opacity must be 0 to 100")
        }
        var mode = DR_BLEND_NORMAL
        if let raw = a["blend_mode"] {
            guard let name = raw as? String,
                  let named = modes.first(where: {
                      Self.name(of: $0).caseInsensitiveCompare(name) == .orderedSame
                  }) else {
                throw refuse(modeRefusal)
            }
            mode = named
        }
        let seed = number("seed") ?? 0
        guard seed.isFinite, seed >= 0, seed <= Double(UInt32.max), seed == seed.rounded() else {
            throw refuse("seed must be an integer from 0 to 4294967295")
        }
        return ArtHistoryOptions(style: style, area: area, tolerance: tolerance,
                                 opacity: opacity / 100, mode: mode, seed: UInt32(seed))
    }
}
