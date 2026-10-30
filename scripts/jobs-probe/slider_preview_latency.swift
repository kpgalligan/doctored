import AppKit

/// The opt-in measurement ("slider preview latency"): the time from a slider
/// change to the first visible preview, and to the exact (full-resolution)
/// preview, of the Surface Blur (r5 T15 → r10) and Gaussian Blur (σ2 → σ12)
/// sheets on a 6000 × 4000 layer at 8 and 16 bits, and of
/// the Reduce Noise (r1 → r2) and Dust & Scratches (r2 → r3) sheets on the
/// two 6000 × 4000 rows. It drives the real sheets
/// (`SliderSheetController.surfaceBlur`, `.gaussianBlur`,
/// `.reduceNoise`, `.dustAndScratches`) through their
/// own slider actions and Esc Cancel button, and watches the canvas through a
/// wrapped `onPreviewImageChange` that still calls the editor's own closure.
/// "Exact" is the first delivery whose width is the canvas's; any narrower
/// image is a proxy. It reports, and asserts only that every exact preview
/// lands: the bound is judged in the ADR, not here, so the same file measures
/// the code before and after a change. A 24 MP 16-bit preview was once
/// refused by the 2 GiB working limit (σ12 also by the legacy Gaussian's work
/// limit); the 16-bit 4000 × 3000 row measured then is gone under the 16 GB
/// minimum. The 16-bit row ends with the Apply stage
/// (`measureApply`), pressed at once and then, with the history cleared,
/// 3 s after the sheet is visible: a refused Apply is reported with the
/// exact stages. The 24 MP 16-bit Surface Blur and Gaussian σ2 previews and
/// both Applies land, and with the radius-independent legacy Gaussian so do
/// the σ12 stages; with Reduce Noise's streamed statistics the 24 MP Reduce
/// Noise and Dust & Scratches rows land at both depths: no FAIL is expected.
/// The 6000 × 4000 8-bit row ends with the same Apply stage, pressed
/// at once only (committed is the document's handle changing): the 8-bit exact
/// preview now holds a `SelectionWorkBudget` reservation, and Apply drains it
/// before admitting its own plan, so the stage FAILs unless
/// `SelectionWorkBudget.scratchBytes` is back at its pre-sheet value the moment
/// the Apply action returns, before any run-loop turn. The 8-bit
/// Apply of the three titled sheets (Surface Blur, Reduce Noise, Dust &
/// Scratches) runs as a cancellable job: an `ApplyWatch` timer clicks the
/// progress panel's Cancel in a first Apply (nothing may change), then watches
/// the immediate Apply's job (its worker gap, the panel's progress).
/// Gaussian Blur is the fourth titled sheet (`dr_image_filter_op` `blur`), and
/// every 8-bit Apply's write and first projection run on that job's worker.
/// The line splits the `Apply gap` into the main thread's stretch before the
/// job (about 270 ms at every filter: the sheet's own dismissal in
/// `dismiss(self)`, AppKit's closing animation, which is the row's largest
/// gap both before and after the write moved to the worker) and the one after
/// it (the publication, which held the write and the projection before that
/// move); both stretches are printed
/// and neither is bounded here, because a wall-clock bound is not a verdict
/// this default code asserts. Both
/// Surface Blur rows (8- and 16-bit 6000 × 4000) end with the busy-slot
/// stage (`measureBusySlot`): an anonymous `WorkerHolder` held 0.5 s is waited
/// for, and one held past `BackgroundWork.anonymousWaitLimit` keeps the sheet
/// open with `BackgroundWork.busyMessage`; the 16-bit row adds case (i) on the
/// Levels sheet (`PrecisionAdjustmentSession.commit(op:)`).
func verifySliderPreviewLatency() throws {
    print("INFO: slider preview latency: working limit \(RasterResources.shared.workingLimit) B")
    try warmSheetPresentation()
    var missing: [String] = []
    for (depth, width, height) in [(8, 6000, 4000), (16, 6000, 4000)] {
        // One document per sheet, inside its own pool: a dismissed sheet's
        // window is never waited on, and a closed 24 MP document is released
        // before the next one is charged against the working limit (AppKit
        // frees the closed window, and with it the document, on a later run
        // loop turn, so the next row waits for the retained set to return).
        for sheet in measuredSheets() {
            let filter = sheet.filter
            let entry = try retainedBytes()
            try autoreleasepool {
                let document = depth == 8
                    ? try fixture(width: width, height: height)
                    : try precisionFixture(width: width, height: height)
                defer { document.close() }
                let controller = try editor(document)
                let make: () -> NSViewController = { sheet.make(document, controller.canvas) }
                missing += try measureSliderPreview(controller, depth: depth, filter: filter,
                                                    one: sheet.one, four: sheet.four, make: make)
                let baseline = try retainedBytes()
                // Is the 16-bit Apply admitted at this size, pressed at
                // once and pressed 3 s after the sheet is visible?
                // The 8-bit Apply: cancelled first, then pressed at once.
                if depth == 8 {
                    if let title = sheet.title {
                        missing += try measureApplyCancel(controller, filter: filter, title: title, make: make)
                    }
                    missing += try measureApply(controller, depth: depth, filter: filter, delay: 0,
                                                title: sheet.title, make: make)
                }
                if depth == 16 {
                    missing += try measureApply(controller, depth: depth, filter: filter, delay: 0, make: make)
                    // The first Apply's history entry holds the pre-Apply
                    // source; the second Apply is measured against the same
                    // retained set.
                    document.undoManager?.removeAllActions()
                    try settleRetained(at: baseline, "before the second \(filter) Apply")
                    missing += try measureApply(controller, depth: depth, filter: filter, delay: 3, make: make)
                }
                // The busy-slot stage, Surface Blur on both rows.
                if filter == "surface blur" {
                    document.undoManager?.removeAllActions()
                    try settleRetained(at: baseline, "before the \(depth)-bit busy-slot stage")
                    missing += try measureBusySlot(controller, depth: depth, filter: filter, baseline: baseline,
                                                   make: make)
                    if depth == 16 {
                        document.undoManager?.removeAllActions()
                        try settleRetained(at: baseline, "before the 16-bit Levels busy-slot case")
                        let levels: () -> NSViewController = {
                            LevelsAdjustmentEditor(document: document, canvas: controller.canvas, mode: .destructive)
                        }
                        missing += try measureSlotHeld(controller, depth: depth, filter: "levels", make: levels,
                                                       prepare: setLevelsBlack)
                    }
                }
            }
            try settleRetained(at: entry, "after the \(filter) document closed")
        }
    }
    try require(missing.isEmpty, "exact previews or Applies that never landed: \(missing.joined(separator: "; "))")
}

