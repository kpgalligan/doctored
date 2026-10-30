import Foundation

/// One point of a custom layer-style contour, both coordinates in
/// 0…255; `corner` true ends a smooth run (Photoshop's `Cnty` false). The
/// Contour editor writes smooth points only; corners come from presets'
/// readers, MCP and PSD.
struct ContourPointValue: Codable, Equatable {
    var x: Double
    var y: Double
    var corner: Bool? = nil
}

/// A layer-style contour as the core's style JSON holds it
/// (`core/src/style_contour_json.rs`): `{"preset": id}` or `{"points":
/// [...]}`, `points` winning when both are present. An effect's nil contour
/// is Linear and writes no key.
struct ContourValue: Codable, Equatable {
    var preset: String? = nil
    var points: [ContourPointValue]? = nil

    /// The ten default contours, in the picker's order: the JSON ids
    /// and the titles the picker shows.
    static let presets = [
        "linear", "cone", "cone_inverted", "gaussian", "half_round", "ring", "ring_double",
        "rolling_slope_descending", "rounded_steps", "sawtooth_1",
    ]
    static let titles = [
        "Linear", "Cone", "Cone - Inverted", "Gaussian", "Half Round", "Ring", "Ring - Double",
        "Rolling Slope - Descending", "Rounded Steps", "Sawtooth 1",
    ]

    /// Linear's two points: what the Contour editor opens on when the effect
    /// holds no custom curve yet.
    static let linearPoints = [ContourPointValue(x: 0, y: 0), ContourPointValue(x: 255, y: 255)]

    var isCustom: Bool { points != nil }

    /// The preset id this contour shows in the picker (Linear for nil).
    static func presetID(_ contour: ContourValue?) -> String {
        contour?.preset ?? "linear"
    }

    /// The value an effect stores for a picked preset: nil for Linear.
    static func picked(_ preset: String) -> ContourValue? {
        preset == "linear" ? nil : ContourValue(preset: preset)
    }
}
