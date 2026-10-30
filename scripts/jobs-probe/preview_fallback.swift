import AppKit

// An interactive preview is never refused for work.
//
// (a) A 32-bit float ⌘T over the interactive budget. A float document has no
//     reduced proxy (`GeometryProxyPreview`), so its preview once read
//     "Preview unavailable: typed geometry exceeds the bounded pixel-work
//     limit". Now `PrecisionGeometryGesture.request` runs the COMMIT request,
//     whose plan carries no work refusal, as the preview's cancellable job, and
//     the preview lands at full size.
// (b) The same ⌘T enlarged until the commit plan is refused for MEMORY at the
//     2 GiB test pin: the status is that plan's own sentence, not the work
//     refusal the interactive plan raised first.
// (c) A 16-bit Free Transform of the selected pixels whose landing is past the
//     2·10⁹ units the core once refused at: its own plan answers and the
//     preview lands.
//
// Sizes are chosen so the work passes 2·10⁹ units while memory admits at the
// pin: a Lanczos3 plane costs 4 channels × (6² taps + 2 visits) = 152 units a
// destination pixel, so about 13.2 MP of destination is the budget.

private let interactiveBudget: UInt64 = 2_000_000_000
private let testPin: UInt64 = 2_147_483_648

/// The committed 11F float fixture (48 × 32) opened through the app's
/// preserving path and installed with its window off-screen, as
/// `depth_targets.swift`'s private `openFloatFixture` does (the master of this
/// copy), then resized to `width` × `height` by `image_size`.
private func floatDocument(width: Int, height: Int) throws -> ImageDocument {
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    let url = root.appendingPathComponent("core/tests/fixtures/float_11f/scene.tiff")
    try require(FileManager.default.fileExists(atPath: url.path),
                "Float fixture missing at \(url.path)")
    let context = try PrecisionWorkContext()
    let precision = try RasterPrecisionDocument.openPreserving(
        url: url, workingLimit: context.limit, otherRetained: context.otherRetained)
    try require(precision.isFloat, "The float fixture did not open as a float document")
    let source = try RasterDocument(precision: precision)
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
    _ = try agentOK("image_size", document,
                    ["width": width, "height": height, "filter": "bilinear"])
    document.undoManager?.removeAllActions()
    document.updateChangeCount(.changeCleared)
    guard let resized = document.doc, resized.isFloat, resized.width == width,
          resized.height == height else {
        throw ProbeFailure(description: "The float fixture did not resize to \(width) x \(height)")
    }
    return document
}