/// One measured sheet: its name, its first slider's one change and four-change
/// burst (from the sheet's opening value), and its constructor.
private struct MeasuredSheet {
    let filter: String
    /// The Apply job's panel title (the sheet's `actionName`); nil for a
    /// sheet whose 8-bit Apply is synchronous (none now).
    let title: String?
    let one: Double
    let four: [Double]
    let make: (ImageDocument, ImageCanvasView) -> NSViewController
}

private func measuredSheets() -> [MeasuredSheet] { [
    MeasuredSheet(filter: "surface blur", title: "Surface Blur", one: 10, four: [7, 8, 9, 10],
                  make: { SliderSheetController.surfaceBlur(document: $0, canvas: $1) }),
    MeasuredSheet(filter: "gaussian blur", title: "Gaussian Blur", one: 12, four: [9, 10, 11, 12],
                  make: { SliderSheetController.gaussianBlur(document: $0, canvas: $1) }),
    MeasuredSheet(filter: "reduce noise", title: "Reduce Noise", one: 2, four: [1.25, 1.5, 1.75, 2],
                  make: { SliderSheetController.reduceNoise(document: $0, canvas: $1) }),
    MeasuredSheet(filter: "dust & scratches", title: "Dust & Scratches", one: 3, four: [2.25, 2.5, 2.75, 3],
                  make: { SliderSheetController.dustAndScratches(document: $0, canvas: $1) }),
] }

