import AppKit

func verifyFiveNativeBlendPersistence() throws {
    let document = try nativeBlendFixture()
    defer { document.close() }
    guard let baseline = document.doc, let source = baseline.precision?.layerSource(1) else {
        throw ProbeFailure(description: "Five-mode native source")
    }
    let exact = try source.withRGBA16 { Array($0) }
    let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] ?? NSTemporaryDirectory())
    let modes: [(DrBlendMode, String)] = [(DR_BLEND_SUBTRACT, "Subtract"),
        (DR_BLEND_VIVID_LIGHT, "Vivid Light"), (DR_BLEND_LINEAR_LIGHT, "Linear Light"),
        (DR_BLEND_PIN_LIGHT, "Pin Light"), (DR_BLEND_DIVIDE, "Divide")]
    for (mode, name) in modes {
        _ = try call(document, "set_layer_properties", ["index": 1, "blend_mode": name,
            "name": "Five modes", "opacity": 0.625])
        guard let changed = document.doc else { throw ProbeFailure(description: "Five-mode property result") }
        try require(changed.layerInfo(1)?.name == "Five modes" && changed.layerInfo(1)?.opacity == 0.625,
                    "Five-mode compound edit publishes all properties")
        try checkNativeBlendArchive(changed, exact: exact, mode: mode)
        let path = directory.appendingPathComponent("five-blend-\(mode.rawValue).rz")
        try changed.saveNative(to: path)
        let data = try Data(contentsOf: path)
        // Since 12E-1 the typed identity word 46 leads the chain; the
        // blend31 -> prior13 run follows it.
        let words = leadingVersionWords(data)
        try require(words.first == 46 && versionChain(words, contains: [31, 13]),
                    "Five-mode native31 preserves prior13 body: \(words)")
        try checkNativeBlendArchive(RasterDocument.open(url: path), exact: exact, mode: mode)
        let snapshot = try document.namedSnapshots.create(name: name)
        let snapshotURL = directory.appendingPathComponent("five-blend-\(mode.rawValue).drsnapshot")
        try document.namedSnapshots.save(id: snapshot.id, to: snapshotURL)
        let restored = try document.namedSnapshots.load(from: snapshotURL)
        try checkNativeBlendArchive(restored.state.document, exact: exact, mode: mode)
        if mode == DR_BLEND_VIVID_LIGHT {
            try verifyNativeBlendRecovery(document, directory: directory, exact: exact, mode: mode)
        }
        document.undoManager?.undo()
        try require(document.doc === baseline && !document.isDocumentEdited,
                    "One five-mode property Undo restores exact clean owner")
    }
    let history = document.history.entries.map(\.id)
    for name in ["Pass Through"] {
        let values: [String: Any] = ["document_id": AgentServer.shared.documentID(for: document),
            "index": 1, "blend_mode": name, "name": "Must not publish"]
        let request = try JSONSerialization.data(withJSONObject: values)
        let result = AgentServer.shared.execute(tool: "set_layer_properties", argumentsJSON: String(decoding: request, as: UTF8.self))
        let parsed = try JSONSerialization.jsonObject(with: Data(result.utf8)) as? [String: Any]
        try require(parsed?["isError"] as? Bool == true && document.doc === baseline
            && document.history.entries.map(\.id) == history && !document.isDocumentEdited,
                    "Pass Through on a raster refuses atomically")
    }
    let group = try call(document, "group_layers", ["layers": [0,1], "name": "Five-mode group"])
    try require(group["group"] as? Int == 2 && document.doc?.layerIsGroup(2) == true,
                "Five-mode group fixture")
    for (_, name) in modes {
        _ = try call(document, "set_layer_properties", ["index": 2, "blend_mode": name])
        let owner = document.doc
        _ = try call(document, "set_layer_properties", ["index": 2, "blend_mode": "Pass Through"])
        try require(document.doc?.layerInfo(2)?.blendMode == DR_BLEND_PASS_THROUGH,
                    "Each new isolated group mode returns to Pass Through")
        document.undoManager?.undo()
        try require(document.doc === owner, "Group transition Undo restores exact blended owner")
    }
    print("PASS: five continuous native modes, native31 reopen/snapshots/recovery, atomic Pass-Through-on-raster refusal and group transitions")
}
