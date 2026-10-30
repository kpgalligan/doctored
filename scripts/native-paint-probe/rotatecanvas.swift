import AppKit

// Phase 12F-1b3: the UI-SOURCE half of ARBITRARY CANVAS ROTATION.
//
// An MCP drive can call `rotate`; it proves the core op, the classification
// and the tool gates. What only the REAL UI can prove lives here: that
// Image ▸ Rotate Arbitrary… VALIDATES at 8 and 16 bits and at 32-bit
// float; that the
// sheet's own angle field, direction control and sampler popup reach the edit
// its Apply button publishes; that the size label is fed by the CORE's plan;
// that the publication is ONE history step one Undo restores and is EXACTLY
// what `RasterDocument.rotatingCanvas` answers for the same numbers; that the
// recorded step carries `degrees` as a NUMBER and replays; that a quarter
// turn typed into the same sheet takes the DELEGATED route and keeps a text
// layer editable; and that a full turn dismisses without an edit.
//
// The SIGN half is here rather than in the drive on purpose: MCP `render`
// renders the DOCUMENT and never the view, so the only place the 12D VIEW
// rotation's sign and this run's CANVAS rotation's sign can be compared is a
// process that owns both.

private func rotationMenuItem(_ selector: Selector) -> NSMenuItem {
    let item = NSMenuItem(title: "Item", action: selector, keyEquivalent: "")
    item.target = nil
    return item
}

private func rotationSheets(_ editor: EditorViewController) -> [RotateCanvasSheetController] {
    (editor.presentedViewControllers ?? []).compactMap { $0 as? RotateCanvasSheetController }
}

/// The sheet just presented — the LAST one AppKit filed, for
/// `canvassize.swift`'s reason: an applied sheet can linger in
/// `presentedViewControllers` in a probe process.
private func presentedRotationSheet(_ editor: EditorViewController, after count: Int) throws
    -> RotateCanvasSheetController {
    try waitUntil({ rotationSheets(editor).count > count },
                  "The Rotate Canvas sheet never came up")
    guard let sheet = rotationSheets(editor).last else {
        throw DoctoredCoreError(message: "The Rotate Canvas sheet never came up")
    }
    _ = sheet.view
    pump()
    return sheet
}

/// The sheet's control carrying `identifier` — the probe's whole contract
/// with `RotateCanvasSheetController`'s five identifiers.
private func rotationControl<T: NSView>(_ sheet: RotateCanvasSheetController,
                                        _ name: String, _ what: String) throws -> T {
    let identifier = NSUserInterfaceItemIdentifier(name)
    guard let found = descendants(sheet.view).compactMap({ $0 as? T })
        .first(where: { $0.identifier == identifier }) else {
        throw DoctoredCoreError(message: "The Rotate Canvas sheet has no \(what) (\(name))")
    }
    return found
}

/// Types an angle, picks a direction and a sampler, and reads the size label
/// back. `direction` is 0 clockwise, 1 counterclockwise.
private func fillRotationSheet(_ sheet: RotateCanvasSheetController, angle: Double,
                               direction: Int, sampler: String) throws -> String {
    let field: NSTextField = try rotationControl(sheet, "rotate-canvas-angle", "angle field")
    let control: NSSegmentedControl =
        try rotationControl(sheet, "rotate-canvas-direction", "direction control")
    let popUp: NSPopUpButton =
        try rotationControl(sheet, "rotate-canvas-sampler", "sampler popup")
    let label: NSTextField = try rotationControl(sheet, "rotate-canvas-size", "size label")
    try require(popUp.titleOfSelectedItem == "Bicubic",
        "The sampler popup opened at \(popUp.titleOfSelectedItem ?? "nothing"), expected Bicubic")
    field.stringValue = String(angle)
    field.sendAction(field.action, to: field.target)
    control.selectedSegment = direction
    control.sendAction(control.action, to: control.target)
    popUp.selectItem(withTitle: sampler)
    pump()
    return label.stringValue
}

/// Drives the REAL sheet all the way through Apply.
func commitRotationSheet(_ editor: EditorViewController, angle: Double,
                         direction: Int, sampler: String) throws -> String {
    let open = rotationSheets(editor).count
    editor.rotateArbitrary(nil)
    let sheet = try presentedRotationSheet(editor, after: open)
    let size = try fillRotationSheet(sheet, angle: angle, direction: direction,
                                     sampler: sampler)
    let apply: NSButton = try rotationControl(sheet, "rotate-canvas-apply", "Apply button")
    try require(apply.isEnabled, "The Rotate Canvas Apply button is disabled")
    apply.performClick(nil)
    pump()
    sheet.dismiss(nil)
    pump()
    return size
}