/// The Apply stage (the 8-bit row too, delay 0): a fresh sheet of the same kind on the
/// same document, its Apply button pressed at the opening value (σ2; r5 T15;
/// Reduce Noise r1; Dust & Scratches r2)
/// `delay` seconds after the sheet is visible ("immediate" at 0). Committed is
/// the document's precision handle changing (at 8 bits its handle); refused is the background job
/// ending with no change, named by the alert it recorded or the sheet's status
/// line. At 8 bits an `ApplyWatch` watches the press; a titled sheet
/// FAILs unless its job ran on the worker with the preview registry back at
/// its pre-sheet value, no worker gap above 250 ms and a moving progress bar.
private func measureApply(
    _ controller: EditorViewController, depth: Int, filter: String, delay: TimeInterval,
    title: String? = nil, make: () -> NSViewController
) throws -> [String] {
    guard let document = controller.document, let canvasWidth = document.doc?.width,
          let canvasHeight = document.doc?.height
    else { throw ProbeFailure(description: "[slider preview latency] no document") }
    let tag = "[slider preview latency, \(canvasWidth)×\(canvasHeight) \(depth)-bit \(filter)]"
    let label = delay == 0 ? "immediate" : "\(Int(delay)) s"
    let before = document.doc?.precision
    let beforeDoc = document.doc
    let committed = { depth == 8 ? document.doc !== beforeDoc : document.doc?.precision !== before }
    let alerts = AutomationAlerts.recorded.count
    // The preview registry before the sheet exists.
    let registered = SelectionWorkBudget.scratchBytes
    let sheet = make()
    controller.presentAsSheet(sheet)
    try pumpUntil({ sheet.view.window?.isVisible == true }, seconds: 30)
    let visible = Date()
    while Date().timeIntervalSince(visible) < delay { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }),
          let action = apply.action else {
        throw ProbeFailure(description: "\(tag) the sheet has no Apply button")
    }
    let watch = depth == 8 ? ApplyWatch(title: title, clickCancel: false) : nil
    watch?.start()
    let pressed = Date()
    try require(NSApp.sendAction(action, to: apply.target, from: apply), "\(tag) Apply was delivered")
    let returned = Date()
    watch?.stop()
    // Read before any run-loop turn — Apply drained the preview first.
    var outlived: [String] = []
    if depth == 8, delay == 0, SelectionWorkBudget.scratchBytes != registered {
        let message = "\(tag) apply (immediate): preview reservation outlived drain"
        fputs("FAIL \(message): \(SelectionWorkBudget.scratchBytes) B registered, \(registered) B before the sheet\n",
              stderr)
        outlived.append(message)
    }
    // Ends on a precision change or an alert naming a refusal; otherwise once
    // the worker is idle at least 3 s after the press (the sheet's own preview
    // work must not read as the Apply job, nor a late enqueue as a refusal).
    try pumpUntil({
        if committed() { return true }
        if AutomationAlerts.recorded.count > alerts { return true }
        return !BackgroundWork.occupied && Date().timeIntervalSince(pressed) >= 3
    }, seconds: 600)
    if committed() {
        let elapsed = Int((Date().timeIntervalSince(pressed) * 1000).rounded())
        guard let watch else {
            print("INFO: slider preview latency \(tag) apply (\(label)): committed in \(elapsed) ms")
            return outlived
        }
        let applyGap = milliseconds(watch.applyGap(pressed: pressed, returned: returned))
        guard title != nil else {
            print("INFO: slider preview latency \(tag) apply (\(label)): committed in \(elapsed) ms, "
                  + "Apply gap \(applyGap) ms")
            return outlived
        }
        let sides = watch.jobSides(pressed: pressed, returned: returned)
        print("INFO: slider preview latency \(tag) apply (\(label)): committed in \(elapsed) ms, "
              + "job gap \(milliseconds(watch.jobGap)) ms, Apply gap \(applyGap) ms "
              + "(before the job \(milliseconds(sides.before)) ms, after the job "
              + "\(milliseconds(sides.after)) ms), progress max \(watch.maxProgress)")
        var failures = outlived
        let check = { (ok: Bool, what: String) in
            if !ok { failures.append(fail("\(tag) apply (\(label)): \(what)")) }
        }
        check(watch.sawOccupied, "the job never occupied the worker")
        check(watch.registryAtFirstOccupied == registered,
              "the preview registry read \(watch.registryAtFirstOccupied.map(String.init) ?? "nothing") B "
              + "at the job's first tick, \(registered) B before the sheet")
        check(watch.jobGap <= 0.25, "job gap \(milliseconds(watch.jobGap)) ms > 250 ms")
        check(watch.maxProgress > 0, "the progress bar never moved")
        return failures
    }
    let status = fields(sheet.view).first { $0.accessibilityIdentifier() == "tone.preview.status" }?
        .stringValue ?? ""
    let reason = AutomationAlerts.recorded.dropFirst(alerts).last
        .map { "[AutomationAlerts] \($0.messageText) | \($0.informativeText)" }
        ?? (status.isEmpty ? "no change and no alert" : status)
    print("INFO: slider preview latency \(tag) apply (\(label)) refused: \(reason)")
    if sheet.presentingViewController != nil { controller.dismiss(sheet) }
    return outlived + ["\(tag) apply (\(label)): \(reason)"]
}

