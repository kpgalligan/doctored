import AppKit

func quickMaskUI(output: URL) throws -> [String: Any] {
    let (document, editor, source) = try openDocument()
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
    let original = [UInt16](repeating: 129, count: source.width * source.height)
    editor.agentSetSelection(CanvasSelection(shape: .mask16(original), canvasWidth: source.width, canvasHeight: source.height))
    editor.toggleQuickMask(nil); pump()
    try require(document.quickMaskActive, "Actual Quick Mask did not enter")
    configure(editor, size: 40, opacity: 73, pressureOpacity: true, airbrush: true)
    document.undoManager?.removeAllActions(); ActionRecorder.shared.start()
    // A fractional center avoids repeated squared radii on the integer grid;
    // >256 unique output codes is then a real coverage precision witness.
    let point = CGPoint(x: 46.375, y: 31.1875)
    try send(editor.canvas, .leftMouseDown, point, pressure: 0.375)
    let start = Date()
    while Date().timeIntervalSince(start) < 0.17 { pump() }
    let live = editor.canvas.nativeStrokeSession?.dabs ?? []
    try require(live.count >= 12, "Actual Quick Mask timer did not deposit native dabs")
    try screenshot(editor.view, output.appendingPathComponent("native-quick-mask-drag.png"))
    try send(editor.canvas, .leftMouseUp, point, pressure: 0.375)
    let steps = ActionRecorder.shared.stop()
    guard steps.count == 1, steps[0].tool == "quick_mask_stroke",
          let canonical = steps[0].arguments["native_stroke"] as? [String: Any],
          let painted = document.canvasSelection?.maskSamples16() else { throw DoctoredCoreError(message: "Quick Mask canonical native Action missing") }
    try require(steps[0].arguments["coverage_rle16"] == nil && editor.canvas.nativeStrokeSession == nil,
        "Native Quick Mask encoded a full-frame RLE or retained the session")
    let footprint = try coverageOracle(canonical, width: source.width, height: source.height, opacity: 0.73)
    let expected = footprint.map { coverage -> UInt16 in
        let numerator = UInt64(129) * UInt64(65535 - coverage) + UInt64(65535) * UInt64(coverage)
        return UInt16((numerator + 32767) / 65535)
    }
    let differences = painted.indices.filter { painted[$0] != expected[$0] }
    try require(differences.isEmpty, "Quick Mask differs from independent native scalar reference at \(differences.count) pixels: \(differences.prefix(5).map { "\($0):\(painted[$0])/\(expected[$0])" })")
    try require(Set(painted).count > 256 && painted[0] == 129,
        "Quick Mask precision/outside gate failed: \(Set(painted).count) codes, outside\(painted[0])")
    try require(document.doc === source && !document.isDocumentEdited, "Quick Mask rewrote source pixels or dirty identity")
    document.undoManager?.undo(); pump()
    try require(document.canvasSelection?.maskSamples16() == original, "Quick Mask Undo lost tiny coverage")
    document.undoManager?.redo(); pump()
    try require(document.canvasSelection?.maskSamples16() == expected, "Quick Mask Redo changed codes")
    document.undoManager?.undo(); pump()
    let reply = try replay(steps[0], document: document)
    try require(document.canvasSelection?.maskSamples16() == expected, "Quick Mask canonical replay differs: \(reply)")
    editor.toggleQuickMask(nil); pump()
    try require(document.canvasSelection?.maskSamples16() == expected && editor.canvas.nativeStrokeSession == nil,
        "Exiting Quick Mask reduced coverage or leaked session")
    return ["name": "quick-mask-ui", "passed": true, "distinct_native_codes": Set(painted).count,
        "actual_dabs": live.count / 4, "coverage_sha256": hashSamples(painted), "session_released": true]
}

