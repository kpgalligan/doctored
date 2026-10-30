import AppKit

// The Crop straighten and the filtered Rotate Canvas are ONE cancellable
// job at both depths — core geometry, then the described-artwork re-render,
// then the display preparation — with no second "Rendering … Artwork" or
// "Preparing 16-bit Display" wait. Every Cancel leaves the document, undo,
// Actions and the alert log untouched. The cancelled 8-bit Rotate Canvas of
// (b) is 3000×2000; the text-layer one of (d) is 4000×3000, whose 12 MP plan
// (≈ 404 MB of scratch, down from ≈ 692 MB) fits the 2 GiB
// working limit. The text-layer Crop of (d) runs at 13 MP 4:3 as its own check
// (`verifyCanvasGeometryLargeCrop`), and the per-stage admission's
// discriminating cases are the halves of `verifyTransformLargeAdmission`,
// registered first. (g) is the Crop tool's whole-canvas no-op straighten
// sentence at both depths.

/// The panel titles a canvas geometry commit could show, old and new.
private let geometryTitles: Set<String> = [
    "Crop", "Rotate Canvas", "Rendering Straightened Artwork", "Rendering Rotated Artwork",
    "Preparing 16-bit Display", "Processing Captured High-Depth Pixels",
]

/// Records, in first-seen order, every `geometryTitles` panel that is ordered
/// in (a delayed panel is ordered in at alpha 0, so it counts from entry).
private final class TitleObserver {
    private var timer: Timer?
    private(set) var titles: [String] = []