private func retainedBytes() throws -> UInt64 {
    try PrecisionDocumentDescriptor.integer(RasterResources.shared.report(), "unique_retained_bytes")
}

/// Pumps (at most 30 s) until the retained set is back within 1 MiB of
/// `baseline`; one that does not settle is reported, not failed.
private func settleRetained(at baseline: UInt64, _ moment: String) throws {
    let deadline = Date().addingTimeInterval(30)
    var retained = try retainedBytes()
    while max(retained, baseline) - min(retained, baseline) > 1 << 20, Date() < deadline {
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        retained = try retainedBytes()
    }
    if max(retained, baseline) - min(retained, baseline) > 1 << 20 {
        print("INFO: slider preview latency: retained \(retained) B did not return "
              + "within 1 MiB of \(baseline) B \(moment)")
    }
}

/// The first sheet a process presents takes about a second longer to become
/// visible (measured: 1074 ms against about 300 ms after it), which would be
/// charged to whichever filter is measured first. One throwaway Hue Rotate
/// sheet on a small layer pays it before any measurement.
private func warmSheetPresentation() throws {
    try autoreleasepool {
        let document = try fixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        let sheet = SliderSheetController.hueRotate(document: document, canvas: controller.canvas)
        controller.presentAsSheet(sheet)
        try pumpUntil({ sheet.view.window?.isVisible == true && controller.canvas.previewImage != nil },
                      seconds: 30)
        guard let cancel = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }),
              let action = cancel.action else {
            throw ProbeFailure(description: "[slider preview latency] the warm-up sheet has no Cancel button")
        }
        try require(NSApp.sendAction(action, to: cancel.target, from: cancel),
                    "[slider preview latency] the warm-up Cancel was delivered")
    }
}

