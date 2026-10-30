import AppKit

// `TransformWork.wait` and `PrecisionGeometryGesture.finishOutcome`
// admit their largest sequential stage, `max(core, artwork)`, as
// `CanvasGeometryJob` does, instead of `core + artwork + W·H·24`
// (at 16 bits, `+ projection`); so do the byte selected-pixels move
// (`core`, was `core + W·H·24`) and floating-transform commit (`max(core, σ)`,
// was `core + W·H·24 + σ`, σ the transformed selection). Registered FIRST in
// `main.swift`: later cases leave documents alive in this one process, which
// admission counts. Each
// half measures its figures against what the working budget has free, so the
// check is deterministic and fails if the summed estimate comes back.
//
// Sizing. Each half prints its residency once its fixture is open
// (`printResidency`). Measured: an 8-bit fixture holds 12.00 B/px — the layer's
// tiles, its contiguous view and the display image, 4 B/px each
// (`layer_pixels`, `contiguous_views`, `image_pixels`); a 16-bit one holds
// 20.04 B/px (`precision_pixels` 16, `image_pixels` 4). With c = core/P,
// r = residency/P, L = 2147 MB and s the old sum's extra B/px, both margins
// balance at P* = 2L/(2r + 2c + s): the Rotate Canvas 17° (c = 33.4,
// s = 24) gives 37.4 MP, so 7040×5280; the 16-bit Crop, a 7° straighten
// of a 5% inset (c = 35.2, s = the 32.0 B/px projection), gives 30.1 MP, so
// 6336×4752 — at the default Catmull-Rom, with the same c. The ⌘T
// stays 22 MP 3:2 (5760×3840). The Move Selected (c = 20.1) gives 48.7 MP,
// so 8064×6048; its floating transform balances at P* = 2L/(2r + max(c, σ/P) +
// c + σ/P + s) with c = 36.1 and σ/P = 28.1 (bilinear: Catmull-Rom trips
// `selected_pixels_transform`'s own 2e9 landing bound above ≈ 31 MP), 29.0 MP,
// so 6208×4656. Each PASS line prints both margins. Between the 16-bit Crop
// and the Move, a 24 MP 16-bit Crop tool session at Catmull-Rom takes
// two successive angle changes: the true preview is over the interactive work
// limit, so it lands at reduced resolution (a 2700×1800 preview of
// the 5400×3600 window, factor 2 from the commit plan's work units) and says
// so, and the commit publishes the window. Then H1: a 24 MP
// 16-bit ⌘T at Catmull-Rom whose preview lands reduced the same way (3000 px
// wide), whose commit (and
// a 24 MP Crop straighten's) Cancel stops within 1 s because the job's
// control reaches the kernel, and whose uncancelled commit takes the one-shot
// limit; and H2: two successive 24 MP bilinear Crop previews both land
// now that a preview worker no longer counts its own reservation twice.
// That fix's diagnosis (snapshots through `RasterResources.liveHandleSummary()`)
// also ran a post-undo 24 MP rotate: its display projection is refused
// because the redo tail's 5400×3600 document (≈ 243 MB with its latent
// off-canvas storage) is still live while the new display is admitted — a
// follow-up needing a decision, so that half was not kept.
//
// The ~90 MB the ⌘T half once lacked (1793 MB free against 1882 MB predicted)
// was a second canvas-sized `image_pixels` image, 88 MB, still alive when the
// free budget was read after `add_text_layer`. It appeared in two runs at
// ed5b15c and in none at 665e405, e43f149, 3984ea1 or this change, nor in a
// third ed5b15c build: a transient display image's lifetime, counted
// correctly, not a missing term.

private let mb = { (bytes: UInt64) in String(format: "%.0f MB", Double(bytes) / 1e6) }

/// A 17° turn about `(cx, cy)`.
private func turn(_ cx: CGFloat, _ cy: CGFloat) -> CGAffineTransform {
    CGAffineTransform(translationX: cx, y: cy)
        .rotated(by: 17 * .pi / 180)
        .translatedBy(x: -cx, y: -cy)
}