private func verifyFloatFallback() throws {
    let tag = "[preview fallback, float ⌘T]"
    try require(RasterResources.shared.workingLimit == testPin,
                "\(tag) the case is sized for the 2 GiB test pin, not "
                + "\(RasterResources.shared.workingLimit) bytes")
    // 3900 × 2600 turned 17°: a 4490 × 3627 destination, 16.3 MP, 2.5·10⁹ units.
    let document = try floatDocument(width: 3900, height: 2600)
    defer { document.close() }
    let controller = try editor(document)
    guard let facade = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    let reserved0 = SelectionWorkBudget.scratchBytes
    controller.transformSampler = DR_FILTER_LANCZOS3
    controller.freeTransform(nil)
    var ended = false
    defer { if !ended { controller.endTransformSession() } }
    controller.transformSession?.sampler = DR_FILTER_LANCZOS3
    controller.transformDegrees = 17
    guard let session = controller.transformSession, let gesture = session.precisionGesture else {
        throw ProbeFailure(description: "\(tag) ⌘T opened no precision gesture")
    }
    // The session's own request is over the budget as a preview and plans as
    // a commit: the landed preview below can only be the fallback's.
    let request = controller.precisionTransformRequest(session)
    var refusal = ""
    do {
        _ = try PrecisionGeometryWork(source: facade, request: request, interactive: true)
            .stageEstimates()
    } catch { refusal = error.localizedDescription }
    try require(refusal.contains("pixel-work limit"),
                "\(tag) the interactive plan is refused for work: \"\(refusal)\"")
    // The commit plan is read once the preview has landed and released its
    // own reservation: while the fallback runs, its reservation is other
    // work to a plan made beside it, and the plan refuses for memory.
    try pumpUntil({ !gesture.isPending }, seconds: 240)
    try require(gesture.status == "Preview current" && gesture.image != nil
                && !gesture.imageIsReduced,
                "\(tag) the preview lands at full size: \"\(gesture.status)\"")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= reserved0 }, seconds: 20)
    let commit = try PrecisionGeometryWork(source: facade, request: request)
    guard let units = commit.commitWorkUnits(), units > interactiveBudget else {
        throw ProbeFailure(description: "\(tag) the commit plan is not over the budget: "
            + "\(commit.commitWorkUnits().map { "\($0)" } ?? "no plan")")
    }
    print("PASS: preview fallback (a) — a float ⌘T of \(units) work units, refused as a preview "
        + "(\"\(refusal)\"), lands \"\(gesture.status)\" through the commit request")

    // (b) Twice the size: a 65 MP float destination, which the commit plan
    // refuses for memory at the pin before anything is allocated.
    controller.transformSession?.transform.scaleX = 2
    controller.transformSession?.transform.scaleY = 2
    controller.transformDegrees = 17
    try pumpUntil({ !gesture.isPending }, seconds: 60)
    let status = gesture.status
    try require(status.hasPrefix("Preview unavailable:"),
                "\(tag) the enlarged preview is refused: \"\(status)\"")
    try require(!status.contains("pixel-work limit"),
                "\(tag) the refusal is the commit plan's, not the work budget's: \"\(status)\"")
    controller.endTransformSession()
    ended = true
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= reserved0 }, seconds: 20)
    print("PASS: preview fallback (b) — the same ⌘T at 200 % reports the commit plan's own "
        + "refusal: \"\(status)\"")
}

private func verifySelectedPixelsPreview() throws {
    let tag = "[preview fallback, 16-bit selected pixels]"
    // A 3400 px square on 4800 × 3400, turned 30°: the landing is the turned
    // square's box, 4644 wide, clipped to the canvas's 3400 rows.
    let (width, height, side) = (4800, 3400, 3400)
    let turned = Double(side) * (cos(Double.pi / 6) + sin(Double.pi / 6))
    let landing = min(Double(width), turned) * min(Double(height), turned)
    let units = landing * 36 * 4
    try require(units > Double(interactiveBudget),
                "\(tag) the landing is over the old bound: \(units) units")
    let document = try precisionFixture(width: width, height: height)
    defer { document.close() }
    let controller = try editor(document)
    _ = try select(document, side: side)
    let reserved0 = SelectionWorkBudget.scratchBytes
    controller.transformSampler = DR_FILTER_LANCZOS3
    controller.freeTransform(nil)
    var ended = false
    defer { if !ended { controller.endTransformSession() } }
    guard controller.transformSession?.floating != nil,
          let gesture = controller.transformSession?.precisionGesture else {
        throw ProbeFailure(description: "\(tag) ⌘T opened no 16-bit floating session")
    }
    controller.transformSession?.sampler = DR_FILTER_LANCZOS3
    controller.transformDegrees = 30
    try pumpUntil({ !gesture.isPending }, seconds: 240)
    try require(gesture.status == "Preview current" && gesture.image != nil,
                "\(tag) the preview lands: \"\(gesture.status)\"")
    controller.endTransformSession()
    ended = true
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= reserved0 }, seconds: 20)
    print("PASS: preview fallback (c) — a 16-bit selected-pixels ⌘T whose Lanczos3 landing is "
        + "\(Int(units)) units (over the 2 billion the core once refused at) lands "
        + "\"Preview current\"")
}

func verifyPreviewFallback() throws {
    try verifyFloatFallback()
    try verifySelectedPixelsPreview()
}
