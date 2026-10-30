import AppKit

func transformResponse(_ name: String, document: ImageDocument,
                               arguments: [String: Any]) throws -> (response: [String: Any], raw: String) {
    var arguments = arguments
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let data = try JSONSerialization.data(withJSONObject: arguments)
    let raw = AgentServer.shared.execute(tool: name, argumentsJSON: String(decoding: data, as: UTF8.self))
    guard let response = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Malformed transform tool response")
    }
    return (response, raw)
}

private func transformCall(_ name: String, document: ImageDocument,
                           arguments: [String: Any]) throws -> [String: Any] {
    let (response, raw) = try transformResponse(name, document: document, arguments: arguments)
    try require(response["isError"] as? Bool != true, "\(name) refused: \(raw)")
    return response
}

func verifyTransformJobs() throws {
    let document = try fixture(width: 1600, height: 1200)
    defer { _ = ActionRecorder.shared.stop(); document.close() }
    let controller = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "No transform source") }
    controller.freeTransform(nil)
    controller.transformDegrees = 17
    controller.transformScaleXPercent = 90
    controller.transformScaleYPercent = 90
    controller.transformSamplerListIndex = 1
    guard let session = controller.transformSession,
          let expected = try base.transformLayers(session.layers, session.transform.matrix, sampler: session.sampler)
    else { throw ProbeFailure(description: "Actual transform session did not open") }
    // A tick counts only while `BackgroundWork.wait` runs, and the
    // first one opens `gate`. The worker queue is held on the gate (capped at
    // 10 s) ahead of the commit's `WorkerTask`, and the queue is serial, so
    // the commit cannot finish before main has serviced a tick inside the
    // wait, however fast the transform is. A wait that never services the
    // run loop still fails, after the cap.
    var ticks = 0
    let gate = ProbeGate()
    let gateTimedOut = ProbeGate()
    let heartbeat = Timer(timeInterval: 0.002, repeats: true) { _ in
        guard BackgroundWork.isWaiting else { return }
        ticks += 1
        gate.open()
    }
    RunLoop.main.add(heartbeat, forMode: .common)
    RunLoop.main.add(heartbeat, forMode: .modalPanel)
    defer { heartbeat.invalidate() }
    BackgroundWork.queue.async {
        let deadline = Date().addingTimeInterval(10)
        while !gate.isOpen, Date() < deadline { usleep(1000) }
        if !gate.isOpen { gateTimedOut.open() }
    }
    ActionRecorder.shared.start()
    controller.commitPendingTransform()
    let steps = ActionRecorder.shared.stop()
    try require(!controller.isTransforming, "Successful transform closed the actual session")
    try require(document.doc?.layerPixels(0) == expected.layerPixels(0), "UI worker equals reference affine")
    try require(ticks > 0, "Transform progress serviced the main run loop"
                + (gateTimedOut.isOpen ? " (the heartbeat gate timed out after 10 s)" : ""))
    try require(steps.count == 1 && steps.first?.tool == "transform_layer", "UI transform records one Action")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "Transform has exactly one undo and restores clean state")
    document.undoManager?.redo()
    try require(document.doc?.layerPixels(0) == expected.layerPixels(0), "Transform redo restores pixels")

    let replay = try fixture(width: 1600, height: 1200)
    defer { replay.close() }
    let report = ActionPlayer.runSingle(steps[0], on: replay, undoName: "Transform Replay")
    try require(report.ok && replay.doc?.layerPixels(0) == expected.layerPixels(0),
                "Recorded transform replays through MCP worker with equal pixels")
    print("PASS: actual Free Transform session, responsive progress, one undo/redo and Actions replay")

    let perspective = try fixture(width: 500, height: 400)
    defer { perspective.close() }
    guard let perspectiveBase = perspective.doc else { throw ProbeFailure(description: "No perspective source") }
    let corners = [CGPoint(x: 13, y: 19), CGPoint(x: 487, y: 6),
                   CGPoint(x: 459, y: 390), CGPoint(x: 22, y: 368)]
    guard let perspectiveExpected = try perspectiveBase.perspectiveLayer(0, quad: corners, sampler: DR_FILTER_BILINEAR)
    else { throw ProbeFailure(description: "Perspective reference refused") }
    _ = try transformCall("distort_layer", document: perspective,
        arguments: ["layer": 0, "corners": corners.map { [Double($0.x), Double($0.y)] }, "sampler": "bilinear"])
    try require(perspective.doc?.layerPixels(0) == perspectiveExpected.layerPixels(0),
                "MCP true perspective matches reference")
    perspective.undoManager?.undo()
    try require(perspective.doc === perspectiveBase && perspective.undoManager?.canUndo != true,
                "MCP perspective makes one undo")

    // Phase 12F-2's artwork-geometry contract (app/CLAUDE.md "Described
    // layers"): a description of an unknown version is not re-rendered, so
    // a group holding one REFUSES the transform in-band and changes nothing,
    // rather than dropping the description as it did before 12F-2.
    let described = try fixture(width: 140, height: 110)
    defer { described.close() }
    guard let describedDuplicate = try described.doc?.duplicatingLayer(0),
          let unknown = describedDuplicate.withLayerMeta(1, "{\"type\":\"text\",\"version\":999}"),
          let describedGroup = try? unknown.groupLayers([0, 1], name: "Described Group") else {
        throw ProbeFailure(description: "Described group transform fixture failed")
    }
    described.doc = describedGroup.document
    described.updateChangeCount(.changeCleared)
    described.undoManager?.removeAllActions()
    let (refusal, refusalRaw) = try transformResponse("transform_layer", document: described,
        arguments: ["layer": describedGroup.group, "translate_x": 11, "translate_y": 7, "sampler": "nearest"])
    try require(refusal["isError"] as? Bool == true
                && refusalRaw.contains("The artwork description or required font is unavailable. Geometry was not applied."),
                "Unknown-version description refuses the group transform in-band: \(refusalRaw)")
    try require(described.doc === describedGroup.document && described.undoManager?.canUndo != true,
                "Refused group transform changed nothing and made no undo")

    let grouped = try fixture(width: 140, height: 110)
    defer { grouped.close() }
    guard let duplicate = try grouped.doc?.duplicatingLayer(0),
          let group = try? duplicate.groupLayers([0, 1], name: "Transform Group") else {
        throw ProbeFailure(description: "Group transform fixture failed")
    }
    grouped.doc = group.document
    grouped.updateChangeCount(.changeCleared)
    grouped.undoManager?.removeAllActions()
    let moved = group.document.movingSet([group.group])
    let matrix = CGAffineTransform(translationX: 11, y: 7)
    guard let groupExpected = try group.document.transformLayers([group.group], matrix, sampler: DR_FILTER_NEAREST)
    else { throw ProbeFailure(description: "Group reference failed") }
    _ = try transformCall("transform_layer", document: grouped, arguments: ["layer": group.group,
        "translate_x": 11, "translate_y": 7, "sampler": "nearest"])
    for index in moved where !groupExpected.layerIsGroup(index) {
        try require(grouped.doc?.layerPixels(index) == groupExpected.layerPixels(index), "Group descendant pixels")
        try require(grouped.doc?.layerInfo(index)?.offsetX == groupExpected.layerInfo(index)?.offsetX,
                    "Group descendant placement")
    }
    grouped.undoManager?.undo()
    try require(grouped.doc === group.document && grouped.undoManager?.canUndo != true,
                "Group transform undoes in one step")
    print("PASS: MCP perspective and grouped transform retain reference pixels and undo once; an unknown-version description refuses in-band")

    // Exercise the shared stale guard without the UI error alert. Mutation plus
    // undo returns to the exact input handle, so identity alone is insufficient.
    let stale = try fixture(width: 1600, height: 1200)
    defer { stale.close() }
    guard let staleBase = stale.doc else { throw ProbeFailure(description: "No stale transform source") }
    let work = try TransformWork(source: staleBase, layers: [0], quad: nil,
        matrix: CGAffineTransform(rotationAngle: 0.2), sampler: DR_FILTER_CATMULL_ROM, droppingDescriptions: [])
    var changed = false
    let mutate = Timer(timeInterval: 0.001, repeats: false) { _ in
        withUndoGroup(stale.undoManager) {
            stale.applyEdit("Intervening Edit", record: .notACommand) { try $0.withLayerName(0, "Newer") }
        }
        stale.undoManager?.undo()
        changed = true
    }
    RunLoop.main.add(mutate, forMode: .common)
    defer { mutate.invalidate() }
    var refused = false
    do { _ = try work.wait(in: stale, title: "Stale Transform Probe") }
    catch { refused = error.localizedDescription.contains("changed while") }
    try require(changed && refused && stale.doc === staleBase,
                "Transform rejects mutation/undo despite exact source handle identity")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "Transform completion released worker reservation")
    print("PASS: transform stale revision rejection after intervening mutation/undo, worker reservation released")
}
