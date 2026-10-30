import AppKit

func nativeBlendFixture() throws -> ImageDocument {
    let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] ?? NSTemporaryDirectory())
    let source = try RasterPrecisionDocument.openPreserving(
        url: directory.appendingPathComponent("native-cutout-foreground.png"),
        workingLimit: RasterResources.shared.workingLimit, otherRetained: 0)
    guard let doubled = try source.duplicateLayer(0) else { throw ProbeFailure(description: "Duplicate native blend fixture") }
    let document = try fixture(width: 3, height: 2)
    let facade = try RasterDocument(precision: doubled)
    withUndoGroup(document.undoManager) {
        document.applyEdit("Native blend fixture", record: .notACommand, intentionalDepthConversion: true) { _ in facade }
    }
    document.setLayerSelection(.single(1))
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    return document
}

func verifyNativeLayerBlending() throws {
    let document = try nativeBlendFixture(), replay = try nativeBlendFixture()
    defer { _ = ActionRecorder.shared.stop(); document.close(); replay.close() }
    guard let original = document.doc, let native = original.precision, let raw = native.layerSource(1) else {
        throw ProbeFailure(description: "Native blend source")
    }
    let exact = try raw.withRGBA16 { Array($0) }
    let selection = document.selectionHistory
    let panel = try editor(document).layersPanel
    guard let popup = allViews(panel?.view ?? NSView()).compactMap({ $0 as? NSPopUpButton })
        .first(where: { $0.itemTitles.contains("Multiply") && $0.itemTitles.contains("Screen") }) else {
        throw ProbeFailure(description: "Native Layers blend menu")
    }
    try require(popup.isEnabled, "Native blend menu is enabled")
    // Since the 10D threshold-mode slice (dd3474f) Hard Mix (21), Dissolve
    // and Darker/Lighter Color are qualified typed modes
    // (`NativeLayerBlending.colorModes`), so every one of the 26 raster
    // modes is enabled; group-only Pass Through (27) is not offered on a
    // raster at all.
    try require((1...26).allSatisfy { popup.menu?.item(withTag: $0)?.isEnabled == true }
        && popup.menu?.item(withTag: Int(DR_BLEND_PASS_THROUGH.rawValue)) == nil,
        "Only qualified native blend choices are enabled")
    _ = try call(document, "start_recording", ["discard": true])
    popup.selectItem(withTag: 1)
    _ = popup.sendAction(popup.action, to: popup.target)
    guard let multiplied = document.doc else { throw ProbeFailure(description: "Missing Multiply result") }
    try require(multiplied !== original && multiplied.layerInfo(1)?.blendMode == DR_BLEND_MULTIPLY,
                "Actual Layers menu publishes Multiply")
    try require(document.history.entries.count == 2 && document.isDocumentEdited,
                "Actual native blend control creates one dirty history transition")
    _ = try call(document, "stop_recording")
    let steps = ActionRecorder.shared.steps
    try require(steps.count == 1 && steps[0].tool == "set_layer_properties", "Native Layers menu records existing property Action")
    let played = ActionPlayer.run(Action(name: "Native Multiply", steps: steps), on: replay)
    try require(played.ok && replay.doc?.layerInfo(1)?.blendMode == DR_BLEND_MULTIPLY,
                "Native blend Action replays through MCP property semantics")
    replay.undoManager?.undo()
    try require(replay.doc?.layerInfo(1)?.blendMode == DR_BLEND_NORMAL && replay.undoManager?.canUndo != true,
                "One Undo reverses the complete native blend Action")
    document.undoManager?.undo()
    try require(document.doc === original && !document.isDocumentEdited && document.undoManager?.canUndo != true,
                "One UI Undo restores exact clean native owner")
    document.undoManager?.redo()
    try require(document.doc === multiplied && document.selectionHistory == selection, "Native redo preserves owner and selection")
    // Every added mode uses the actual popup, records the existing Action,
    // replays through MCP and restores the exact previous owner with one Undo.
    for tag in Array(3...9) + [10,11,12,15,13,18,19,20,22] {
        let previous = document.doc
        _ = try call(document, "start_recording", ["discard": true])
        popup.selectItem(withTag: tag)
        let presented = try capturedPresentationError { _ = popup.sendAction(popup.action, to: popup.target) }
        try require(presented.isEmpty, "Native blend mode \(tag) UI error: \(presented)")
        guard let current = document.doc else { throw ProbeFailure(description: "Missing separable result") }
        try require(current.layerInfo(1)?.blendMode.rawValue == UInt32(tag), "Actual popup enables each added native mode")
        _ = try call(document, "stop_recording")
        let recorded = ActionRecorder.shared.steps
        try require(recorded.count == 1 && recorded[0].tool == "set_layer_properties", "Each separable mode records one property Action")
        let replayed = ActionPlayer.run(Action(name: "Native separable", steps: recorded), on: replay)
        try require(replayed.ok && replay.doc?.layerInfo(1)?.blendMode.rawValue == UInt32(tag), "Each separable Action replays its mode")
        replay.undoManager?.undo()
        try require(replay.doc?.layerInfo(1)?.blendMode == DR_BLEND_NORMAL, "Each separable replay has one Undo")
        document.undoManager?.undo()
        try require(document.doc === previous, "Each separable UI edit has one exact-owner Undo")
    }
    let noOp = try call(document, "set_layer_properties", ["index": 1, "blend_mode": "Multiply"])
    try require(noOp["changed"] as? Bool == false && document.doc === multiplied, "Exact MCP blend no-op preserves owner/history")
    let bad = AgentServer.shared.execute(tool: "set_layer_properties", argumentsJSON:
        "{\"document_id\":\"\(AgentServer.shared.documentID(for: document))\",\"index\":1,\"blend_mode\":\"Pass Through\",\"name\":\"Must not rename\"}")
    let rejected = try JSONSerialization.jsonObject(with: Data(bad.utf8)) as? [String: Any]
    try require(rejected?["isError"] as? Bool == true && document.doc === multiplied,
                "Unsupported combined property call refuses atomically")
    let originalHistory = document.history.entries.map(\.id)
    let originalDirty = document.isDocumentEdited
    let undoAvailable = document.undoManager?.canUndo
    let redoAvailable = document.undoManager?.canRedo
    let refusedProperties: [[String: Any]] = [
        ["index": 1, "blend_mode": "Linear Dodge (Add)", "name": String(repeating: "x", count: 65537)],
        ["index": 1, "blend_mode": "Linear Dodge (Add)", "offset_x": 2147483648],
        ["index": 1, "blend_mode": "Linear Dodge (Add)", "opacity": "nan"],
        ["index": 1, "blend_mode": "Linear Dodge (Add)", "visible": ["invalid"]],
        ["index": 1, "blend_mode": [1], "name": "Must not rename"]
    ]
    for properties in refusedProperties {
        var values = properties
        values["document_id"] = AgentServer.shared.documentID(for: document)
        let encoded = try JSONSerialization.data(withJSONObject: values)
        let raw = AgentServer.shared.execute(tool: "set_layer_properties", argumentsJSON: String(decoding: encoded, as: UTF8.self))
        let refusal = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any]
        try require(refusal?["isError"] as? Bool == true && document.doc === multiplied
            && document.selectionHistory == selection && document.history.entries.map(\.id) == originalHistory
            && document.isDocumentEdited == originalDirty && document.undoManager?.canUndo == undoAvailable
            && document.undoManager?.canRedo == redoAvailable, "Valid blend followed by a refused property preserves owner, selection, history, dirty state and Undo/Redo")
        let errorText = (refusal?["content"] as? [[String: Any]])?.first?["text"] as? String ?? ""
        let expected = properties["blend_mode"] is [Int] ? "Invalid native layer property: blend_mode"
            : properties["name"] != nil ? "64 KiB" : properties["offset_x"] != nil ? "outside supported bounds"
            : properties["opacity"] != nil ? "must be finite" : "Invalid native layer property: visible"
        try require(errorText.contains(expected), "Typed property refusal reports its actual error: \(errorText)")
    }
    _ = try call(document, "set_layer_properties", ["index": 1, "blend_mode": "Color Burn", "name": "ColorBurn16", "visible": false])
    try require(document.doc?.layerInfo(1)?.blendMode == DR_BLEND_COLOR_BURN && document.doc?.layerInfo(1)?.name == "ColorBurn16" && document.doc?.layerInfo(1)?.visible == false,
                "MCP blend plus name publishes complete requested properties")
    guard let committed = document.doc else { throw ProbeFailure(description: "Missing Color Burn result") }
    document.undoManager?.undo()
    try require(document.doc === multiplied, "One Undo reverses combined blend/name/visibility properties")
    document.undoManager?.redo()
    try require(document.doc === committed, "One Redo restores all combined properties")
    let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] ?? NSTemporaryDirectory())
    let path = directory.appendingPathComponent("native-blending.rz")
    try committed.saveNative(to: path)
    let bytes = try Data(contentsOf: path)
    // The typed identity word 46 leads every file since 12E-1; the blend
    // wrapper's word 30 follows further down the chain.
    let words = leadingVersionWords(bytes)
    try require(words.first == 46 && words.contains(30), "Four-mode native blend writes version30: \(words)")
    let reopened = try RasterDocument(precision: RasterPrecisionDocument.openPreserving(url: path,
        workingLimit: RasterResources.shared.workingLimit, otherRetained: 0))
    try checkNativeBlendArchive(reopened, exact: exact)
    let snapshot = try document.namedSnapshots.create(name: "Native blend")
    let snapshotURL = directory.appendingPathComponent("native-blending.drsnapshot")
    try document.namedSnapshots.save(id: snapshot.id, to: snapshotURL)
    let loaded = try document.namedSnapshots.load(from: snapshotURL)
    try checkNativeBlendArchive(loaded.state.document, exact: exact)
    try verifyNativeBlendRecovery(document, directory: directory, exact: exact)
    _ = try call(document, "set_selected_layers", ["layers": [0,1]])
    _ = try call(document, "group_layers", ["layers": [0,1], "name": "Blend group"])
    _ = try call(document, "set_layer_properties", ["index": 2, "blend_mode": "Color Dodge"])
    _ = try call(document, "set_layer_properties", ["index": 1, "name": "Descendant"])
    try require(document.doc?.layerInfo(2)?.blendMode == DR_BLEND_COLOR_DODGE,
                "Descendant property edit preserves parent blending")
    let openNoOp = try call(document, "set_layer_properties", ["index": 2, "blend_mode": "Color Dodge", "open": true])
    try require(openNoOp["changed"] as? Bool == false, "Native open:true is a known no-op")
    // Screen and the disclosure are both native: one step that
    // one Undo reverts completely.
    let collapse = try call(document, "set_layer_properties", ["index": 2, "blend_mode": "Screen", "open": false])
    try require(collapse["ok"] as? Bool == true
                    && document.doc?.layerInfo(2)?.blendMode == DR_BLEND_SCREEN
                    && document.doc?.layerInfo(2)?.open == false,
                "Native blend and disclosure apply together: "
                    + "\(String(describing: document.doc?.layerInfo(2)))")
    document.undoManager?.undo()
    try require(document.doc?.layerInfo(2)?.blendMode == DR_BLEND_COLOR_DODGE
                    && document.doc?.layerInfo(2)?.open == true,
                "One Undo reverts the native blend and disclosure together")
    _ = try call(document, "set_layer_properties", ["index": 2, "blend_mode": "Pass Through"])
    try require(document.doc?.layerInfo(2)?.blendMode == DR_BLEND_PASS_THROUGH,
                "MCP restores a blended group to Pass Through")
    try verifyFiveNativeBlendPersistence()
    try verifyNativeBlendMaskWrappers()
    print("PASS: native blend Layers control, strict MCP, recorded Action, Undo/Redo, exact raw codes, v30 reopen, snapshots and private recovery")
}