private func placements(_ doc: RasterDocument) -> [String] {
    (0..<doc.layerCount).compactMap { index in
        guard let info = doc.layerInfo(index) else { return nil }
        return "\(info.name)@\(info.offsetX),\(info.offsetY)"
    }
}

private func guideList(_ doc: RasterDocument) -> [String] {
    (0..<doc.guideCount).compactMap { index in
        guard let guide = doc.guideInfo(index) else { return nil }
        return "\(guide.orientation.rawValue):\(guide.position)"
    }
}

@discardableResult
private func rotationMCP(_ tool: String, _ arguments: [String: Any],
                         _ document: ImageDocument) throws -> [String: Any] {
    let response = try call(tool, arguments, document: document)
    try require(!(try responseIsError(response)), "\(tool) refused: \(response)")
    guard let envelope = try JSONSerialization.jsonObject(with: Data(response.utf8))
            as? [String: Any],
          let content = envelope["content"] as? [[String: Any]],
          let text = content.first?["text"] as? String else {
        throw DoctoredCoreError(message: "Invalid MCP response: \(response)")
    }
    return (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
}

// ---------------------------------------------------- the menu validation --

private func rotationMenuValidation() throws -> [String: Any] {
    let item = rotationMenuItem(#selector(EditorViewController.rotateArbitrary(_:)))
    var answers: [String: Bool] = [:]

    let (byteDocument, byteEditor, _) = try openByteDocument(width: 40, height: 28)
    answers["byte"] = byteEditor.validateUserInterfaceItem(item)
    byteDocument.close()
    pump()

    let (typedDocument, typedEditor, _) = try openDocument(width: 40, height: 28)
    answers["typed"] = typedEditor.validateUserInterfaceItem(item)
    typedDocument.close()
    pump()

    let (floatDocument, floatEditor, _) = try openFloatDocument()
    answers["float"] = floatEditor.validateUserInterfaceItem(item)
    floatDocument.close()
    pump()

    try require(answers["byte"] == true, "Rotate Arbitrary… must validate at 8 bits")
    try require(answers["typed"] == true, "Rotate Arbitrary… must validate at 16 bits")
    try require(answers["float"] == true, "Rotate Arbitrary… must validate at 32-bit float")
    return ["validates": answers]
}

private func typedFilteredRotationCommit() throws -> [String: Any] {
    let (document, editor, before) = try openDocument(width: 40, height: 28)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    guard let reference = try before.rotatingCanvas(
        degrees: 12.5, sampler: DR_FILTER_BILINEAR, described: [])?.document else {
        throw DoctoredCoreError(message: "The typed rotation reference answered nil")
    }
    let referenceSamples = try raw(reference)
    document.undoManager?.removeAllActions()
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let label = try commitRotationSheet(
        editor, angle: 12.5, direction: 0, sampler: "Bilinear")
    let recorded = ActionRecorder.shared.stop()
    guard let after = document.doc else {
        throw DoctoredCoreError(message: "The typed sheet lost its document")
    }
    try require(label == "46 × 36 px", "The typed sheet size label was \(label)")
    try require(try raw(after) == referenceSamples,
        "The typed sheet result differs from the numeric typed rotation")
    try require(document.history.entries.count == history + 1,
        "The typed sheet did not publish exactly one history step")
    try require(recorded.count == 1 && recorded[0].tool == "rotate",
        "The typed sheet did not record one rotate step")
    document.undoManager?.undo(); pump()
    try require(document.doc?.width == 40 && document.doc?.height == 28,
        "Undo did not restore the typed canvas")

    let (twin, _, _) = try openDocument(width: 40, height: 28)
    defer { twin.close(); pump() }
    _ = try replay(recorded[0], document: twin)
    guard let replayed = twin.doc else {
        throw DoctoredCoreError(message: "The typed replay lost its document")
    }
    try require(try raw(replayed) == referenceSamples,
        "The typed recorded step did not replay sample-for-sample")
    return ["canvas": [after.width, after.height], "history_steps": 1,
            "numeric_equal": true, "recorded_replay_equal": true]
}

// --------------------------------------------- the filtered commit itself --

private func filteredRotationCommit() throws -> [String: Any] {
    let (document, editor, _) = try openByteDocument(width: 40, height: 28)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
        pump()
    }
    // Chrome and one saved channel through the REAL tools, so the reference
    // has something to move besides the layer offsets.
    try rotationMCP("add_guide", ["orientation": "vertical", "position": 3.5], document)
    try rotationMCP("add_guide", ["orientation": "horizontal", "position": 2.0], document)
    try rotationMCP("select_rect", ["x": 1, "y": 1, "width": 6, "height": 5], document)
    try rotationMCP("save_selection", ["name": "Cover"], document)
    try rotationMCP("deselect", [:], document)

    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    // The size label the sheet must show, from the CORE's own plan.
    guard let plan = RasterDocument.canvasRotationPlan(
        width: before.width, height: before.height, degrees: 12.5) else {
        throw DoctoredCoreError(message: "The core refused a 12.5 degree plan")
    }
    try require((plan.width, plan.height) == (46, 36),
        "The core plans \(plan.width) x \(plan.height) for 40 x 28 at 12.5 degrees")
    guard let reference = try before.rotatingCanvas(
        degrees: 12.5, sampler: DR_FILTER_BILINEAR, described: [])?.document else {
        throw DoctoredCoreError(message: "The reference rotatingCanvas answered nil")
    }
    let referenceHash = hashBytes(try flatBytes(reference))

    document.undoManager?.removeAllActions()
    let steps = document.history.entries.count
    let restoreOffsets = placements(before)
    let restoreGuides = guideList(before)
    let restoreRuler = before.rulerOrigin

    ActionRecorder.shared.start()
    let label = try commitRotationSheet(editor, angle: 12.5, direction: 0,
                                        sampler: "Bilinear")
    let recorded = ActionRecorder.shared.stop()
    try require(label == "46 × 36 px", "The size label read \"\(label)\", expected 46 × 36 px")

    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after the sheet")
    }
    try require((after.width, after.height) == (46, 36),
        "The sheet committed \(after.width) x \(after.height)")
    try require(hashBytes(try flatBytes(after)) == referenceHash,
        "The committed composite is not rotatingCanvas(12.5)'s")
    try require(placements(after) == placements(reference),
        "offsets \(placements(after)) != \(placements(reference))")
    try require(guideList(after) == guideList(reference),
        "guides \(guideList(after)) != \(guideList(reference))")
    try require(after.rulerOrigin == reference.rulerOrigin, "the ruler origin diverged")
    try require(document.history.entries.count == steps + 1,
        "the sheet made \(document.history.entries.count - steps) history steps")

    // ONE recorded step, `degrees` a NUMBER.
    let recordedTools: [String] = recorded.map { $0.tool }
    try require(recorded.count == 1 && recordedTools == ["rotate"],
        "The Apply click recorded \(recordedTools)")
    let arguments = recorded[0].arguments
    let rawDegrees = arguments["degrees"]
    guard let number = rawDegrees as? NSNumber, !(rawDegrees is Bool) else {
        throw DoctoredCoreError(message: "The recorded degrees is not a number")
    }
    try require(abs(number.doubleValue - 12.5) < 1e-9,
        "The recorded degrees is \(number), expected 12.5")
    try require(arguments["sampler"] as? String == "bilinear",
        "The recorded sampler is \(arguments["sampler"] ?? "nothing")")

    // One UNDO restores the canvas, the placements and the chrome.
    document.undoManager?.undo()
    pump()
    guard let undone = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after Undo")
    }
    try require((undone.width, undone.height) == (40, 28),
        "Undo left the canvas at \(undone.width) x \(undone.height)")
    try require(placements(undone) == restoreOffsets, "Undo left the offsets moved")
    try require(guideList(undone) == restoreGuides, "Undo left the guides moved")
    try require(undone.rulerOrigin == restoreRuler, "Undo left the ruler origin moved")
    document.undoManager?.redo()
    pump()

    // THE REPLAY: the recorded step through the REAL server on a twin.
    let (twin, _, _) = try openByteDocument(width: 40, height: 28)
    defer { twin.close(); pump() }
    try rotationMCP("add_guide", ["orientation": "vertical", "position": 3.5], twin)
    try rotationMCP("add_guide", ["orientation": "horizontal", "position": 2.0], twin)
    try rotationMCP("select_rect", ["x": 1, "y": 1, "width": 6, "height": 5], twin)
    try rotationMCP("save_selection", ["name": "Cover"], twin)
    try rotationMCP("deselect", [:], twin)
    _ = try replay(recorded[0], document: twin)
    guard let replayed = twin.doc else {
        throw DoctoredCoreError(message: "Owned twin handle missing")
    }
    try require(hashBytes(try flatBytes(replayed)) == referenceHash,
        "The replayed step is not hash-equal to the sheet's commit")

    // COUNTERCLOCKWISE 12.5 is numeric -12.5.
    let (ccwDocument, ccwEditor, _) = try openByteDocument(width: 40, height: 28)
    defer { ccwDocument.close(); pump() }
    _ = try commitRotationSheet(ccwEditor, angle: 12.5, direction: 1, sampler: "Bilinear")
    let (negativeDocument, _, _) = try openByteDocument(width: 40, height: 28)
    defer { negativeDocument.close(); pump() }
    guard let negativeBase = negativeDocument.doc,
          let negative = try negativeBase.rotatingCanvas(
            degrees: -12.5, sampler: DR_FILTER_BILINEAR, described: [])?.document else {
        throw DoctoredCoreError(message: "The -12.5 reference refused")
    }
    guard let ccw = ccwDocument.doc else {
        throw DoctoredCoreError(message: "Owned counterclockwise handle missing")
    }
    try require(hashBytes(try flatBytes(ccw)) == hashBytes(try flatBytes(negative)),
        "Counterclockwise 12.5 is not numeric -12.5")

    var findings: [String: Any] = [:]
    findings["canvas"] = [46, 36] as [Int]
    findings["size_label"] = label
    findings["history_steps"] = 1
    findings["equals_rotatingCanvas"] = true
    findings["recorded_degrees"] = number.doubleValue
    findings["recorded_sampler"] = "bilinear"
    findings["replay_hash_equal"] = true
    findings["counterclockwise_equals_negative"] = true
    return findings
}

