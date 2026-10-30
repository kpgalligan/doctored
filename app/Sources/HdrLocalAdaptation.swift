import Foundation

// HDR Toning's Local Adaptation, the default method: its ten values and
// the Toning Curve, their core request fields and their MCP parse. The pixel math is the core's
// (`hdr_local_adaptation.rs`: an edge-aware base/detail split of the log luminance through the
// guided filter Presence and Dehaze share); ranges are the core's, which refuses a value outside
// them by name. Measured against Photoshop at its defaults: it differs (19 / 11 codes, max
// / mean at 8-bit scale) and ships as declared. The dialog's pane is
// `HdrToningLocalAdaptationPane`; the settings that carry it are `HdrToningSettings.local`.

/// One Local Adaptation request, with its defaults.
struct HdrLocalAdaptation: Equatable {
    var radius = 16.0, strength = 0.52, smoothEdges = false, gamma = 1.0, exposure = 0.0,
        detail = 30.0, shadow = 0.0, highlight = 0.0, vibrance = 0.0, saturation = 20.0
    var toningCurve = CurvePoint.identity

    /// The `hdr_toning` arguments only Local Adaptation takes (`exposure` and `gamma` are shared
    /// with Exposure and Gamma).
    static let ownKeys = ["radius", "strength", "smooth_edges", "detail", "shadow", "highlight",
                          "vibrance", "saturation", "toning_curve"]

    /// The core request's keys, which are `hdr_toning`'s step arguments too: every value explicit.
    var fields: [String: Any] {
        [
            "radius": radius, "strength": strength, "smooth_edges": smoothEdges, "gamma": gamma,
            "exposure": exposure, "detail": detail, "shadow": shadow, "highlight": highlight,
            "vibrance": vibrance, "saturation": saturation,
            "toning_curve": toningCurve.map { [$0.x, $0.y] },
        ]
    }

    /// Reads the keys `a` carries (numbers or their string spellings, `smooth_edges` a boolean or
    /// "true"/"false", `toning_curve` an array of `[in, out]` number pairs); an absent key keeps
    /// its value. Ranges and the curve's point count are checked by the core.
    mutating func parse(_ a: [String: Any]) throws {
        func number(_ key: String) throws -> Double? {
            guard let value = a[key] else { return nil }
            return try Self.number(value, key)
        }
        if let value = try number("radius") { radius = value }
        if let value = try number("strength") { strength = value }
        if let value = try number("gamma") { gamma = value }
        if let value = try number("exposure") { exposure = value }
        if let value = try number("detail") { detail = value }
        if let value = try number("shadow") { shadow = value }
        if let value = try number("highlight") { highlight = value }
        if let value = try number("vibrance") { vibrance = value }
        if let value = try number("saturation") { saturation = value }
        if let value = a["smooth_edges"] {
            if let flag = value as? Bool {
                smoothEdges = flag
            } else if let text = value as? String, text == "true" || text == "false" {
                smoothEdges = text == "true"
            } else {
                throw DoctoredCoreError(message: "smooth_edges must be true or false.")
            }
        }
        if let value = a["toning_curve"] {
            guard let pairs = value as? [Any] else {
                throw DoctoredCoreError(message: "toning_curve must be an array of [in, out] pairs.")
            }
            toningCurve = try pairs.map { pair in
                guard let pair = pair as? [Any], pair.count == 2 else {
                    throw DoctoredCoreError(message: "toning_curve must be an array of [in, out] pairs.")
                }
                return CurvePoint(x: try Self.number(pair[0], "toning_curve"),
                                  y: try Self.number(pair[1], "toning_curve"))
            }
        }
    }

    /// A finite number, or its string spelling (the argument helpers' rule).
    private static func number(_ value: Any, _ key: String) throws -> Double {
        let parsed = (value as? NSNumber)?.doubleValue ?? (value as? String).flatMap(Double.init)
        guard let parsed, parsed.isFinite else {
            throw DoctoredCoreError(message: "\(key) must be a number.")
        }
        return parsed
    }
}
