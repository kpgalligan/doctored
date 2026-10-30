import AppKit

// No `ImageDocument` is involved: the window owns its own `RasterImage`, there is no `applyEdit`,
// no `registerUndo` and no dirty state, and Undo/Redo are recorded gestures replayed through the
// core rather than history.
//
// The controller intercepts the nil-target `terminate:` when its window is key — NOT
// `applicationShouldTerminate(_:)`, which would shadow `NSDocumentController`'s unsaved-document
// review in this NSDocument-based app — and `applicationWillTerminate` covers a quit approved
// elsewhere.

/// The gated assisted annotation window: one case, one clock, one replay.
///
/// It is deliberately NOT an editor window. There is no `ImageDocument`, no
/// `applyEdit`, no undo manager and no dirty state — the window owns its own
/// `RasterImage`, Undo and Redo are recorded gestures rather than history, and
/// the only persistent output is `<workspace>/replays/<case_id>.json`. At most
/// one session exists at a time (`current`).
///
/// Quit is intercepted here rather than in an `applicationShouldTerminate(_:)`:
/// the app is NSDocument-based, and a delegate implementation would shadow
/// `NSDocumentController`'s unsaved-document review. The nil-target
/// `terminate:` reaches this controller through the responder chain whenever
/// this window is key; a quit approved from any other window is covered by
/// `applicationWillTerminate` calling `finishForTermination()`.
final class AssistedAnnotationWindowController: NSWindowController, NSWindowDelegate,
                                                NSUserInterfaceValidations {
    static private(set) var current: AssistedAnnotationWindowController?

    let session: AssistedAnnotationSession

    private let canvas = AssistedAnnotationCanvasView()
    private let scroll = NSScrollView()
    private let labelPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let diameterPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let priorToggle = NSButton(checkboxWithTitle: "Prior (P)", target: nil, action: nil)
    private let guidanceToggle = NSButton(
        checkboxWithTitle: "Guidance (G)", target: nil, action: nil)
    private let readout = NSTextField(labelWithString: "")
    private let status = NSTextField(wrappingLabelWithString: "")
    private let host = NSViewController()
    private var ticker: Timer?
    private var strokePoints: [[Int]] = []
    private var replayPath: URL?

    private init(session: AssistedAnnotationSession) {
        self.session = session
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 980, height: 720),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered, defer: false)
        super.init(window: window)
        window.title = "Assisted Annotation — \(session.caseID)"
        window.delegate = self
        window.isReleasedWhenClosed = false
        window.contentViewController = host
        host.view = buildContent()
        session.onFinished = { [weak self] outcome in self?.sessionDidFinish(outcome) }
        canvas.onStrokeBegin = { [weak self] point in self?.beginStroke(point) ?? false }
        canvas.onStrokeMove = { [weak self] point in self?.continueStroke(point) }
        canvas.onStrokeEnd = { [weak self] in self?.endStroke() }
        canvas.onUndo = { [weak self] in self?.performUndo() }
        canvas.onRedo = { [weak self] in self?.performRedo() }
        canvas.canUndo = { [weak self] in self?.session.undoAvailable ?? false }
        canvas.canRedo = { [weak self] in self?.session.redoAvailable ?? false }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AssistedAnnotationWindowController does not support NSCoder")
    }

    deinit { ticker?.invalidate() }

    /// Opens one case. Decoding and hash checking happen HERE, before the
    /// clock: a human session then waits on the task card, a synthetic one
    /// begins at once.
    @discardableResult
    static func open(workspace: URL, caseID: String?, synthetic: Bool) throws
        -> AssistedAnnotationWindowController
    {
        guard AssistedAnnotation.isEnabled else {
            throw AssistedAnnotation.failure(AssistedAnnotation.disabledMessage)
        }
        if let existing = current, existing.session.isActive {
            throw AssistedAnnotation.failure("An annotation session is already open.")
        }
        let prepared = try AssistedWorkspace(root: workspace)
        let chosen: AssistedCase
        if let caseID {
            chosen = try prepared.caseNamed(caseID)
        } else if let pending = prepared.nextPendingCase() {
            chosen = pending
        } else {
            throw AssistedAnnotation.failure("Every case in this workspace already has a replay.")
        }
        guard !prepared.isLocked(chosen.anonymousID) else {
            throw AssistedAnnotation.failure(
                "Case \(chosen.anonymousID) is locked: it already has a replay.")
        }
        let loaded = try AssistedLoadedCase(workspace: prepared, case: chosen)
        let session = AssistedAnnotationSession(
            workspace: prepared, loaded: loaded, origin: synthetic ? .synthetic : .human)
        let controller = AssistedAnnotationWindowController(session: session)
        current?.close()
        current = controller
        BackgroundLaunch.show(controller)
        controller.window?.makeFirstResponder(controller.canvas)
        if synthetic {
            try session.begin(annotatorID: nil, reportsPriorExposure: nil, attestation: nil)
            controller.sessionDidBegin()
        } else {
            try controller.presentTaskCard()
        }
        return controller
    }

    // MARK: - Layout

    private func buildContent() -> NSView {
        canvas.frame = NSRect(x: 0, y: 0, width: session.width, height: session.height)
        canvas.diameter = 9
        scroll.documentView = canvas
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = true
        scroll.allowsMagnification = true
        scroll.minMagnification = 0.02
        scroll.maxMagnification = 8
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.backgroundColor = .black

        for label in ["Foreground (F)", "Background (B)", "Unknown (U)"] {
            labelPopup.addItem(withTitle: label)
        }
        labelPopup.identifier = NSUserInterfaceItemIdentifier("assisted.label")
        labelPopup.target = self
        labelPopup.action = #selector(labelChanged(_:))
        for diameter in AssistedAnnotation.diameters {
            diameterPopup.addItem(withTitle: "\(diameter) px")
        }
        diameterPopup.selectItem(at: 1)
        diameterPopup.identifier = NSUserInterfaceItemIdentifier("assisted.diameter")
        diameterPopup.target = self
        diameterPopup.action = #selector(diameterChanged(_:))
        priorToggle.target = self
        priorToggle.action = #selector(togglePrior(_:))
        guidanceToggle.state = .on
        guidanceToggle.target = self
        guidanceToggle.action = #selector(toggleGuidance(_:))
        readout.font = DS.mono(12)
        readout.textColor = DS.textStrong
        status.font = DS.sans(12)
        status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 600

        let fit = StickerButton(
            title: "Fit", style: .secondary, target: self, action: #selector(zoomFitAction(_:)))
        let actual = StickerButton(
            title: "100%", style: .secondary, target: self,
            action: #selector(zoomActualAction(_:)))
        let submit = StickerButton(
            title: "Submit", style: .primary, target: self, action: #selector(submit(_:)))
        submit.identifier = NSUserInterfaceItemIdentifier("assisted.submit")
        let controls = NSStackView(views: [
            labelPopup, diameterPopup, priorToggle, guidanceToggle, fit, actual, submit,
        ])
        controls.orientation = .horizontal
        controls.alignment = .centerY
        controls.spacing = 10
        controls.translatesAutoresizingMaskIntoConstraints = false

        let stack = NSStackView(views: [controls, readout, scroll, status])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false
        let content = NSView()
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 12),
            stack.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),
            scroll.widthAnchor.constraint(equalTo: stack.widthAnchor),
            controls.widthAnchor.constraint(equalTo: stack.widthAnchor),
            status.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])
        refreshReadout()
        return content
    }

    private func presentTaskCard() throws {
        let card = try session.workspace.taskCardText()
        let sheet = AssistedTaskCardSheetController(
            taskCard: card, annotatorID: session.workspace.annotatorID, caseID: session.caseID,
            onBegin: { [weak self] identifier, exposed, attestation in
                self?.beginFromCard(identifier, exposed, attestation)
            },
            onCancel: { [weak self] in self?.cancelBeforeStart() })
        host.presentAsSheet(sheet)
    }

    // MARK: - Session lifecycle

    private func beginFromCard(
        _ annotatorID: String, _ exposed: Bool, _ attestation: AssistedAttestation
    ) {
        do {
            try session.begin(
                annotatorID: annotatorID, reportsPriorExposure: exposed, attestation: attestation)
            sessionDidBegin()
        } catch {
            status.stringValue = error.localizedDescription
        }
    }

    private func cancelBeforeStart() {
        close()
    }

    /// The source reaches the canvas only now, so the recorded clock and the
    /// first moment the photograph is visible are the same instant.
    private func sessionDidBegin() {
        canvas.sourceImage = session.loaded.sourceImage
        canvas.priorOverlay = AssistedAnnotationCanvasView.priorOverlayImage(
            session.loaded.prior, width: session.width, height: session.height)
        refreshGuidance()
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        ticker = timer
        zoomFitAction(nil)
        refreshReadout()
    }

    private func sessionDidFinish(_ outcome: AssistedOutcome) {
        ticker?.invalidate()
        ticker = nil
        canvas.endStrokeNow()
        do {
            let url = session.workspace.replayURL(for: session.caseID)
            try AssistedReplayWriter.write(try session.replayJSON(), to: url)
            replayPath = url
            status.stringValue = "Session \(outcome.rawValue). Replay written to \(url.path)."
        } catch {
            status.stringValue = "The replay could not be written: \(error.localizedDescription)"
        }
        refreshReadout()
    }

    private func tick() {
        if session.isOverTime { canvas.endStrokeNow() }
        _ = session.tick()
        refreshReadout()
    }

    // MARK: - Gestures

    private func beginStroke(_ point: [Int]) -> Bool {
        guard session.isActive else {
            NSSound.beep()
            return false
        }
        strokePoints = [point]
        return true
    }

    private func continueStroke(_ point: [Int]) {
        strokePoints.append(point)
    }

    private func endStroke() {
        let points = strokePoints
        strokePoints = []
        guard !points.isEmpty else { return }
        do {
            let accepted = try session.stroke(
                tool: currentLabel, diameter: currentDiameter, rawPoints: points)
            if !accepted { NSSound.beep() }
        } catch {
            status.stringValue = error.localizedDescription
            NSSound.beep()
        }
        refreshGuidance()
        refreshReadout()
    }

    func performUndo() {
        _ = session.undo()
        refreshGuidance()
        refreshReadout()
    }

    func performRedo() {
        _ = session.redo()
        refreshGuidance()
        refreshReadout()
    }

    // MARK: - Controls

    private var currentLabel: AssistedLabel {
        switch labelPopup.indexOfSelectedItem {
        case 1: return .background
        case 2: return .unknown
        default: return .foreground
        }
    }

    private var currentDiameter: Int {
        let index = max(0, min(AssistedAnnotation.diameters.count - 1,
                               diameterPopup.indexOfSelectedItem))
        return AssistedAnnotation.diameters[index]
    }

    @objc private func labelChanged(_ sender: Any?) { refreshReadout() }

    @objc private func diameterChanged(_ sender: Any?) {
        canvas.diameter = currentDiameter
        refreshReadout()
    }

    @objc func togglePrior(_ sender: Any?) {
        canvas.showPrior.toggle()
        priorToggle.state = canvas.showPrior ? .on : .off
        session.view("prior_overlay=\(canvas.showPrior)")
        refreshReadout()
    }

    @objc func toggleGuidance(_ sender: Any?) {
        canvas.showGuidance.toggle()
        guidanceToggle.state = canvas.showGuidance ? .on : .off
        session.view("guidance_overlay=\(canvas.showGuidance)")
        refreshReadout()
    }

    @objc func zoomFitAction(_ sender: Any?) {
        let size = scroll.contentSize
        let scale = min(size.width / max(1, canvas.bounds.width),
                        size.height / max(1, canvas.bounds.height), 1)
        scroll.magnification = max(scroll.minMagnification, scale)
        session.view("zoom=fit")
        refreshReadout()
    }

    @objc func zoomActualAction(_ sender: Any?) {
        scroll.magnification = 1
        session.view("zoom=100")
        refreshReadout()
    }

    @objc func submit(_ sender: Any?) {
        do {
            try session.submit()
        } catch {
            status.stringValue = error.localizedDescription
            NSSound.beep()
        }
        refreshReadout()
    }

    override func keyDown(with event: NSEvent) {
        guard let characters = event.charactersIgnoringModifiers, characters.count == 1,
              !event.modifierFlags.contains(.command)
        else {
            super.keyDown(with: event)
            return
        }
        switch characters {
        case "f": labelPopup.selectItem(at: 0)
        case "b": labelPopup.selectItem(at: 1)
        case "u": labelPopup.selectItem(at: 2)
        case "1", "2", "3", "4":
            diameterPopup.selectItem(at: (Int(characters) ?? 1) - 1)
            diameterChanged(nil)
        case "p": togglePrior(nil)
        case "g": toggleGuidance(nil)
        default:
            super.keyDown(with: event)
            return
        }
        refreshReadout()
    }

    // MARK: - Readouts

    private func refreshGuidance() {
        canvas.guidanceOverlay = AssistedAnnotationCanvasView.guidanceOverlayImage(
            session.labelPlane, width: session.width, height: session.height)
    }

    private func refreshReadout() {
        let remaining = AssistedAnnotation.budgetMS > session.elapsedMS
            ? AssistedAnnotation.budgetMS - session.elapsedMS : 0
        readout.stringValue = String(
            format: "%@  ·  %d:%02d left  ·  %d of %d gestures  ·  %d strokes applied",
            session.stateName, remaining / 60000, (remaining / 1000) % 60,
            session.modifyingGestures, AssistedAnnotation.budgetGestures, session.activeStrokes)
        canvas.diameter = currentDiameter
    }

    // MARK: - Closing and quitting

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard session.isActive else { return true }
        return promptAbandon()
    }

    func windowWillClose(_ notification: Notification) {
        ticker?.invalidate()
        ticker = nil
        if Self.current === self { Self.current = nil }
    }

    /// Cancel keeps the session open; Abandon records `incomplete` and writes
    /// the replay, which locks the case.
    func promptAbandon() -> Bool {
        guard session.isActive else { return true }
        let alert = NSAlert()
        alert.messageText = "Abandon case \(session.caseID)?"
        alert.informativeText =
            "This case has one attempt. Abandoning records an incomplete replay and the case "
            + "cannot be annotated again."
        alert.addButton(withTitle: "Cancel")
        alert.addButton(withTitle: "Abandon")
        guard alert.runModal() == .alertSecondButtonReturn else { return false }
        do {
            try session.abandon()
        } catch {
            status.stringValue = error.localizedDescription
            return false
        }
        return true
    }

    /// Quit approved elsewhere: record `incomplete` synchronously, no prompt.
    func finishForTermination() {
        guard session.isActive else { return }
        do {
            try session.abandon()
        } catch {
            NSSound.beep()
        }
    }

    @objc func terminate(_ sender: Any?) {
        if session.isActive, !promptAbandon() { return }
        NSApp.terminate(sender)
    }

    // MARK: - MCP seams

    /// A synthetic driver's stroke. A human session refuses one outright: an
    /// annotator's replay must record the annotator's own gestures.
    func synthesizeStroke(tool: AssistedLabel, diameter: Int, points: [[Int]]) throws -> Bool {
        guard session.origin == .synthetic else {
            throw AssistedAnnotation.failure("A human session does not accept synthesized strokes.")
        }
        let accepted = try session.stroke(tool: tool, diameter: diameter, rawPoints: points)
        refreshGuidance()
        refreshReadout()
        return accepted
    }

    func finalizeFromAgent() throws -> URL {
        guard session.origin == .synthetic else {
            throw AssistedAnnotation.failure("A human session is finalized from the window.")
        }
        if session.isActive { try session.submit() }
        guard let path = replayPath else {
            throw AssistedAnnotation.failure("No replay was written for this session.")
        }
        return path
    }

    // MARK: - Validation

    func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        switch item.action {
        case #selector(terminate(_:)): return true
        case #selector(submit(_:)): return session.isActive
        case #selector(zoomFitAction(_:)), #selector(zoomActualAction(_:)),
             #selector(togglePrior(_:)), #selector(toggleGuidance(_:)):
            return !isAwaitingStart
        default: return false
        }
    }

    private var isAwaitingStart: Bool {
        if case .awaitingStart = session.state { return true }
        return false
    }
}
