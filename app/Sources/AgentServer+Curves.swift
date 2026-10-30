import AppKit

// This file's `validatedCurvesParams` is the one MCP validator for both forms.

/// The agent side of Curves: the ONE strict validator every curves params
/// object passes, in either form — point lists (`rgb`, `r`, `g`, `b`, or a
/// CMYK document's `cmyk`, `c`, `m`, `y`, `k`) or 256-entry maps
/// (`rgb_map`, `r_map`, `g_map`, `b_map`, or `cmyk_map`, `c_map`, `m_map`,
/// `y_map`, `k_map` on ink: a curve drawn with
/// the pencil; `CurvesMeta`). Which colour mode's keys a document takes is
/// `CmykDocument`'s gate.
extension AgentServer {
    /// The validated curves params, unchanged.
    ///
    /// UI paths: Layer ▸ New Adjustment Layer ▸ Curves… and Layer ▸
    /// Adjustment Options… (the Curves dialog; `add_adjustment_layer` /
    /// `edit_adjustment_layer` through `adjustmentParams`), and Image ▸
    /// Adjustments ▸ Curves… (`apply_filter` through
    /// `adjustmentFilterParams`). The dialog's Pencil draws a map curve
    /// (`CurvePencil`)
    /// and its Points converts one to control points; Smooth is UI-only, as a
    /// model computes the map it passes.
    ///
    /// Stricter than the core, which clamps point values: every violation
    /// is refused naming the key, so a model corrects itself in one turn.
    func validatedCurvesParams(_ params: [String: Any]) throws -> [String: Any] {
        let hasPoints = CurvesMeta.pointKeys.contains { params[$0] != nil }
        let hasMaps = CurvesMeta.mapKeys.contains { params[$0] != nil }
        if hasPoints && hasMaps {
            throw ToolError(
                message: "curves takes either point lists (rgb, r, g, b; cmyk, c, m, y, k) or "
                    + "256-entry maps (rgb_map, r_map, g_map, b_map; cmyk_map, c_map, m_map, y_map, "
                    + "k_map), not both")
        }
        for key in CurvesMeta.mapKeys {
            guard let value = params[key] else { continue }
            guard let list = value as? [Any], list.count == 256,
                list.allSatisfy({ Self.isMapEntry($0) })
            else {
                throw ToolError(
                    message: "curves \(key) must be an array of 256 integers from 0 to 255 "
                        + "(the output for each input code)")
            }
        }
        for key in CurvesMeta.pointKeys {
            guard let value = params[key] else { continue }
            guard let list = value as? [Any] else {
                throw ToolError(
                    message: "curves \(key) must be an array of [in, out] pairs")
            }
            guard (2...16).contains(list.count) else {
                throw ToolError(
                    message: "curves \(key) needs 2 to 16 [in, out] points "
                        + "(got \(list.count))")
            }
            var inputs = Set<Double>()
            for entry in list {
                guard let pair = entry as? [Any], pair.count == 2,
                    let x = (pair[0] as? NSNumber)?.doubleValue, x.isFinite,
                    let y = (pair[1] as? NSNumber)?.doubleValue, y.isFinite
                else {
                    throw ToolError(
                        message: "curves \(key): every point must be an [in, out] pair "
                            + "of two numbers")
                }
                guard x >= 0, x <= 255, y >= 0, y <= 255 else {
                    throw ToolError(
                        message: "curves \(key): point values must be between 0 and 255 "
                            + "(got [\(x), \(y)])")
                }
                inputs.insert(x)
            }
            guard inputs.count >= 2 else {
                throw ToolError(
                    message: "curves \(key) needs at least 2 points with distinct "
                        + "\"in\" values")
            }
        }
        return params
    }

    /// One map entry: a JSON number (not a boolean) that is an integer
    /// 0…255 — the core's rule, so a map passing here always parses.
    private static func isMapEntry(_ value: Any) -> Bool {
        guard let number = value as? NSNumber, !ActionJSON.isBoolean(number) else { return false }
        let v = number.doubleValue
        return v.isFinite && v == v.rounded() && v >= 0 && v <= 255
    }
}
