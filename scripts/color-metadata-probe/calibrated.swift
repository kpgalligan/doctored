import AppKit

private func calibratedPixels(_ image: RasterImage?) throws -> [UInt8] {
    guard let image else { throw ProbeFailure(description: "Missing calibrated result") }
    var bytes: [UInt8] = []
    for y in 0..<image.height {
        for x in 0..<image.width {
            guard let sample = image.sample(x: x, y: y, reach: 0) else {
                throw ProbeFailure(description: "Cannot sample calibrated result")
            }
            bytes += [sample.r, sample.g, sample.b, sample.a]
        }
    }
    return bytes
}

private func calibratedDisplay(_ image: CGImage?) -> Data? {
    image?.dataProvider?.data.map { $0 as Data }
}

private func calibratedDocument(_ base: RasterDocument) throws -> ImageDocument {
    guard let image = base.layerImage(0), let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Cannot create calibrated document")
    }
    // This command-line probe pumps a bare run loop for preview callbacks,
    // without NSApplication's event/undo lifecycle. Every edit below already
    // opens an explicit group; disable automatic event grouping before the
    // first registration so a queued AppKit checkpoint cannot close a group
    // the explicit off-event helper has already closed.
    document.undoManager?.groupsByEvent = false
    document.doc = base
    document.activeLayerIndex = 0
    document.planeEditTarget = .layer
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    NSDocumentController.shared.addDocument(document)
    return document
}

private func calibratedCall(_ tool: String, _ document: ImageDocument, _ extra: [String: Any]) throws -> [String: Any] {
    var args = extra
    args["document_id"] = AgentServer.shared.documentID(for: document)
    let encoded = try JSONSerialization.data(withJSONObject: args)
    let response = AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: encoded, as: UTF8.self))
    guard let object = try JSONSerialization.jsonObject(with: Data(response.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Malformed MCP response to \(tool)")
    }
    return object
}

private func calibratedSliders(_ view: NSView) -> [NSSlider] {
    if let slider = view as? NSSlider { return [slider] }
    return view.subviews.flatMap(calibratedSliders)
}

private func calibratedSetFirstSlider(_ sheet: AdjustmentSheet, _ value: Double) throws {
    guard let slider = calibratedSliders(sheet.view).first else {
        throw ProbeFailure(description: "No adjustment slider")
    }
    slider.doubleValue = value
    _ = slider.sendAction(slider.action, to: slider.target)
}

/// Real AppKit presentation owns dismissal bookkeeping. The private parent
/// window stays offscreen and is retained until the verification finishes.
private func calibratedPresent(_ sheet: AdjustmentSheet) -> NSWindow {
    let frame = NSRect(x: 0, y: 0, width: 460, height: 320)
    let presenter = NSViewController()
    presenter.view = NSView(frame: frame)
    let window = NSWindow(contentRect: frame, styleMask: [.titled], backing: .buffered, defer: false)
    window.title = "Doctored Calibration Probe"
    window.isReleasedWhenClosed = false
    window.contentViewController = presenter
    window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    window.orderFront(nil)
    presenter.presentAsSheet(sheet)
    return window
}

private func calibratedApply(_ sheet: AdjustmentSheet) {
    withUndoGroup(sheet.document.undoManager) {
        _ = sheet.perform(NSSelectorFromString("applyClicked:"), with: nil)
    }
}

