import AppKit

func verifySelectionActions() throws {
    let document = try fixture(), replay = try fixture()
    defer { _ = ActionRecorder.shared.stop(); document.close(); replay.close() }
    _ = try call(document, "start_recording", ["discard": true])
    _ = try call(document, "select_freehand", ["points": [[3, 3], [20, 3], [20, 20], [3, 20]]])
    _ = try call(document, "select_color_range", ["range": "sampled", "samples": [[255, 0, 0]], "tolerance": 0, "softness": 0])
    _ = try call(document, "transform_selection", ["translate_x": 5])
    _ = try call(document, "quick_select", ["seeds": [[75, 10]], "mode": "add", "tolerance": 25, "edge": 12])
    let final = document.canvasSelection?.maskBytes()
    _ = try call(document, "stop_recording")
    let steps = ActionRecorder.shared.steps
    try require(steps.map(\.tool) == ["select_freehand", "select_color_range", "transform_selection", "quick_select"],
                "Every new selection MCP mutation records exactly one matching Action step")
    let action = Action(name: "Selection replay probe", steps: steps)
    let played = ActionPlayer.run(action, on: replay)
    try require(played.ok && replay.canvasSelection?.maskBytes() == final,
                "Recorded selection pipeline replays exact coverage on a fresh document")
    try require(replay.history.entries.count == 2 && !replay.isDocumentEdited,
                "Four-step selection Action produces one clean history transition")
    replay.undoManager?.undo()
    try require(replay.canvasSelection == nil && replay.undoManager?.canUndo != true,
                "One Undo reverses the complete selection Action")
    ActionRecorder.shared.start()
    let seeds = Array(repeating: [10, 10], count: 4096)
    for _ in 0..<50 {
        ActionRecorder.shared.recordAgentCall("quick_select", ["seeds": seeds], on: document)
    }
    let bounded = ActionRecorder.shared.stop()
    try require(bounded.filter { $0.tool == "quick_select" }.count == 49
                && bounded.count == 50 && bounded.last?.isUnrecorded == true,
                "Quick Selection seed payloads participate in the existing recording size limit")
    print("PASS: new selection commands record/replay exact coverage as one clean Undo; seed payload budget is enforced")
}
