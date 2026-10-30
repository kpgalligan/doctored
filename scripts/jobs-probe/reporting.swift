import AppKit

/// Decode the actual MCP text payload, so assertions cover returned reporting
/// fields as well as the main-thread model changes behind the tool dispatch.
private func reportingCall(_ tool: String, document: ImageDocument,
                           arguments: [String: Any] = [:]) throws -> [String: Any] {
    var arguments = arguments
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let encoded = try JSONSerialization.data(withJSONObject: arguments)
    let raw = AgentServer.shared.execute(tool: tool,
        argumentsJSON: String(decoding: encoded, as: UTF8.self))
    guard let reply = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Malformed MCP response to \(tool)")
    }
    try require(reply["isError"] as? Bool != true, "\(tool) refused: \(raw)")
    guard let content = reply["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String,
          let result = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Missing MCP JSON text from \(tool): \(raw)")
    }
    return result
}

private func requireRecordedFill(_ steps: [[String: Any]], seed: Int) throws {
    try require(steps.count == 1, "Pending fill must produce exactly one recorded step: \(steps)")
    guard let step = steps.first, let arguments = step["arguments"] as? [String: Any] else {
        throw ProbeFailure(description: "Missing recorded fill arguments")
    }
    try require(step["tool"] as? String == "content_aware_fill", "Recorded tool matches final edit")
    // Since Phase 9E (694c39f) the handler records the CANONICAL resolved
    // settings (`ContentAwareFillSettings.record(layer:)`), which spell the
    // sampling as `sample`, not the caller's legacy `sample_all_layers`.
    try require(arguments["layer"] as? Int == 0 && arguments["ring"] as? Int == 48
                && arguments["seed"] as? Int == seed && arguments["sample"] as? String == "current",
                "Recorded fill retains resolved target and exact parameters: \(arguments)")
    try require(arguments["background"] == nil && arguments["document_id"] == nil,
                "Recorded fill omits transient execution/document routing")
}

func verifyJobReporting() throws {
    let document = try fixture(width: 700, height: 600)
    defer { _ = ActionRecorder.shared.stop(); document.close() }
    _ = try editor(document)
    let metadata = "{\"type\":\"text\",\"version\":99}"
    guard let described = document.doc?.withLayerMeta(0, metadata) else {
        throw ProbeFailure(description: "Described raster fixture")
    }
    document.doc = described
    document.updateChangeCount(.changeCleared)
    _ = try select(document, side: 120)
    try require(document.layerDescribesSource(0), "Unknown text version still claims source description")

    let started = try reportingCall("content_aware_fill", document: document,
        arguments: ["ring": 48, "seed": 17, "sample_all_layers": false, "background": true])
    guard let id = started["job_id"] as? String, let job = DocumentJobs.find(id) else {
        throw ProbeFailure(description: "Background reporting job missing")
    }
    try require(started["state"] as? String == "running" && started["rasterized_text"] == nil,
                "Pending result does not claim successful rasterization")
    try require(started["selected_pixels"] as? Int == 120 * 120
                && started["ring"] as? Int == 48 && started["seed"] as? Int == 17
                // The report is `ContentAwareFillSettings.arguments(layer:)`,
                // which names the resolved `sample` mode, never the legacy
                // `sample_all_layers` alias the caller passed.
                && started["sample"] as? String == "current",
                "Fill preserves established selected/ring/seed/sample reporting: \(started)")
    try pumpUntil({ job.state != "running" })
    let completed = try reportingCall("job_status", document: document, arguments: ["job_id": id])
    try require(completed["state"] as? String == "completed"
                && completed["rasterized_text"] as? Bool == true,
                "Completed fill reports dropped text description: \(completed)")
    try require((completed["note"] as? String)?.contains("Undo restores") == true,
                "Rasterization report explains Undo restoration")
    try require(document.doc?.layerMeta(0) == nil && document.doc?.layerPixels(0) != described.layerPixels(0),
                "Successful fill changes pixels and removes obsolete source metadata")
    document.undoManager?.undo()
    try require(document.doc === described && document.doc?.layerMeta(0) == metadata
                && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "One Undo restores source description, pixels and saved identity")
    print("PASS: MCP background/completed fill reporting, described-layer rasterization flag and one-Undo restoration")

    // Both unnamed response-only Stop and named Save Recording must drain the
    // pending operation BEFORE reading/saving their step snapshots.
    let names: [String?] = [nil, "Phase Four Pending Reporting"]
    for (index, name) in names.enumerated() {
        _ = try select(document, side: 300)
        _ = try reportingCall("start_recording", document: document, arguments: ["discard": true])
        let seed = 31 + index
        let pending = try reportingCall("content_aware_fill", document: document,
            arguments: ["ring": 48, "seed": seed, "sample_all_layers": false, "background": true])
        guard let pendingID = pending["job_id"] as? String,
              let pendingJob = DocumentJobs.find(pendingID) else {
            throw ProbeFailure(description: "Pending recorded fill missing")
        }
        try require(pendingJob.state == "running", "Stop begins while recorded fill remains pending")
        let stopped = try reportingCall("stop_recording", document: document,
            arguments: name.map { ["name": $0] } ?? [:])
        try require(pendingJob.state == "completed" && DocumentJobs.active == nil
                    && !ActionRecorder.shared.isRecording,
                    "Stop Recording waits for final commit before ending the recording")
        guard let steps = stopped["steps"] as? [[String: Any]] else {
            throw ProbeFailure(description: "Stop Recording omitted returned steps")
        }
        try requireRecordedFill(steps, seed: seed)
        if let name {
            let listed = try reportingCall("list_actions", document: document, arguments: ["name": name])
            guard let action = listed["action"] as? [String: Any],
                  let savedSteps = action["steps"] as? [[String: Any]] else {
                throw ProbeFailure(description: "Named Stop Recording did not save Action steps")
            }
            try requireRecordedFill(savedSteps, seed: seed)
        }
        document.undoManager?.undo()
        try require(document.doc === described && document.undoManager?.canUndo != true,
                    "Recorded background fill still has exactly one Undo")
    }
    print("PASS: pending background fill is included exactly once by response-only and saved Stop Recording")

    // Default waiting compatibility still returns the old spot-stroke fields,
    // and described-layer reporting follows the same successful completion.
    let spot = try reportingCall("spot_heal_stroke", document: document,
        arguments: ["points": [["x": 350, "y": 300]], "size": 40, "opacity": 70,
                    "ring": 48, "seed": 5, "sample_all_layers": false])
    guard let applied = spot["applied"] as? [String: Any] else {
        throw ProbeFailure(description: "Spot result omitted applied parameters")
    }
    try require(spot["state"] as? String == "completed" && spot["points"] as? Int == 1
                && spot["rasterized_text"] as? Bool == true,
                "Spot completion preserves points and source-rasterization reporting")
    try require(applied["size"] as? Int == 40 && applied["opacity"] as? Int == 70
                && applied["ring"] as? Int == 48 && applied["seed"] as? Int == 5
                && applied["sample_all_layers"] as? Bool == false,
                "Spot completion reports exact applied settings: \(applied)")
    document.undoManager?.undo()
    try require(document.doc === described && document.doc?.layerMeta(0) == metadata
                && document.undoManager?.canUndo != true,
                "Spot rasterization and pixels undo together")
    print("PASS: waiting MCP spot-heal preserves applied settings and described-layer Undo reporting")
}
