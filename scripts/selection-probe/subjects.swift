import AppKit

func verifySubjectTargets() throws {
    guard let directory = ProcessInfo.processInfo.environment["DR_SELECTION_PHOTO_DIR"] else {
        print("SKIP: real-photo subject targeting; pass --photos-dir from phase6_quality_fixtures.py")
        return
    }
    let image = try RasterImage.open(url: URL(fileURLWithPath: directory).appendingPathComponent("portrait.png"))
    guard let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Portrait allocation")
    }
    defer { document.close() }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    document.windowControllers.first?.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    document.windowControllers.first?.window?.orderFront(nil)
    document.undoManager?.removeAllActions()
    let native = document.doc
    let begin = Date()
    let capture = try SelectionCapture(document: document)
    let analysis = try SubjectTargeting.analyze(capture, in: document)
    try require(analysis.instanceCount > 0, "Real portrait has foreground instances")
    let all = Array(1...analysis.instanceCount)
    let coverage = try SubjectTargeting.mask(analysis, instances: all, capture: capture, in: document)
    try require(coverage.contains(255), "Multi-instance union produces foreground coverage")
    let objectsMS = Date().timeIntervalSince(begin) * 1000
    guard let index = coverage.firstIndex(where: { $0 >= 128 }) else {
        throw ProbeFailure(description: "Portrait foreground has no point")
    }
    let point = CGPoint(x: index % image.width, y: index / image.width)
    // Choose an interior mapped pixel rather than a soft edge whose coarse
    // instance cell may legitimately remain background.
    var hitPoint: CGPoint?
    for y in stride(from: 0, to: image.height, by: 4) {
        for x in stride(from: 0, to: image.width, by: 4) where hitPoint == nil {
            if analysis.subject(at: CGPoint(x: x, y: y)) != nil { hitPoint = CGPoint(x: x, y: y) }
        }
    }
    let hit = hitPoint ?? point
    try require(analysis.subject(at: hit) != nil && !analysis.subjects(intersecting:
        CGRect(x: hit.x, y: hit.y, width: 2, height: 2)).isEmpty, "Object point and region hit testing share instance map")
    let controller = try editor(document)
    let sheet = try SubjectTargetsSheetController(editor: controller, document: document)
    try present(sheet, in: controller)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    guard let table = allViews(sheet.view).compactMap({ $0 as? NSTableView }).first,
          let apply = allViews(sheet.view).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Apply" }) else {
        throw ProbeFailure(description: "Objects picker table/Apply missing")
    }
    try require(table.numberOfRows == analysis.instanceCount, "Actual picker enumerates each object")
    sheet.toggleObject(at: hit)
    try require(sheet.selectedInstances.count == analysis.instanceCount - 1, "Actual image hit toggles object membership")
    sheet.toggleObject(at: hit)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    apply.performClick(nil)
    try require(document.canvasSelection != nil && document.doc === native && !document.isDocumentEdited,
                "Actual object picker Apply commits coverage without changing native pixels")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil && document.undoManager?.canUndo != true,
                "Object picker has one selection Undo")
    let inspection = try call(document, "inspect_subjects")
    try require(inspection["instance_count"] as? Int == analysis.instanceCount && document.canvasSelection == nil,
                "MCP object inspection is read-only and reports actual count")
    _ = try call(document, "select_subjects", ["point": [hit.x, hit.y]])
    try require(document.canvasSelection != nil, "MCP point targeting selects foreground object")
    document.undoManager?.undo()
    let peopleStart = Date()
    let people = try call(document, "select_people")
    let coldMS = Date().timeIntervalSince(peopleStart) * 1000
    try require(people["target"] as? String == "all_people" && document.canvasSelection != nil,
                "Dedicated accurate person model produces explicitly aggregate people coverage")
    let selected = document.selectionHistory
    document.undoManager?.undo()
    let warmStart = Date()
    _ = try call(document, "select_people")
    let warmMS = Date().timeIntervalSince(warmStart) * 1000
    try require(document.canvasSelection?.maskBytes() == selected.selection?.maskBytes()
                && !document.isDocumentEdited, "People coverage is repeatable and selection-only")
    ActionRecorder.shared.start()
    _ = try call(document, "inspect_subjects")
    _ = try call(document, "select_subjects", ["instances": [1], "expected_revision": document.revision])
    let recorded = ActionRecorder.shared.stop()
    try require(recorded.count == 1 && recorded.first?.tool == "select_subjects"
                && recorded.first?.arguments["expected_revision"] == nil,
                "Read-only inspection is not recorded and expected image revision is not baked into replay")
    document.applyEdit("Object replay revision", record: .notACommand) { try $0.withLayerName(0, "Later revision") }
    let playback = ActionPlayer.run(Action(name: "Object targeting replay", steps: recorded), on: document)
    try require(playback.ok, "Object targeting Action replays after the native revision advances")
    guard let white = RasterImage.from(rgba: [UInt8](repeating: 255, count: 1280 * 600 * 4),
                                      width: 1280, height: 600),
          let joined = try RasterDocument.from(image: white)?
            .addingImageLayer(above: 0, image, name: "Left person")?
            .withLayerOffset(1, 20, 20)?
            .addingImageLayer(above: 1, image, name: "Right person")?
            .withLayerOffset(2, 700, 20),
          let joinedImage = joined.flattened() else {
        throw ProbeFailure(description: "Two-person source fixture")
    }
    guard let multiple = ImageDocument.makeUntitled(with: joinedImage) else {
        throw ProbeFailure(description: "Two-person document fixture")
    }
    defer { multiple.close() }
    multiple.undoManager?.groupsByEvent = false
    multiple.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(multiple)
    multiple.makeWindowControllers()
    multiple.windowControllers.first?.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    let bothCapture = try SelectionCapture(document: multiple)
    let two = try SubjectTargeting.analyze(bothCapture, in: multiple)
    try require(two.instanceCount >= 2, "Separated licensed portraits yield multiple independently targetable objects")
    let first = try SubjectTargeting.mask(two, instances: [1], capture: bothCapture, in: multiple)
    let second = try SubjectTargeting.mask(two, instances: [2], capture: bothCapture, in: multiple)
    let together = try SubjectTargeting.mask(two, instances: [1, 2], capture: bothCapture, in: multiple)
    try require(zip(first, second).contains(where: { $0 > 128 && $1 == 0 })
                && zip(first, second).contains(where: { $0 == 0 && $1 > 128 }),
                "Two foreground IDs address distinct full-resolution coverage")
    try require(zip(together, first).allSatisfy { $0 >= $1 }
                && zip(together, second).allSatisfy { $0 >= $1 },
                "Selecting multiple objects retains each object's coverage in one generated mask")
    // The person segmenter answers a mask for any image, so
    // All People must refuse a photo where no body or face is detected.
    let sRGB = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
    for name in ["fur.png", "product.jpg"] {
        let photo = try RasterImage.open(url: URL(fileURLWithPath: directory).appendingPathComponent(name))
        guard let cgImage = photo.makeCGImage(in: sRGB) else {
            throw ProbeFailure(description: "\(name) CGImage")
        }
        var refusal: String?
        do { _ = try SubjectTargeting.people(in: cgImage) } catch { refusal = error.localizedDescription }
        try require(refusal == "No people were found in this image.",
                    "All People refuses \(name), where no person is detected (got \(refusal ?? "a mask"))")
    }
    print("PASS: All People refuses fur.png and product.jpg (no human body or face detected)")
    print("PASS: actual Vision multi-instance fixture has \(two.instanceCount) objects with separate and union coverage")
    print(String(format: "PASS: licensed real-photo object picker/list/hit/region/MCP and All People; objects %.1f ms, person cold %.1f ms/warm %.1f ms", objectsMS, coldMS, warmMS))
}
