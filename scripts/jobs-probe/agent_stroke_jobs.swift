import AppKit

// 16-bit MCP brush_stroke, eraser_stroke, clone_stamp and dodge_burn layer strokes commit as
// one `DocumentJobs` job. `background: true` returns the job without a modal session and records
// one step at completion; a waiting call answers `changed: true`; a zero-pressure stroke stays a
// no-op with no undo step; 8-bit calls record exactly the arguments they were called with.
// A 16-bit `background` call records exactly what the same waiting call records — no
// pinned `layer` — so both recorded steps replay onto the ACTIVE layer, identically.

private let strokeCalls: [(String, [String: Any])] = [
    ("brush_stroke", ["points": [[20, 20], [120, 20]], "size": 12, "color": "#ff0000"]),
    ("eraser_stroke", ["points": [[20, 45], [120, 45]], "size": 12]),
    ("clone_stamp", ["points": [[20, 70], [120, 70]], "size": 12, "source_x": 20, "source_y": 110]),
    ("dodge_burn", ["points": [[20, 95], [120, 95]], "size": 12, "exposure": 80]),
]

/// The steps a recording captured while `body` ran.
private func strokeRecording(_ document: ImageDocument, _ body: () throws -> Void) throws -> [[String: Any]] {
    _ = try agentOK("start_recording", document, ["discard": true])
    try body()
    let stopped = try agentOK("stop_recording", document)
    guard let steps = stopped["steps"] as? [[String: Any]] else {
        throw ProbeFailure(description: "stop_recording returned no steps: \(stopped)")
    }
    return steps
}

/// A `background: true` call's job; a `.modalPanel` timer counts any modal
/// session the call ran.
private func backgroundStroke(_ tool: String, _ document: ImageDocument,
                              _ arguments: [String: Any]) throws -> DocumentJob {
    var arguments = arguments
    arguments["background"] = true
    var modalTicks = 0
    let timer = Timer(timeInterval: 0.001, repeats: true) { _ in modalTicks += 1 }
    RunLoop.main.add(timer, forMode: .modalPanel)
    let started = try agentOK(tool, document, arguments)
    timer.invalidate()
    try require(modalTicks == 0, "[\(tool)] a background call ran no modal session: \(modalTicks)")
    guard let id = started["job_id"] as? String, let job = DocumentJobs.find(id),
          ["running", "completed"].contains(started["state"] as? String ?? "") else {
        throw ProbeFailure(description: "[\(tool)] background call returned no running job: \(started)")
    }
    return job
}

private func verifyPrecisionStrokeJobCalls() throws {
    let document = try precisionFixture(width: 160, height: 120)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "No 16-bit base") }
    for (tool, call) in strokeCalls {
        var job: DocumentJob?
        let steps = try strokeRecording(document) {
            let started = try backgroundStroke(tool, document, call)
            try pumpUntil({ started.state != "running" })
            job = started
        }
        try require(job?.state == "completed" && document.doc !== base,
                    "[\(tool)] a 16-bit background stroke completes: \(job?.report ?? [:])")
        try require(job?.report["changed"] as? Bool == true, "[\(tool)] the job reports changed: \(job?.report ?? [:])")
        let arguments = steps.first?["arguments"] as? [String: Any] ?? [:]
        try require(steps.count == 1 && steps.first?["tool"] as? String == tool
                    && NSDictionary(dictionary: arguments).isEqual(to: call),
                    "[\(tool)] a 16-bit background stroke records exactly one step with its own "
                    + "arguments: \(steps) for \(call)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[\(tool)] the background stroke is exactly one undo")

        var waited: [String: Any] = [:]
        let waitedSteps = try strokeRecording(document) { waited = try agentOK(tool, document, call) }
        try require(waited["ok"] as? Bool == true && waited["changed"] as? Bool == true
                    && waited["storage"] != nil && document.doc !== base,
                    "[\(tool)] a waiting 16-bit stroke answers changed:true: \(waited)")
        let waitedArguments = waitedSteps.first?["arguments"] as? [String: Any] ?? [:]
        try require(waitedSteps.count == 1 && waitedSteps.first?["tool"] as? String == tool
                    && NSDictionary(dictionary: waitedArguments).isEqual(to: call),
                    "[\(tool)] a waiting 16-bit stroke records exactly one step with its own "
                    + "arguments: \(waitedSteps) for \(call)")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[\(tool)] the waiting stroke is exactly one undo")
    }

    // The zero-pressure no-op (`native-paint-probe/contracts.swift`), waiting and background.
    let zero: [String: Any] = ["points": [[12.5, 15.5], [30.5, 15.5]], "size": 1,
        "stroke_version": 1, "pressure_opacity": true, "pressures": [0, 0]]
    let waited = try agentCall("brush_stroke", document, zero)
    try require(!waited.isError && waited.result["changed"] as? Bool == false
                && document.doc === base && document.undoManager?.canUndo != true,
                "A waiting zero-pressure stroke is a no-op: \(waited.text)")
    let job = try backgroundStroke("brush_stroke", document, zero)
    try pumpUntil({ job.state != "running" })
    try require(job.state == "unchanged" && job.report["changed"] as? Bool == false
                && document.doc === base && document.undoManager?.canUndo != true,
                "A background zero-pressure stroke ends unchanged: \(job.report)")

    // The scalar strokes still wait: background is refused by name.
    let quick = try agentCall("brush_stroke", document, ["points": [[20, 20], [60, 20]], "target": "red",
                                                        "background": true])
    try require(quick.isError && quick.text.contains("background applies only to 16-bit or 32-bit float layer strokes")
                && document.doc === base,
                "A background colour-plane stroke is refused: \(quick.text)")
}