// ---------------------------------- the DELEGATED quarter turn and the no-op --

private func delegatedRotationCommits() throws -> [String: Any] {
    // 90 DEGREES through the same sheet takes today's path, so a text layer
    // keeps its description and the recorded step keeps its INTEGER form.
    let (document, editor, _) = try openByteDocument(width: 40, height: 28)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
        pump()
    }
    try rotationMCP("add_text_layer",
        ["text": "Turn", "x": 4, "y": 6, "size": 12], document)
    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let described = (0..<before.layerCount).filter { document.layerDescribesSource($0) }
    try require(!described.isEmpty, "The text layer did not register a description")
    guard let reference = try before.applyingDocumentGeometry(.rotate90) else {
        throw DoctoredCoreError(message: "The reference rotate90 refused")
    }
    let referenceHash = hashBytes(try flatBytes(reference))
    document.undoManager?.removeAllActions()
    let steps = document.history.entries.count

    ActionRecorder.shared.start()
    _ = try commitRotationSheet(editor, angle: 90, direction: 0, sampler: "Nearest")
    let recorded = ActionRecorder.shared.stop()

    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after the sheet")
    }
    try require((after.width, after.height) == (28, 40),
        "A quarter turn through the sheet gave \(after.width) x \(after.height)")
    try require(hashBytes(try flatBytes(after)) == referenceHash,
        "A quarter turn through the sheet is not applyingDocumentGeometry(.rotate90)")
    try require(document.history.entries.count == steps + 1, "one history step")
    for index in described {
        try require(after.layerMeta(index) != nil,
            "The quarter turn dropped layer \(index)'s description")
    }
    let quarterTools: [String] = recorded.map { $0.tool }
    try require(recorded.count == 1 && quarterTools == ["rotate"],
        "The quarter turn recorded \(quarterTools)")
    let arguments = recorded[0].arguments
    guard let degrees = arguments["degrees"] as? NSNumber else {
        throw DoctoredCoreError(message: "The recorded degrees is missing")
    }
    try require(CFNumberIsFloatType(degrees) == false && degrees.intValue == 90,
        "The quarter turn recorded \(degrees), expected the integer 90")
    try require(arguments["sampler"] == nil,
        "The quarter turn recorded a sampler it never used")

    // 360 DEGREES dismisses with no edit at all.
    let (idleDocument, idleEditor, _) = try openByteDocument(width: 40, height: 28)
    defer { idleDocument.close(); pump() }
    guard let idleBefore = idleDocument.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    idleDocument.undoManager?.removeAllActions()
    let idleSteps = idleDocument.history.entries.count
    let size = try commitRotationSheet(idleEditor, angle: 360, direction: 0,
                                       sampler: "Bicubic")
    try require(size == "40 × 28 px",
        "A full turn's size label read \"\(size)\", expected 40 × 28 px")
    try require(idleDocument.doc === idleBefore, "A full turn replaced the document handle")
    try require(idleDocument.history.entries.count == idleSteps,
        "A full turn made \(idleDocument.history.entries.count - idleSteps) history steps")
    try require(idleDocument.undoManager?.canUndo != true, "A full turn left an Undo step")
    try require(!idleDocument.isDocumentEdited, "A full turn dirtied the document")

    var quarter: [String: Any] = [:]
    quarter["canvas"] = [28, 40] as [Int]
    quarter["recorded_degrees"] = 90
    quarter["recorded_sampler"] = NSNull()
    quarter["text_layer_still_described"] = true
    var full: [String: Any] = [:]
    full["size_label"] = size
    full["history_steps"] = 0
    full["can_undo"] = false
    full["is_dirty"] = false
    full["same_handle"] = true
    var findings: [String: Any] = [:]
    findings["quarter_turn"] = quarter
    findings["full_turn"] = full
    return findings
}

