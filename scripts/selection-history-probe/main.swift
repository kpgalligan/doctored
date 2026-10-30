import AppKit
import Darwin

struct ProbeFailure: Error, CustomStringConvertible {
    let description: String
}
func require(_ ok: @autoclosure () -> Bool, _ message: String) throws {
    if !ok() { throw ProbeFailure(description: message) }
}
func makeDocument(width: Int = 12, height: Int = 10) throws -> ImageDocument {
    var rgba: [UInt8] = []
    for index in 0..<(width * height) {
        let r = UInt8(30 + (index % 100))
        let g = UInt8(50 + (index % 150))
        let b = UInt8(10 + (index % 200))
        rgba.append(contentsOf: [r, g, b, 255])
    }
    guard let image = RasterImage.from(rgba: rgba, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Could not make fixture")
    }
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    return document
}
func coverage(_ document: ImageDocument) -> [UInt8]? { document.canvasSelection?.maskBytes() }
func resetHistory(_ document: ImageDocument) {
    document.undoManager?.removeAllActions()
    document.updateChangeCount(.changeCleared)
}
func edit(_ document: ImageDocument, _ body: () throws -> Void) rethrows {
    try withUndoGroup(document.undoManager, body)
}
func makeEditor(_ document: ImageDocument) throws -> EditorViewController {
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw ProbeFailure(description: "No editor")
    }
    _ = editor.view
    return editor
}

func verifyHistory() throws {
    let document = try makeDocument()
    let editor = try makeEditor(document)
    guard let initial = CanvasSelection(shape: .rect(CGRect(x: 2, y: 2, width: 6, height: 5)),
                                        canvasWidth: 12, canvasHeight: 10),
          let soft = initial.feathered(by: 1) else { throw ProbeFailure(description: "Selection fixture") }
    edit(document) { editor.commitCanvasSelection(initial, actionName: "Rectangle", record: .notACommand) }
    edit(document) { editor.commitCanvasSelection(soft, actionName: "Feather", record: .notACommand) }
    try require(!document.isDocumentEdited && coverage(document) == soft.maskBytes(), "selection dirty/feather")
    document.undoManager?.undo()
    try require(coverage(document) == initial.maskBytes(), "undo feather")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil && editor.canvas.selection == nil, "undo rectangle")
    document.undoManager?.redo()
    document.undoManager?.redo()
    edit(document) { editor.deselect(nil) }
    try require(document.canvasSelection == nil && document.canReselect, "deselect/reselect state")
    edit(document) { editor.reselect(nil) }
    try require(coverage(document) == soft.maskBytes() && !document.isDocumentEdited, "reselect coverage/dirty")
    resetHistory(document)
    let before = coverage(document)
    edit(document) { editor.cropToSelection(nil) }
    try require(document.canvasSelection == nil && document.isDocumentEdited, "crop invalidates selection")
    document.undoManager?.undo()
    try require(document.doc.width == 12 && document.doc.height == 10, "undo crop dimensions")
    try require(coverage(document) == before && editor.canvas.selection?.maskBytes() == before,
                "undo crop restores model and canvas coverage")
    try require(!document.isDocumentEdited, "undo crop returns file clean")
    document.undoManager?.redo()
    try require(document.canvasSelection == nil, "redo crop selection")
    document.undoManager?.undo()
    resetHistory(document)
    edit(document) { editor.rotate180(nil) }
    try require(document.canvasSelection == nil && document.doc.width == 12, "same-size rotate clears selection")
    document.undoManager?.undo()
    try require(coverage(document) == before && !document.isDocumentEdited, "rotate undo selection/dirty")
    // A live document update must not discard unchanged transient coverage.
    document.beginLiveEdit()
    if let updated = try document.doc.withLayerOpacity(0, 0.5) { document.updateLiveEdit(updated) }
    edit(document) { document.endLiveEdit("Opacity", record: .notACommand) }
    try require(coverage(document) == before, "live edit selection")
    document.undoManager?.undo()
    try require(coverage(document) == before, "live undo selection")
    let liveBase = document.doc
    let liveSelection = coverage(document)
    document.beginLiveEdit()
    if let changed = try document.doc.withLayerOpacity(0, 0.7) { document.updateLiveEdit(changed) }
    let liveCurrent = document.doc
    let id = AgentServer.shared.documentID(for: document)
    for (tool, extra): (String, [String: Any]) in [
        ("flip", ["axis": "horizontal"]),
        ("select_rect", ["x": 0, "y": 0, "width": 1, "height": 1]),
        ("undo", [:]),
    ] {
        let arguments = extra.merging(["document_id": id]) { first, _ in first }
        let data = try JSONSerialization.data(withJSONObject: arguments)
        let output = AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: data, as: UTF8.self))
        let object = try JSONSerialization.jsonObject(with: Data(output.utf8)) as? [String: Any]
        try require(object?["isError"] as? Bool == true, "MCP \(tool) must refuse during live gesture")
        try require(document.doc === liveCurrent && coverage(document) == liveSelection,
                    "MCP refusal changed the live revision/selection")
    }
    if let liveBase { document.updateLiveEdit(liveBase) }
    edit(document) { document.endLiveEdit("Cancelled Live Edit", record: .notACommand) }
    let flipArgs = try JSONSerialization.data(withJSONObject: ["document_id": id, "axis": "horizontal"])
    let flipOutput = AgentServer.shared.execute(tool: "flip", argumentsJSON: String(decoding: flipArgs, as: UTF8.self))
    let flipResult = try JSONSerialization.jsonObject(with: Data(flipOutput.utf8)) as? [String: Any]
    try require(flipResult?["isError"] as? Bool != true && document.doc !== liveBase,
                "MCP mutation must resume after live gesture ends")
    print("PASS: selection undo/redo, Reselect, geometry snapshots, live edits and file-clean state")
}

