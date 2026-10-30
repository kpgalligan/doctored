import AppKit

// Final raster transforms (Free Transform, Distort, `transform_layers`, Warp and the 16-bit
// move/⌘T/crop gestures) carry a `RasterJobControl`, as `DocumentJobs`' jobs do, so Cancel/Esc
// stops the core kernel; save/export also uses the `BackgroundWork` queue with responsive,
// noncancellable progress until its kernels expose cancellation. **A user cancel is
// `JobCancelled`** (this file): `BackgroundWork.wait` maps a failed task under a cancelled control
// to it, it bridges to `NSUserCancelledError` so no `presentError` alerts (the automation hook
// mirrors AppKit), a caller that keeps a session open catches it by type, and new code throws it
// instead of writing its own "…cancelled." sentence. Compound RAW development checks cancellation
// between platform camera calls and through native mask/composition stages; an individual Core
// Image render remains noncancellable. Workers capture values only, never an `ImageDocument`,
// controller or other AppKit state. `RasterDocument`/`RasterImage` pure reads may overlap; core
// retained caches and the Swift ICC memo are synchronized. The owner must retain every handle until
// work finishes.
//
// An HTTP MCP call waits on the serve thread between main hops, so events keep dispatching;
// direct main callers pump `.default` in place.
//
// `BackgroundWork.wait(…, revealAfter:)` (`PrecisionCapturedWork.run` forwards it) checks its
// panel out hidden — `JobProgressController.checkout(…, hidden: true)`: alpha 0 and click-through
// before it is presented — and `reveal()`s it once that long has passed since entry; the modal session and the
// Esc monitor are the same either way, and every caller that passes nothing keeps the immediate
// panel. A wait does not build a panel: `checkout` reuses an idle one from a pool of at most two,
// rebuilding its card only when the title or cancellability changed, and `checkin` orders it out
// and drops its cancel action; `DocumentJobs.poll`'s nonmodal panel still builds its own.
// `PrecisionGeometryGesture.finishOutcome` (every typed Move, ⌘T, ⌥⌘T and Crop commit, UI and MCP)
// and `TransformWork.wait` (8-bit ⌘T and ⌥⌘T, `transform_layers` at both depths,
// `AgentServer+TransformJobs` and both Warp callers) pass `DocumentJobs.panelDelay`.
// `JobProgressController`'s panel sets `window.animationBehavior = .none` before it is ordered in,
// and that line is load-bearing: a titled `NSPanel`'s default appearance animation is a THREADED
// `NSAnimation`, which parked one `-[NSAnimation _runBlocking]` thread on the user-interactive root
// queue PER PRESENTATION until the 64-thread dispatch soft limit was reached and
// `BackgroundWork.wait` could never be scheduled again (measured with `sample`: 3 parked threads
// per `place_image` before, flat across 200 consecutive worker-backed edits after).

/// The final-operation worker seam. One admitted operation app-wide bounds both
/// retained inputs and scratch. Workers capture immutable values only; AppKit,
/// admission, result consumption and document commit stay on the main thread.
enum BackgroundWork {
    static let queue = DispatchQueue(label: "com.kgalligan.doctored.final-work", qos: .userInitiated)
    static var occupied = false
    static let scratch = JobScratch()
    static var scratchEstimate: UInt64 {
        get { scratch.bytes }
        set { scratch.bytes = newValue }
    }
    static var isWaiting = false

    static func wait<T>(_ task: WorkerTask<T>, title: String,
                        control: RasterJobControl? = nil,
                        revealAfter: TimeInterval = 0) throws -> T {
        try wait(title: title, control: control, revealAfter: revealAfter) { task }
    }