/// One line of what the working budget holds once a half's fixture is open:
/// `report()`'s retained and transient bytes, B/px over the open documents'
/// canvases, and every category of at least 1 MB, largest first.
private func printResidency(_ name: String) throws {
    let report = try RasterResources.shared.report()
    let retained = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
    let transient = try PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes")
    let pixels = NSDocumentController.shared.documents
        .compactMap { ($0 as? ImageDocument)?.doc }
        .reduce(0) { $0 + $1.width * $1.height }
    let categories = (report["categories"] as? [String: Any] ?? [:])
        .compactMap { key, value -> (String, UInt64)? in
            guard let bytes = ((value as? [String: Any])?["unique_bytes"] as? NSNumber)?
                .uint64Value, bytes >= 1_000_000 else { return nil }
            return (key, bytes)
        }
        .sorted { $0.1 > $1.1 }
        .map { "\($0.0) \(mb($0.1))" }
    let perPixel = Double(retained + transient) / Double(max(pixels, 1))
    print("transform admission: \(name) residency \(mb(retained)) retained + "
          + "\(mb(transient)) transient = " + String(format: "%.2f", perPixel)
          + " B/px over \(pixels) px: " + categories.joined(separator: ", "))
}

/// The Rotate half's 4:3 fixture (multiples of 16), sized by the header's rule.
private let rotateWidth = 7040, rotateHeight = 5280
private let rotateName = "\(rotateWidth * rotateHeight / 1_000_000) MP Rotate"

/// `CanvasGeometryJob`'s per-stage admission, proven at 37 MP 4:3: the
/// Rotate Canvas core plan fits what is free and the old sum would not.
private func verifyLargeRotateAdmitted() throws {
    let document = try fixture(width: rotateWidth, height: rotateHeight)
    defer { document.close() }
    try printResidency(rotateName)
    let controller = try editor(document)
    guard let source = document.doc,
          let plan = RasterDocument.canvasRotationPlan(width: source.width,
                                                       height: source.height, degrees: 17)
    else { throw ProbeFailure(description: "[\(rotateName)] no source or plan") }
    let command = try PrecisionPixelCommand.rotateCanvas(
        degrees: 17, filter: RasterDocument.precisionFilter(DR_FILTER_BILINEAR))
    let core = try PrecisionDocumentDescriptor.integer(
        source.geometryEditPlan(request: command.request), "scratch_bytes")
    let free = try RasterResources.shared.admit(scratch: 0)
    let summed = core + UInt64(source.width) * UInt64(source.height) * 24
    try require(core <= free,
                "[\(rotateName)] the largest stage fits: \(mb(core)) of \(mb(free)) free")
    try require(summed > free, "[\(rotateName)] the fixture discriminates: the summed "
                + "\(mb(summed)) must exceed the \(mb(free)) free")
    let alerts = AutomationAlerts.recorded.count
    controller.commitCanvasRotation(degrees: 17, sampler: DR_FILTER_BILINEAR)
    try require(document.doc !== source && document.doc?.width == plan.width
                && document.doc?.height == plan.height,
                "[\(rotateName)] the rotate is admitted and publishes \(plan.width) × "
                + "\(plan.height) (alerts: \(AutomationAlerts.recorded.suffix(1)))")
    try require(AutomationAlerts.recorded.count == alerts, "[\(rotateName)] no alert")
    print("PASS: \(rotateName) 4:3 (\(rotateWidth)×\(rotateHeight)) Rotate Canvas 17°: "
          + "largest stage \(mb(core)) fits \(mb(free)) free by \(mb(free - core)); the summed "
          + "\(mb(summed)) would have refused by \(mb(summed - free))")
}

/// A 22 MP 3:2 ⌘T of the background and a text layer through MCP
/// `transform_layers` (`TransformWork.wait`).
private func verifyLargeTransformAdmitted() throws {
    let document = try fixture(width: 5760, height: 3840)
    defer { document.close() }
    try printResidency("22 MP ⌘T")
    _ = try agentOK("add_text_layer", document,
                    ["text": "Probe", "x": 400, "y": 300, "size": 160])
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    guard let source = document.doc, source.layerCount == 2, source.hasArtworkDescription(1) else {
        throw ProbeFailure(description: "[22 MP ⌘T] the text layer is not described artwork")
    }
    let matrix = turn(2880, 1920)
    let work = try TransformWork(source: source, layers: [0, 1], quad: nil, matrix: matrix,
                                 sampler: DR_FILTER_BILINEAR, droppingDescriptions: [])
    let stages = try work.stageEstimates()
    let free = try RasterResources.shared.admit(scratch: 0)
    let staged = max(stages.core, stages.artwork)
    let summed = stages.core + stages.artwork + UInt64(source.width) * UInt64(source.height) * 24
    try require(staged <= free, "[22 MP ⌘T] the largest stage fits: \(mb(staged)) of \(mb(free)) free")
    try require(summed > free, "[22 MP ⌘T] the fixture discriminates: the summed "
                + "\(mb(summed)) must exceed the \(mb(free)) free")
    let reply = try agentOK("transform_layers", document, [
        "layers": [0, 1],
        "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init),
        "sampler": "bilinear",
    ])
    try require(reply["changed"] as? Bool == true
                && (reply["rasterized_layers"] as? [Int])?.isEmpty == true,
                "[22 MP ⌘T] transform_layers is admitted and changes the set: \(reply)")
    try require((document.doc?.layerInfo(0)?.width ?? 0) > 5760,
                "[22 MP ⌘T] the background turned past 5760 px wide")
    try require(document.doc?.hasArtworkDescription(1) == true,
                "[22 MP ⌘T] the text layer stays editable")
    print("PASS: 22 MP 3:2 ⌘T 17°: largest stage \(mb(staged)) fits \(mb(free)) free by "
          + "\(mb(free - staged)); the summed \(mb(summed)) would have refused by "
          + "\(mb(summed - free))")
}

