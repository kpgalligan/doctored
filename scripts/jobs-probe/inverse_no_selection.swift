import AppKit

/// Select Inverse with nothing selected is not a failure.
/// At 8-bit, 16-bit and float, with no selection: (a) the menu item stays
/// disabled; (b) MCP `modify_selection` `invert` is a no-op success that sets
/// nothing and adds no undo step; (c) an action of that one step plays
/// without a prompt, as a clean run that changes nothing; (d) with a
/// selection (8-bit), `invert` still inverts it.
func verifyInverseNoSelection() throws {
    let openers: [(String, () throws -> ImageDocument)] = [
        ("8-bit", { try fixture(width: 64, height: 48) }),
        ("16-bit", { try precisionFixture(width: 64, height: 48) }),
        ("float", { try floatFixture() }),
    ]
    for (label, open) in openers {
        let document = try open()
        defer { document.close() }
        try require(document.canvasSelection == nil, "[\(label)] the fixture has a selection")
        let controller = try editor(document)

        // (a) Select ▸ Inverse is disabled with nothing selected.
        let item = NSMenuItem(title: "", action: #selector(EditorViewController.invertSelection(_:)),
                              keyEquivalent: "")
        try require(!controller.validateUserInterfaceItem(item),
                    "[\(label) a] Inverse is enabled with nothing selected")

        // (b) MCP: a no-op success.
        let reply = try inverseCall(document)
        try require(!reply.isError && reply.result["changed"] as? Bool == false
                        && reply.result["ok"] as? Bool == true,
                    "[\(label) b] modify_selection invert answered \(reply.text)")
        try require(document.canvasSelection == nil, "[\(label) b] a selection was set")
        try require(document.undoManager?.canUndo != true, "[\(label) b] an undo step was added")

        // (c) Play: no prompt, a clean run, nothing changed.
        let base = document.doc
        var calls = 0
        let report = ActionPlayer.run(
            Action(name: "Inverse none", steps: [
                ActionStep(tool: "modify_selection", arguments: ["operation": "invert"]),
            ]), on: document, prompt: { _ in calls += 1; return .stop })
        try require(calls == 0 && report.ok && report.stepsRun == 1 && report.failures.isEmpty,
                    "[\(label) c] \(report.jsonObject()), \(calls) prompts")
        try require(document.doc === base && document.canvasSelection == nil,
                    "[\(label) c] the play changed the document or the selection")
        try require(document.undoManager?.canUndo != true, "[\(label) c] the play left an undo step")

        // (d) Regression: with a selection, invert still inverts.
        if label == "8-bit" {
            _ = try select(document, side: 16)
            let inverted = try inverseCall(document)
            let bounds = document.canvasSelection?.bounds
            try require(!inverted.isError && bounds == CGRect(x: 0, y: 0, width: 64, height: 48),
                        "[\(label) d] invert with a selection: \(inverted.text), bounds \(String(describing: bounds))")
        }
    }
    print("PASS: inverse no selection — at 8-bit, 16-bit and float the menu item is disabled, MCP "
          + "invert is a no-op success with no undo step, and Play runs it without a prompt; "
          + "with a selection invert still inverts")
}

/// `modify_selection` `invert` through `AgentServer.execute`, the door HTTP
/// MCP reaches.
private func inverseCall(_ document: ImageDocument)
    throws -> (isError: Bool, text: String, result: [String: Any]) {
    let arguments: [String: Any] = [
        "operation": "invert", "document_id": AgentServer.shared.documentID(for: document),
    ]
    let encoded = try JSONSerialization.data(withJSONObject: arguments, options: .sortedKeys)
    let raw = AgentServer.shared.execute(tool: "modify_selection",
                                         argumentsJSON: String(decoding: encoded, as: UTF8.self))
    guard let reply = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
          let content = reply["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String
    else {
        throw ProbeFailure(description: "Malformed MCP response to modify_selection: \(raw)")
    }
    let isError = reply["isError"] as? Bool == true
    let result = isError ? [:]
        : (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
    return (isError, text, result)
}