    /// Prepare the real progress/Cancel UI before dispatching a new worker.
    /// The main-thread factory must return its WorkerTask immediately: never
    /// start a task then throw, which would abandon an unconsumed mailbox.
    /// Existing-task callers above retain their original scheduling behavior.
    /// `revealAfter > 0` builds the panel hidden and reveals it only once that
    /// long has passed since entry, so a short wait never flashes; the
    /// modal session and the Esc monitor are the same either way.
    static func wait<T>(title: String, control: RasterJobControl? = nil,
                        revealAfter: TimeInterval = 0,
                        makeTask: () throws -> WorkerTask<T>) throws -> T {
        assert(Thread.isMainThread)
        let entered = ProcessInfo.processInfo.systemUptime
        let previousWaiting = isWaiting
        let panel = JobProgressController.checkout(title: title,
            cancel: control.map { token in { token.cancel() } }, hidden: revealAfter > 0)
        var hidden = revealAfter > 0
        let session = NSApp.beginModalSession(for: panel.window)
        // Esc cancels a cancellable modal wait wherever the key press lands:
        // the panel is never made key, so it cannot take the key equivalent
        // itself. Scoped to this wait; the nonmodal DocumentJobs panel is
        // untouched.
        let escapeMonitor: Any? = control == nil ? nil :
            NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak panel] event in
                guard event.keyCode == 53,
                      event.modifierFlags.intersection([.command, .option, .control]).isEmpty
                else { return event }
                panel?.requestCancel()
                return nil
            }
        isWaiting = true
        defer {
            if let escapeMonitor { NSEvent.removeMonitor(escapeMonitor) }
            isWaiting = previousWaiting
            NSApp.endModalSession(session)
            JobProgressController.checkin(panel)
        }
        let task = try makeTask()
        while true {
            if let result = task.take() {
                // Under a cancelled control a failed task IS the cancel,
                // whatever sentence the worker's kernel phrased it in. A
                // success after Cancel stays a success.
                if case .failure(let error) = result, control?.isCancelled == true {
                    throw error as? JobCancelled ?? JobCancelled(kernelMessage: error.localizedDescription)
                }
                // One more frame from the control before a successful return,
                // so the panel's last frame is the job's final progress and
                // not whatever the 5 ms poll last happened to read.
                if case .success = result, let control { panel.update(control: control) }
                return try result.get()
            }
            panel.update(control: control)
            if hidden, ProcessInfo.processInfo.systemUptime - entered >= revealAfter {
                hidden = false
                panel.reveal()
            }
            if NSApp.runModalSession(session) != .continue { control?.cancel() }
            // Bounded mailbox polling services AppKit without dispatch-sync
            // dependency on the main queue (including MCP's nested trampoline).
            Thread.sleep(forTimeInterval: 0.005)
        }
    }
}

/// The one error a user cancel surfaces as. It bridges to
/// `NSUserCancelledError`, so `presentError` shows nothing (AppKit skips that
/// code, and the automation hook mirrors it); a caller that keeps a session
/// open catches it by type. New code throws this rather than writing its own
/// "…cancelled." sentence.
struct JobCancelled: LocalizedError, CustomNSError {
    static let message = "Operation cancelled; no edit was applied."
    /// The worker's own failure text when the cancel surfaced as a failed task.
    var kernelMessage: String? = nil
    var errorDescription: String? { Self.message }
    static var errorDomain: String { NSCocoaErrorDomain }
    var errorCode: Int { NSUserCancelledError }
    var errorUserInfo: [String: Any] { [NSLocalizedDescriptionKey: Self.message] }
}

final class WorkerTask<Value> {
    private let lock = NSLock()
    private var result: Result<Value, Error>?
    private var consumed = false

    init(scratchEstimate: UInt64 = 0, _ compute: @escaping () throws -> Value) throws {
        assert(Thread.isMainThread)
        guard !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: BackgroundWork.busyMessage)
        }
        BackgroundWork.occupied = true
        BackgroundWork.scratchEstimate = scratchEstimate
        BackgroundWork.queue.async { [self] in
            let completed: Result<Value, Error> = autoreleasepool {
                AppActivity.userInitiated("processing an image") { Result { try compute() } }
            }
            lock.lock()
            result = completed
            lock.unlock()
        }
    }

    func take() -> Result<Value, Error>? {
        assert(Thread.isMainThread)
        guard !consumed else { return nil }
        lock.lock()
        let completed = result
        result = nil
        lock.unlock()
        if completed != nil {
            consumed = true
            BackgroundWork.occupied = false
            BackgroundWork.scratchEstimate = 0
        }
        return completed
    }
}

/// Nonmodal for interactive edits: the photograph remains usable while its
/// captured revision computes. Synchronous compatibility paths use a modal
/// session over this same panel, retaining the working Cancel button.
final class JobProgressController: NSObject {
    let window: NSPanel
    private let label = NSTextField(labelWithString: "Preparing…")
    private let bar = NSProgressIndicator()
    private var button: NSButton?
    private var cancelAction: (() -> Void)?
    private var cancelled = false
    /// The title and cancellability the card was last built for.
    private var cardKey: (title: String, cancellable: Bool)?

    /// `BackgroundWork.wait`'s idle panels, at most `poolLimit`.
    private static var idle: [JobProgressController] = []
    private static let poolLimit = 2

