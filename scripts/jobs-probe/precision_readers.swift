import AppKit

/// At 16 bits the eyedropper, the Info readout (the composite and scoped
/// cursor) and the Info histogram name a read failure instead of discarding
/// it — off-canvas stays a silent freeze — and the typed Evaluation sheet
/// measures against the retained typed projection.

private func storedValue(_ subject: Any, _ label: String) -> Any? {
    Mirror(reflecting: subject).children.first(where: { $0.label == label })?.value
}

private func footnoteText(_ panel: InfoPanelViewController) throws -> String {
    guard let footnote = storedValue(panel, "footnote") as? NSTextField else {
        throw ProbeFailure(description: "Info panel footnote")
    }
    return footnote.stringValue
}

/// Chooses the Info panel's source popup item `tag` the way a click does.
private func chooseInfoSource(_ panel: InfoPanelViewController, tag: Int) throws {
    guard let popup = storedValue(panel, "sourcePopup") as? NSPopUpButton,
          let action = popup.action else {
        throw ProbeFailure(description: "Info panel source popup")
    }
    popup.selectItem(withTag: tag)
    NSApp.sendAction(action, to: popup.target, from: popup)
}

/// A flag written on the loader's queue and read on main.
private final class LoaderFlag {
    private let lock = NSLock()
    private var value = false
    var isSet: Bool { lock.lock(); defer { lock.unlock() }; return value }
    func set() { lock.lock(); value = true; lock.unlock() }
}

/// Every delivery one `HistogramLoader` makes, in order.
private final class LoaderDeliveries {
    var bins: [HistogramBins?] = []
    var reports: [[String: Any]?] = []
    var failures: [String] = []
    var count: Int { bins.count + reports.count + failures.count }

    func attach(_ loader: HistogramLoader, failure: Bool) {
        loader.onBins = { [unowned self] in self.bins.append($0) }
        loader.onReport = { [unowned self] in self.reports.append($0) }
        if failure { loader.onFailure = { [unowned self] in self.failures.append($0) } }
    }
}

private func settle(_ seconds: Double) {
    RunLoop.main.run(until: Date().addingTimeInterval(seconds))
}

private func verifyLoaderFailures() throws {
    // A throwing request reaches onFailure with its sentence.
    let failing = HistogramLoader()
    let failed = LoaderDeliveries()
    failed.attach(failing, failure: true)
    failing.request { throw DoctoredCoreError(message: "probe histogram failure") }
    try pumpUntil({ failed.count == 1 }, seconds: 5)
    try require(failed.failures == ["probe histogram failure"] && failed.bins.isEmpty,
                "(d) a throwing request reaches onFailure: \(failed.failures)")

    // With no onFailure it reaches onBins(nil).
    let unset = HistogramLoader()
    let fallback = LoaderDeliveries()
    fallback.attach(unset, failure: false)
    unset.request { throw DoctoredCoreError(message: "probe histogram failure") }
    try pumpUntil({ fallback.count == 1 }, seconds: 5)
    try require(fallback.bins.count == 1 && fallback.bins.first.map { $0 == nil } == true,
                "(d) with no onFailure a throw reaches onBins(nil)")

    // JobCancelled reaches onBins(nil) even with onFailure set.
    let cancelled = HistogramLoader()
    let cancel = LoaderDeliveries()
    cancel.attach(cancelled, failure: true)
    cancelled.request { throw JobCancelled() }
    try pumpUntil({ cancel.count == 1 }, seconds: 5)
    try require(cancel.failures.isEmpty && cancel.bins.count == 1,
                "(d) JobCancelled reaches onBins(nil): \(cancel.failures)")

    // A mid-run cancel: the kernel's own sentence under a cancelled control
    // is the cancel, delivered as nil.
    let midRun = HistogramLoader()
    let mid = LoaderDeliveries()
    mid.attach(midRun, failure: true)
    let control = try RasterJobControl()
    control.cancel()
    midRun.request {
        try HistogramLoader.reading(under: control) { throw DoctoredCoreError(message: "kernel stopped") }
    }
    try pumpUntil({ mid.count == 1 }, seconds: 5)
    try require(mid.failures.isEmpty && mid.bins.count == 1,
                "(d) a mid-run cancel yields nil, not a failure: \(mid.failures)")

    // A throwing requestReport reaches onFailure.
    let report = HistogramLoader()
    let reported = LoaderDeliveries()
    reported.attach(report, failure: true)
    report.requestReport { throw DoctoredCoreError(message: "probe report failure") }
    try pumpUntil({ reported.count == 1 }, seconds: 5)
    try require(reported.failures == ["probe report failure"] && reported.reports.isEmpty,
                "(d) a throwing requestReport reaches onFailure: \(reported.failures)")

    // Superseded requests — one in flight, one still debounced — never land
    // after a shown failure.
    let superseded = HistogramLoader()
    let late = LoaderDeliveries()
    late.attach(superseded, failure: true)
    let started = LoaderFlag()
    superseded.request {
        started.set()
        usleep(300_000)
        throw DoctoredCoreError(message: "stale in-flight failure")
    }
    try pumpUntil({ started.isSet }, seconds: 5)
    superseded.request { nil }
    superseded.supersede()
    settle(0.8)
    try require(late.count == 0,
                "(d) superseded requests delivered nothing: \(late.failures) \(late.bins.count)")
}

