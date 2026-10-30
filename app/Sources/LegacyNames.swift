import Foundation

/// The one home of the names a renamed feature used before (2026-10-29): Camera Raw Filter
/// became Develop Filter and Photomerge became Merge Panorama. Only PERSISTED files are read
/// under the old names — an Action step (`Action.decode`, so the Actions library, a `.draction`,
/// `save_action`/`run_action` and the summaries alike), a Develop Filter preset
/// (`DevelopFilterPreset.decode`) and a shortcut override (`ShortcutBindings.decode`); each
/// re-save writes the new name. MCP and the core accept the new names only.
enum LegacyNames {
    /// An Action step's earlier tool name → its current one.
    static let stepTools: [String: String] = ["photomerge": "merge_panorama"]

    /// An `apply_filter` step's earlier `filter` value → its current one.
    static let filterOps: [String: String] = ["camera_raw": "develop_filter"]

    /// A shortcut override's earlier menu identity (its action selector) → its current one.
    static let shortcutIdentities: [String: String] = [
        "showCameraRawFilter:": "showDevelopFilter:",
        "photomerge:": "mergePanorama:",
    ]

    /// The controls key a Develop Filter preset saved before the rename carries in place of
    /// `develop_filter`.
    static let developFilterPresetKey = "camera_raw"

    /// `tool` under its current name.
    static func currentTool(_ tool: String) -> String {
        stepTools[tool] ?? tool
    }

    /// A step's arguments with an earlier `apply_filter` `filter` value renamed; every other
    /// tool's arguments, and every other key, unchanged.
    static func currentArguments(tool: String, _ arguments: [String: Any]) -> [String: Any] {
        guard tool == "apply_filter", let filter = arguments["filter"] as? String,
              let current = filterOps[filter] else { return arguments }
        var renamed = arguments
        renamed["filter"] = current
        return renamed
    }
}
