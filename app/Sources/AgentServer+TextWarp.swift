import AppKit

/// The `warp` argument add_text_layer and edit_text_layer take — the agent mirror of
/// Layer ▸ Warp Text… (EditorViewController+WarpText.swift, which runs edit_text_layer through the
/// dispatch) — its refusals, and the `warp` object their replies and get_document's text object
/// carry. The value and its rect are TextWarp.swift.
extension AgentServer {
    static let warpOnPathRefusal =
        "a text layer on a path cannot be warped: take it off its path first (on_path: null)"
    static let onPathWarpRefusal =
        "a warped text layer cannot go on a path: remove the warp first (warp: null)"
    static let warpObjectRefusal =
        "warp must be an object {style, bend?, horizontal?, vertical?}, or null to remove the warp"
    static let warpStyleRefusal = "warp.style must be one of "
        + TextWarp.styles.joined(separator: ", ")

    /// `warp` read against the layer's `current` warp: nil when the argument is absent;
    /// `.some(nil)` for JSON null or a warp whose three numbers are all 0 (no warp, never
    /// stored); else the warp. `{style, bend?, horizontal?, vertical?}`, each number −1…1: an
    /// absent `style` or number keeps `current`'s, and without a current warp `style` is
    /// required and an absent number is 0.
    func textWarpArg(_ a: [String: Any], current: TextWarp?) throws -> TextWarp?? {
        guard let raw = a["warp"] else { return nil }
        if raw is NSNull { return .some(nil) }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: Self.warpObjectRefusal)
        }
        let style: String
        if Self.present(object["style"]) {
            guard let given = object["style"] as? String, TextWarp.styles.contains(given) else {
                throw ToolError(message: Self.warpStyleRefusal)
            }
            style = given
        } else {
            guard let current else { throw ToolError(message: Self.warpStyleRefusal) }
            style = current.style
        }
        func number(_ key: String, keeping kept: Double?) throws -> Double {
            guard Self.present(object[key]) else { return kept ?? 0 }
            guard !Self.isJSONBoolean(object[key] as Any), let value = doubleArg(object, key),
                  value.isFinite, (-1...1).contains(value)
            else { throw ToolError(message: "warp.\(key) must be a number from -1 to 1") }
            return value
        }
        let warp = TextWarp(
            style: style, bend: try number("bend", keeping: current?.bend),
            horizontal: try number("horizontal", keeping: current?.horizontal),
            vertical: try number("vertical", keeping: current?.vertical))
        return .some(warp.isIdentity ? nil : warp)
    }

    /// The refusal for a call whose RESULTING layer would be both warped and on a path: named
    /// for what the call asked for (`warp` as an object asks for the warp).
    func refuseWarpOnPath(_ a: [String: Any], warp: TextWarp?, onPath: TextOnPath?) throws {
        guard warp != nil, onPath != nil else { return }
        throw ToolError(
            message: a["warp"] is [String: Any] ? Self.warpOnPathRefusal : Self.onPathWarpRefusal)
    }

    /// Whether an edit_text_layer call asks only for `warp` (beside `document_id` and
    /// `layer`): such a call that leaves the stored warp as it is changes nothing.
    static func asksOnlyForWarp(_ a: [String: Any]) -> Bool {
        a["warp"] != nil && Set(a.keys).isSubset(of: ["document_id", "layer", "warp"])
    }

    /// The `warp` object of a text object, its four keys; nil for a layer with no warp.
    static func textWarpFields(_ payload: TextLayerPayload) -> [String: Any]? {
        guard let warp = payload.warp else { return nil }
        return ["style": warp.style, "bend": warp.bend, "horizontal": warp.horizontal,
                "vertical": warp.vertical]
    }
}