/// The 16-bit Crop half's 4:3 fixture, sized by the header's rule.
private let cropWidth = 6336, cropHeight = 4752
private let cropName = "\(cropWidth * cropHeight / 1_000_000) MP 16-bit Crop"

/// `PrecisionGeometryGesture.finishOutcome`'s per-stage admission, proven
/// at 16 bits: a 7° straighten of a 5% inset fits as its largest stage,
/// while the old core + artwork + projection sum would not, and the Crop
/// tool publishes the window with no alert.
private func verifyLargePrecisionCropAdmitted() throws {
    let document = try precisionFixture(width: cropWidth, height: cropHeight)
    defer { document.close() }
    let controller = try editor(document)
    // The default Catmull-Rom (the typed filtered turn's work is
    // bounded by `MAX_DESTRUCTIVE_WORK`, so it is no longer refused above
    // ≈ 21 MP).
    try require(controller.transformSampler == DR_FILTER_CATMULL_ROM,
                "[\(cropName)] the Crop runs at the default Catmull-Rom sampler")
    controller.beginCropSession()
    defer { controller.endCropSession() }
    let rect = CGRect(x: cropWidth / 20, y: cropHeight / 20,
                      width: cropWidth * 9 / 10, height: cropHeight * 9 / 10)
    controller.cropSession?.rect = rect
    controller.cropSession?.angle = 7
    try printResidency(cropName)
    guard let source = document.doc, source.isHighDepth, controller.cropSession != nil else {
        throw ProbeFailure(description: "[\(cropName)] no 16-bit crop session")
    }
    let work = try PrecisionGeometryWork(source: source, request: .crop(
        rect: rect, angle: 7, sampler: controller.transformSampler))
    let stages = try work.stageEstimates()
    let projection = try source.projectionScratchEstimate()
    let free = try RasterResources.shared.admit(scratch: 0)
    let staged = max(stages.core, stages.artwork)
    let summed = stages.core + stages.artwork + projection
    try require(staged <= free,
                "[\(cropName)] the largest stage fits: \(mb(staged)) of \(mb(free)) free")
    try require(summed > free, "[\(cropName)] the fixture discriminates: the summed "
                + "\(mb(summed)) must exceed the \(mb(free)) free")
    let alerts = AutomationAlerts.recorded.count
    controller.commitCropSession()
    try require(document.doc !== source && document.doc?.width == Int(rect.width)
                && document.doc?.height == Int(rect.height),
                "[\(cropName)] the Crop is admitted and publishes \(Int(rect.width)) × "
                + "\(Int(rect.height)) (alerts: \(AutomationAlerts.recorded.suffix(1)))")
    try require(AutomationAlerts.recorded.count == alerts, "[\(cropName)] no alert")
    print("PASS: \(cropName) 4:3 (\(cropWidth)×\(cropHeight)) Catmull-Rom straighten 7°: "
          + "largest stage "
          + "\(mb(staged)) fits \(mb(free)) free by \(mb(free - staged)); the summed "
          + "\(mb(summed)) would have refused by \(mb(summed - free))")
}

