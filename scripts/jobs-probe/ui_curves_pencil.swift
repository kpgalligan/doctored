import AppKit

// "curves pencil": the Curves dialog's Pencil through its own controls — the Points |
// Pencil segmented control (`curves-edit-mode`), a stroke drawn with mouse events on the plot,
// Smooth (`curves-smooth`) and Apply — in the destructive arm at 8 and 16 bits and 32-bit float on
// RGB and at 8 and 16 bits on a CMYK document's ink, and in the create and edit arms of an
// adjustment layer. Every map is checked against this file's own implementation of the stroke
// (a straight run between samples, rounded) and of Smooth (entries 0 and 255 kept, the rounded
// mean of i − 2 … i + 2 clipped to the ends), never `CurvePencil` itself; points → map is
// checked against the core's own byte table (a point curve and its map render alike at 8 bits).

private let pencilCMYKProfile = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// The probe's own stroke: `map` with every input from `a.x` to `b.x` set on the straight line.
private func expectedStroke(_ map: [Int], _ a: (x: Int, y: Int), _ b: (x: Int, y: Int)) -> [Int] {
    var out = map
    let (lo, hi) = (min(a.x, b.x), max(a.x, b.x))
    for x in lo...hi {
        let y = a.x == b.x ? Double(b.y)
            : Double(a.y) + Double(b.y - a.y) * Double(x - a.x) / Double(b.x - a.x)
        out[x] = max(0, min(255, Int((y + 0.5).rounded(.down))))
    }
    return out
}

/// The probe's own Smooth.
private func expectedSmooth(_ map: [Int]) -> [Int] {
    var out = map
    for i in 1...254 {
        var sum = 0, count = 0
        for j in (i - 2)...(i + 2) where j >= 0 && j <= 255 {
            sum += map[j]
            count += 1
        }
        out[i] = Int((Double(sum) / Double(count) + 0.5).rounded(.down))
    }
    return out
}

/// A mouse event at value-space `(x, y)` on `curve`'s plot.
private func plotEvent(_ curve: CurveEditorView, _ type: NSEvent.EventType, _ x: Int, _ y: Int,
                       _ tag: String) throws -> NSEvent {
    guard let window = curve.window else { throw ProbeFailure(description: "\(tag) the curve has no window") }
    let inset: CGFloat = 8
    let side = min(curve.bounds.width, curve.bounds.height) - inset * 2
    let local = NSPoint(x: inset + CGFloat(x) / 255 * side, y: inset + CGFloat(y) / 255 * side)
    guard let event = NSEvent.mouseEvent(
        with: type, location: curve.convert(local, to: nil), modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "\(tag) could not create a mouse event")
    }
    return event
}

/// Draws one stroke through `samples` on the plot, as the mouse does.
private func drawStroke(_ curve: CurveEditorView, _ samples: [(x: Int, y: Int)], _ tag: String) throws {
    guard let first = samples.first, let last = samples.last else { return }
    curve.mouseDown(with: try plotEvent(curve, .leftMouseDown, first.x, first.y, tag))
    for sample in samples.dropFirst() {
        curve.mouseDragged(with: try plotEvent(curve, .leftMouseDragged, sample.x, sample.y, tag))
    }
    curve.mouseUp(with: try plotEvent(curve, .leftMouseUp, last.x, last.y, tag))
}

/// Opens a Curves sheet through `open` and answers it.
private func pencilSheet(_ controller: EditorViewController, _ tag: String,
                         _ open: () -> Void) throws -> CurvesAdjustmentSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    open()
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? CurvesAdjustmentSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Curves sheet") }
    return sheet
}

