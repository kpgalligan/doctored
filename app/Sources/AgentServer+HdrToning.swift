import Foundation

// HDR Toning over MCP: `hdr_toning`, the twin of Image ▸ Adjustments ▸ HDR
// Toning… (`EditorViewController+HdrToning.swift`, `HdrToningSheetController`). The flatten the
// menu asks about is made silently here, as `performPixelEdit` makes a destructive edit's
// rasterize silently; the job and its one undo step are `HdrToningRun`'s. `preset` (phase 4c) is
// the dialog's Preset popup: `HdrToningSettings.parse` starts from `HdrToningPreset.named`, and
// the recorder keeps the preset's explicit values, never its name (`recordedArguments`).
// `import_hdr_toning_preset` is the popup's Import Preset…: a library write, so `nonRecordable`
// and `documentFree`, like `import_style_presets`.

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let hdrToningHandlers: [String: ToolHandler] = [
        "hdr_toning": { $0.hdrToning },
        "import_hdr_toning_preset": { $0.importHdrToningPreset },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift. `hdr_toning`: recordable; answered at 8, 16 and 32 bits; a CMYK
    /// document is refused by `CmykDocument.refusal(tool:)`. The import resolves no document.
    static let hdrToningToolFacts: [String: ToolFacts] = [
        "hdr_toning": [.commonTool, .floatTool],
        "import_hdr_toning_preset": [.nonRecordable, .documentFree],
    ]

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
        do {
            let imported = try HdrToningPresetLibrary.importFile(
                url, name: stringArg(a, "name"), replace: boolArg(a, "replace") ?? false)
            return try jsonResult([
                "ok": true, "name": imported.preset.name, "hdr_toning": imported.preset.arguments,
                "report": imported.report, "file": url.path,
            ])
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