func verifyQuickMask() throws {
    let document = try makeDocument(width: 8, height: 6)
    let editor = try makeEditor(document)
    let total = 48
    let base = (0..<total).map { UInt8(($0 * 11) % 256) }
    edit(document) {
        document.setCanvasSelection(CanvasSelection(shape: .mask(base), canvasWidth: 8, canvasHeight: 6))
    }
    resetHistory(document)
    ActionRecorder.shared.start()
    edit(document) { editor.toggleQuickMask(nil) }
    try require(document.quickMaskActive && editor.canvas.quickMaskActive, "Quick Mask entry")
    let raw = (0..<total).flatMap { i -> [UInt8] in
        let alpha = UInt8((i * 17) % 256)
        let gray: UInt8 = i % 3 == 0 ? 0 : alpha
        return [gray, gray, gray, alpha]
    }
    let scale: UInt8 = 173
    let stroke = raw.withUnsafeBufferPointer { pixels -> QuickMaskStroke? in
        guard let base = pixels.baseAddress else { return nil }
        return QuickMaskStroke(overlay: base, width: 8, height: 6, scale: CGFloat(scale) / 255)
    }
    guard let stroke else { throw ProbeFailure(description: "Stroke fixture") }
    let expected = (0..<total).map { i -> UInt8 in
        let gray = Int(raw[i * 4]) * Int(scale) / 255
        let alpha = Int(raw[i * 4 + 3]) * Int(scale) / 255
        return UInt8(gray + (Int(base[i]) * (255 - alpha) + 127) / 255)
    }
    var preview = base
    let previewOK = raw.withUnsafeBufferPointer { pixels -> Bool in
        guard let pointer = pixels.baseAddress else { return false }
        return RasterSelection.paintQuickMask(&preview, width: 8, height: 6, overlay: pointer, scale: scale)
    }
    try require(previewOK && preview == expected, "Quick Mask raw preview reference")
    edit(document) { editor.canvas.onQuickMaskStrokeCommit?(stroke) }
    try require(coverage(document) == expected && document.quickMaskActive, "UI stroke canonical result")
    document.undoManager?.undo()
    try require(coverage(document) == base && document.quickMaskActive && editor.canvas.quickMaskActive,
                "undo individual stroke retains Quick Mask")
    document.undoManager?.redo()
    try require(coverage(document) == expected, "redo individual stroke")
    edit(document) { editor.toggleQuickMask(nil) }
    let steps = ActionRecorder.shared.stop()
    try require(steps.map(\.tool) == ["set_quick_mask", "quick_mask_stroke", "set_quick_mask"],
                "canonical stroke recording without placeholder")
    try require(!document.isDocumentEdited && editor.canvas.selection?.maskBytes() == expected,
                "Quick Mask exit visible selection stays file clean")
    let replay = try makeDocument(width: 8, height: 6)
    _ = try makeEditor(replay)
    edit(replay) { replay.setCanvasSelection(CanvasSelection(shape: .mask(base), canvasWidth: 8, canvasHeight: 6)) }
    resetHistory(replay)
    let action = ActionRecorder.shared.recording(named: "Quick Mask Probe")
    let report = ActionPlayer.run(action, on: replay)
    try require(report.ok && report.failures.isEmpty, "Quick Mask replay: \(report.failures)")
    try require(coverage(replay) == expected && !replay.isDocumentEdited, "UI/Actions Quick Mask byte equivalence")
    replay.undoManager?.undo()
    try require(coverage(replay) == base && !replay.quickMaskActive && !replay.isDocumentEdited,
                "selection-only action is one clean undo")
    replay.undoManager?.redo()
    try require(coverage(replay) == expected && !replay.isDocumentEdited, "selection-only action redo")
    var rejected = false
    do { _ = try QuickMaskStroke(width: 8, height: 6, base64: "AQAAAAAA") }
    catch { rejected = true }
    try require(rejected && coverage(replay) == expected, "malformed short coverage rejected")
    let noOp = try makeDocument(width: 8, height: 6)
    guard let rectangle = CanvasSelection(shape: .rect(CGRect(x: 0, y: 0, width: 8, height: 6)),
                                           canvasWidth: 8, canvasHeight: 6) else {
        throw ProbeFailure(description: "No-op rectangle fixture")
    }
    edit(noOp) { noOp.setCanvasSelection(rectangle) }
    resetHistory(noOp)
    let priorState = noOp.selectionHistory
    ActionRecorder.shared.start()
    let zeroStroke = try QuickMaskStroke(width: 8, height: 6,
                                         base64: Data([48, 0, 0, 0, 0, 0]).base64EncodedString())
    try noOp.applyQuickMaskStroke(zeroStroke, record: zeroStroke.actionSteps)
    let whiteStroke = try QuickMaskStroke(width: 8, height: 6,
                                          base64: Data([48, 0, 0, 0, 255, 255]).base64EncodedString())
    try noOp.applyQuickMaskStroke(whiteStroke, record: whiteStroke.actionSteps)
    noOp.setCanvasSelection(rectangle)
    let noOpSteps = ActionRecorder.shared.stop()
    try require(noOp.selectionHistory == priorState && noOpSteps.isEmpty,
                "no-op Quick Mask must preserve geometric selection/history and record no command")
    try require(noOp.undoManager?.canUndo != true && !noOp.isDocumentEdited,
                "no-op Quick Mask and repeated selection must create no empty undo groups")
    print("PASS: Quick Mask per-stroke undo, raw/RLE/reference equivalence, Action recording/replay")
}