/// Pencil on, one stroke, Smooth: answers the composite channel's expected map.
private func pencilDraw(_ sheet: CurvesAdjustmentSheetController, _ tag: String) throws -> [Int] {
    let mode = try uiControl(sheet.view, id: "curves-edit-mode", as: NSSegmentedControl.self)
    let labels = (0..<mode.segmentCount).map { mode.label(forSegment: $0) ?? "" }
    try require(labels == ["Points", "Pencil"] && mode.selectedSegment == 0,
                "\(tag) the edit control: \(labels) at \(mode.selectedSegment)")
    let smooth = try uiControl(sheet.view, id: "curves-smooth", as: NSButton.self)
    try require(smooth.isHidden, "\(tag) Smooth is hidden in Points")
    mode.selectedSegment = 1
    try uiSend(mode, "\(tag) Pencil")
    guard let curve = uiViews(sheet.view, as: CurveEditorView.self).first else {
        throw ProbeFailure(description: "\(tag) no curve")
    }
    try require(curve.map == CurvesMeta.identityMap && !smooth.isHidden,
                "\(tag) Pencil turns the identity curve into the identity map and shows Smooth")
    // A stroke with a fast jump (40 → 200 in one drag step) and a lone press.
    let stroke: [(x: Int, y: Int)] = [(40, 220), (90, 180), (200, 30)]
    try drawStroke(curve, stroke, tag)
    var want = CurvesMeta.identityMap
    want[40] = 220
    want = expectedStroke(want, stroke[0], stroke[1])
    want = expectedStroke(want, stroke[1], stroke[2])
    try require(curve.map == want, "\(tag) the stroke wrote the straight runs between its samples")
    try drawStroke(curve, [(230, 250)], tag)
    want[230] = 250
    try require(curve.map == want, "\(tag) a lone press writes one entry")
    try uiSend(smooth, "\(tag) Smooth")
    want = expectedSmooth(want)
    try require(curve.map == want, "\(tag) Smooth is the clipped five-entry mean, ends kept")
    return want
}

private func pencilApply(_ sheet: CurvesAdjustmentSheetController, _ document: ImageDocument,
                         base: RasterDocument?, _ tag: String) throws {
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no Apply button")
    }
    try uiSend(apply, "\(tag) Apply")
    try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                    && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
}

/// One destructive arm: draw, Smooth, Apply; one "Curves" step recorded with the map.
private func pencilDestructive(_ document: ImageDocument, mapKey: String, _ tag: String) throws {
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let base = document.doc
    let entries = document.history.entries.count
    var want: [Int] = []
    let recorded = try uiRecorded {
        let sheet = try pencilSheet(controller, tag) { controller.showCurves(nil) }
        want = try pencilDraw(sheet, tag)
        try pencilApply(sheet, document, base: base, tag)
    }
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Curves",
                "\(tag) one \"Curves\" step: \(document.history.entries.map(\.name))")
    let params = recorded.first?.arguments["params"] as? [String: Any]
    try require(recorded.map(\.tool) == ["apply_filter"]
                && recorded.first?.arguments["filter"] as? String == "curves"
                && (params?[mapKey] as? [NSNumber])?.map(\.intValue) == want,
                "\(tag) the recorded step carries the drawn \(mapKey): "
                    + "\(recorded.map { "\($0.tool) \(($0.arguments["params"] as? [String: Any])?.keys.sorted() ?? [])" })")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the original")
}

/// The CMYK document of the 8-bit fixture at `depth`.
private func pencilCmykDocument(depth: Int, _ tag: String) throws -> ImageDocument {
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    var options = CmykConversionOptions()
    options.depth = depth
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: pencilCMYKProfile), destination: [:],
                               options: options, document: document)
    document.undoManager?.removeAllActions()
    guard document.doc?.isCmyk == true else {
        document.close()
        throw ProbeFailure(description: "\(tag) not CMYK")
    }
    return document
}