// ------------------------------------------- the SIGN, against 12D's view --

private func rotationSignAgainstView() throws -> [String: Any] {
    // THE VIEW, turned +30 through the entry `rotate-view-stroke-ui` uses.
    // `host.convert(p, from: canvas) − hostCentre` is the screen-space
    // displacement a +30 VIEW rotation gives a canvas point.
    let (viewDocument, viewEditor, _) = try openByteDocument(width: 41, height: 41)
    defer { viewDocument.close(); pump() }
    _ = try viewEditor.applyViewState(zoom: 1, center: nil)
    viewEditor.setViewRotation(30)
    viewEditor.view.layoutSubtreeIfNeeded()
    pump()
    try require(abs(viewEditor.viewRotation - 30) < 1e-9,
        "The view rotation is \(viewEditor.viewRotation), expected 30")
    let marker = CGPoint(x: 20.5, y: 2.5)
    let host = viewEditor.canvasHost.frame.size
    let inHost = viewEditor.canvasHost.convert(marker, from: viewEditor.canvas)
    let viewDelta = CGPoint(x: inHost.x - host.width / 2, y: inHost.y - host.height / 2)

    // THE CANVAS, turned +30 with Nearest, through the real op. One loud
    // marker at the top centre of a transparent canvas; its centroid minus
    // the NEW canvas centre must agree with the view's displacement.
    guard let image = RasterImage.from(
        rgba: markerCanvas(width: 41, height: 41), width: 41, height: 41),
          let source = RasterDocument.from(image: image) else {
        throw DoctoredCoreError(message: "Owned marker document unavailable")
    }
    guard let rotated = try source.rotatingCanvas(
        degrees: 30, sampler: DR_FILTER_NEAREST, described: [])?.document else {
        throw DoctoredCoreError(message: "The marker rotation answered nil")
    }
    let bytes = try flatBytes(rotated)
    var sumX = 0.0, sumY = 0.0, count = 0.0
    for y in 0..<rotated.height {
        for x in 0..<rotated.width where bytes[(y * rotated.width + x) * 4 + 3] > 0 {
            sumX += Double(x) + 0.5
            sumY += Double(y) + 0.5
            count += 1
        }
    }
    try require(count > 0, "The marker did not survive the canvas rotation")
    let canvasDelta = CGPoint(x: sumX / count - Double(rotated.width) / 2,
                              y: sumY / count - Double(rotated.height) / 2)
    let drift = hypot(canvasDelta.x - viewDelta.x, canvasDelta.y - viewDelta.y)
    try require(drift <= 1.0,
        "A +30 CANVAS rotation moves the marker to \(canvasDelta) while a +30 VIEW "
        + "rotation moves it to \(viewDelta): the two signs disagree by \(drift) px")
    var findings: [String: Any] = [:]
    findings["view_delta"] = [Double(viewDelta.x), Double(viewDelta.y)]
    findings["canvas_delta"] = [Double(canvasDelta.x), Double(canvasDelta.y)]
    findings["drift_px"] = Double(drift)
    findings["tolerance_px"] = 1.0
    return findings
}

/// A transparent canvas with ONE opaque marker at the top centre.
private func markerCanvas(width: Int, height: Int) -> [UInt8] {
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    let index = (2 * width + width / 2) * 4
    bytes[index] = 255
    bytes[index + 3] = 255
    return bytes
}

// ------------------------------------------------------------ the case --

func rotateCanvasSheetUI() throws -> [String: Any] {
    let validation = try rotationMenuValidation()
    let filtered = try filteredRotationCommit()
    let typed = try typedFilteredRotationCommit()
    let delegated = try delegatedRotationCommits()
    let sign = try rotationSignAgainstView()
    return ["case": "rotate-canvas-sheet-ui", "menu": validation,
            "filtered_commit": filtered, "typed_filtered_commit": typed,
            "delegated": delegated, "sign": sign,
            "scope": "The REAL Rotate Arbitrary… sheet driven through its five "
                + "identifiers; the committed document is compared with "
                + "RasterDocument.rotatingCanvas / applyingDocumentGeometry for the same "
                + "numbers, and the +30 CANVAS rotation's sign is compared with the 12D "
                + "+30 VIEW rotation's in the one process that owns both"]
}