/// Real sheet controls and render callbacks, document/capture writes, MCP
/// normalization and Actions replay. Private offscreen windows own the sheets.
func verifyCalibratedOperations() throws {
    let rgba: [UInt8] = [35, 85, 125, 255, 70, 95, 150, 255,
                         120, 110, 95, 255, 160, 120, 75, 255]
    let coverage: [UInt8] = [0, 128, 255, 0]
    let scalar: [UInt8] = [40, 80, 120, 160]
    guard let image = RasterImage.from(rgba: rgba, width: 4, height: 1),
          let plain = RasterDocument.from(image: image),
          let p3 = try plain.assigningProfile(RasterProfile.builtin(.displayP3)),
          let masked = p3.addingLayerMask(0, kind: DR_MASK_FROM_SELECTION, selection: scalar),
          let base = try masked.addingChannel(name: "Calibrated", plane: scalar, width: 4, height: 1),
          let selection = CanvasSelection(shape: .mask(coverage), canvasWidth: 4, canvasHeight: 1)
    else { throw ProbeFailure(description: "Cannot make P3 calibration fixture") }
    var documents: [ImageDocument] = []
    var windows: [NSWindow] = []
    defer {
        _ = ActionRecorder.shared.stop()
        for window in windows { window.orderOut(nil) }
        for document in documents { NSDocumentController.shared.removeDocument(document) }
    }

    for (op, value) in [(AdjustmentLayerOp.exposure, 0.8), (.whiteBalance, -1e6 / 5200.0)] {
        let document = try calibratedDocument(base)
        documents.append(document)
        document.setCanvasSelection(selection)
        let canvas = ImageCanvasView(frame: .zero)
        guard let sheet = AdjustmentSheets.make(op: op, document: document, canvas: canvas, mode: .destructive),
              let capture = document.captureOperation()
        else { throw ProbeFailure(description: "Cannot make calibrated sheet/capture") }
        try require(sheet.initial.string("evaluation", default: "") == "document_v2", "New RGB sheet evaluation")
        windows.append(calibratedPresent(sheet))
        sheet.viewDidAppear()
        try calibratedSetFirstSlider(sheet, value)
        var params = sheet.currentParams()
        params["evaluation"] = "document_v2"
        guard let corrected = capture.adjustmentImage(capture.source, op: op.rawValue, params: params),
              let expected = capture.writing(corrected, selection: coverage),
              let display = calibratedDisplay(expected.flattened()?.makeCGImage(in: expected.colorSpace))
        else { throw ProbeFailure(description: "Cannot evaluate expected calibrated preview") }
        let deadline = Date(timeIntervalSinceNow: 3)
        while calibratedDisplay(canvas.previewImage) != display && Date() < deadline {
            RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.01))
        }
        try require(calibratedDisplay(canvas.previewImage) == display, "Actual \(op) sheet preview differs from captured result")
        ActionRecorder.shared.start()
        calibratedApply(sheet)
        let steps = ActionRecorder.shared.stop()
        let actual = try calibratedPixels(document.doc?.layerImage(0))
        let expectedPixels = try calibratedPixels(expected.layerImage(0))
        try require(actual == expectedPixels, "Actual \(op) sheet commit differs from preview")
        try require(document.isDocumentEdited, "Calibrated sheet did not dirty document")
        try require(steps.count == 1 && (steps.first?.arguments["params"] as? [String: Any])?["evaluation"] as? String == "document_v2",
                    "UI action omitted evaluation version")
        document.undoManager?.undo()
        try require(document.doc === base, "Calibrated sheet undo did not restore source")
        document.undoManager?.redo()
        try require(canvas.previewImage == nil, "Sheet preview was not cleared on Apply")
    }
    print("PASS calibrated UI: Exposure/WB sheet controls, actual previews, selected commits, versioned Actions and undo")

    let direct = try calibratedDocument(base)
    documents.append(direct)
    ActionRecorder.shared.start()
    let response = try calibratedCall("apply_filter", direct, ["filter": "exposure", "params": ["exposure": 0.8]])
    let steps = ActionRecorder.shared.stop()
    try require(response["isError"] as? Bool != true, "Calibrated MCP filter refused")
    try require(steps.count == 1 && (steps[0].arguments["params"] as? [String: Any])?["evaluation"] as? String == "document_v2",
                "MCP recorder omitted resolved evaluation")
    let replay = try calibratedDocument(base)
    documents.append(replay)
    let report = ActionPlayer.runSingle(steps[0], on: replay, undoName: "Calibrated Replay")
    try require(report.ok, "New calibrated action replay failed")
    let directPixels = try calibratedPixels(direct.doc?.layerImage(0))
    let replayPixels = try calibratedPixels(replay.doc?.layerImage(0))
    try require(directPixels == replayPixels, "MCP and new Action calibration differ")
    let oldReplay = try calibratedDocument(base)
    documents.append(oldReplay)
    let oldStep = ActionStep(tool: "apply_filter", arguments: ["filter": "exposure", "params": ["exposure": 0.8]])
    let oldReport = ActionPlayer.runSingle(oldStep, on: oldReplay, undoName: "Legacy Replay")
    let oldExpected = try calibratedPixels(image.applyingAdjustment(op: "exposure", params: ["exposure": 0.8]))
    let oldPixels = try calibratedPixels(oldReplay.doc?.layerImage(0))
    try require(oldReport.ok && oldExpected == oldPixels, "Old Action changed legacy evaluation")
    print("PASS calibrated MCP/Actions: normalized version persists; new replay agrees and old recordings retain legacy pixels")

    let layerDocument = try calibratedDocument(base)
    documents.append(layerDocument)
    let canvas = ImageCanvasView(frame: .zero)
    guard let create = AdjustmentSheets.make(op: .exposure, document: layerDocument, canvas: canvas, mode: .create(selection: nil)) else {
        throw ProbeFailure(description: "Cannot create adjustment-layer sheet")
    }
    windows.append(calibratedPresent(create))
    try calibratedSetFirstSlider(create, 0.8)
    calibratedApply(create)
    try require(layerDocument.doc?.adjustmentPayload(1)?.string("evaluation", default: "") == "document_v2", "New layer omitted calibrated semantics")
    guard let newLayer = layerDocument.doc,
          let legacyMeta = AdjustmentLayerPayload(op: .exposure, params: ["exposure": 0.8]).json(),
          let legacy = base.addingAdjustmentLayer(active: 0, name: "Legacy", meta: legacyMeta, selection: nil)
    else { throw ProbeFailure(description: "Cannot build described-layer fixtures") }
    let native = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("calibrated-probe-\(UUID().uuidString).rz")
    defer { try? FileManager.default.removeItem(at: native) }
    try newLayer.saveNative(to: native)
    let reopened = try RasterDocument.open(url: native)
    let newPixels = try calibratedPixels(newLayer.flattened())
    let openedPixels = try calibratedPixels(reopened.flattened())
    try require(openedPixels == newPixels && reopened.layerMeta(1) == newLayer.layerMeta(1), "Calibrated native reopen changed appearance or evaluation")
    let legacyDocument = try calibratedDocument(legacy)
    documents.append(legacyDocument)
    legacyDocument.activeLayerIndex = 1
    guard let oldPayload = legacy.adjustmentPayload(1),
          let editing = AdjustmentSheets.make(op: .exposure, document: legacyDocument, canvas: canvas,
                                              mode: .edit(layer: 1, original: oldPayload))
    else { throw ProbeFailure(description: "Cannot open legacy adjustment options") }
    windows.append(calibratedPresent(editing))
    try calibratedSetFirstSlider(editing, 1.1)
    calibratedApply(editing)
    try require(legacyDocument.doc?.adjustmentPayload(1)?.string("evaluation", default: "") == "legacy_srgb", "UI re-edit silently upgraded old layer")
    let legacyEdit = try calibratedCall("edit_adjustment_layer", legacyDocument, ["layer": 1, "params": ["exposure": 1.2]])
    try require(legacyEdit["isError"] as? Bool != true && legacyDocument.doc?.adjustmentPayload(1)?.string("evaluation", default: "") == "legacy_srgb", "MCP re-edit silently upgraded old layer")
    print("PASS calibrated layers: real create/edit sheets, native reopen and legacy UI/MCP evaluation retention")

    for target in [PaintTarget.mask, .plane(.red), .plane(.alpha), .channel(0)] {
        let scalarDocument = try calibratedDocument(base)
        documents.append(scalarDocument)
        scalarDocument.planeEditTarget = target
        guard let capture = scalarDocument.captureOperation(),
              let legacyResult = capture.source.applyingAdjustment(op: "exposure", params: ["exposure": -0.7]),
              let expected = capture.writing(legacyResult, selection: nil)
        else { throw ProbeFailure(description: "Cannot capture scalar target") }
        let scalarReply = try calibratedCall("apply_filter", scalarDocument,
            ["filter": "exposure", "target": target.agentName(in: base), "params": ["exposure": -0.7]])
        let actualImage = OperationCapture(doc: scalarDocument.doc, layerIndex: 0, target: target, selection: nil)?.source
        let expectedImage = OperationCapture(doc: expected, layerIndex: 0, target: target, selection: nil)?.source
        let actual = try calibratedPixels(actualImage)
        let wanted = try calibratedPixels(expectedImage)
        try require(scalarReply["isError"] as? Bool != true && actual == wanted, "Scalar default no longer uses legacy grayscale")
        let before = scalarDocument.doc
        let refused = try calibratedCall("apply_filter", scalarDocument,
            ["filter": "exposure", "target": target.agentName(in: base),
             "params": ["exposure": 1, "evaluation": "document_v2"]])
        try require(refused["isError"] as? Bool == true && scalarDocument.doc === before, "Scalar explicit calibration did not refuse unchanged")
    }
    var unmodelled = RasterProfile.builtin(.sRGB).prefix(128)
    unmodelled.replaceSubrange(0..<4, with: [0, 0, 0, 132])
    unmodelled.append(contentsOf: [0, 0, 0, 0])
    guard let tagged = try base.assigningProfile(Data(unmodelled)) else {
        throw ProbeFailure(description: "Cannot create unmodellable RGB profile")
    }
    let unmodelledDocument = try calibratedDocument(tagged)
    documents.append(unmodelledDocument)
    let before = unmodelledDocument.doc
    let refused = try calibratedCall("apply_filter", unmodelledDocument, ["filter": "white_balance", "params": ["temperature": 5200]])
    try require(refused["isError"] as? Bool == true && unmodelledDocument.doc === before, "LUT-context operation did not refuse unchanged")
    try require(CalibratedAdjustment.sheetRefusal(op: .whiteBalance, mode: .destructive, imageDocument: unmodelledDocument) != nil,
                "UI did not share LUT profile refusal")
    print("PASS calibrated target policy: mask/RGBA plane/channel legacy semantics, explicit scalar refusal and shared LUT-profile refusal")
}