func verifyCurvesPencil() throws {
    let tag = "[curves pencil]"
    // Points → map is the core's own byte table: at 8 bits a point curve and its map render alike.
    let points = [CurvePoint(x: 0, y: 10), CurvePoint(x: 70, y: 150), CurvePoint(x: 190, y: 90),
                  CurvePoint(x: 255, y: 240)]
    try require(CurvePencil.map(fromPoints: CurvePoint.identity) == CurvesMeta.identityMap,
                "\(tag) the identity points are the identity map")
    let doc = try fixture(width: 64, height: 48)
    defer { doc.close() }
    guard let raster = doc.doc else { throw ProbeFailure(description: "\(tag) no fixture") }
    let byPoints = AdjustmentLayerPayload(op: .curves, params: CurvesMeta.params([.rgb: points])).json()
    let byMap = AdjustmentLayerPayload(op: .curves, params: CurvesMeta.params(
        maps: [.rgb: CurvePencil.map(fromPoints: points)])).json()
    guard let byPoints, let byMap,
          let a = raster.addingAdjustmentLayer(active: 0, name: "P", meta: byPoints, selection: nil)?
            .flattened(),
          let b = raster.addingAdjustmentLayer(active: 0, name: "M", meta: byMap, selection: nil)?
            .flattened() else {
        throw ProbeFailure(description: "\(tag) the point and map renders")
    }
    func pixels(_ image: RasterImage) -> [UInt8] {
        (0..<image.height).flatMap { y in
            (0..<image.width).flatMap { x -> [UInt8] in
                guard let p = image.pixelRGBA(x: x, y: y) else { return [] }
                return [p.r, p.g, p.b, p.a]
            }
        }
    }
    try require(pixels(a) == pixels(b) && pixels(a).count == 64 * 48 * 4,
                "\(tag) points → map renders exactly as the point curve at 8 bits")
    // The destructive arm: 8 and 16 bits, 32-bit float, CMYK ink at 8 and 16 bits.
    try pencilDestructive(try fixture(width: 160, height: 120), mapKey: "rgb_map", "\(tag) 8-bit")
    try pencilDestructive(try precisionFixture(width: 160, height: 120), mapKey: "rgb_map", "\(tag) 16-bit")
    try pencilDestructive(try floatFixture(), mapKey: "rgb_map", "\(tag) 32-bit")
    try pencilDestructive(try pencilCmykDocument(depth: 8, tag), mapKey: "cmyk_map", "\(tag) CMYK 8-bit")
    try pencilDestructive(try pencilCmykDocument(depth: 16, tag), mapKey: "cmyk_map", "\(tag) CMYK 16-bit")
    // Create, then edit: the layer reopens in map state with its map, and a second stroke edits it.
    let document = try fixture(width: 160, height: 120)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    var want: [Int] = []
    var base = document.doc
    let created = try uiRecorded {
        let sheet = try pencilSheet(controller, "\(tag) create") { controller.newAdjustmentLayerCurves(nil) }
        want = try pencilDraw(sheet, "\(tag) create")
        try pencilApply(sheet, document, base: base, "\(tag) create")
    }
    let layerParams = created.first?.arguments["params"] as? [String: Any] ?? [:]
    try require(created.map(\.tool) == ["add_adjustment_layer"]
                && (layerParams["rgb_map"] as? [NSNumber])?.map(\.intValue) == want,
                "\(tag) create records add_adjustment_layer with the map: \(created.map(\.tool))")
    guard let layer = document.doc.map({ $0.layerCount - 1 }),
          let payload = document.doc?.adjustmentPayload(layer),
          CurvesMeta.maps(in: payload)?[.rgb] == want else {
        throw ProbeFailure(description: "\(tag) the new layer holds the drawn map")
    }
    base = document.doc
    let edited = try uiRecorded {
        let sheet = try pencilSheet(controller, "\(tag) edit") { controller.editAdjustmentLayer(layer) }
        let mode = try uiControl(sheet.view, id: "curves-edit-mode", as: NSSegmentedControl.self)
        guard let curve = uiViews(sheet.view, as: CurveEditorView.self).first else {
            throw ProbeFailure(description: "\(tag) edit: no curve")
        }
        try require(mode.selectedSegment == 1 && curve.map == want,
                    "\(tag) edit reopens in Pencil with the layer's map")
        try drawStroke(curve, [(10, 60), (30, 0)], "\(tag) edit")
        want[10] = 60
        want = expectedStroke(want, (10, 60), (30, 0))
        try pencilApply(sheet, document, base: base, "\(tag) edit")
    }
    guard let after = document.doc?.adjustmentPayload(layer) else {
        throw ProbeFailure(description: "\(tag) the edited layer")
    }
    try require(edited.map(\.tool) == ["edit_adjustment_layer"] && CurvesMeta.maps(in: after)?[.rgb] == want,
                "\(tag) edit records edit_adjustment_layer with the redrawn map")
    print("PASS: \(tag) Pencil draws straight runs between drag samples, Smooth averages five entries "
        + "with the ends kept, and Apply commits the map in the destructive arm at 8, 16 and 32 bits on "
        + "RGB and on CMYK ink at 8 and 16, and in the create and edit arms; points → map equals the core's "
        + "byte table")
}