/// The 24 MP 16-bit Crop tool at the default Catmull-Rom sampler.
/// Two successive angle changes go through a drag tick's route
/// (`refreshPrecisionCropPreview`), the second past the renderer's debounce.
/// The true preview sends `interactive` and keeps the interactive work limit
/// (a Catmull-Rom 24 MP straighten is above it; the memory refusal batch 81
/// saw was the preview's own reservation counted twice, since fixed — see
/// H2), so each is refused at planning. The gesture then previews
/// at reduced resolution: the 6° request is held while the proxy source
/// builds, the 7° one supersedes it, and the preview that lands is the window
/// at the proxy's size (factor 2 from the commit plan's work units: 2700 ×
/// 1800), drawn into the window, with `GeometryProxyPreview.status`. The
/// commit is admitted at full size and publishes the window.
private func verifyLargePrecisionCropPreview() throws {
    let name = "24 MP 16-bit Crop preview"
    let document = try precisionFixture(width: 6000, height: 4000)
    defer { document.close() }
    let controller = try editor(document)
    try require(controller.transformSampler == DR_FILTER_CATMULL_ROM,
                "[\(name)] the Crop runs at the default Catmull-Rom sampler")
    controller.beginCropSession()
    defer { controller.endCropSession() }
    guard let gesture = controller.cropSession?.precisionGesture, let source = document.doc else {
        throw ProbeFailure(description: "[\(name)] no 16-bit crop session")
    }
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    try printResidency(name)
    let rect = CGRect(x: 300, y: 200, width: 5400, height: 3600)
    let alerts = AutomationAlerts.recorded.count
    let tick = { (angle: Double) throws in
        controller.cropSession?.rect = rect
        controller.cropSession?.angle = angle
        guard let session = controller.cropSession else {
            throw ProbeFailure(description: "[\(name)] the crop session closed")
        }
        controller.refreshPrecisionCropPreview(session)
    }
    try tick(6)
    // Past the renderer's 60 ms debounce (the 6° true preview, refused at
    // planning, never renders; its reduced one waits for the proxy source).
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    try tick(7)
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    guard let units = try PrecisionGeometryWork(source: source, request: .crop(
        rect: rect, angle: 7, sampler: controller.transformSampler)).commitWorkUnits(),
          let scale = GeometryProxyScale(width: source.width, height: source.height,
                                         workUnits: units) else {
        throw ProbeFailure(description: "[\(name)] the commit plan sizes no proxy")
    }
    let preview = controller.canvas.precisionCropPreview
    let shown = preview.map { "\($0.width)×\($0.height)" } ?? "none"
    try require(gesture.status == GeometryProxyPreview.status && gesture.imageIsReduced
                && scale.factor == 2 && preview?.width == 2700 && preview?.height == 1800
                && controller.canvas.precisionCropWindow == rect,
                "[\(name)] the preview lands reduced, factor 2, 2700 × 1800 in the window: "
                + "\(gesture.status), factor \(scale.factor), \(shown)")
    let start = DispatchTime.now().uptimeNanoseconds
    controller.commitCropSession()
    let seconds = Double(DispatchTime.now().uptimeNanoseconds - start) / 1e9
    try require(document.doc !== source && document.doc?.width == 5400
                && document.doc?.height == 3600,
                "[\(name)] the 7° Crop is admitted and publishes 5400 × 3600 "
                + "(alerts: \(AutomationAlerts.recorded.suffix(1)))")
    try require(AutomationAlerts.recorded.count == alerts, "[\(name)] no alert")
    print("PASS: \(name) Catmull-Rom straighten 6° then 7° (box 300,200,5400×3600): the "
          + "preview says \"\(GeometryProxyPreview.status)\" at \(shown) (factor \(scale.factor) "
          + "from the commit plan's \(units) work units); the commit published 5400×3600 in "
          + String(format: "%.2f s", seconds))
}

/// A handle snapshot: the report's retained, transient, preview, cache and
/// contiguous-view bytes, the host handle counts, the gesture's outstanding
/// reservations and status, and `liveHandleSummary()` grouped by line.
private func handleSnapshot(_ label: String, gesture: PrecisionGeometryGesture) throws {
    let report = try RasterResources.shared.report()
    let value = { (key: String) in (report[key] as? NSNumber)?.uint64Value ?? 0 }
    var grouped: [String: Int] = [:]
    for line in RasterResources.shared.liveHandleSummary() { grouped[line, default: 0] += 1 }
    let handles = grouped.sorted { $0.key < $1.key }
        .map { $0.value == 1 ? $0.key : "\($0.key) ×\($0.value)" }
    let counts = report.keys.filter { $0.hasPrefix("host_") && $0 != "host_tracking" }.sorted()
        .map { "\($0.dropFirst(5))=\(report[$0] ?? "-")" }
    print("transform admission: \(label): retained \(value("unique_retained_bytes")) transient "
          + "\(value("transient_estimate_bytes")) previews \(value("selection_preview_scratch_bytes")) "
          + "cache \(value("cache_bytes")) views \(value("contiguous_view_bytes")); "
          + counts.joined(separator: " ") + "; handles [" + handles.joined(separator: ", ") + "]"
          + "; outstanding \(gesture.outstandingPreviewReservations), displayed "
          + (gesture.image.map { "\($0.width)×\($0.height)" } ?? "none")
          + ", status \"\(gesture.status)\"")
}

