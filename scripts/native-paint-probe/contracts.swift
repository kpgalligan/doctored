import AppKit

func responseIsError(_ response: String) throws -> Bool {
    guard let value = try JSONSerialization.jsonObject(with: Data(response.utf8)) as? [String: Any],
          let error = value["isError"] as? Bool else { throw DoctoredCoreError(message: "Invalid MCP response: \(response)") }
    return error
}
func versionedMCP() throws -> [String: Any] {
    let (document, _, source) = try openDocument()
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
    document.undoManager?.removeAllActions(); ActionRecorder.shared.start()
    let arguments: [String: Any] = ["points": [[48.5, 30.5]], "size": 30, "hardness": 20, "flow": 37,
        "rgba": [0.713219, 0.193517, 0.831271, 0.671123], "color_space": "document", "opacity": 0.73,
        "stroke_version": 1, "pressures": [0.25], "times": [0.0], "duration": 0.15,
        "pressure_size": true, "pressure_opacity": true, "airbrush": true]
    let response = try call("brush_stroke", arguments, document: document)
    try require(try !responseIsError(response), "Versioned generation refused: \(response)")
    let recorded = ActionRecorder.shared.stop()
    guard recorded.count == 1, let canonical = recorded[0].arguments["native_stroke"] as? [String: Any],
          let after = document.doc else { throw DoctoredCoreError(message: "Generated stroke was not canonicalized before recording") }
    let dabs = try decodeDabs(canonical)
    try require(dabs.count == 16, "A 150ms hold must resolve exactly initial + three 50ms deposits")
    for i in stride(from: 0, to: dabs.count, by: 4) {
        try require(dabs[i] == 48.5 && dabs[i+1] == 30.5 && dabs[i+2] == 7.5 && dabs[i+3] == 0.37 * 0.25,
            "Versioned generated dab differs from independent pressure/timing reference")
    }
    for key in ["stroke_version", "pressures", "times", "duration", "pressure_size", "pressure_opacity", "airbrush"] {
        try require(recorded[0].arguments[key] == nil, "Resolved Action retained generation field \(key)")
    }
    let expected = try raw(after)
    _ = try verifyPaint(fixture(), after: expected, canonical: canonical,
        ink: [0.713219, 0.193517, 0.831271, 0.671123], opacity: 0.73, selection: nil, width: 96, height: 64)
    try validateReplay(document, source: source, result: after, step: recorded[0], expected: expected)
    document.undoManager?.undo(); pump()
    try require(document.doc === source, "Generated Action Undo lost original owner")
    var malformed: [[String: Any]] = []
    for (key, value) in [("version", true as Any), ("version", 2 as Any), ("pixel", 1 as Any),
                         ("hardness", true as Any), ("roundness", true as Any), ("angle_degrees", false as Any),
                         ("unknown", 1 as Any), ("dabs", "AAAA" as Any)] {
        var bad = canonical; bad[key] = value
        malformed.append(["points": [[48.5, 30.5]], "native_stroke": bad])
    }
    for version in [true as Any, 1.9 as Any, 2 as Any] {
        var bad = arguments; bad["stroke_version"] = version; malformed.append(bad)
    }
    var booleanPressure = arguments; booleanPressure["pressures"] = [true]; malformed.append(booleanPressure)
    var mixed = arguments; mixed["native_stroke"] = canonical; malformed.append(mixed)
    var long = canonical
    long["dabs"] = Data(repeating: 0, count: (NativeBrushStroke.maximumDabs + 1) * 32).base64EncodedString()
    malformed.append(["points": [[48.5, 30.5]], "native_stroke": long])
    for (index, bad) in malformed.enumerated() {
        let reply = try call("brush_stroke", bad, document: document)
        try require(try responseIsError(reply), "Malformed versioned request \(index) was accepted: \(reply)")
        try require(document.doc === source && document.undoManager?.canUndo != true && !document.isDocumentEdited,
            "Refused versioned stroke changed state at case \(index)")
    }
    let zero: [String: Any] = ["points": [[12.5, 15.5], [30.5, 15.5]], "size": 1,
        "stroke_version": 1, "pressure_opacity": true, "pressures": [0, 0]]
    let zeroReply = try call("brush_stroke", zero, document: document)
    try require(try !responseIsError(zeroReply), "Zero-pressure MCP was not an accepted no-op")
    try require(document.doc === source && document.undoManager?.canUndo != true, "Zero-pressure MCP movement painted")
    return ["name": "versioned-mcp", "passed": true, "strict_refusals": malformed.count,
        "resolved_dabs": dabs.count / 4, "zero_pressure_noop": true]
}

