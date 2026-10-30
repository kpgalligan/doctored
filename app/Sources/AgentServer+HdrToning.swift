import Foundation

// HDR Toning over MCP: `hdr_toning`, the twin of Image ▸ Adjustments ▸ HDR
// Toning… (`EditorViewController+HdrToning.swift`, `HdrToningSheetController`). The flatten the
// menu asks about is made silently here, as `performPixelEdit` makes a destructive edit's
// rasterize silently; the job and its one undo step are `HdrToningRun`'s. `preset` (phase 4c) is
// the dialog's Preset popup: `HdrToningSettings.parse` starts from `HdrToningPreset.named`, and
// the recorder keeps the preset's explicit values, never its name (`recordedArguments`).

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let hdrToningHandlers: [String: ToolHandler] = [
        "hdr_toning": { $0.hdrToning },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift. Recordable; answered at 8, 16 and 32 bits; a CMYK document is
    /// refused by `CmykDocument.refusal(tool:)`.
    static let hdrToningToolFacts: [String: ToolFacts] = [
        "hdr_toning": [.commonTool, .floatTool],
    ]

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
