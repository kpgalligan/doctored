import AppKit

/// Preview and commit share one source/option capture. Native Patch sends
/// scalar coverage to the existing retouch solver, 8-bit Patch its overlay to
/// the healing solve; both publish through one nonmodal job.
extension EditorViewController {
    func patchCapture() -> PatchEditCapture? {
        // Content-Aware Move shares the canvas's `patchSession`.
        if currentTool == .contentAwareMove { return contentAwareMoveCapture() }
        guard let document else { return nil }
        let options = ToolOptionsStore.shared.patch
        do {
            return try PatchEditCapture(document: document, layer: document.activeLayerIndex,
                choice: RetouchSamplingChoice(mode: options.sample,
                    ignoreAdjustments: options.ignoreAdjustments),
                destination: options.directionIndex == 1, preview: true)
        } catch {
            document.presentError(error)
            return nil
        }
    }

    func patchCommitted(_ result: PatchSession.Result) {
        if currentTool == .contentAwareMove { return contentAwareMoveCommitted(result) }
        // Refused HERE rather than at capture: a nil capture cancels the
        // session and throws the placed region away, while a refused commit
        // leaves it placed for another drag once the worker is free.
        if let document, DocumentJobs.refusesGesture(on: document, workerBound: true) {
            NSSound.beep()
            return
        }
        guard !refuseChannelTargetEdit(), !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit(),
              let document, let source = result.capture,
              !refuseLockedEdit(layer: source.layer, kind: DR_EDIT_PIXELS) else { return }
        do {
            try source.capture.requireCurrent(document)
            guard document.confirmRasterize(layer: source.layer) else { return }
            try source.capture.requireCurrent(document)
            let record: [ActionStep] = .patchRegion(region: result.region, offset: result.offset,
                direction: source.destination ? "destination" : "source",
                sample: source.choice.mode, ignoreAdjustments: source.choice.ignoreAdjustments)
            let job = source.capture.source.isHighDepth
                ? try source.startNative(result, document: document, record: record, showProgress: true)
                : try source.startByte(result, document: document, record: record, showProgress: true)
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
