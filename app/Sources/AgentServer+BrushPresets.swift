import Foundation

// The five tools are `nonRecordable` and `documentFree`: a library write is a preference,
// never a step, and none resolves a document. LOADING a preset is the popup filling the tool's
// options, which are app-wide and exempt from parity like every tool option, so the caller
// passes a listed row's values to a stroke tool instead (the adjustment-preset rule).

/// The brush preset library over MCP.
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `listBrushPresets` | the paint options bar's Preset popup: its rows |
/// | `saveBrushPreset` | the popup's Save Preset… |
/// | `deleteBrushPreset` | the popup's Delete Preset… (`name`); `file` clears a broken file |
/// | `importBrushPresets` | the popup's Import Presets… |
/// | `exportBrushPresets` | the popup's Export Presets… |
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let brushPresetsHandlers: [String: ToolHandler] = [
        "list_brush_presets": { $0.listBrushPresets },
        "save_brush_preset": { $0.saveBrushPreset },
        "delete_brush_preset": { $0.deleteBrushPreset },
        "import_brush_presets": { $0.importBrushPresets },
        "export_brush_presets": { $0.exportBrushPresets },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift.
    static let brushPresetsToolFacts: [String: ToolFacts] = [
        "list_brush_presets": [.nonRecordable, .documentFree],
        "save_brush_preset": [.nonRecordable, .documentFree],
        "delete_brush_preset": [.nonRecordable, .documentFree],
        "import_brush_presets": [.nonRecordable, .documentFree],
        "export_brush_presets": [.nonRecordable, .documentFree],
    ]

    /// Mirrors the Preset popup's rows: the built-ins, then the library.
    func listBrushPresets(_ a: [String: Any]) throws -> String {
        try brushPresetLibrary {
            let listing = try BrushPresetLibrary.listing()
            let builtIns: [[String: Any]] = BrushPreset.builtIns.map { preset in
                var row = preset.fields
                row["builtin"] = true
                return row
            }
            let saved: [[String: Any]] = listing.entries.map { entry in
                var row = entry.item.fields
                row["builtin"] = false
                row["file"] = entry.file
                return row
            }
            return try jsonResult([
                "presets": builtIns + saved,
                "broken": listing.broken.map(\.json),
                "maximum_presets": BrushPresetCodec.maximumEntries,
                "maximum_file_bytes": BrushPresetCodec.maximumFileBytes,
            ])
        }
    }

    /// Mirrors Save Preset…: the six tip values under a name.
    func saveBrushPreset(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        var values: [String: Any] = [:]
        for field in BrushPreset.fieldNames where field != "airbrush" {
            guard let number = doubleArg(a, field) else {
                throw ToolError(message: "Missing required argument: \(field)")
            }
            values[field] = number
        }
        values["airbrush"] = boolArg(a, "airbrush") ?? false
        return try brushPresetLibrary {
            let saved = try BrushPresetLibrary.save(name: name, values: values,
                                                    replace: boolArg(a, "replace") ?? false)
            return try jsonResult(["ok": true, "preset": saved.fields])
        }
    }

    /// Mirrors Delete Preset…; `file` removes one library file by its listing
    /// basename — the only way to clear a broken one.
    func deleteBrushPreset(_ a: [String: Any]) throws -> String {
        let name = stringArg(a, "name"), file = stringArg(a, "file")
        guard (name == nil) != (file == nil) else {
            throw ToolError(message: "delete_brush_preset takes exactly one of name or file — "
                + "a broken entry has no name.")
        }
        return try brushPresetLibrary {
            if let file {
                try BrushPresetLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            try BrushPresetLibrary.delete(name: name ?? "")
            return try jsonResult(["ok": true, "name": name ?? ""])
        }
    }

    /// Mirrors Import Presets…: one Doctored brush preset file into the library.
    func importBrushPresets(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else {
            throw ToolError(message: "No file at \(url.path)")
        }
        return try brushPresetLibrary {
            let result = try BrushPresetLibrary.importFile(url)
            return try jsonResult(["ok": true, "imported": result.imported, "skipped": result.skipped,
                                   "file": url.path])
        }
    }

    /// Mirrors Export Presets… (every user preset), or `names`'s presets alone.
    func exportBrushPresets(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var names: [String]?
        if let raw = a["names"] {
            guard let list = raw as? [Any], !list.isEmpty,
                  list.allSatisfy({ $0 is String }) else {
                throw ToolError(message: "names must be a non-empty array of preset names.")
            }
            names = list.compactMap { $0 as? String }
        }
        return try brushPresetLibrary {
            let written = try BrushPresetLibrary.exportFile(to: url, names: names)
            return try jsonResult(["ok": true, "exported": written, "file": url.path])
        }
    }

    /// A library refusal becomes an in-band `ToolError` with its own sentence.
    private func brushPresetLibrary<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