    /// `hidden` builds the panel invisible and click-through; `reveal()`
    /// shows it (`BackgroundWork.wait`'s `revealAfter`).
    init(title: String, cancel: (() -> Void)?, hidden: Bool = false) {
        self.cancelAction = cancel
        window = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 360, height: 160),
                         styleMask: [.titled], backing: .buffered, defer: false)
        super.init()
        label.font = DS.mono(12)
        label.textColor = DS.textMuted
        bar.isIndeterminate = false
        bar.minValue = 0
        bar.maxValue = 1
        bar.widthAnchor.constraint(equalToConstant: 310).isActive = true
        buildCard(title: title, cancellable: cancel != nil)
        window.hidesOnDeactivate = false
        // A titled panel's DEFAULT appearance animation runs as a threaded
        // `NSAnimation`, which parks one `-[NSAnimation _runBlocking]` thread
        // on the user-interactive root queue for the life of the process —
        // measured at exactly one per presentation. Past the 64-thread
        // dispatch soft limit nothing on that queue can be scheduled again and
        // `BackgroundWork.wait` never returns. This panel appears and
        // disappears in place, so it has nothing to animate.
        window.animationBehavior = .none
        present(hidden: hidden)
    }

    /// Main thread: an idle panel from the pool, or a new one, set up
    /// for one `BackgroundWork.wait` exactly as `init` would build it.
    static func checkout(title: String, cancel: (() -> Void)?, hidden: Bool) -> JobProgressController {
        assert(Thread.isMainThread)
        guard let panel = idle.popLast() else {
            return JobProgressController(title: title, cancel: cancel, hidden: hidden)
        }
        if panel.cardKey.map({ $0.title != title || $0.cancellable != (cancel != nil) }) ?? true {
            panel.buildCard(title: title, cancellable: cancel != nil)
        }
        panel.cancelAction = cancel
        panel.cancelled = false
        panel.label.stringValue = "Preparing…"
        panel.bar.stopAnimation(nil)
        panel.bar.isIndeterminate = false
        panel.bar.doubleValue = 0
        panel.present(hidden: hidden)
        return panel
    }

    /// Main thread: orders `panel` out and keeps it for the next
    /// wait. An idle panel holds no cancel action, so it retains no control.
    static func checkin(_ panel: JobProgressController) {
        assert(Thread.isMainThread)
        panel.close()
        panel.bar.stopAnimation(nil)
        panel.cancelAction = nil
        if idle.count < poolLimit, !idle.contains(where: { $0 === panel }) { idle.append(panel) }
    }

    /// The card for `title`; the label, bar and Cancel button are reused.
    private func buildCard(title: String, cancellable: Bool) {
        label.removeFromSuperview()
        bar.removeFromSuperview()
        let content = NSStackView(views: [label, bar])
        content.orientation = .vertical
        content.alignment = .leading
        let button = button ?? sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        self.button = button
        button.removeFromSuperview()
        button.isHidden = !cancellable
        // Esc presses Cancel in the panel, like every sheet's Cancel.
        button.keyEquivalent = cancellable ? "\u{1b}" : ""
        let buttons = NSStackView(views: [NSView(), button])
        buttons.orientation = .horizontal
        let card = makeSheetView(title: title,
            hint: cancellable ? "You can cancel before the result is applied." : "Processing the captured revision…",
            content: content, buttonRow: buttons, width: 360)
        window.title = title
        window.contentView = card
        window.setContentSize(card.fittingSize)
        cardKey = (title, cancellable)
    }

    private func present(hidden: Bool) {
        window.center()
        window.alphaValue = hidden ? 0 : 1
        window.ignoresMouseEvents = hidden
        BackgroundLaunch.present(window, makeKey: false)
    }

    func reveal() {
        window.alphaValue = 1
        window.ignoresMouseEvents = false
    }

    func update(control: RasterJobControl?) {
        guard let control else { bar.isIndeterminate = true; bar.startAnimation(nil); return }
        bar.doubleValue = control.progress
        let stages = ["Preparing…", "Reading pixels…", "Planning…", "Filling…", "Blending…", "Finishing…", "Rendering…"]
        label.stringValue = cancelled || control.isCancelled ? "Cancelling…" :
            stages[min(Int(control.stage), stages.count - 1)]
    }

    func close() { window.orderOut(nil) }

    @objc private func cancelClicked(_ sender: Any?) { requestCancel() }

    /// The Cancel button's action, and Esc's during a modal wait.
    func requestCancel() {
        cancelled = true
        cancelAction?()
    }
}