private func fields(_ view: NSView) -> [NSTextField] {
    (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
}

private func sliders(_ view: NSView) -> [NSSlider] {
    (view as? NSSlider).map { [$0] } ?? view.subviews.flatMap(sliders)
}

/// Measures one sheet; answers the stages whose exact preview never landed
/// (a 16-bit preview the working limit refuses says so in its status line).
private func measureSliderPreview(
    _ controller: EditorViewController, depth: Int, filter: String,
    one: Double, four: [Double], make: () -> NSViewController
) throws -> [String] {
    let canvas = controller.canvas
    guard let canvasWidth = controller.document?.doc?.width, let canvasHeight = controller.document?.doc?.height
    else { throw ProbeFailure(description: "[slider preview latency] no document") }
    let tag = "[slider preview latency, \(canvasWidth)×\(canvasHeight) \(depth)-bit \(filter)]"
    var deliveries: [(time: Date, width: Int)] = []
    let previous = canvas.onPreviewImageChange
    canvas.onPreviewImageChange = { image in
        previous?(image)
        if let image { deliveries.append((Date(), image.width)) }
    }
    defer { canvas.onPreviewImageChange = previous }
    let ms = { (from: Date, to: Date) in Int((to.timeIntervalSince(from) * 1000).rounded()) }
    let full = { (after: Date) in deliveries.filter { $0.time >= after && $0.width == canvasWidth } }
    let sheet = make()
    let status = { fields(sheet.view).first { $0.accessibilityIdentifier() == "tone.preview.status" }?
        .stringValue ?? "" }
    var missing: [String] = []
    var visible: Date?
    // One stage: first-visible is the first delivery of any size after
    // `start`, exact the first full-size one (`final`: the last, once none has
    // arrived for `quiet` after `last`). A refusal ends the stage once a proxy
    // has had 3 s to land.
    func stage(_ name: String, start: Date, last: Date, quiet: Double?) throws -> TimeInterval? {
        let deadline = Date().addingTimeInterval(600)
        while true {
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
            if visible == nil, sheet.view.window?.isVisible == true { visible = Date() }
            let exacts = full(start)
            if let quiet {
                let since = exacts.last?.time ?? last
                if let end = exacts.last, end.time >= last, Date().timeIntervalSince(since) >= quiet { break }
            } else if !exacts.isEmpty { break }
            if !status().isEmpty, Date().timeIntervalSince(last) > 3 { break }
            if Date() > deadline { throw ProbeFailure(description: "\(tag) \(name): timed out") }
        }
        let first = deliveries.first { $0.time >= start }
        let seen = first.map { "\(ms(start, $0.time)) ms (\($0.width) px wide)" } ?? "none"
        let exacts = full(start)
        guard let exact = quiet == nil ? exacts.first : exacts.last, exact.time >= last else {
            print("INFO: slider preview latency \(tag) \(name): first-visible \(seen), "
                  + "exact refused: \(status())")
            missing.append("\(tag) \(name): \(status())")
            return nil
        }
        let label = quiet == nil ? "exact" : "final exact"
        let tail = quiet == nil ? "" : " (\(ms(last, exact.time)) ms after the last change; "
            + "\(exacts.count) full-size deliveries)"
        print("INFO: slider preview latency \(tag) \(name): first-visible \(seen), "
              + "\(label) \(ms(start, exact.time)) ms\(tail)")
        return exact.time.timeIntervalSince(start)
    }

    let opened = Date()
    controller.presentAsSheet(sheet)
    _ = try stage("opening", start: opened, last: opened, quiet: nil)
    print("INFO: slider preview latency \(tag) opening: the sheet became visible within "
          + "\(visible.map { "\(ms(opened, $0)) ms" } ?? "the stage")")
    guard let slider = sliders(sheet.view).first else {
        throw ProbeFailure(description: "\(tag) the sheet has no slider")
    }
    let change = { (value: Double) in
        slider.doubleValue = value
        slider.sendAction(slider.action, to: slider.target)
    }
    let changed = Date()
    change(one)
    let single = try stage("one change", start: changed, last: changed, quiet: nil)

    let burst = Date()
    var last = burst
    for (index, value) in four.enumerated() {
        if index > 0 { RunLoop.main.run(until: Date().addingTimeInterval(0.1)) }
        last = Date()
        change(value)
    }
    _ = try stage("four changes 100 ms apart", start: burst, last: last,
                  quiet: max(5, 1.5 * (single ?? 0)))

    guard let cancel = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }),
          let action = cancel.action else {
        throw ProbeFailure(description: "\(tag) the sheet has no Esc Cancel button")
    }
    try require(NSApp.sendAction(action, to: cancel.target, from: cancel), "\(tag) Cancel was delivered")
    try require(canvas.previewImage == nil, "\(tag) Cancel clears the preview")
    if missing.isEmpty { print("PASS: slider preview latency \(tag) — every exact preview landed") }
    return missing
}

private func milliseconds(_ interval: TimeInterval) -> Int { Int((interval * 1000).rounded()) }

/// Prints a FAIL line and answers its message for the case's list.
private func fail(_ message: String) -> String {
    fputs("FAIL \(message)\n", stderr)
    return message
}