/// Clicks the progress panel's Cancel `delay` seconds after the worker slot
/// is first seen occupied, so the core kernel is already running.
private final class DelayedCanceller {
    private let title: String
    private let delay: TimeInterval
    private var timer: Timer?
    private var occupiedAt: Date?
    private(set) var clickedAt: Date?

    init(title: String, delay: TimeInterval) {
        self.title = title
        self.delay = delay
        let timer = Timer(timeInterval: 0.01, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        self.timer = timer
    }

    private func tick() {
        guard clickedAt == nil, BackgroundWork.occupied else { return }
        let now = Date()
        if occupiedAt == nil { occupiedAt = now }
        guard let occupiedAt, now.timeIntervalSince(occupiedAt) >= delay,
              let panel = NSApp.windows.first(where: { $0.title == title && $0.isVisible }),
              let content = panel.contentView,
              let cancel = buttons(content).first(where: { !$0.isHidden && $0.title == "Cancel" })
        else { return }
        clickedAt = now
        cancel.performClick(nil)
        stop()
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }
}

/// One cancelled 24 MP commit: `commit` runs with Cancel clicked 0.4 s
/// into the job; it must end cancelled — the same document and revision, no
/// undo step, no Action, no alert, a free worker — within 1 s of the click.
private func requireCancelledWithin(_ name: String, title: String, document: ImageDocument,
                                    commit: () -> Void) throws -> Double {
    let base = document.doc
    let revision = document.revision
    let alerts = AutomationAlerts.recorded.count
    let canceller = DelayedCanceller(title: title, delay: 0.4)
    ActionRecorder.shared.start()
    commit()
    let returned = Date()
    let steps = ActionRecorder.shared.stop()
    canceller.stop()
    guard let clicked = canceller.clickedAt else {
        throw ProbeFailure(description: "[\(name)] the panel's Cancel was never clicked "
                           + "(edited: \(document.doc !== base))")
    }
    let latency = returned.timeIntervalSince(clicked)
    try require(document.doc === base && document.revision == revision
                && document.undoManager?.canUndo != true && steps.isEmpty,
                "[\(name)] Cancel published nothing and recorded nothing "
                + "(steps: \(steps.map(\.tool)))")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "[\(name)] Cancel released the worker slot")
    try require(AutomationAlerts.recorded.count == alerts,
                "[\(name)] Cancel presented no alert: \(AutomationAlerts.recorded.suffix(1))")
    try require(latency < 1, "[\(name)] Cancel reached the kernel: the commit returned "
                + String(format: "%.2f s", latency) + " after the click")
    return latency
}

/// H1: a 24 MP 16-bit ⌘T at the default Catmull-Rom. The true
/// preview keeps the interactive work limit, so it lands at reduced
/// resolution (factor 2 from the commit plan's work units: 3000 px wide) and
/// says so; a commit cancelled 0.4 s in ends
/// within 1 s of the click (the job's control reaches the kernel); the
/// uncancelled commit takes the one-shot limit and publishes a layer wider
/// than the canvas.
private func verifyLargePrecisionTransformCommit() throws {
    let name = "H1 24 MP 16-bit ⌘T"
    let started = Date()
    let document = try precisionFixture(width: 6000, height: 4000)
    defer { document.close() }
    let controller = try editor(document)
    try require(controller.transformSampler == DR_FILTER_CATMULL_ROM,
                "[\(name)] the ⌘T runs at the default Catmull-Rom sampler")
    controller.freeTransform(nil)
    defer { controller.endTransformSession() }
    guard let gesture = controller.transformSession?.precisionGesture, let source = document.doc else {
        throw ProbeFailure(description: "[\(name)] no 16-bit transform session")
    }
    controller.transformDegrees = 17
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    guard let session = controller.transformSession,
          let units = try PrecisionGeometryWork(
              source: source, request: controller.precisionTransformRequest(session))
              .commitWorkUnits(),
          let scale = GeometryProxyScale(width: source.width, height: source.height,
                                         workUnits: units) else {
        throw ProbeFailure(description: "[\(name)] the commit plan sizes no proxy")
    }
    let shown = gesture.image.map { "\($0.width)×\($0.height)" } ?? "none"
    try require(gesture.status == GeometryProxyPreview.status && gesture.imageIsReduced
                && scale.factor == 2 && gesture.image?.width == 3000,
                "[\(name)] the preview lands reduced, factor 2, 3000 px wide: "
                + "\(gesture.status), factor \(scale.factor), \(shown)")
    let transformLatency = try requireCancelledWithin(
        "\(name) cancel", title: "Transform Layer", document: document) {
        controller.commitPendingTransform()
    }
    try require(controller.isTransforming, "[\(name)] the cancelled session stays open")
    try pumpUntil({ controller.transformSession?.precisionGesture?.isPending != true }, seconds: 120)
    let alerts = AutomationAlerts.recorded.count
    let commitStart = Date()
    let committed = controller.commitTransformSession()
    let seconds = Date().timeIntervalSince(commitStart)
    try require(committed && document.doc !== source
                && (document.doc?.layerInfo(0)?.width ?? 0) > 6000,
                "[\(name)] the 17° commit is admitted and publishes a layer wider than 6000 px "
                + "(alerts: \(AutomationAlerts.recorded.suffix(1)))")
    try require(AutomationAlerts.recorded.count == alerts, "[\(name)] no alert")
    print("PASS: \(name) Catmull-Rom 17°: the preview says \"\(GeometryProxyPreview.status)\" at "
          + "\(shown) (factor \(scale.factor) from the commit plan's \(units) work units); Cancel "
          + "0.4 s into the commit returned in " + String(format: "%.0f ms", transformLatency * 1000)
          + "; the commit published a \(document.doc?.layerInfo(0)?.width ?? 0) px wide layer in "
          + String(format: "%.2f s", seconds) + "; half "
          + String(format: "%.2f s", Date().timeIntervalSince(started)))
}