    init() {
        let timer = Timer(timeInterval: 0.005, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        self.timer = timer
    }

    private func tick() {
        for window in NSApp.windows where window is NSPanel && window.isVisible
            && geometryTitles.contains(window.title) && !titles.contains(window.title) {
            titles.append(window.title)
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }
}

private func progressIndicators(_ view: NSView) -> [NSProgressIndicator] {
    (view as? NSProgressIndicator).map { [$0] } ?? view.subviews.flatMap(progressIndicators)
}

/// Clicks the panel's Cancel once its progress bar reads at least `threshold`
/// — past the core child, inside the artwork re-render or the display step.
private final class ProgressCanceller {
    private let title: String
    private let threshold: Double
    private var timer: Timer?
    private(set) var clickedAt: Date?
    private(set) var sawOccupied = false
    private(set) var progressAtClick: Double?

    init(title: String, threshold: Double) {
        self.title = title
        self.threshold = threshold
        let timer = Timer(timeInterval: 0.005, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        self.timer = timer
    }

    private func tick() {
        guard clickedAt == nil, BackgroundWork.occupied else { return }
        sawOccupied = true
        guard let panel = NSApp.windows.first(where: { $0.title == title && $0.isVisible }),
              let content = panel.contentView,
              let bar = progressIndicators(content).first, bar.doubleValue >= threshold,
              let cancel = buttons(content).first(where: { !$0.isHidden && $0.title == "Cancel" })
        else { return }
        progressAtClick = bar.doubleValue
        clickedAt = Date()
        cancel.performClick(nil)
        stop()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }
}

/// transform_cancel's private `requireCancelled`, twinned for both cancellers:
/// the same document and revision, no undo or dirty state, no Action, a free
/// worker slot and no alert.
private func requireGeometryCancelled(_ name: String, _ document: ImageDocument,
                                      base: RasterDocument?, revision: UInt64,
                                      steps: [ActionStep], alerts: Int,
                                      clicked: Bool, sawOccupied: Bool) throws {
    try require(clicked,
                "[\(name)] the panel's Cancel was never clicked (worker seen: \(sawOccupied), "
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

/// One UI or MCP commit under a `PanelCanceller`, then the shared verdict.
private func cancelled(_ name: String, _ document: ImageDocument, title: String,
                       _ body: () throws -> Void) throws {
    let base = document.doc
    let revision = document.revision
    let alerts = AutomationAlerts.recorded.count
    let canceller = PanelCanceller(title: title)
    ActionRecorder.shared.start()
    try body()
    let steps = ActionRecorder.shared.stop()
    canceller.stop()
    try requireGeometryCancelled(name, document, base: base, revision: revision, steps: steps,
                                 alerts: alerts, clicked: canceller.clickedAt != nil,
                                 sawOccupied: canceller.sawOccupied)
    print("PASS: \(name) Cancel leaves the document untouched (\(canceller.latency(name)))")
}

/// Clears the fixture's setup steps so a Cancel's verdict sees a clean slate.
private func settle(_ document: ImageDocument) {
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
}

/// Review finding 1: a 13 MP 4:3 straighten at 7° must be admitted.
/// The job admits its largest stage (`max(core, artwork)`), measured here
/// against what the working budget has free. The summed
/// `core + artwork + W·H·24` fits this document too, so the check no longer
/// tells the two apart; the 37 MP Rotate Canvas half of
/// `verifyTransformLargeAdmission` does.
private func requireStraightenAdmitted(_ source: RasterDocument, rect: CGRect,
                                       angle: Double) throws {
    let center = CGPoint(x: rect.midX, y: rect.midY)
    let matrix = CGAffineTransform(translationX: center.x, y: center.y)
        .rotated(by: CGFloat(-angle) * .pi / 180)
        .translatedBy(x: -center.x, y: -center.y)
    let command = try PrecisionPixelCommand.straighten(
        matrix: matrix, filter: RasterDocument.precisionFilter(DR_FILTER_BILINEAR),
        x: Int(rect.minX), y: Int(rect.minY), width: Int(rect.width), height: Int(rect.height))
    let core = try PrecisionDocumentDescriptor.integer(
        source.geometryEditPlan(request: command.request), "scratch_bytes")
    var artworkMap = matrix
    artworkMap.tx -= rect.minX
    artworkMap.ty -= rect.minY
    let artwork = try ArtworkGeometryWork(
        source: source, affineMap: artworkMap,
        canvas: (Int(rect.width), Int(rect.height))).scratchBytes
    let free = try RasterResources.shared.admit(scratch: 0)
    let summed = core + artwork + UInt64(source.width) * UInt64(source.height) * 24
    let staged = max(core, artwork)
    let mb = { (bytes: UInt64) in String(format: "%.0f MB", Double(bytes) / 1e6) }
    try require(staged <= free,
                "[13 MP Crop] the largest stage fits: \(mb(staged)) of \(mb(free)) free")
    print("PASS: 13 MP 4:3 straighten 7°: largest stage \(mb(staged)) fits \(mb(free)) free "
          + "(summed \(mb(summed)))")
}

/// Adds the probe's text layer and clears the setup steps.
private func textLayer(_ document: ImageDocument) throws -> RasterDocument {
    _ = try agentOK("add_text_layer", document,
                    ["text": "Probe", "x": 400, "y": 300, "size": 160])
    settle(document)
    guard let base = document.doc, base.layerCount == 2, base.hasArtworkDescription(1) else {
        throw ProbeFailure(description: "[text] the text layer is not described artwork")
    }
    return base
}

/// One commit cancelled once the panel's progress reads ≥ 0.6 (past the core
/// step), then the shared verdict and the kept text description.
private func cancelPastCore(_ name: String, _ document: ImageDocument, base: RasterDocument,
                            _ commit: () -> Void) throws {
    let revision = document.revision
    let alerts = AutomationAlerts.recorded.count
    let canceller = ProgressCanceller(title: name, threshold: 0.6)
    ActionRecorder.shared.start()
    commit()
    let steps = ActionRecorder.shared.stop()
    canceller.stop()
    try requireGeometryCancelled("text \(name) past the core step", document,
                                 base: base, revision: revision, steps: steps,
                                 alerts: alerts, clicked: canceller.clickedAt != nil,
                                 sawOccupied: canceller.sawOccupied)
    try require(document.doc?.hasArtworkDescription(1) == true,
                "[text \(name)] the text layer keeps its description")
    print("PASS: text \(name) (\(base.width) × \(base.height)) Cancel at progress "
          + String(format: "%.2f", canceller.progressAtClick ?? -1)
          + " leaves the document untouched")
}

/// (d), Crop half, at 13 MP 4:3 (4160 × 3120) on a fresh fixture with a text
/// layer: the job is admitted (review finding 1), a Crop cancelled past the
/// core step keeps the description, and an uncancelled Crop keeps it and
/// shows ONE panel. Registered FIRST in `main.swift`: the later cases leave
/// hundreds of MB of documents alive in this one process, which admission
/// counts, so at their end even the core plan alone would not fit.
func verifyCanvasGeometryLargeCrop() throws {
    defer { _ = ActionRecorder.shared.stop() }
    let document = try fixture(width: 4160, height: 3120)
    defer { document.close() }
    let controller = try editor(document)
    let base = try textLayer(document)
    let rect = CGRect(x: 208, y: 156, width: 3744, height: 2808)
    try requireStraightenAdmitted(base, rect: rect, angle: 7)
    try cancelPastCore("Crop", document, base: base) {
        controller.beginCropSession()
        controller.cropSession?.rect = rect
        controller.cropSession?.angle = 7
        controller.commitCropSession()
    }
    controller.endCropSession()
    controller.beginCropSession()
    controller.cropSession?.rect = rect
    controller.cropSession?.angle = 7
    let observer = TitleObserver()
    controller.commitCropSession()
    observer.stop()
    try require(document.doc !== base && document.doc?.width == 3744
                && document.doc?.height == 2808,
                "[text Crop] an uncancelled 13 MP Crop is admitted and publishes the window "
                + "(alerts: \(AutomationAlerts.recorded.suffix(1)))")
    try require(document.doc?.hasArtworkDescription(1) == true,
                "[text Crop] the published Crop keeps the text description")
    try require(observer.titles == ["Crop"], "[text Crop] one job, one panel: \(observer.titles)")
    controller.endCropSession()
    print("PASS: a 13 MP text-layer Crop straighten is one job with one panel: \(observer.titles)")
}

func verifyCanvasGeometryCancel() throws {
    defer { _ = ActionRecorder.shared.stop() }

    // (a) 8-bit UI Crop straighten: Cancel keeps the session; a retry lands.
    do {
        let document = try fixture(width: 3000, height: 2000)
        defer { document.close() }
        let controller = try editor(document)
        guard let base = document.doc else { throw ProbeFailure(description: "No crop source") }
        let rect = CGRect(x: 150, y: 100, width: 2400, height: 1600)
        controller.beginCropSession()
        controller.cropSession?.rect = rect
        controller.cropSession?.angle = 7
        try cancelled("8-bit UI Crop", document, title: "Crop") {
            controller.commitCropSession()
        }
        try require(controller.cropSession?.rect == rect && controller.cropSession?.angle == 7,
                    "[8-bit UI Crop] the cancelled session keeps its rect and angle: "
                    + "\(String(describing: controller.cropSession?.rect)) "
                    + "\(String(describing: controller.cropSession?.angle))")
        controller.commitCropSession()
        try require(document.doc?.width == 2400 && document.doc?.height == 1600,
                    "[8-bit UI Crop] a retry publishes the 2400 × 1600 window")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[8-bit UI Crop] the retry is ONE undo step")
        controller.endCropSession()
        print("PASS: 8-bit UI Crop retry publishes 2400 × 1600 as one undo step")
    }

    // (b) 8-bit UI Rotate Canvas 17° Lanczos, Cancel.
    do {
        let document = try fixture(width: 3000, height: 2000)
        defer { document.close() }
        let controller = try editor(document)
        try cancelled("8-bit UI Rotate Canvas", document, title: "Rotate Canvas") {
            controller.commitCanvasRotation(degrees: 17, sampler: DR_FILTER_LANCZOS3)
        }
    }

    // (c) MCP crop 7° and rotate 17°, each an in-band JobCancelled error.
    do {
        let document = try fixture(width: 3000, height: 2000)
        defer { document.close() }
        for (tool, title, arguments) in [
            ("crop", "Crop", ["x": 150, "y": 100, "width": 2400, "height": 1600, "angle": 7]),
            ("rotate", "Rotate Canvas", ["degrees": 17, "sampler": "lanczos"]),
        ] as [(String, String, [String: Any])] {
            var reply: (response: [String: Any], raw: String) = ([:], "")
            try cancelled("MCP \(tool)", document, title: title) {
                reply = try transformResponse(tool, document: document, arguments: arguments)
            }
            try require(reply.response["isError"] as? Bool == true
                        && reply.raw.contains(JobCancelled.message),
                        "[MCP \(tool)] a cancelled call answers JobCancelled in-band: \(reply.raw)")
        }
    }

    // (d) A text layer on its own fresh fixture: a Rotate Canvas cancelled
    // PAST the core step (≥ 0.6: the artwork re-render or the display
    // preparation) keeps the description. The Crop half is its own check,
    // `verifyCanvasGeometryLargeCrop`, at 13 MP. This half runs at 12 MP
    // (4000 × 3000), which fits.
    do {
        let document = try fixture(width: 4000, height: 3000)
        defer { document.close() }
        let controller = try editor(document)
        let base = try textLayer(document)
        try cancelPastCore("Rotate Canvas", document, base: base) {
            controller.commitCanvasRotation(degrees: 17, sampler: DR_FILTER_BILINEAR)
        }
    }

    // (e) 16 bits: Rotate Canvas 17°, Cancel, then one job with one panel.
    do {
        let deep = try precisionFixture(width: 3000, height: 2000)
        defer { deep.close() }
        let controller = try editor(deep)
        guard let base = deep.doc, base.isHighDepth else {
            throw ProbeFailure(description: "No 16-bit rotate source")
        }
        try cancelled("16-bit Rotate Canvas", deep, title: "Rotate Canvas") {
            controller.commitCanvasRotation(degrees: 17, sampler: DR_FILTER_BILINEAR)
        }
        let observer = TitleObserver()
        controller.commitCanvasRotation(degrees: 17, sampler: DR_FILTER_BILINEAR)
        observer.stop()
        try require(deep.doc !== base, "[16-bit Rotate Canvas] an uncancelled run publishes")
        try require(observer.titles == ["Rotate Canvas"],
                    "[16-bit Rotate Canvas] one job, one panel: \(observer.titles)")
        print("PASS: 16-bit Rotate Canvas is one job with one panel: \(observer.titles)")
    }

    // (f) The crop and canvas_size no-ops record no step, and a straighten
    // window outside the canvas refuses by name and records nothing.
    do {
        let document = try fixture(width: 64, height: 48)
        defer { document.close() }
        guard let base = document.doc else { throw ProbeFailure(description: "No no-op source") }
        ActionRecorder.shared.start()
        let whole = try agentOK("crop", document,
            ["x": 0, "y": 0, "width": 64, "height": 48, "angle": 1e-10])
        let plain = try agentOK("crop", document, ["x": 0, "y": 0, "width": 64, "height": 48])
        let same = try agentOK("canvas_size", document, ["width": 64, "height": 48])
        // A window past the canvas extends it; one over 100 MP refuses.
        let outside = try agentCall("crop", document,
            ["x": 0, "y": 0, "width": 20_000, "height": 6_000, "angle": 7])
        let steps = ActionRecorder.shared.stop()
        for (name, reply) in [("tiny-angle crop", whole), ("whole crop", plain),
                              ("canvas_size", same)] {
            try require(reply["ok"] as? Bool == true && reply["changed"] as? Bool == false,
                        "[\(name)] a whole-canvas no-op answers changed:false: \(reply)")
        }
        try require(outside.isError
                    && outside.text.contains("The canvas cannot exceed 100 megapixels"),
                    "[over-limit crop] refuses by name: \(outside.text)")
        try require(steps.isEmpty, "[no-ops] recorded no Action: \(steps.map(\.tool))")
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[no-ops] no edit and no undo step")
        print("PASS: crop/canvas_size no-ops record nothing; an over-limit window refuses by name")
    }

    // (g) The Crop tool's whole-canvas straighten that changes no pixel says
    // so at both depths: one beep whose sentence is
    // `CropSession.unchangedStraightenSentence`, with no edit, undo step or
    // Action. At 16 bits the kept session re-arms, so a later box edit
    // previews again.
    for deep in [false, true] {
        let depth = deep ? "16-bit" : "8-bit"
        let document = deep ? try precisionFixture(width: 64, height: 48)
            : try fixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        settle(document)
        guard let base = document.doc else { throw ProbeFailure(description: "No (g) source") }
        controller.beginCropSession()
        defer { controller.endCropSession() }
        controller.cropSession?.angle = 1e-10
        let revision = document.revision
        ActionRecorder.shared.start()
        let capture = SessionCommitRefusal.capturing {
            controller.commitCropSession()
            return false
        }
        let steps = ActionRecorder.shared.stop()
        let expected = CropSession.unchangedStraightenSentence(width: 64, height: 48, angle: 1e-10)
        try require(capture.sentence == expected,
                    "[\(depth) no-op Crop] the beep says \"\(expected)\": "
                    + "\(String(describing: capture.sentence))")
        try require(document.doc === base && document.revision == revision
                    && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                    "[\(depth) no-op Crop] no edit and no undo step")
        try require(steps.isEmpty, "[\(depth) no-op Crop] recorded no Action: \(steps.map(\.tool))")
        if deep {
            guard let gesture = controller.cropSession?.precisionGesture else {
                throw ProbeFailure(description: "[16-bit no-op Crop] the session was not kept")
            }
            controller.canvas.precisionCropPreview = nil
            controller.cropRectWidth = 32
            try require(gesture.isPending, "[16-bit no-op Crop] a box edit requests a preview")
            try pumpUntil({ !gesture.isPending }, seconds: 60)
            // A non-zero angle (1e-10 here) fits the typed box inside the
            // straightened image (CropFit), so read back the fitted size.
            let width = Int(controller.cropRectWidth)
            let height = Int(controller.cropRectHeight)
            try require(width <= 32 && controller.canvas.precisionCropPreview?.width == width
                        && controller.canvas.precisionCropPreview?.height == height,
                        "[16-bit no-op Crop] a new \(width) × \(height) preview arrives: "
                        + gesture.status)
        }
        print("PASS: \(depth) whole-canvas no-op Crop says \"\(expected)\" and changes nothing")
    }
}
