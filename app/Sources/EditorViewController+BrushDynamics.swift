import AppKit

/// The Brush's, Pencil's and Eraser's Dynamics… button and sheet
/// (`BrushDynamicsSheetController`). The values are tool options, app-wide preferences:
/// no undo step, no dirty flag, no Action step. Over MCP a stroke passes its own
/// `dynamics` (`AgentServer+BrushDynamics.swift`), so the options themselves are exempt
/// from agent parity as every tool option is.
extension EditorViewController {
    /// The options bar's Dynamics… button for `tool`, or nil for a tool that takes none.
    func brushDynamicsCluster(_ tool: EditorTool) -> OptionCluster? {
        guard tool.takesBrushDynamics else { return nil }
        return OptionCluster([
            OptionDescriptor(
                id: "paint.dynamics", overflowLabel: "Dynamics…",
                kind: .button(title: "Dynamics…", action: { [weak self] in
                    self?.showBrushDynamicsSheet(for: tool)
                })),
        ])
    }

    /// Opens the sheet on `tool`'s stored options; Apply writes them back.
    func showBrushDynamicsSheet(for tool: EditorTool) {
        guard tool.takesBrushDynamics,
              let options = ToolOptionsStore.shared.paintOptions(for: tool) else {
            NSSound.beep()
            return
        }
        presentAsSheet(BrushDynamicsSheetController(
            options: options, shapes: tool.takesTipShapeDynamics) { [weak self] chosen in
            self?.applyBrushDynamics(chosen, for: tool)
        })
    }

    /// The sheet's Apply: the tool's options, the canvas and the bar brought up to date.
    func applyBrushDynamics(_ options: PaintToolOptions, for tool: EditorTool) {
        ToolOptionsStore.shared.setPaintOptions(options, for: tool)
        syncCanvasPaintState()
        optionsBar.refreshValues()
    }
}