/// H1's second part: a 24 MP 16-bit Crop straighten 7° at
/// Catmull-Rom, cancelled 0.4 s into its commit, ends within 1 s of the click
/// and keeps its session.
private func verifyLargePrecisionCropCancel() throws {
    let name = "H1 24 MP 16-bit Crop cancel"
    let started = Date()
    let crop = try precisionFixture(width: 6000, height: 4000)
    defer { crop.close() }
    let controller = try editor(crop)
    controller.beginCropSession()
    defer { controller.endCropSession() }
    controller.cropSession?.rect = CGRect(x: 300, y: 200, width: 5400, height: 3600)
    controller.cropSession?.angle = 7
    guard let session = controller.cropSession, let gesture = session.precisionGesture else {
        throw ProbeFailure(description: "[\(name)] no 16-bit crop session")
    }
    controller.refreshPrecisionCropPreview(session)
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    let latency = try requireCancelledWithin(name, title: "Crop", document: crop) {
        controller.commitCropSession()
    }
    try require(controller.cropSession?.precisionGesture === gesture,
                "[\(name)] the cancelled crop session stays open")
    print("PASS: \(name) Catmull-Rom straighten 7°: Cancel 0.4 s into the commit returned in "
          + String(format: "%.0f ms", latency * 1000) + "; half "
          + String(format: "%.2f s", Date().timeIntervalSince(started)))
}

/// H2: two successive bilinear angle changes of a 24 MP 16-bit Crop
/// tool session both land. The second was refused before: each preview
/// worker's `otherRetained` counted its OWN reservation, which its core plan
/// charges again (2,183,707,515 bytes = R + L + (L − 65,536) at R 481,084,739
/// and L 851,344,156); `runAsOwnLease` leaves it out on the worker's thread.
/// The commit publishes the window and every reservation drains.
private func verifyLargePrecisionCropPreviewsLand() throws {
    let name = "H2 24 MP 16-bit Crop preview (bilinear)"
    let started = Date()
    let document = try precisionFixture(width: 6000, height: 4000)
    defer { document.close() }
    let controller = try editor(document)
    controller.transformSampler = DR_FILTER_BILINEAR
    defer { controller.transformSampler = DR_FILTER_CATMULL_ROM }
    controller.beginCropSession()
    defer { controller.endCropSession() }
    guard let gesture = controller.cropSession?.precisionGesture, let source = document.doc else {
        throw ProbeFailure(description: "[\(name)] no 16-bit crop session")
    }
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    let rect = CGRect(x: 300, y: 200, width: 5400, height: 3600)
    let tick = { (angle: Double) throws in
        controller.cropSession?.rect = rect
        controller.cropSession?.angle = angle
        guard let session = controller.cropSession else {
            throw ProbeFailure(description: "[\(name)] the crop session closed")
        }
        controller.refreshPrecisionCropPreview(session)
    }
    let landed = { (angle: Double) throws in
        try require(gesture.status == "Preview current"
                    && controller.canvas.precisionCropPreview?.width == 5400
                    && controller.canvas.precisionCropPreview?.height == 3600,
                    "[\(name)] the \(Int(angle))° preview lands: \(gesture.status)")
    }
    try tick(6)
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    try landed(6)
    try handleSnapshot("\(name) S2", gesture: gesture)
    let lease = try PrecisionGeometryWork(source: source, request: .crop(
        rect: rect, angle: 7, sampler: DR_FILTER_BILINEAR), interactive: true).scratchEstimate()
    try tick(7)
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    try landed(7)
    let alerts = AutomationAlerts.recorded.count
    controller.commitCropSession()
    try require(document.doc !== source && document.doc?.width == 5400
                && document.doc?.height == 3600,
                "[\(name)] the 7° Crop publishes 5400 × 3600 "
                + "(alerts: \(AutomationAlerts.recorded.suffix(1)))")
    try require(AutomationAlerts.recorded.count == alerts, "[\(name)] no alert")
    try pumpUntil({ gesture.outstandingPreviewReservations == 0
                    && SelectionWorkBudget.scratchBytes == 0 }, seconds: 10)
    print("PASS: \(name) 6° then 7° (box 300,200,5400×3600): both previews land (L \(lease)); "
          + "the commit published 5400×3600 and the preview registry drained; half "
          + String(format: "%.2f s", Date().timeIntervalSince(started)))
}

