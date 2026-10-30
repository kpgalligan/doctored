import AppKit

/// Clicks the modal progress panel's visible Cancel button once, as soon as
/// the worker slot is occupied. The timer runs in the modal-panel mode too,
/// because `BackgroundWork.wait` pumps its own modal session.
final class PanelCanceller {
    private let title: String
    private var timer: Timer?
    private(set) var clickedAt: Date?
    private(set) var sawOccupied = false

    init(title: String) {
        self.title = title
        let timer = Timer(timeInterval: 0.01, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        self.timer = timer
    }

    private func tick() {
        guard clickedAt == nil, BackgroundWork.occupied else { return }
        sawOccupied = true
        guard let panel = NSApp.windows.first(where: { $0.title == title && $0.isVisible }),
              let content = panel.contentView,
              let cancel = buttons(content).first(where: { !$0.isHidden && $0.title == "Cancel" })
        else { return }
        clickedAt = Date()
        cancel.performClick(nil)
        stop()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    /// Click to return, for information only (no verdict).
    func latency(_ name: String) -> String {
        guard let clickedAt else { return "\(name): no click" }
        return String(format: "%@ cancel→return %.1f ms", name, Date().timeIntervalSince(clickedAt) * 1000)
    }
}

/// Everything a cancelled transform must leave behind: the same document and
/// revision, no undo or dirty state, no Action, a free worker slot and no alert.
func requireCancelled(_ name: String, _ document: ImageDocument, base: RasterDocument?,
                      revision: UInt64, steps: [ActionStep], alerts: Int,
                      canceller: PanelCanceller) throws {
    try require(canceller.clickedAt != nil,
                "[\(name)] the panel's Cancel was never clicked (worker seen: \(canceller.sawOccupied), "
                + "edited: \(document.doc !== base), alerts: \(AutomationAlerts.recorded.suffix(1)))")
    try require(document.doc === base && document.revision == revision,
                "[\(name)] Cancel preserved the document and its revision")
    try require(document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "[\(name)] Cancel made no undo step and no dirty state")
    try require(steps.isEmpty, "[\(name)] Cancel recorded no Action: \(steps.map(\.tool))")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "[\(name)] Cancel released the worker slot and its scratch")
    try require(AutomationAlerts.recorded.count == alerts,
                "[\(name)] Cancel presented no alert or error: \(AutomationAlerts.recorded.suffix(2))")
}

/// A 16-bit document installed the way `native-paint-probe` opens one;
/// `prepare` edits the source before it is installed.
func precisionFixture(width: Int, height: Int,
                      prepare: (RasterDocument) throws -> RasterDocument = { $0 }) throws -> ImageDocument {
    var samples: [UInt16] = []
    samples.reserveCapacity(width * height * 4)
    for i in 0..<(width * height) {
        samples.append(UInt16(1001 + (i * 113) % 50000))
        samples.append(UInt16(2013 + (i * 67) % 48000))
        samples.append(UInt16(3017 + (i * 37) % 45000))
        samples.append(65535)
    }
    let source = try prepare(RasterDocument.fromRawRGBA16(samples, width: width, height: height,
                                                          profile: RasterProfile.builtin(.sRGB)))
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
    document.undoManager?.groupsByEvent = false
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    return document
}

/// Cancel on the transform progress panel stops the kernel and leaves the
/// document, undo, Actions and the alert log untouched — from the UI commit
/// (the session stays open and a later commit still lands), from MCP (an
/// in-band `JobCancelled` error) and from the 16-bit ⌘T and Crop gestures
/// (each session stays open and re-arms its preview).
func verifyTransformCancel() throws {
    defer { _ = ActionRecorder.shared.stop() }

    // (i) UI: a masked 12 MP layer, Free Transform 17° Lanczos, Cancel.
    let document = try fixture(width: 4000, height: 3000)
    defer { document.close() }
    let controller = try editor(document)
    _ = try transformResponse("add_layer_mask", document: document,
                              arguments: ["layer": 0, "kind": "reveal_all"])
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    guard let base = document.doc else {
        throw ProbeFailure(description: "No masked transform source")
    }
    controller.freeTransform(nil)
    controller.transformDegrees = 17
    controller.transformSampler = DR_FILTER_LANCZOS3
    controller.transformSession?.sampler = DR_FILTER_LANCZOS3
    guard let session = controller.transformSession else {
        throw ProbeFailure(description: "Actual transform session did not open")
    }
    // Let the masked preview settle so this case measures Cancel alone; a
    // commit made with the preview still in flight is verifyCommitDuringPreview.
    try pumpUntil({ controller.transformSession?.corePreview?.isPending != true }, seconds: 120)
    var revision = document.revision
    var alerts = AutomationAlerts.recorded.count
    var canceller = PanelCanceller(title: "Transform Layer")
    ActionRecorder.shared.start()
    controller.commitPendingTransform()
    var steps = ActionRecorder.shared.stop()
    canceller.stop()
    let uiLatency = canceller.latency("UI")
    try requireCancelled("UI", document, base: base, revision: revision, steps: steps,
                         alerts: alerts, canceller: canceller)
    try require(controller.isTransforming, "[UI] the cancelled session stays open")
    // The kept session re-requests its masked preview; let it settle before
    // the reference and the retry are admitted.
    try pumpUntil({ controller.transformSession?.corePreview?.isPending != true }, seconds: 120)
    guard let expected = try base.transformLayers(session.layers, session.transform.matrix,
                                                  sampler: session.sampler) else {
        throw ProbeFailure(description: "UI transform reference refused")
    }
    controller.commitPendingTransform()
    try require(!controller.isTransforming && document.doc?.layerPixels(0) == expected.layerPixels(0),
                "[UI] a second, uncancelled commit lands the reference pixels")
    print("PASS: UI Free Transform Cancel stops the job, keeps the session and a retry lands (\(uiLatency))")

    // (ii) MCP transform_layer: an in-band JobCancelled error.
    let agent = try fixture(width: 4000, height: 3000)
    defer { agent.close() }
    guard let agentBase = agent.doc else { throw ProbeFailure(description: "No MCP transform source") }
    revision = agent.revision
    alerts = AutomationAlerts.recorded.count
    canceller = PanelCanceller(title: "Transform Layer")
    ActionRecorder.shared.start()
    let (response, raw) = try transformResponse("transform_layer", document: agent,
        arguments: ["layer": 0, "rotate": 17, "sampler": "lanczos"])
    steps = ActionRecorder.shared.stop()
    canceller.stop()
    let mcpLatency = canceller.latency("MCP")
    try requireCancelled("MCP", agent, base: agentBase, revision: revision, steps: steps,
                         alerts: alerts, canceller: canceller)
    try require(response["isError"] as? Bool == true && raw.contains(JobCancelled.message),
                "[MCP] a cancelled transform_layer answers JobCancelled in-band: \(raw)")
    print("PASS: MCP transform_layer Cancel answers \"\(JobCancelled.message)\" (\(mcpLatency))")

    // (iii) 16 bits: ⌘T and Crop, each cancelled, each session kept and re-armed.
    let deep = try precisionFixture(width: 3000, height: 2000)
    defer { deep.close() }
    let deepController = try editor(deep)
    guard let deepBase = deep.doc, deepBase.isHighDepth else {
        throw ProbeFailure(description: "No 16-bit transform source")
    }
    deepController.freeTransform(nil)
    deepController.transformDegrees = 17
    deepController.transformSampler = DR_FILTER_LANCZOS3
    deepController.transformSession?.sampler = DR_FILTER_LANCZOS3
    guard let gesture = deepController.transformSession?.precisionGesture else {
        throw ProbeFailure(description: "16-bit transform session has no precision gesture")
    }
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    revision = deep.revision
    alerts = AutomationAlerts.recorded.count
    canceller = PanelCanceller(title: "Transform Layer")
    ActionRecorder.shared.start()
    deepController.commitPendingTransform()
    steps = ActionRecorder.shared.stop()
    canceller.stop()
    let deepLatency = canceller.latency("16-bit ⌘T")
    try requireCancelled("16-bit ⌘T", deep, base: deepBase, revision: revision, steps: steps,
                         alerts: alerts, canceller: canceller)
    try require(deepController.isTransforming
                && deepController.transformSession?.precisionGesture === gesture,
                "[16-bit ⌘T] the cancelled session stays open on its gesture")
    deepController.transformDegrees = 18
    try require(gesture.isPending, "[16-bit ⌘T] a handle change after Cancel requests a preview")
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    try require(gesture.image != nil && gesture.status == "Preview current",
                "[16-bit ⌘T] the re-armed gesture renders a preview: \(gesture.status)")
    deepController.endTransformSession()
    print("PASS: 16-bit ⌘T Cancel keeps the session and re-arms its preview (\(deepLatency))")

    deepController.beginCropSession()
    deepController.cropSession?.angle = 7
    guard let cropGesture = deepController.cropSession?.precisionGesture else {
        throw ProbeFailure(description: "16-bit crop session has no precision gesture")
    }
    try pumpUntil({ !cropGesture.isPending }, seconds: 120)
    revision = deep.revision
    alerts = AutomationAlerts.recorded.count
    canceller = PanelCanceller(title: "Crop")
    ActionRecorder.shared.start()
    deepController.commitCropSession()
    steps = ActionRecorder.shared.stop()
    canceller.stop()
    let cropLatency = canceller.latency("16-bit Crop")
    try requireCancelled("16-bit Crop", deep, base: deepBase, revision: revision, steps: steps,
                         alerts: alerts, canceller: canceller)
    try require(deepController.cropSession?.precisionGesture === cropGesture,
                "[16-bit Crop] the cancelled crop session stays open on its gesture")
    deepController.transformSampler = DR_FILTER_BILINEAR
    try require(cropGesture.isPending, "[16-bit Crop] a change after Cancel requests a preview")
    try pumpUntil({ !cropGesture.isPending }, seconds: 120)
    try require(cropGesture.image != nil && deepController.canvas.precisionCropPreview != nil,
                "[16-bit Crop] the re-armed gesture renders the crop preview: \(cropGesture.status)")
    deepController.endCropSession()
    print("PASS: 16-bit Crop Cancel keeps the crop box and re-arms its preview (\(cropLatency))")
}

/// Records the preview registry's bytes on the first tick that sees the
/// commit's own worker admitted (`occupied` inside `BackgroundWork.wait`).
private final class AdmissionObserver {
    private var timer: Timer?
    private(set) var observed: UInt64?