func checkNativeBlendArchive(_ owner: RasterDocument, exact: [UInt16], mode: DrBlendMode = DR_BLEND_COLOR_BURN) throws {
    guard let image = owner.precision?.layerSource(1) else { throw ProbeFailure(description: "Missing archived native blend pixels") }
    let pixels = try image.withRGBA16 { Array($0) }
    try require(owner.layerInfo(1)?.blendMode == mode && pixels == exact,
                "Archive preserves mode and every genuine native/hidden source code")
}

func verifyNativeBlendRecovery(_ document: ImageDocument, directory: URL, exact: [UInt16], mode: DrBlendMode = DR_BLEND_COLOR_BURN) throws {
    guard let state = document.captureEditingState() else { throw ProbeFailure(description: "Missing native blend editing state") }
    let captured = try EditingStateArchive.capture(state)
    let id = UUID().uuidString
    let location = try RecoveryStore.directory(root: directory.appendingPathComponent("blend-recovery"), id: id)
    guard let lease = try RecoveryLease.acquire(directory: location, create: true) else {
        throw ProbeFailure(description: "Private native blend recovery lease")
    }
    defer { lease.release() }
    let recovery = RecoveryStore.Captured(id: id, title: "Native blend recovery", sourceURL: nil, sourceType: nil,
        generation: .init(folder: UUID().uuidString, revision: document.revision, serial: 1, captured: Date()),
        previous: nil, state: captured, snapshots: [])
    _ = try RecoveryCapacity.validate(recovery)
    let gate = RecoveryPublicationGate()
    let published = try BackgroundWork.wait(WorkerTask(scratchEstimate: recovery.scratchEstimate) {
        try RecoveryStore.write(recovery, directory: location, gate: gate)
    }, title: "Writing Native Blend Recovery")
    let candidate = RecoveryStore.Candidate(id: id, directory: location, manifest: published, failure: nil)
    let read = try BackgroundWork.wait(WorkerTask { try RecoveryStore.read(candidate) }, title: "Reading Native Blend Recovery")
    try checkNativeBlendArchive(read.state.document, exact: exact, mode: mode)
}