/// The Move half's 4:3 fixture (multiples of 16), sized by the header's rule.
private let moveWidth = 8064, moveHeight = 6048
private let moveName = "\(moveWidth * moveHeight / 1_000_000) MP Move Selected"

/// `MoveSelectedPixels.run`'s move admits the core plan, where the
/// old sum added W·H·24: the plan fits what is free and the sum would not,
/// and MCP `move_selected_pixels` publishes with no alert.
private func verifyLargeMoveSelectedAdmitted() throws {
    let document = try fixture(width: moveWidth, height: moveHeight)
    defer { document.close() }
    _ = try agentOK("select_all", document)
    try printResidency(moveName)
    guard let source = document.doc, let selection = document.canvasSelection else {
        throw ProbeFailure(description: "[\(moveName)] no source or selection")
    }
    let command = try MoveSelectedPixels.command(layer: 0, dx: 16, dy: 0, duplicate: false,
                                                 bounds: selection.bounds)
    let core = try PrecisionDocumentDescriptor.integer(
        source.selectedPixelsPlan(request: command.request), "scratch_bytes")
    let free = try RasterResources.shared.admit(scratch: 0)
    let summed = core + UInt64(source.width) * UInt64(source.height) * 24
    try require(core <= free,
                "[\(moveName)] the largest stage fits: \(mb(core)) of \(mb(free)) free")
    try require(summed > free, "[\(moveName)] the fixture discriminates: the summed "
                + "\(mb(summed)) must exceed the \(mb(free)) free")
    let alerts = AutomationAlerts.recorded.count
    _ = try agentOK("move_selected_pixels", document, ["dx": 16, "dy": 0])
    try require(document.doc !== source, "[\(moveName)] the move published")
    try require(AutomationAlerts.recorded.count == alerts,
                "[\(moveName)] no alert: \(AutomationAlerts.recorded.suffix(1))")
    print("PASS: \(moveName) 4:3 (\(moveWidth)×\(moveHeight)) move_selected_pixels 16 px: "
          + "largest stage \(mb(core)) fits \(mb(free)) free by \(mb(free - core)); the summed "
          + "\(mb(summed)) would have refused by \(mb(summed - free))")
}

/// The floating-transform half's 4:3 fixture, sized by the header's rule.
private let floatWidth = 6208, floatHeight = 4656
private let floatName = "\(floatWidth * floatHeight / 1_000_000) MP floating transform"