    init() {
        let timer = Timer(timeInterval: 0.002, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        self.timer = timer
    }

    private func tick() {
        guard observed == nil, BackgroundWork.occupied, BackgroundWork.isWaiting else { return }
        observed = SelectionWorkBudget.scratchBytes
        stop()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }
}

/// A commit made while the transform's own preview is still rendering
/// releases that preview's reservation before its admission — the byte Free
/// Transform (`TransformMaskPreview.cancelReleasingReservations`) and the
/// 16-bit ⌘T gesture (`PrecisionGeometryGesture.finish`) alike.
func verifyCommitDuringPreview() throws {
    // (i) 8 bits: a masked 12 MP layer, Free Transform 17° Lanczos.
    let document = try fixture(width: 4000, height: 3000)
    defer { document.close() }
    let controller = try editor(document)
    _ = try transformResponse("add_layer_mask", document: document,
                              arguments: ["layer": 0, "kind": "reveal_all"])
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    guard let base = document.doc else { throw ProbeFailure(description: "No masked source") }
    controller.freeTransform(nil)
    controller.transformDegrees = 17
    controller.transformSampler = DR_FILTER_LANCZOS3
    controller.transformSession?.sampler = DR_FILTER_LANCZOS3
    guard let session = controller.transformSession, let preview = session.corePreview else {
        throw ProbeFailure(description: "Masked transform session has no core preview")
    }
    RunLoop.main.run(until: Date().addingTimeInterval(0.15))
    try require(preview.isPending && SelectionWorkBudget.scratchBytes > 0,
                "[8-bit] the preview is still in flight at commit (pending: \(preview.isPending), "
                + "bytes: \(SelectionWorkBudget.scratchBytes))")
    let alerts = AutomationAlerts.recorded.count
    let observer = AdmissionObserver()
    controller.commitPendingTransform()
    observer.stop()
    try require(!controller.isTransforming, "[8-bit] the commit closed the session")
    try require(observer.observed == 0,
                "[8-bit] no preview bytes were registered at the commit's admission: "
                + "\(observer.observed.map { "\($0)" } ?? "worker never seen")")
    try require(preview.outstandingReservations == 0, "[8-bit] the preview's reservations left")
    try require(AutomationAlerts.recorded.count == alerts,
                "[8-bit] the commit presented no alert: \(AutomationAlerts.recorded.suffix(2))")
    guard let expected = try base.transformLayers(session.layers, session.transform.matrix,
                                                  sampler: session.sampler) else {
        throw ProbeFailure(description: "[8-bit] transform reference refused")
    }
    try require(document.doc?.layerPixels(0) == expected.layerPixels(0),
                "[8-bit] the commit landed the reference pixels")
    document.undoManager?.undo()
    try require(document.doc === base, "[8-bit] one Undo restores the base")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0
                && SelectionWorkBudget.scratchBytes == 0,
                "[8-bit] the worker slot and the preview scratch are free")

