import AppKit

// "text on a path" (#19, EditorViewController+TextOnPath.swift): Layer ▸ Place Text on Path and
// Remove Text from Path run off screen through the editor's own methods. Their validation in
// every document kind (each mode's closed `selectors` set, the float list, `sixteenBitActions`),
// the one recorded `edit_text_layer` step each and its replay, a text-session commit that keeps
// the layer on its path, and a session line break refused with its alert recorded.

/// An open arch across a 64 × 48 fixture.
private let textOnPathArch: [[String: Any]] = [["closed": false, "anchors": [
    ["x": 6, "y": 40, "out": [18, 4]], ["x": 58, "y": 40, "in": [46, 4]],
]]]

/// The pixels, placement and description of `layer`.
private func textOnPathLayerState(_ document: ImageDocument, _ layer: Int) -> [String] {
    let info = document.doc?.layerInfo(layer)
    let pixels = document.doc?.layerPixels(layer) ?? []
    return [document.doc?.layerMeta(layer) ?? "-",
            "\(info.map { [$0.offsetX, $0.offsetY, $0.width, $0.height] } ?? [])",
            "\(pixels.count):\(pixels.reduce(0) { ($0 &* 31) &+ Int($1) })"]
}

func verifyTextOnPath() throws {
    let tag = "[text on a path]"
    var failures: [String] = []
    // Every document kind: with a one-line text layer active and a current path, Place is on
    // wherever a text layer can be made and off in Indexed and Bitmap; Remove is on once placed.
    for row in colourModeRows() {
        let (document, controller, _) = try openColourModeRow(row, tag: tag)
        defer {
            if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
            document.close()
        }
        let refusedHere = ["Indexed", "Bitmap"].contains(row.name)
        let made = try agentCall("add_text_layer", document,
                                 ["text": "Arc", "x": 4, "y": 4, "size": 12, "wrap_width": 0])
        _ = try agentOK("set_path", document, ["subpaths": textOnPathArch])
        guard let work = document.doc?.paths.workPath else {
            failures.append("\(row.name): no work path")
            continue
        }
        controller.setCurrentPath(.path(work.id))
        let place = uiValidates(controller, "placeTextOnPath:")
        let remove = uiValidates(controller, "removeTextFromPath:")
        if refusedHere {
            if place || remove { failures.append("\(row.name): an item is on (\(place), \(remove))") }
            continue
        }
        if made.isError { failures.append("\(row.name): add_text_layer refused: \(made.text)") }
        if !place { failures.append("\(row.name): Place Text on Path is off") }
        if remove { failures.append("\(row.name): Remove Text from Path is on before placing") }
        controller.placeTextOnPath(nil)
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
        if document.doc?.textPayload(document.activeLayerIndex)?.onPath == nil {
            failures.append("\(row.name): the layer is not on the path: \(uiAlerts())")
        } else if !uiValidates(controller, "removeTextFromPath:") {
            failures.append("\(row.name): Remove Text from Path is off once placed")
        }
    }
    try require(failures.isEmpty, "\(tag) \(failures.joined(separator: "; "))")

    // The recorded steps and their replay, the session commit and its line-break refusal (RGB 8).
    let document = try fixture(width: 64, height: 48)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let made = try agentOK("add_text_layer", document,
                           ["text": "Arc", "x": 4, "y": 4, "size": 12, "wrap_width": 0])
    guard let index = made["layer"] as? Int else {
        throw ProbeFailure(description: "\(tag) no text layer: \(made)")
    }
    _ = try agentOK("set_path", document, ["subpaths": textOnPathArch])
    guard let work = document.doc?.paths.workPath else {
        throw ProbeFailure(description: "\(tag) no work path")
    }
    controller.setCurrentPath(.path(work.id))
    document.undoManager?.removeAllActions()

    let unplaced = document.doc
    let placing = uiRecorded { controller.placeTextOnPath(nil) }
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
    guard let placedPayload = document.doc?.textPayload(index), let onPath = placedPayload.onPath
    else { throw ProbeFailure(description: "\(tag) Place did not place the layer: \(uiAlerts())") }
    let onPathArgument = placing.first?.arguments["on_path"] as? [String: Any]
    try require(placing.count == 1 && placing[0].tool == "edit_text_layer"
                && onPathArgument?["path_name"] as? String == PathSet.workPathName
                && onPathArgument?["subpath"] as? Int == 0,
                "\(tag) Place records one edit_text_layer step naming the work path: "
                + "\(placing.map { "\($0.tool) \($0.arguments)" })")
    try require(document.undoManager?.undoActionName == "Place Text on Path",
                "\(tag) one Place Text on Path step: \(document.undoManager?.undoActionName ?? "-")")
    let placedState = textOnPathLayerState(document, index)
    document.undoManager?.undo()
    try require(document.doc === unplaced, "\(tag) one undo takes the layer off the path")
    let replay = ActionPlayer.runSingle(placing[0], on: document, undoName: "Replay")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
    try require(replay.ok && textOnPathLayerState(document, index) == placedState,
                "\(tag) the recorded Place replays to the same layer: \(replay.failures)")

    // A session commit keeps the path; a line break keeps the session open with its alert.
    controller.selectTool(.text)
    controller.editTextLayer(index)
    let canvas: ImageCanvasView = controller.canvas
    guard let view = canvas.activeTextView else {
        throw ProbeFailure(description: "\(tag) the placed layer did not reopen")
    }
    view.setSelectedRange(NSRange(location: (view.string as NSString).length, length: 0))
    view.insertText("s", replacementRange: NSRange(location: NSNotFound, length: 0))
    try require(canvas.commitTextSession(), "\(tag) the edit committed: \(uiAlerts())")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
    guard let edited = document.doc?.textPayload(index) else {
        throw ProbeFailure(description: "\(tag) the edited layer is not text")
    }
    try require(edited.string == "Arcs" && edited.onPath == onPath,
                "\(tag) the session commit keeps the path: \(edited.string) "
                + "\(String(describing: edited.onPath))")
    controller.editTextLayer(index)
    guard let broken = canvas.activeTextView else {
        throw ProbeFailure(description: "\(tag) the layer did not reopen for the line break")
    }
    broken.setSelectedRange(NSRange(location: 1, length: 0))
    broken.insertText("\n", replacementRange: NSRange(location: NSNotFound, length: 0))
    AutomationAlerts.reset()
    let committed = canvas.commitTextSession()
    let alerts = AutomationAlerts.recorded.map { "\($0.messageText) \($0.informativeText)" }
    try require(!committed && canvas.hasActiveTextSession,
                "\(tag) a line break keeps the session open")
    try require(alerts.contains { $0.contains("Text on a path is one line") },
                "\(tag) the line break's alert is recorded: \(alerts)")
    try require(document.doc?.textPayload(index) == edited, "\(tag) the refused commit changed the layer")
    canvas.cancelTextSession()

    // Remove: one recorded step with on_path null, one undo, and its replay.
    try require(uiValidates(controller, "removeTextFromPath:"), "\(tag) Remove is on")
    let onTheArc = document.doc
    let removing = uiRecorded { controller.removeTextFromPath(nil) }
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
    try require(document.doc?.textPayload(index)?.onPath == nil,
                "\(tag) Remove takes the layer off the path: \(uiAlerts())")
    try require(removing.count == 1 && removing[0].tool == "edit_text_layer"
                && removing[0].arguments["on_path"] is NSNull,
                "\(tag) Remove records one edit_text_layer step with on_path null: "
                + "\(removing.map { "\($0.tool) \($0.arguments)" })")
    let removedState = textOnPathLayerState(document, index)
    document.undoManager?.undo()
    try require(document.doc === onTheArc, "\(tag) one undo puts the layer back on the path")
    let again = ActionPlayer.runSingle(removing[0], on: document, undoName: "Replay")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
    try require(again.ok && textOnPathLayerState(document, index) == removedState,
                "\(tag) the recorded Remove replays to the same layer: \(again.failures)")
    try require(!uiValidates(controller, "removeTextFromPath:"), "\(tag) Remove is off again")
    print("PASS: \(tag) both items in every document kind, one recorded step each that replays, "
          + "a session commit keeping the path and its line break refused")
}