func verifyPrecisionReaders() throws {
    let deep = try precisionFixture(width: 64, height: 48)
    defer { deep.close() }
    let controller = try editor(deep)
    guard let panel = controller.infoPanel else { throw ProbeFailure(description: "Info panel") }
    _ = panel.view
    try pumpUntil({ deep.retainedPrecisionProjection != nil }, seconds: 10)
    guard let retained = deep.retainedPrecisionProjection else {
        throw ProbeFailure(description: "(a) no retained typed projection")
    }

    // (a) Baseline: the readout reads the retained projection; off-canvas is nil.
    let reading = try PrecisionReadout.at((5, 5), document: deep)
    let expected = try retained.precisionSample(x: 5, y: 5)["mean_rgba16"] as? [Double]
    try require(reading != nil && reading?.codes == expected,
                "(a) PrecisionReadout.at equals the retained sample: \(reading?.codes ?? []) \(expected ?? [])")
    let far = try PrecisionReadout.at((5000, 5000), document: deep)
    try require(far == nil, "(a) (5000,5000) is nil")
    let corner = try PrecisionReadout.at((-1, -1), document: deep, reach: 1)
    try require(corner != nil, "(a) (-1,-1) reach 1 touches the canvas")
    let outside = try PrecisionReadout.at((-2, -2), document: deep, reach: 1)
    try require(outside == nil, "(a) (-2,-2) reach 1 is off the canvas")

    // (b) A held worker: the projection cannot refresh, and every reader names why.
    let held = try WorkerHolder()
    var released = false
    defer { if !released { try? held.releaseNow() } }
    deep.refreshProjection()
    guard let sentence = deep.projectionError else {
        throw ProbeFailure(description: "(b) the held worker left no projection error")
    }
    do {
        _ = try PrecisionReadout.at((5, 5), document: deep)
        throw ProbeFailure(description: "(b) PrecisionReadout.at did not throw")
    } catch let error as ProbeFailure {
        throw error
    } catch {
        try require(error.localizedDescription.contains(sentence),
                    "(b) PrecisionReadout.at throws S: \(error.localizedDescription)")
    }
    let heldFar = try PrecisionReadout.at((5000, 5000), document: deep)
    try require(heldFar == nil, "(b) off-canvas is still nil without throwing")
    panel.setPrecisionCursor((2, 2))
    try require(panel.readoutFailure?.contains(sentence) == true,
                "(b) the readout carries S: \(panel.readoutFailure ?? "nil")")
    try require(panel.valueLabels[.rgb]?.stringValue == "—", "(b) the rgb row reads —")
    var footnote = try footnoteText(panel)
    try require(footnote.contains(sentence) && footnote.contains("16-bit authoritative samples"),
                "(b) the footnote carries S after the fixed sentence: \(footnote)")
    let sampleText = controller.lastSampleText
    let alerts = AutomationAlerts.recorded.count
    controller.canvas.onEyedropper?(CGPoint(x: 2.5, y: 2.5))
    try require(controller.lastSampleText == sampleText && AutomationAlerts.recorded.count == alerts,
                "(b) the eyedropper sampled nothing and presented nothing")
    panel.requestScopedHistogram(scope: .composite)
    try require(panel.histogramFailure?.contains("background operation") == true,
                "(b) the histogram names its failure: \(panel.histogramFailure ?? "nil")")
    try require(panel.clippingLabel.stringValue == "Histogram unavailable",
                "(b) the clipping label: \(panel.clippingLabel.stringValue)")
    footnote = try footnoteText(panel)
    try require(footnote.contains("Histogram unavailable: \(panel.histogramFailure ?? "")"),
                "(b) the footnote names the histogram failure: \(footnote)")
    try chooseInfoSource(panel, tag: 1)
    try require(panel.readoutFailure?.contains("background operation") == true,
                "(b) the scoped cursor names its failure: \(panel.readoutFailure ?? "nil")")
    try require(panel.valueLabels[.rgb]?.stringValue == "—", "(b) the scoped rgb row reads —")
    let frozen = panel.lastPixel
    panel.requestScopedCursor((5000, 5000))
    try require(panel.lastPixel?.x == frozen?.x && panel.lastPixel?.y == frozen?.y,
                "(b) an off-canvas scoped cursor freezes")
    try chooseInfoSource(panel, tag: 0)

    // (c) Released: the next readout, scan and press clear the failures.
    try held.releaseNow()
    released = true
    try pumpUntil({ deep.retainedPrecisionProjection != nil }, seconds: 10)
    panel.setPrecisionCursor((3, 3))
    footnote = try footnoteText(panel)
    try require(panel.readoutFailure == nil && !footnote.contains(sentence),
                "(c) the next readout clears the failure: \(footnote)")
    panel.requestScopedHistogram(scope: .composite)
    try pumpUntil({ panel.histogramFailure == nil && panel.clippingLabel.stringValue.hasPrefix("Clipped") },
                  seconds: 10)
    controller.canvas.onEyedropper?(CGPoint(x: 2.5, y: 2.5))
    try require(controller.lastSampleText != sampleText, "(c) the eyedropper sampled")

    // (d) The loader's failure channel.
    try verifyLoaderFailures()

    // (e) The typed Evaluation sheet measures against the retained projection.
    guard let sheet = EvaluationSheetController(editor: controller) else {
        throw ProbeFailure(description: "(e) Evaluation sheet")
    }
    let before = storedValue(sheet, "retainedPrecisionBefore") as? RasterPrecisionImage
    try require(before != nil && before === deep.retainedPrecisionProjection,
                "(e) the sheet holds the retained typed projection")
    _ = sheet.view
    guard let popup = storedValue(sheet, "policyPopup") as? NSPopUpButton,
          let readout = storedValue(sheet, "readout") as? NSTextField,
          let renderer = storedValue(sheet, "renderer") as? PreviewRenderer,
          let item = popup.itemArray.first(where: {
              $0.representedObject as? String == EvaluationPolicy.linearLight.rawValue }),
          let action = popup.action else {
        throw ProbeFailure(description: "(e) Evaluation sheet controls")
    }
    defer {
        renderer.cancel()
        controller.canvas.previewImage = nil
    }
    popup.select(item)
    NSApp.sendAction(action, to: popup.target, from: popup)
    try pumpUntil({ readout.stringValue.hasPrefix("Against the current projection") }, seconds: 20)
    print("PASS: precision readers — the 16-bit eyedropper, Info readout, scoped cursor and "
        + "histogram name a read failure and freeze off-canvas; HistogramLoader delivers "
        + "failures, cancels and superseded requests correctly; the typed Evaluation sheet "
        + "measures against the retained projection")
}
