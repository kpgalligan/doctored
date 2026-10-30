import AppKit

/// Edit ▸ Fade (v1, `designs/v1-shortcuts-fade.md`):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `fade` | Edit ▸ Fade… ▸ Apply |
///
/// The candidate is the document's (`ImageDocument.currentFadeCandidate`): the
/// last filter, adjustment, stroke or fill, while nothing else has changed
/// since. `commitFade` groups its own undo step, so the handler does not go
/// through `performGroupedEdit`; `execute` records the call.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let fadeHandlers: [String: ToolHandler] = [
        "fade": { $0.fade },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let fadeToolFacts: [String: ToolFacts] = [
        // 8, 16 and 32 bits and CMYK (#1); a region step's fade reads the selection.
        "fade": [.commonTool, .floatTool, .selectionSensitive, .cmykTool],
    ]

    /// Edit ▸ Fade… ▸ Apply: the last step blended back with the state
    /// before it, as ONE undo step "Fade <step>" in place of that step.
    func fade(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let opacity = doubleArg(a, "opacity") else {
            throw ToolError(message: "Missing required argument: opacity")
        }
        guard opacity.isFinite, (0...1).contains(opacity) else {
            throw ToolError(message: "opacity must be between 0 and 1.")
        }
        // `blendModeArg`'s vocabulary and refusals, under the key `mode`.
        let mode = try blendModeArg(["blend_mode": stringArg(a, "mode") ?? "Normal"])
            ?? DR_BLEND_NORMAL
        guard let candidate = document.currentFadeCandidate else {
            throw ToolError(message: FadeStep.nothingToFade)
        }
        let name = "Fade \(candidate.name)"
        let result: RasterDocument?
        do {
            result = try document.fadedDocument(
                candidate, mode: mode, opacity: opacity, control: nil)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        guard let result else {
            return try jsonResult([
                "ok": true, "changed": false, "faded": candidate.name, "name": name,
            ])
        }
        guard document.commitFade(candidate, result: result, record: .notACommand) else {
            throw ToolError(message: "\(name) was refused and nothing changed.")
        }
        return try jsonResult(["ok": true, "changed": true, "faded": candidate.name, "name": name])
    }
}