    // (ii) 16 bits: the ⌘T gesture's own in-flight preview.
    let deep = try precisionFixture(width: 3000, height: 2000)
    defer { deep.close() }
    let deepController = try editor(deep)
    guard let deepBase = deep.doc, deepBase.isHighDepth else {
        throw ProbeFailure(description: "No 16-bit source")
    }
    deepController.freeTransform(nil)
    deepController.transformDegrees = 17
    deepController.transformSampler = DR_FILTER_LANCZOS3
    deepController.transformSession?.sampler = DR_FILTER_LANCZOS3
    guard let gesture = deepController.transformSession?.precisionGesture else {
        throw ProbeFailure(description: "16-bit transform session has no precision gesture")
    }
    RunLoop.main.run(until: Date().addingTimeInterval(0.15))
    try require(gesture.isPending && SelectionWorkBudget.scratchBytes > 0,
                "[16-bit] the preview is still in flight at commit (pending: \(gesture.isPending), "
                + "bytes: \(SelectionWorkBudget.scratchBytes))")
    let deepAlerts = AutomationAlerts.recorded.count
    let deepObserver = AdmissionObserver()
    deepController.commitPendingTransform()
    deepObserver.stop()
    try require(!deepController.isTransforming, "[16-bit] the commit closed the session")
    try require(deepObserver.observed == 0,
                "[16-bit] no preview bytes were registered at the commit's admission: "
                + "\(deepObserver.observed.map { "\($0)" } ?? "worker never seen")")
    try require(gesture.outstandingPreviewReservations == 0,
                "[16-bit] the gesture's preview reservations left")
    try require(AutomationAlerts.recorded.count == deepAlerts,
                "[16-bit] the commit presented no alert: \(AutomationAlerts.recorded.suffix(2))")
    try require(deep.doc !== deepBase, "[16-bit] the commit landed an edit")
    deep.undoManager?.undo()
    try require(deep.doc === deepBase, "[16-bit] one Undo restores the base")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0
                && SelectionWorkBudget.scratchBytes == 0,
                "[16-bit] the worker slot and the preview scratch are free")
    print("PASS: commit during preview releases the in-flight reservation before admission "
        + "(8-bit Free Transform, 16-bit ⌘T)")
}
