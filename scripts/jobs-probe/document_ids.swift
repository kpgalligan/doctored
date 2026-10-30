import AppKit

// The MCP `document_id` map (`AgentDocumentIDs`). Ids are distinct and stable, resolve to
// their own document, refuse once it closes and are never reused.

/// One MCP call naming `id` explicitly, so a closed document can be asked for.
private func callByID(_ tool: String, id: Int) throws -> (isError: Bool, result: [String: Any], text: String) {
    let encoded = try JSONSerialization.data(withJSONObject: ["document_id": id])
    let raw = AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: encoded, as: UTF8.self))
    guard let reply = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
          let content = reply["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String else {
        throw ProbeFailure(description: "Malformed MCP response to \(tool): \(raw)")
    }
    if reply["isError"] as? Bool == true { return (true, [:], text) }
    let result = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] ?? [:]
    return (false, result, text)
}

func verifyAgentDocumentIDs() throws {
    let first = try fixture(width: 64, height: 48)
    var firstOpen = true
    defer { if firstOpen { first.close() } }
    let second = try fixture(width: 80, height: 40)
    defer { second.close() }
    let firstID = AgentServer.shared.documentID(for: first)
    let secondID = AgentServer.shared.documentID(for: second)
    try require(firstID != secondID, "Two open documents have distinct ids: \(firstID) \(secondID)")
    try require(AgentServer.shared.documentID(for: first) == firstID
                && AgentServer.shared.documentID(for: second) == secondID,
                "Ids are stable across calls")
    for (id, width) in [(firstID, 64), (secondID, 80)] {
        let reply = try callByID("get_document", id: id)
        try require(!reply.isError && reply.result["id"] as? Int == id && reply.result["width"] as? Int == width,
                    "get_document by id \(id) answers its own document: \(reply.text)")
    }
    first.close()
    firstOpen = false
    let closed = try callByID("get_document", id: firstID)
    try require(closed.isError && closed.text.contains("No open document has id \(firstID)"),
                "A closed document's id is refused: \(closed.text)")
    let third = try fixture(width: 32, height: 32)
    defer { third.close() }
    let thirdID = AgentServer.shared.documentID(for: third)
    try require(thirdID > max(firstID, secondID),
                "A document opened later gets a larger id: \(thirdID) after \(firstID), \(secondID)")
    let still = try callByID("get_document", id: secondID)
    try require(!still.isError && still.result["width"] as? Int == 80,
                "The open document keeps its id after another closes: \(still.text)")
    print("PASS: agent document ids — distinct, stable, resolved by id, refused once closed, "
        + "never reused")
}
