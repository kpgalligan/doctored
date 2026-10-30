import AppKit

/// The Content-Aware Move tool. Its outline and drag are the canvas's
/// `patchSession`, so the canvas's two Patch callbacks arrive here through
/// `patchCapture` / `patchCommitted` (EditorViewController+Patch) while this
/// tool is selected. The plan and the one job are `ContentAwareMoveWork.start`'s,
/// the same call `content_aware_move` makes
/// (AgentServer+ContentAwareTools.contentAwareMove).
extension EditorViewController {
    @objc func selectContentAwareMoveTool(_ sender: Any?) { selectTool(.contentAwareMove) }

    /// The gesture's one source and options capture. Always a Destination
    /// capture: the pixels are read where the region was outlined and land
    /// where it is dragged, in both modes, so the drag previews them there.
    func contentAwareMoveCapture() -> PatchEditCapture? {
        guard let document else { return nil }
        let options = ToolOptionsStore.shared.contentAwareMove
        do {
            return try PatchEditCapture(document: document, layer: document.activeLayerIndex,
                choice: RetouchSamplingChoice(mode: options.sample,
                    ignoreAdjustments: options.ignoreAdjustments),
                destination: true, preview: true)
        } catch {
            document.presentError(ContentAwareMoveWork.reworded(error))
            return nil
        }
    }

    /// Mouse-up of a Content-Aware Move drag: `patchCommitted`'s guards in its
    /// order, then one job and one recorded `content_aware_move` step. The
    /// mode is read here, at the commit, as Patch reads its direction at the
    /// capture: nothing can change the option mid-press.
    func contentAwareMoveCommitted(_ result: PatchSession.Result) {
        // Refused here rather than at capture, for `patchCommitted`'s reason:
        // a refused commit leaves the region placed for another drag.
        if let document, DocumentJobs.refusesGesture(on: document, workerBound: true) {
            NSSound.beep()
            return
        }
        guard !refuseChannelTargetEdit(), !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit(),
              let document, let source = result.capture,
              !refuseLockedEdit(layer: source.layer, kind: DR_EDIT_PIXELS) else { return }
        let extend = ToolOptionsStore.shared.contentAwareMove.modeIndex == 1
        do {
            try source.capture.requireCurrent(document)
            guard document.confirmRasterize(layer: source.layer) else { return }
            try source.capture.requireCurrent(document)
            // An outlined polygon is explicit; an adopted selection is the
            // Action's current selection, as a recorded Patch has it.
            var arguments: [String: Any] = [
                "layer": ActionArgs.activeLayer,
                "offset_x": ActionArgs.number(result.offset.dx),
                "offset_y": ActionArgs.number(result.offset.dy),
                "mode": extend ? "extend" : "move", "seed": 0,
            ]
            if case .polygon(let points) = result.region.shape {
                arguments["points"] = ActionArgs.points(points)
            }
            arguments.merge(ActionArgs.sampling(
                source.choice.mode, source.choice.ignoreAdjustments)) { _, new in new }
            let job = try ContentAwareMoveWork.start(
                capture: source, result: result, extend: extend, seed: 0, document: document,
                record: .step("content_aware_move", arguments, note: "Content-Aware Move tool"),
                showProgress: true)
            job.completion = { [weak document] completed in
                if ["failed", "stale"].contains(completed.state), let message = completed.message {
                    document?.presentError(DoctoredCoreError(message: message))
                } else if completed.state == "unchanged" {
                    NSSound.beep()
                }
            }
        } catch { document.presentError(error) }
    }
}