/// A 16-bit layer's four planes, alpha included (an eraser lowers alpha,
/// which `precisionLayerSamples`'s red and green would not see).
private func precisionLayerRGBA(_ doc: RasterDocument?, layer: Int) throws -> [UInt16] {
    guard let doc else { throw ProbeFailure(description: "No 16-bit document to read") }
    return try [RasterPlane.red, .green, .blue, .alpha].flatMap {
        try PrecisionScalarOperations.read(doc, target: $0.agentName, index: layer).samples
    }
}

/// With layer 1 (a copy of layer 0) active, each tool's `background`
/// and waiting 16-bit calls are recorded; with layer 0 active, each recorded
/// step replays through `ActionPlayer.runSingle` onto layer 0 alone, and the
/// two replays leave identical layer-0 samples.
private func verifyPrecisionStrokeReplays() throws {
    let document = try precisionFixture(width: 160, height: 120)
    defer { document.close() }
    _ = try editor(document)
    _ = try agentOK("duplicate_layer", document, ["index": 0])
    try require(document.activeLayerIndex == 1,
                "duplicate_layer activates the copy: \(document.activeLayerIndex)")
    guard let base = document.doc else { throw ProbeFailure(description: "No 16-bit replay base") }
    let base0 = try precisionLayerRGBA(base, layer: 0)
    let base1 = try precisionLayerRGBA(base, layer: 1)
    for (tool, call) in strokeCalls {
        let background = try strokeRecording(document) {
            let job = try backgroundStroke(tool, document, call)
            try pumpUntil({ job.state != "running" })
            try require(job.state == "completed", "[\(tool)] the recorded background stroke completes: \(job.report)")
        }
        document.undoManager?.undo()
        let waiting = try strokeRecording(document) { _ = try agentOK(tool, document, call) }
        document.undoManager?.undo()
        try require(document.doc === base, "[\(tool)] both recorded strokes are undone")
        _ = try agentOK("set_active_layer", document, ["index": 0])
        var replayed: [[UInt16]] = []
        for (label, steps) in [("background", background), ("waiting", waiting)] {
            guard steps.count == 1, let arguments = steps[0]["arguments"] as? [String: Any] else {
                throw ProbeFailure(description: "[\(tool)] the \(label) call recorded one step: \(steps)")
            }
            let report = ActionPlayer.runSingle(ActionStep(tool: tool, arguments: arguments), on: document,
                                                undoName: "Stroke Replay")
            let layer0 = try precisionLayerRGBA(document.doc, layer: 0)
            let layer1 = try precisionLayerRGBA(document.doc, layer: 1)
            try require(report.ok && layer0 != base0 && layer1 == base1,
                        "[\(tool)] the \(label) step replays onto the active layer 0 alone: ok \(report.ok), "
                        + "layer 0 changed \(layer0 != base0), layer 1 unchanged \(layer1 == base1), "
                        + "arguments \(arguments)")
            replayed.append(layer0)
            document.undoManager?.undo()
            try require(document.doc === base, "[\(tool)] the \(label) replay is one undo")
        }
        try require(replayed[0] == replayed[1],
                    "[\(tool)] the background and waiting steps replay to equal layer-0 samples")
        _ = try agentOK("set_active_layer", document, ["index": 1])
    }
}

private func verifyByteStrokeRecordings() throws {
    let document = try fixture(width: 160, height: 120)
    defer { document.close() }
    _ = try editor(document)
    for (tool, call) in strokeCalls {
        let steps = try strokeRecording(document) { _ = try agentOK(tool, document, call) }
        let recorded = steps.first?["arguments"] as? [String: Any] ?? [:]
        try require(steps.count == 1 && NSDictionary(dictionary: recorded).isEqual(to: call),
                    "[\(tool)] an 8-bit stroke records its own arguments: \(recorded) for \(call)")
        document.undoManager?.undo()
    }
}

func verifyAgentStrokeJobs() throws {
    defer { _ = ActionRecorder.shared.stop() }
    try verifyPrecisionStrokeJobCalls()
    try verifyPrecisionStrokeReplays()
    try verifyByteStrokeRecordings()
    print("PASS: agent stroke jobs — 16-bit brush, eraser, clone and dodge/burn: background returns "
        + "a job with no modal session, background and waiting each record one step with exactly "
        + "their own arguments, waiting answers changed:true, zero pressure is a no-op both ways, "
        + "a plane stroke refuses background; both recorded steps replay onto the active layer "
        + "alone (RGBA, alpha included) with equal samples; 8-bit recordings unchanged")
}