/// The floating-transform commit admits `max(core, selection)`, where
/// the old sum was core + W·H·24 + selection: the larger stage fits and the
/// sum would not, and MCP `transform_selected_pixels` (bilinear: Catmull-Rom
/// trips `selected_pixels_transform`'s own 2e9 landing bound above ≈ 31 MP)
/// publishes over the canvas box with no alert.
private func verifyLargeTransformSelectedAdmitted() throws {
    let document = try fixture(width: floatWidth, height: floatHeight)
    defer { document.close() }
    _ = try agentOK("select_all", document)
    try printResidency(floatName)
    guard let source = document.doc, let selection = document.canvasSelection else {
        throw ProbeFailure(description: "[\(floatName)] no source or selection")
    }
    let box = CGRect(x: 0, y: 0, width: source.width, height: source.height)
    var transform = LayerTransform(pivot: CGPoint(x: box.midX, y: box.midY))
    transform.degrees = 15
    let matrix = transform.matrix
    let command = try TransformSelectedPixels.command(
        layer: 0, box: box, matrix: matrix, quad: nil, sampler: DR_FILTER_BILINEAR,
        bounds: selection.bounds, duplicate: false)
    let core = try PrecisionDocumentDescriptor.integer(
        source.selectedPixelsPlan(request: command.request), "scratch_bytes")
    let sigma = try autoreleasepool {
        try TransformedSelection(
            source: source, command: command, captured: document.selectionHistory,
            coverage: selection.maskSamples16(), box: box, matrix: matrix, quad: nil,
            sampler: DR_FILTER_BILINEAR).scratchEstimate
    }
    let free = try RasterResources.shared.admit(scratch: 0)
    let staged = max(core, sigma)
    let summed = core + UInt64(source.width) * UInt64(source.height) * 24 + sigma
    try require(staged <= free,
                "[\(floatName)] the largest stage fits: \(mb(staged)) of \(mb(free)) free "
                + "(core \(mb(core)), selection \(mb(sigma)))")
    try require(summed > free, "[\(floatName)] the fixture discriminates: the summed "
                + "\(mb(summed)) must exceed the \(mb(free)) free")
    let alerts = AutomationAlerts.recorded.count
    let reply = try agentOK("transform_selected_pixels", document,
                            ["rotate": 15, "sampler": "bilinear"])
    let replyBox = reply["box"] as? [String: Any]
    let edges = ["x", "y", "width", "height"].map { (replyBox?[$0] as? NSNumber)?.doubleValue }
    try require(edges == [0, 0, Double(source.width), Double(source.height)],
                "[\(floatName)] the transform ran over the canvas box: \(reply)")
    try require(document.doc !== source, "[\(floatName)] the transform published")
    try require(AutomationAlerts.recorded.count == alerts,
                "[\(floatName)] no alert: \(AutomationAlerts.recorded.suffix(1))")
    print("PASS: \(floatName) 4:3 (\(floatWidth)×\(floatHeight)) transform_selected_pixels "
          + "15° bilinear: largest stage \(mb(staged)) (core \(mb(core)), selection "
          + "\(mb(sigma))) fits \(mb(free)) free by \(mb(free - staged)); the summed "
          + "\(mb(summed)) would have refused by \(mb(summed - free))")
}

/// Pumps the run loop until the free working budget climbs back to
/// `baseline` (within 32 MB) and reports how long that took. The verdict is
/// load-independent: the document's memory is released. How fast it drains
/// depends on how busy the machine is (a 37 MP Rotate took over 10 s while
/// other lanes built), so the only bound is a 120 s ceiling that a real leak
/// trips and a slow drain never does; the time is printed, not asserted.
private func drain(to baseline: UInt64, after name: String) throws {
    let start = Date()
    let deadline = start.addingTimeInterval(120)
    var free = try RasterResources.shared.admit(scratch: 0)
    while free + 32_000_000 < baseline {
        guard Date() < deadline else {
            throw ProbeFailure(description: "[\(name)] its document's memory was not released "
                               + "within 120 s (a leak): \(mb(free)) free, \(mb(baseline)) before it")
        }
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        free = try RasterResources.shared.admit(scratch: 0)
    }
    let seconds = String(format: "%.2f", Date().timeIntervalSince(start))
    print("transform admission: \(mb(free)) free after the \(name) (\(mb(baseline)) before), "
          + "drained in \(seconds) s")
}

func verifyTransformLargeAdmission() throws {
    // Each half's document is closed, and its autoreleased window, undo and
    // display objects drained until the free budget is back where it was,
    // before the next fixture opens, so all three halves (and the 13 MP Crop
    // after them) see the baseline residency.
    let beforeRotate = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargeRotateAdmitted() }
    try drain(to: beforeRotate, after: rotateName)
    let beforeTransform = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargeTransformAdmitted() }
    try drain(to: beforeTransform, after: "22 MP ⌘T")
    let beforeCrop = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargePrecisionCropAdmitted() }
    try drain(to: beforeCrop, after: cropName)
    let beforePreview = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargePrecisionCropPreview() }
    try drain(to: beforePreview, after: "24 MP 16-bit Crop preview")
    let beforeH1 = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargePrecisionTransformCommit() }
    try drain(to: beforeH1, after: "H1 24 MP 16-bit ⌘T")
    let beforeCropCancel = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargePrecisionCropCancel() }
    try drain(to: beforeCropCancel, after: "H1 24 MP 16-bit Crop cancel")
    let beforeH2 = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargePrecisionCropPreviewsLand() }
    try drain(to: beforeH2, after: "H2 24 MP 16-bit Crop preview")
    let beforeMove = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargeMoveSelectedAdmitted() }
    try drain(to: beforeMove, after: moveName)
    let beforeFloat = try RasterResources.shared.admit(scratch: 0)
    try autoreleasepool { try verifyLargeTransformSelectedAdmitted() }
    try drain(to: beforeFloat, after: floatName)
}
