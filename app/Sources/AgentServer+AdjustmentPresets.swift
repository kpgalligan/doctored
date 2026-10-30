import Foundation

// The four adjustment preset library tools are `nonRecordable` and `documentFree`: they resolve no
// document, so they answer at every depth, and a library write is a preference, never a step.

/// The adjustment-preset library over MCP.
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `listAdjustmentPresets` | an adjustment dialog's Preset popup (`AdjustmentPresetControl`) |
/// | `saveAdjustmentPreset` | the Preset popup ▸ Save Preset… |
/// | `renameAdjustmentPreset` | the Preset popup ▸ Rename Preset… |
/// | `deleteAdjustmentPreset` | the Preset popup ▸ Delete Preset… (`op` + `name`); `file` clears a broken file, which has no popup row |
///
/// Library writes are PREFERENCES: outside document Undo and never Action
/// steps. LOADING a preset has no tool of its own: the popup only fills the
/// dialog's controls, and a caller passes a listed row's `op` and `params`
/// to `add_adjustment_layer`, `edit_adjustment_layer` or `apply_filter`,
/// exactly as the dialog's Apply records.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let adjustmentPresetsHandlers: [String: ToolHandler] = [
        "list_adjustment_presets": { $0.listAdjustmentPresets },
        "save_adjustment_preset": { $0.saveAdjustmentPreset },
        "rename_adjustment_preset": { $0.renameAdjustmentPreset },
        "delete_adjustment_preset": { $0.deleteAdjustmentPreset },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let adjustmentPresetsToolFacts: [String: ToolFacts] = [
        // The library's writes are preferences, its list only reads,
        // and none of the four resolves a document.
        "list_adjustment_presets": [.nonRecordable, .documentFree],
        "save_adjustment_preset": [.nonRecordable, .documentFree],
        "rename_adjustment_preset": [.nonRecordable, .documentFree],
        "delete_adjustment_preset": [.nonRecordable, .documentFree],
    ]

    /// Mirrors an adjustment dialog's Preset popup: the saved presets, all
    /// of them or one op's, and every file that is not a readable preset.
    func listAdjustmentPresets(_ a: [String: Any]) throws -> String {
        let op = try a["op"].map { _ in try adjustmentPresetOp(a) }
        return try adjustmentPresetLibrary {
            let listing = try AdjustmentPresetLibrary.listing()
            let presets: [[String: Any]] = listing.entries
                .filter { op == nil || $0.item.op == op }
                .map { entry in
                    ["name": entry.item.name, "op": entry.item.op.rawValue,
                     "params": entry.item.params, "file": entry.file]
                }
            return try jsonResult([
                "presets": presets,
                "broken": listing.broken.map(\.json),
                "maximum_presets": AdjustmentPresetLibrary.maximumPresets,
                "maximum_file_bytes": AdjustmentPresetLibrary.maximumFileBytes,
            ])
        }
    }

    /// Mirrors the Preset popup ▸ Save Preset…: `params` under a name, for
    /// one op.
    func saveAdjustmentPreset(_ a: [String: Any]) throws -> String {
        let op = try adjustmentPresetOp(a)
        let name = try requiredString(a, "name")
        guard let params = a["params"] as? [String: Any] else {
            throw ToolError(message: "params must be a JSON object: the op's parameters, "
                + "exactly as add_adjustment_layer takes them.")
        }
        return try adjustmentPresetLibrary {
            let saved = try AdjustmentPresetLibrary.save(
                AdjustmentPreset.validated(name: name, op: op, params: params),
                replace: boolArg(a, "replace") ?? false)
            return try jsonResult(try adjustmentPresetReply(saved))
        }
    }

    /// Mirrors the Preset popup ▸ Rename Preset….
    func renameAdjustmentPreset(_ a: [String: Any]) throws -> String {
        let op = try adjustmentPresetOp(a)
        let name = try requiredString(a, "name")
        let newName = try requiredString(a, "new_name")
        return try adjustmentPresetLibrary {
            let renamed = try AdjustmentPresetLibrary.rename(op: op, name: name, to: newName)
            return try jsonResult(try adjustmentPresetReply(renamed))
        }
    }

    /// Mirrors the Preset popup ▸ Delete Preset… (`op` + `name`). `file`
    /// removes one library file by its listing basename — the only way to
    /// clear a broken file, which has no name and no popup row.
    func deleteAdjustmentPreset(_ a: [String: Any]) throws -> String {
        let hasName = stringArg(a, "name") != nil, hasFile = stringArg(a, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(
                message: "delete_adjustment_preset takes exactly one of name (with op) or file "
                    + "— a broken entry has no name.")
        }
        if hasFile {
            guard a["op"] == nil else {
                throw ToolError(message: "delete_adjustment_preset: file names one library file "
                    + "by itself; pass op only with name.")
            }
            return try adjustmentPresetLibrary {
                let file = try PreferenceLibrary<AdjustmentPresetCodec>.validatedFileName(
                    requiredString(a, "file"))
                try AdjustmentPresetLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
        }
        let op = try adjustmentPresetOp(a)
        let name = try requiredString(a, "name")
        return try adjustmentPresetLibrary {
            // The library trims and normalizes the name before it compares,
            // so the reply's `file` is looked up the same way.
            let key = PreferenceLibrary<AdjustmentPresetCodec>.key(
                try AdjustmentPreset.normalizedName(name))
            let file = try AdjustmentPresetLibrary.listing().entries.first {
                $0.item.op == op && PreferenceLibrary<AdjustmentPresetCodec>.key($0.item.name) == key
            }?.file
            try AdjustmentPresetLibrary.delete(op: op, name: name)
            var reply: [String: Any] = ["ok": true, "op": op.rawValue]
            if let file { reply["file"] = file }
            return try jsonResult(reply)
        }
    }

    /// The required `op`: an adjustment op the library holds presets for.
    private func adjustmentPresetOp(_ a: [String: Any]) throws -> AdjustmentLayerOp {
        let name = try requiredString(a, "op")
        guard let op = AdjustmentLayerOp(rawValue: name),
              AdjustmentPresetLibrary.supportedOps.contains(op)
        else {
            throw ToolError(message: "op must be one of "
                + AdjustmentPresetLibrary.supportedOpNames + ".")
        }
        return op
    }

    /// `{ok, preset: {name, op, params}, file}` for a saved or renamed preset.
    private func adjustmentPresetReply(_ preset: AdjustmentPreset) throws -> [String: Any] {
        var reply: [String: Any] = [
            "ok": true,
            "preset": ["name": preset.name, "op": preset.op.rawValue, "params": preset.params],
        ]
        if let file = try AdjustmentPresetLibrary.listing().entries
            .first(where: { $0.item.op == preset.op && $0.item.name == preset.name })?.file {
            reply["file"] = file
        }
        return reply
    }

    /// A library refusal becomes an in-band `ToolError` with its own sentence.
    private func adjustmentPresetLibrary<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
