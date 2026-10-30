import AppKit

private func geometryResult(_ tool: String, _ arguments: [String: Any],
                            _ document: ImageDocument) throws -> [String: Any] {
    let response = try call(tool, arguments, document: document)
    try require(!(try responseIsError(response)), "\(tool) refused: \(response)")
    guard let envelope = try JSONSerialization.jsonObject(with: Data(response.utf8))
            as? [String: Any],
          let content = envelope["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"]
            as? String,
          let payload = try JSONSerialization.jsonObject(with: Data(text.utf8))
            as? [String: Any] else {
        throw DoctoredCoreError(message: "Invalid MCP result for \(tool)")
    }
    return payload
}

private func geometryNoOpsDepth(_ depth: String) throws -> [String: Any] {
    let opened: (ImageDocument, EditorViewController, RasterDocument)
    switch depth {
    case "8": opened = try openByteDocument(width: 5, height: 5)
    case "16": opened = try openDocument(width: 5, height: 5)
    default: opened = try openFloatDocument()
    }
    let (document, editor, _) = opened
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }

    // A one-pixel canvas makes the stored raster symmetric at every depth;
    // centering the ruler removes the chrome change too.
    _ = try geometryResult("crop", ["x": 0, "y": 0, "width": 1, "height": 1], document)
    _ = try geometryResult("flatten_image", [:], document)
    _ = try geometryResult("set_ruler_origin", ["x": 0.5, "y": 0.5], document)
    _ = try geometryResult("select_rect", ["x": 0, "y": 0, "width": 1, "height": 1], document)
    document.undoManager?.removeAllActions()
    document.history.reset(document.captureEditingState())
    document.updateChangeCount(.changeCleared)
    // Seed one real transition and undo it. Every no-op below must preserve
    // the redo tail as well as the active handle and history cursor.
    _ = try geometryResult("add_guide", ["orientation": "vertical", "position": 0.25], document)
    document.undoManager?.undo(); pump()
    try require(document.undoManager?.canRedo == true, "\(depth): could not seed redo")
    guard let original = document.doc, let editing = document.captureEditingState() else {
        throw DoctoredCoreError(message: "The \(depth)-bit no-op fixture is unavailable")
    }
    let revision = document.revision
    let history = document.history.entries.count
    let cursor = document.history.cursor

    // The real Image-menu actions and the quarter route through the real
    // Rotate Canvas sheet must leave the handle and all editing state alone.
    editor.flipH(nil); pump()
    editor.rotate180(nil); pump()
    if depth != "float" {
        _ = try commitRotationSheet(editor, angle: 90, direction: 0, sampler: "Bicubic")
    }
    try require(document.doc === original, "\(depth): a menu no-op replaced the handle")
    try require(document.revision == revision, "\(depth): a menu no-op advanced revision")
    try require(document.history.entries.count == history, "\(depth): a menu no-op added history")
    try require(document.history.cursor == cursor && document.undoManager?.canRedo == true,
        "\(depth): a menu no-op consumed redo")
    try require(document.isCurrentEditingState(editing), "\(depth): a menu no-op changed editing state")
    try require(!document.isDocumentEdited, "\(depth): a menu no-op dirtied the document")

    // Mandatory recorder route: start/stop are MCP calls, stop has NO name.
    _ = try geometryResult("start_recording", ["discard": true], document)
    let noOps = [
        try geometryResult("flip", ["axis": "horizontal"], document),
        try geometryResult("flip", ["axis": "vertical"], document),
        try geometryResult("rotate", ["degrees": 180], document),
        try geometryResult("rotate", ["degrees": 0], document),
    ]
    for result in noOps {
        try require(result["ok"] as? Bool == true && result["changed"] as? Bool == false,
            "\(depth): exact no-op did not report changed:false")
    }
    try require(ActionRecorder.shared.steps.isEmpty,
        "\(depth): recorder state contains a suppressed no-op")
    try require(ActionRecorder.shared.recordedCanvas == nil,
        "\(depth): a suppressed no-op captured a canvas")
    let stopped = try geometryResult("stop_recording", [:], document)
    let steps = stopped["steps"] as? [[String: Any]] ?? []
    try require(steps.isEmpty, "\(depth): no-op-only recording contains \(steps.count) steps")
    try require(document.doc === original && document.revision == revision,
        "\(depth): MCP no-ops changed document identity")
    try require(document.history.entries.count == history && !document.isDocumentEdited,
        "\(depth): MCP no-ops changed history or dirty state")
    try require(document.history.cursor == cursor && document.undoManager?.canRedo == true,
        "\(depth): MCP no-ops consumed redo")
    try require(document.isCurrentEditingState(editing), "\(depth): MCP no-ops changed selection")

    // Malformed typed controls are failures rather than successful no-ops,
    // and they leave the same handle/history/redo state untouched.
    for (tool, arguments) in [
        ("flip", ["axis": "diagonal"]),
        ("rotate", ["degrees": 0, "sampler": "bogus"]),
    ] as [(String, [String: Any])] {
        let response = try call(tool, arguments, document: document)
        try require(try responseIsError(response),
            "\(depth): malformed \(tool) was reported as a no-op")
    }
    try require(document.doc === original && document.history.cursor == cursor,
        "\(depth): a typed refusal changed the document")
    try require(document.undoManager?.canRedo == true,
        "\(depth): a typed refusal consumed redo")

    // Suppression is per result: an unchanged call followed by a changed
    // exact call records exactly the changed one against the original canvas.
    _ = try geometryResult("add_guide", ["orientation": "vertical", "position": 0.25], document)
    _ = try geometryResult("start_recording", ["discard": true], document)
    _ = try geometryResult("rotate", ["degrees": 0], document)
    let changed = try geometryResult("flip", ["axis": "horizontal"], document)
    try require(changed["changed"] == nil, "\(depth): the asymmetric guide flip was a no-op")
    let mixed = try geometryResult("stop_recording", [:], document)
    let mixedSteps = mixed["steps"] as? [[String: Any]] ?? []
    try require(mixedSteps.count == 1 && mixedSteps[0]["tool"] as? String == "flip",
        "\(depth): mixed recording did not contain exactly the changed flip")
    try require(ActionRecorder.shared.recordedCanvas?.width == 1
            && ActionRecorder.shared.recordedCanvas?.height == 1,
        "\(depth): mixed recording lost the original 1x1 canvas")

    // Described layers are admitted at byte and u16 depth. Their transform
    // composes into the description and must publish one real state; float
    // keeps the exact no-op/redo/refusal/recording controls above.
    var describedContentChanged = false
    if depth != "float" {
        _ = try geometryResult("add_text_layer",
            ["text": "Turn", "x": 0, "y": 0, "size": 12], document)
        guard let describedBefore = document.doc,
              let described = (0..<describedBefore.layerCount).first(where: {
                  document.layerDescribesSource($0)
              }),
              let descriptionBefore = describedBefore.layerMeta(described) else {
            throw DoctoredCoreError(message: "\(depth): text layer has no description")
        }
        let describedRevision = document.revision
        let describedResult = try geometryResult("rotate", ["degrees": 180], document)
        try require(describedResult["changed"] == nil && document.revision == describedRevision + 1,
            "\(depth): described exact geometry was suppressed")
        guard let descriptionAfter = document.doc?.layerMeta(described) else {
            throw DoctoredCoreError(message: "\(depth): exact geometry dropped a description")
        }
        try require(descriptionAfter != descriptionBefore,
            "\(depth): exact geometry did not compose the description")
        describedContentChanged = true
    }

    return ["depth": depth, "menu_handle_unchanged": true,
            "redo_preserved": true, "typed_refusals": 2,
            "no_op_recorded_steps": steps.count, "mixed_recorded_steps": mixedSteps.count,
            "mixed_recorded_canvas": [1, 1],
            "described_content_changed": describedContentChanged]
}

func geometryNoOpsUI() throws -> [String: Any] {
    let findings = try ["8", "16", "float"].map(geometryNoOpsDepth)
    return ["case": "geometry-noops-ui", "depths": findings,
            "scope": "Real Image-menu flip/180 and quarter-sheet paths plus MCP recording; "
                + "unchanged handle, revision, history, dirty state and selection at every depth"]
}
