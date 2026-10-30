import Foundation

/// The catalog entries for the adjustment-preset library
/// (`AgentServer+AdjustmentPresets.swift`), in their own file behind an
/// explicit non-inlining boundary for the reason `AgentCatalog.swift`'s
/// header gives.
extension AgentServer {
    @inline(never) func adjustmentPresetCatalog() -> [[String: Any]] {
        let ops = AdjustmentPresetLibrary.supportedOps.map(\.rawValue).sorted()
        let op: [String: Any] = [
            "type": "string", "enum": ops,
            "description": "The adjustment op the preset belongs to. Names are unique per op.",
        ]
        let length = "1–\(AdjustmentPreset.maximumNameLength) characters"
        let name: [String: Any] = [
            "type": "string", "description": "Adjustment preset name, \(length).",
        ]
        let preference = "A library write is a preference: no Undo step, no Action step, and "
            + "no document changes."
        return [
            tool("list_adjustment_presets",
                 "An adjustment dialog's Preset popup: list the bounded versioned library of "
                 + "saved adjustment presets, all of them or one op's. Each preset is {name, "
                 + "op, params, file}: params are exactly what add_adjustment_layer, "
                 + "edit_adjustment_layer and apply_filter take for that op (never an "
                 + "`evaluation` — the document decides that), so LOADING a preset is passing "
                 + "its op and params to one of those tools; for bcs, apply_filter's filter is "
                 + "\"adjust\" with the params flat. A file that is not a readable preset is a "
                 + "`broken` row — {file, code, reason, repairable} with code one of "
                 + "unreadable, oversize, not_json, unknown_version, invalid_schema, "
                 + "duplicate_name — instead of refusing the whole library; repairable is "
                 + "always false here. Resolves no document, so it answers at every depth.",
                 ["op": op]),
            tool("save_adjustment_preset",
                 "An adjustment dialog's Preset popup > Save Preset…: store params for one op "
                 + "under a name. params are valid exactly when the op's destructive form "
                 + "accepts them, an `evaluation` key is dropped, a bcs preset with no `legacy` "
                 + "is stored with legacy true (the curve those params run as a layer), and a "
                 + "colour in them is in the DOCUMENT's numbers, as everywhere in an "
                 + "adjustment. Presets exist for "
                 + ops.joined(separator: ", ") + ". A name already used for that op refuses "
                 + "unless replace is true; the same name under another op is a different "
                 + "preset. " + preference,
                 ["op": op, "name": name,
                  "params": ["type": "object",
                             "description": "The op's parameters (see add_adjustment_layer)."],
                  "replace": ["type": "boolean", "default": false]],
                 required: ["op", "name", "params"]),
            tool("rename_adjustment_preset",
                 "An adjustment dialog's Preset popup > Rename Preset…: give one op's preset "
                 + "a new name, keeping its params. Refuses when that op already has a preset "
                 + "under the new name; a change of letter case alone renames in place. "
                 + preference,
                 ["op": op, "name": name,
                  "new_name": ["type": "string",
                               "description": "The new name, \(length)."]],
                 required: ["op", "name", "new_name"]),
            tool("delete_adjustment_preset",
                 "An adjustment dialog's Preset popup > Delete Preset…: remove ONE library "
                 + "file. Pass either op and name (a listed preset) or file alone (any library "
                 + "file, including a broken one that has no name); name and file together, "
                 + "or neither, is refused. " + preference,
                 ["op": op, "name": name,
                  "file": libraryFileProperty("list_adjustment_presets")]),
        ]
    }
}
