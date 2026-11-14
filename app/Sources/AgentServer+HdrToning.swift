import Foundation

// HDR Toning over MCP: `hdr_toning`, the twin of Image ▸ Adjustments ▸ HDR
// Toning… (`EditorViewController+HdrToning.swift`, `HdrToningSheetController`). The flatten the
// menu asks about is made silently here, as `performPixelEdit` makes a destructive edit's
// rasterize silently; the job and its one undo step are `HdrToningRun`'s. `preset` (phase 4c) is
// the dialog's Preset popup: `HdrToningSettings.parse` starts from `HdrToningPreset.named`, and
// the recorder keeps the preset's explicit values, never its name (`recordedArguments`).
// The preset library tools are library writes and reads, so `nonRecordable` and `documentFree`,
// like the brush preset library's. LOADING a preset is `hdr_toning`'s `preset`.
//
// | Handler | UI path it mirrors |
// |---|---|
// | `hdrToning` | Image ▸ Adjustments ▸ HDR Toning… |
// | `listHdrToningPresets` | the dialog's Preset popup: its rows |
// | `saveHdrToningPreset` | the popup's Save Preset… |
// | `importHdrToningPreset` | the popup's Import Preset… |
// | `deleteHdrToningPreset` | the popup's Delete Preset… (`name`); `file` clears a broken file |

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let hdrToningHandlers: [String: ToolHandler] = [
        "hdr_toning": { $0.hdrToning },
        "list_hdr_toning_presets": { $0.listHdrToningPresets },
        "save_hdr_toning_preset": { $0.saveHdrToningPreset },
        "import_hdr_toning_preset": { $0.importHdrToningPreset },
        "delete_hdr_toning_preset": { $0.deleteHdrToningPreset },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift. `hdr_toning`: recordable; answered at 8, 16 and 32 bits; a CMYK
    /// document is refused by `CmykDocument.refusal(tool:)`. The library tools resolve no document.
    static let hdrToningToolFacts: [String: ToolFacts] = [
        "hdr_toning": [.commonTool, .floatTool],
        "list_hdr_toning_presets": [.nonRecordable, .documentFree],
        "save_hdr_toning_preset": [.nonRecordable, .documentFree],
        "import_hdr_toning_preset": [.nonRecordable, .documentFree],
        "delete_hdr_toning_preset": [.nonRecordable, .documentFree],
    ]

    /// Mirrors the dialog's Preset popup rows: the built-ins in the popup's order (Default first),
    /// then the user's library by name. Each row's `hdr_toning` is what `hdr_toning` takes to
    /// reproduce the preset.
    func listHdrToningPresets(_ a: [String: Any]) throws -> String {
        try hdrToningPresetLibrary {
            let listing = try HdrToningPresetLibrary.listing()
            let builtIns: [[String: Any]] = HdrToningPreset.builtIns.map { preset in
                var row: [String: Any] = ["name": preset.name, "builtin": true,
                                          "hdr_toning": preset.arguments]
                if let purpose = preset.purpose { row["purpose"] = purpose }
                return row
            }
            let saved: [[String: Any]] = listing.entries.map { entry in
                ["name": entry.item.name, "builtin": false, "file": entry.file,
                 "hdr_toning": entry.item.arguments]
            }
            return try jsonResult([
                "presets": builtIns + saved,
                "broken": listing.broken.map(\.json),
                "maximum_presets": HdrToningPresetCodec.maximumEntries,
                "maximum_file_bytes": HdrToningPresetCodec.maximumFileBytes,
            ])
        }
    }

    /// Mirrors Save Preset…: `hdr_toning`'s method and values (never a depth) under `name`,
    /// through `HdrToningPreset.validated`, which refuses a built-in's name and any other key.
    func saveHdrToningPreset(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        var arguments = a
        for key in ["name", "replace", "document_id"] { arguments.removeValue(forKey: key) }
        return try hdrToningPresetLibrary {
            let saved = try HdrToningPresetLibrary.save(name: name, arguments: arguments,
                                                        replace: boolArg(a, "replace") ?? false)
            return try jsonResult(["ok": true, "name": saved.name, "hdr_toning": saved.arguments])
        }
    }

    /// Mirrors Delete Preset…, which acts on a saved preset only, so a built-in's name refuses
    /// before the library is asked; `file` removes one library file by its listing basename —
    /// the only way to clear a broken one.
    func deleteHdrToningPreset(_ a: [String: Any]) throws -> String {
        let name = stringArg(a, "name"), file = stringArg(a, "file")
        guard (name == nil) != (file == nil) else {
            throw ToolError(message: "delete_hdr_toning_preset takes exactly one of name or file — "
                + "a broken entry has no name.")
        }
        return try hdrToningPresetLibrary {
            if let file {
                try HdrToningPresetLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            let name = name ?? ""
            if let builtIn = HdrToningPreset.builtIn(named: name) {
                throw ToolError(message: "“\(builtIn.name)” is a built-in HDR Toning preset; only "
                    + "a saved preset can be deleted.")
            }
            try HdrToningPresetLibrary.delete(name: name)
            return try jsonResult(["ok": true, "name": name])
        }
    }

    /// Mirrors the dialog's Preset ▸ Import Preset…: a `.hdt` file saved into the HDR Toning user
    /// preset library (`HdrToningPresetLibrary.importFile`), named `name` or the file's name.
    func importHdrToningPreset(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else {
            throw ToolError(message: "No file at \(url.path)")
        }
        return try hdrToningPresetLibrary {
            let imported = try HdrToningPresetLibrary.importFile(
                url, name: stringArg(a, "name"), replace: boolArg(a, "replace") ?? false)
            return try jsonResult([
                "ok": true, "name": imported.preset.name, "hdr_toning": imported.preset.arguments,
                "report": imported.report, "file": url.path,
            ])
        }
    }

    /// A library refusal becomes an in-band `ToolError` with its own sentence.
    private func hdrToningPresetLibrary<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors Image ▸ Adjustments ▸ HDR Toning… (and Image ▸ Mode ▸ 8/16 Bits/Channel on a
    /// 32-bit document): the same settings, plan, job and one undo step, with no prompt.
    func hdrToning(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "The document has no image.") }
        let flattened = HdrToning.needsFlatten(doc)
        let settings: HdrToningSettings
        let run: HdrToningRun
        do {
            settings = try HdrToningSettings.parse(a, source: doc)
            run = try HdrToningRun(document, settings: settings)
            let prepared = try run.compute()
            try run.publish(prepared, to: document, record: .notACommand)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try jsonResult([
            "ok": true, "method": settings.method.rawValue, "bit_depth": settings.bitDepth,
            "flattened": flattened, "working_memory_plan_bytes": run.scratch,
            "document": summary(document),
        ])
    }
}