private func progressBars(_ view: NSView) -> [NSProgressIndicator] {
    (view as? NSProgressIndicator).map { [$0] } ?? view.subviews.flatMap(progressBars)
}

/// One Apply press seen from the run loop. A 10 ms timer in `.common`
/// and `.modalPanel` (so it fires inside `BackgroundWork.wait`'s modal
/// session) records each tick and whether the worker was occupied, the
/// preview registry at the first occupied tick, and the largest progress of
/// the visible panel titled `title`; with `clickCancel` it clicks that panel's
/// Cancel at the first occupied tick whose bar reads above 0.
private final class ApplyWatch {
    private let title: String?
    private let clickCancel: Bool
    private var timer: Timer?
    private(set) var ticks: [(time: Date, occupied: Bool)] = []
    private(set) var registryAtFirstOccupied: UInt64?
    private(set) var maxProgress: Double = 0
    private(set) var clickedAt: Date?
    var sawOccupied: Bool { ticks.contains { $0.occupied } }

    init(title: String?, clickCancel: Bool) {
        self.title = title
        self.clickCancel = clickCancel
    }

    func start() {
        let timer = Timer(timeInterval: 0.01, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        self.timer = timer
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    /// The largest interval between two consecutive ticks that both saw the
    /// worker occupied.
    var jobGap: TimeInterval {
        zip(ticks, ticks.dropFirst()).filter { $0.occupied && $1.occupied }
            .map { $1.time.timeIntervalSince($0.time) }.max() ?? 0
    }

    /// The largest gap among the press, the ticks and the action's return.
    func applyGap(pressed: Date, returned: Date) -> TimeInterval {
        let times = [pressed] + ticks.map(\.time).filter { $0 >= pressed && $0 <= returned } + [returned]
        return zip(times, times.dropFirst()).map { $1.timeIntervalSince($0) }.max() ?? 0
    }

    /// The main thread's two unticked stretches around the job: from the
    /// press to the first tick (the timer first fires inside
    /// `BackgroundWork.wait`'s modal session, so this is what Apply does
    /// before the job starts: the drain, the sheet's dismissal, admission),
    /// and from the last tick to the action's return (what it does after the
    /// job: the publication). Both are the whole press-to-return time when no
    /// tick fell between them.
    func jobSides(pressed: Date, returned: Date) -> (before: TimeInterval, after: TimeInterval) {
        let inside = ticks.map(\.time).filter { $0 >= pressed && $0 <= returned }
        guard let first = inside.first, let last = inside.last else {
            let whole = returned.timeIntervalSince(pressed)
            return (whole, whole)
        }
        return (first.timeIntervalSince(pressed), returned.timeIntervalSince(last))
    }

    private func tick() {
        let now = Date(), occupied = BackgroundWork.occupied
        if occupied, !sawOccupied { registryAtFirstOccupied = SelectionWorkBudget.scratchBytes }
        ticks.append((now, occupied))
        guard let title,
              let panel = NSApp.windows.first(where: { $0 is NSPanel && $0.title == title && $0.isVisible }),
              let content = panel.contentView, let bar = progressBars(content).first else { return }
        maxProgress = max(maxProgress, bar.doubleValue)
        guard clickCancel, clickedAt == nil, occupied, bar.doubleValue > 0,
              let cancel = buttons(content).first(where: { $0.title == "Cancel" && !$0.isHidden }),
              let action = cancel.action else { return }
        clickedAt = now
        _ = NSApp.sendAction(action, to: cancel.target, from: cancel)
    }
}

/// Presses a sheet's Apply (its Return button).
private func pressApply(_ sheet: NSViewController, _ tag: String) throws {
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }),
          let action = apply.action else {
        throw ProbeFailure(description: "\(tag) the sheet has no Apply button")
    }
    try require(NSApp.sendAction(action, to: apply.target, from: apply), "\(tag) Apply was delivered")
}