/// These requests use the same Quick Mask commit owner and real progress panel
/// as the gesture. A long but admitted hard-dab sequence makes live progress
/// observable without test-only hooks or timing a pre-cancelled token.
func quickMaskLifecycle() throws -> [String: Any] {
    let (document, editor, source) = try openDocument(width: 384, height: 320)
    defer { document.close(); pump() }
    let n = source.width * source.height
    let baseline = [UInt16](repeating: 129, count: n)
    editor.agentSetSelection(CanvasSelection(shape: .mask16(baseline), canvasWidth: source.width, canvasHeight: source.height))
    editor.toggleQuickMask(nil); pump(); document.undoManager?.removeAllActions()
    let dabs = Array(repeating: [192.5, 160.5, 300.0, 0.13], count: 32).flatMap { $0 }
    let brush = try NativeBrushStroke(tip: BrushTip(hardness: 1), pixel: false, dabs: dabs)
    let stroke = try QuickMaskStroke(nativeBrush: brush, width: source.width, height: source.height, opacity: 0.73, erase: false)
    var cancelled = false, observedProgress = 0.0, scratchDuring: UInt64 = 0
    let cancelTimer = Timer(timeInterval: 0.001, repeats: true) { timer in
        // `$0.isVisible` is not decoration: a closed progress panel stays in
        // NSApp.windows, so an earlier case's Quick Mask coverage panel is
        // found FIRST and reports progress 0 for ever — the job below then
        // runs to completion uncancelled and this case measures nothing.
        guard let panel = NSApp.windows.first(where: {
                  $0.title == "Native Brush Coverage" && $0.isVisible
              }),
              let content = panel.contentView,
              let progress = descendants(content).compactMap({ $0 as? NSProgressIndicator }).first,
              progress.doubleValue > 0,
              let button = descendants(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Cancel" }) else { return }
        observedProgress = progress.doubleValue; scratchDuring = BackgroundWork.scratchEstimate
        cancelled = true; timer.invalidate(); button.performClick(nil)
    }
    RunLoop.main.add(cancelTimer, forMode: .common); RunLoop.main.add(cancelTimer, forMode: .modalPanel)
    var cancellationError = ""
    do { try document.applyQuickMaskStroke(stroke) }
    catch { cancellationError = error.localizedDescription }
    cancelTimer.invalidate()
    try require(cancelled && observedProgress > 0 && scratchDuring > 0 && !cancellationError.isEmpty,
        "Did not observe active brush progress and cancel through actual panel: \(cancellationError)")
    try require(document.doc === source && document.canvasSelection?.maskSamples16() == baseline && document.undoManager?.canUndo != true,
        "Cancelled Quick Mask changed pixels/selection/history")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0, "Cancelled Quick Mask retained worker scratch")
    let replacement = [UInt16](repeating: 32769, count: n)
    var changedDuringWork = false
    let staleTimer = Timer(timeInterval: 0.001, repeats: true) { timer in
        guard BackgroundWork.occupied else { return }
        changedDuringWork = true; timer.invalidate()
        document.setCanvasSelection(CanvasSelection(shape: .mask16(replacement), canvasWidth: source.width, canvasHeight: source.height))
    }
    RunLoop.main.add(staleTimer, forMode: .common); RunLoop.main.add(staleTimer, forMode: .modalPanel)
    var staleError = ""
    do { try document.applyQuickMaskStroke(stroke) }
    catch { staleError = error.localizedDescription }
    staleTimer.invalidate()
    try require(changedDuringWork && staleError.contains("changed") && document.canvasSelection?.maskSamples16() == replacement,
        "Quick Mask accepted a changed selection during coverage generation: \(staleError)")
    try require(document.doc === source && !BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
        "Stale Quick Mask changed native source or retained scratch")
    return ["name": "quick-mask-cancel-stale", "passed": true, "observed_progress": observedProgress,
        "scratch_during": scratchDuring, "scratch_after": BackgroundWork.scratchEstimate,
        "cancelled_via_actual_button": true, "stale_first_worker_refused": true]
}