func verifyFeatheredCombineReplay() throws {
    let document = try makeDocument(width: 20, height: 16)
    let editor = try makeEditor(document)
    let initialShape = CGRect(x: 1, y: 2, width: 7, height: 8)
    let newShape = CGRect(x: 10, y: 5, width: 6, height: 6)
    guard let initial = CanvasSelection(shape: .rect(initialShape), canvasWidth: 20, canvasHeight: 16),
          let next = CanvasSelection(shape: .rect(newShape), canvasWidth: 20, canvasHeight: 16)?.feathered(by: 1)
    else { throw ProbeFailure(description: "Combine fixture") }
    ActionRecorder.shared.start()
    edit(document) {
        editor.commitCanvasSelection(initial, actionName: "Rectangle", record: .selectShape(
            "select_rect", ["x": 1, "y": 2, "width": 7, "height": 8], mode: "replace", feather: 0))
    }
    edit(document) {
        editor.commitCanvasSelection(CanvasSelection.combine(initial, with: next, mode: .add),
                                     actionName: "Add Feathered Rectangle", record: .selectShape(
            "select_rect", ["x": 10, "y": 5, "width": 6, "height": 6], mode: "add", feather: 1))
    }
    let steps = ActionRecorder.shared.stop()
    try require(steps.count == 2 && steps.allSatisfy { $0.tool == "select_rect" }, "single-step feather-before-combine")
    let replay = try makeDocument(width: 20, height: 16)
    _ = try makeEditor(replay)
    let report = ActionPlayer.run(ActionRecorder.shared.recording(named: "Feathered Combine"), on: replay)
    try require(report.ok && coverage(document) == coverage(replay), "feather-before-combine Action equivalence")
    print("PASS: feathered additive selection records/replays before combination")
}

setbuf(stdout, nil)
_ = NSApplication.shared
AutomationAlerts.installIfEnabled()
do {
    try verifyHistory()
    try verifyQuickMask()
    try verifyFeatheredCombineReplay()
    try verifyOperationScope()
    print("PASS: all Phase 2 AppKit source probes")
} catch {
    print("FAIL: \(error)")
    exit(1)
}