/// The cancel stage: a fresh 8-bit sheet's Apply, with the watch clicking
/// the job panel's Cancel. FAILs unless the click happened and nothing
/// changed: the document's handle and revision, no undo step and a clean
/// document, no alert, a free worker slot with no scratch, and the preview
/// registry back at its pre-sheet value.
private func measureApplyCancel(
    _ controller: EditorViewController, filter: String, title: String, make: () -> NSViewController
) throws -> [String] {
    guard let document = controller.document, let canvasWidth = document.doc?.width,
          let canvasHeight = document.doc?.height
    else { throw ProbeFailure(description: "[slider preview latency] no document") }
    let tag = "[slider preview latency, \(canvasWidth)×\(canvasHeight) 8-bit \(filter)]"
    let beforeDoc = document.doc, beforeRevision = document.revision
    let alerts = AutomationAlerts.recorded.count
    let registered = SelectionWorkBudget.scratchBytes
    let sheet = make()
    controller.presentAsSheet(sheet)
    try pumpUntil({ sheet.view.window?.isVisible == true }, seconds: 30)
    let watch = ApplyWatch(title: title, clickCancel: true)
    watch.start()
    try pressApply(sheet, tag)
    let returned = Date()
    watch.stop()
    var failures: [String] = []
    let check = { (ok: Bool, what: String) in
        if !ok { failures.append(fail("\(tag) apply (cancel): \(what)")) }
    }
    check(watch.clickedAt != nil, "the job panel's Cancel was never clicked")
    check(document.doc === beforeDoc && document.revision == beforeRevision, "the document changed")
    check(document.undoManager?.canUndo == false && !document.isDocumentEdited,
          "an undo step or a dirty document")
    check(AutomationAlerts.recorded.count == alerts, "an alert was recorded")
    check(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0, "the worker slot is still held")
    check(SelectionWorkBudget.scratchBytes == registered,
          "\(SelectionWorkBudget.scratchBytes) B registered, \(registered) B before the sheet")
    if sheet.view.window?.isVisible == true { controller.dismiss(sheet) }
    let cancelled = watch.clickedAt.map { "\(milliseconds(returned.timeIntervalSince($0))) ms" } ?? "no click"
    print("INFO: slider preview latency \(tag) apply (cancel): cancel→return \(cancelled)")
    return failures
}

/// The busy-slot stage on one row: case (i) through `measureSlotHeld`, then
/// case (ii) — Apply pressed while a `WorkerHolder` keeps the slot past
/// `BackgroundWork.anonymousWaitLimit` must keep the sheet open, unchanged,
/// with `busyMessage` on its status line, and a second Apply after the holder
/// leaves must commit and dismiss it.
private func measureBusySlot(
    _ controller: EditorViewController, depth: Int, filter: String, baseline: UInt64,
    make: () -> NSViewController
) throws -> [String] {
    var failures = try measureSlotHeld(controller, depth: depth, filter: filter, make: make)
    guard let document = controller.document, let canvasWidth = document.doc?.width,
          let canvasHeight = document.doc?.height
    else { throw ProbeFailure(description: "[slider preview latency] no document") }
    document.undoManager?.removeAllActions()
    try settleRetained(at: baseline, "before the \(depth)-bit slot-held-past-limit case")
    let tag = "[slider preview latency, \(canvasWidth)×\(canvasHeight) \(depth)-bit \(filter)]"
    let sheet = make()
    controller.presentAsSheet(sheet)
    try pumpUntil({ sheet.view.window?.isVisible == true }, seconds: 30)
    let slider = sliders(sheet.view).first
    let value = slider?.doubleValue
    let beforeDoc = document.doc, beforePrecision = document.doc?.precision
    let beforeRevision = document.revision
    let committed = { depth == 8 ? document.doc !== beforeDoc : document.doc?.precision !== beforePrecision }
    let alerts = AutomationAlerts.recorded.count
    let holder = try WorkerHolder()
    defer { if !holder.taken { try? holder.releaseNow() } }
    let pressed = Date()
    try pressApply(sheet, tag)
    let returned = Date()
    let check = { (ok: Bool, what: String) in
        if !ok { failures.append(fail("\(tag) apply (slot held past limit): \(what)")) }
    }
    let status = fields(sheet.view).first { $0.accessibilityIdentifier() == "tone.preview.status" }?
        .stringValue ?? ""
    check(returned.timeIntervalSince(pressed) >= BackgroundWork.anonymousWaitLimit - 0.1,
          "returned after \(milliseconds(returned.timeIntervalSince(pressed))) ms")
    // A dismissed sheet stays in this process's `presentedViewControllers`
    // (see the README), so open is also its window still visible.
    check(sheet.presentingViewController != nil && sheet.view.window?.isVisible == true,
          "the sheet was dismissed")
    check(status == BackgroundWork.busyMessage, "the status line reads \"\(status)\"")
    check(document.doc === beforeDoc && document.revision == beforeRevision, "the document changed")
    check(document.undoManager?.canUndo == false, "an undo step was added")
    check(AutomationAlerts.recorded.count == alerts, "an alert was recorded")
    check(slider?.doubleValue == value, "the first slider moved")
    try holder.releaseNow()
    let again = Date()
    try pressApply(sheet, tag)
    let landed = (try? pumpUntil({ committed() && sheet.view.window?.isVisible != true }, seconds: 30)) != nil
    check(landed, "the second Apply did not commit and dismiss the sheet")
    if sheet.view.window?.isVisible == true { controller.dismiss(sheet) }
    print("INFO: slider preview latency \(tag) apply (slot held past limit): kept open after "
          + "\(milliseconds(returned.timeIntervalSince(pressed))) ms; re-Apply committed in "
          + "\(milliseconds(Date().timeIntervalSince(again))) ms")
    return failures
}