/// Actual native writer and named snapshot/recovery readers, preserving raw
/// samples and native selection. This is a recovery roundtrip, not a crash test.
func verifyPersistence(_ document: ImageDocument, expected: [UInt16], output: URL) throws -> [String: Any] {
    guard let source = document.doc, let state = document.captureEditingState() else { throw DoctoredCoreError(message: "Persistence state missing") }
    let nativeURL = output.appendingPathComponent("native-pressure.rz")
    let save = try call("save_copy", ["path": nativeURL.path], document: document)
    try require(try !responseIsError(save), "Native save_copy refused: \(save)")
    let reopened = try RasterDocument.open(url: nativeURL)
    try require(try raw(reopened) == expected, "Native reopen changed stored RGBA codes")
    let snapshot = try document.namedSnapshots.create(name: "Native pressure")
    let snapshotURL = output.appendingPathComponent("native-pressure.drsnapshot")
    try document.namedSnapshots.save(id: snapshot.id, to: snapshotURL)
    let loaded = try document.namedSnapshots.load(from: snapshotURL)
    try require(try raw(loaded.state.document) == expected && loaded.state.selection == state.selection,
        "Named snapshot changed native pixels or fractional selection")
    try document.namedSnapshots.restore(id: loaded.id)
    try require(try raw(document.doc ?? source) == expected, "Actual snapshot restore changed native paint")
    let captured = try EditingStateArchive.capture(state)
    let id = UUID().uuidString
    let directory = try RecoveryStore.directory(root: output.appendingPathComponent("private-recovery"), id: id)
    guard let lease = try RecoveryLease.acquire(directory: directory, create: true) else { throw DoctoredCoreError(message: "Private recovery lease failed") }
    defer { lease.release() }
    let recovery = RecoveryStore.Captured(id: id, title: "Native pressure recovery", sourceURL: nil, sourceType: nil,
        generation: .init(folder: UUID().uuidString, revision: document.revision, serial: 1, captured: Date()),
        previous: nil, state: captured,
        snapshots: [.init(header: .init(version: 1, name: "Native pressure", created: Date()), state: captured)])
    _ = try RecoveryCapacity.validate(recovery)
    let gate = RecoveryPublicationGate()
    let manifest = try BackgroundWork.wait(WorkerTask(scratchEstimate: recovery.scratchEstimate) {
        try RecoveryStore.write(recovery, directory: directory, gate: gate)
    }, title: "Writing Private Native Paint Recovery")
    let recovered = try BackgroundWork.wait(WorkerTask {
        try RecoveryStore.read(.init(id: id, directory: directory, manifest: manifest, failure: nil))
    }, title: "Reading Private Native Paint Recovery")
    let recoveredState = try recovered.state.editingState()
    try require(try raw(recoveredState.document) == expected && recoveredState.selection == state.selection,
        "Recovery changed native pixels or fractional selection")
    try require(recovered.snapshots.count == 1, "Recovery dropped native snapshot")
    try require(try raw(recovered.snapshots[0].state.document) == expected, "Recovered snapshot changed native pixels")
    return ["native": true, "named_snapshot": true, "recovery_roundtrip": true, "fractional_selection": true,
        "crash_recovery": "Covered separately by external process qualification"]
}