/// The busy-slot case (i): a fresh sheet, a `WorkerHolder` released 0.5 s
/// after Apply is pressed. FAILs unless Apply commits (within 30 s) after the
/// holder left, at least 400 ms after the press, with no alert.
private func measureSlotHeld(
    _ controller: EditorViewController, depth: Int, filter: String, make: () -> NSViewController,
    prepare: ((NSViewController) -> Void)? = nil
) throws -> [String] {
    guard let document = controller.document, let canvasWidth = document.doc?.width,
          let canvasHeight = document.doc?.height
    else { throw ProbeFailure(description: "[slider preview latency] no document") }
    let tag = "[slider preview latency, \(canvasWidth)×\(canvasHeight) \(depth)-bit \(filter)]"
    let sheet = make()
    controller.presentAsSheet(sheet)
    try pumpUntil({ sheet.view.window?.isVisible == true }, seconds: 30)
    prepare?(sheet)
    let beforeDoc = document.doc, beforePrecision = document.doc?.precision
    let committed = { depth == 8 ? document.doc !== beforeDoc : document.doc?.precision !== beforePrecision }
    let alerts = AutomationAlerts.recorded.count
    let holder = try WorkerHolder()
    defer { if !holder.taken { try? holder.releaseNow() } }
    holder.release(after: 0.5)
    let pressed = Date()
    try pressApply(sheet, tag)
    let landed = (try? pumpUntil(committed, seconds: 30)) != nil
    let elapsed = Date().timeIntervalSince(pressed)
    var failures: [String] = []
    let check = { (ok: Bool, what: String) in
        if !ok { failures.append(fail("\(tag) apply (slot held 0.5 s): \(what)")) }
    }
    check(landed, "Apply never committed")
    check(holder.taken, "the holder was never taken")
    check(AutomationAlerts.recorded.count == alerts, "an alert was recorded")
    check(elapsed >= 0.4, "committed \(milliseconds(elapsed)) ms after the press, before the holder left")
    if sheet.view.window?.isVisible == true { controller.dismiss(sheet) }
    print("INFO: slider preview latency \(tag) apply (slot held 0.5 s): committed in \(milliseconds(elapsed)) ms")
    return failures
}

/// The Levels sheet's Black point at 0.1, so its Apply changes pixels.
private func setLevelsBlack(_ sheet: NSViewController) {
    guard let black = sliders(sheet.view).first(where: { $0.accessibilityIdentifier() == "levels-black" })
    else { return }
    black.doubleValue = 0.1
    black.sendAction(black.action, to: black.target)
}
