import AppKit
import CryptoKit

// Phase 12A: the UI-SOURCE half of the replay-gap gates. An MCP drive cannot
// drag a mouse, so the three things only a gesture can prove live here:
// a BYTE pressure stroke and a BYTE held-airbrush stroke record ONE step
// carrying canonical dabs and replay byte-identically on a fresh document,
// and a Move drag records the right thing for one layer and for a set.

/// A deterministic 8-bit fixture, the byte twin of `fixture()`.
func byteFixture(width: Int = 96, height: Int = 64) -> [UInt8] {
    let alpha: [UInt8] = [0, 1, 17, 200, 255]
    var pixels: [UInt8] = []
    pixels.reserveCapacity(width * height * 4)
    for i in 0..<(width * height) {
        pixels.append(UInt8(11 + (i * 113) % 220))
        pixels.append(UInt8(23 + (i * 67) % 200))
        pixels.append(UInt8(37 + (i * 37) % 180))
        pixels.append(alpha[i % alpha.count])
    }
    return pixels
}

/// The byte twin of `openDocument()`: an 8-bit single-layer document in a
/// real window, with a real editor driving a real canvas.
func openByteDocument(width: Int = 96, height: Int = 64)
    throws -> (ImageDocument, EditorViewController, RasterDocument)
{
    guard let image = RasterImage.from(rgba: byteFixture(width: width, height: height),
            width: width, height: height),
          let source = RasterDocument.from(image: image) else {
        throw DoctoredCoreError(message: "Owned 8-bit fixture document unavailable")
    }
    let document = ImageDocument(); document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90,
        embedColorProfile: true, stripMetadata: false, exportBitDepth: 8, exportDither: "none"),
        markSaved: true)
    NSDocumentController.shared.addDocument(document); document.makeWindowControllers()
    document.showWindows()
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Actual editor missing")
    }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true); pump()
    try require(document.doc?.isHighDepth != true, "Owned fixture is not an 8-bit document")
    return (document, editor, source)
}

/// The flattened straight-RGBA8 composite — what "byte-identical output"
/// means for an 8-bit document.
func flatBytes(_ document: RasterDocument) throws -> [UInt8] {
    guard let image = document.flattened(),
          let bytes = image.withPlateBytes({ pointer -> [UInt8] in
              Array(UnsafeBufferPointer(start: pointer, count: image.width * image.height * 4))
          }) else { throw DoctoredCoreError(message: "Owned byte composite unavailable") }
    return bytes
}

func hashBytes(_ values: [UInt8]) -> String {
    SHA256.hash(data: Data(values)).map { String(format: "%02x", $0) }.joined()
}

/// A recorded step replayed through the REAL server, with the two layer
/// symbols bound the way `ActionPlayer` binds them (`call` speaks MCP, which
/// takes indices).
func replayBinding(_ step: ActionStep, document: ImageDocument) throws -> String {
    var arguments = step.arguments
    if let symbol = arguments["layers"] as? [String: Any], symbol["$layers"] as? String == "selected" {
        arguments["layers"] = document.selectedLayerIndices
    }
    if let symbol = arguments["layer"] as? [String: Any], symbol["$layer"] as? String == "active" {
        arguments["layer"] = document.activeLayerIndex
    }
    if let symbol = arguments["index"] as? [String: Any], symbol["$layer"] as? String == "active" {
        arguments["index"] = document.activeLayerIndex
    }
    return try call(step.tool, arguments, document: document)
}

/// byte-pressure-ui / byte-airbrush-ui.
///
/// A pressure-varying (or held-airbrush) stroke on an 8-bit document records
/// ONE `brush_stroke` carrying canonical `native_stroke` dabs and no
/// generation fields; replayed on an identical FRESH 8-bit document it must
/// reproduce the flattened composite byte for byte, and one Undo must put the
/// recorded document back where it started.
func byteStrokeUI(output: URL, airbrush: Bool) throws -> [String: Any] {
    let name = airbrush ? "byte-airbrush-ui" : "byte-pressure-ui"
    let (document, editor, source) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    configure(editor, size: 24, hardness: 0.2, opacity: 73, flow: 37,
              pressureSize: !airbrush, airbrush: airbrush)
    try require(editor.canvas.nativeBrushEnabled == false, "8-bit canvas claimed the typed session")
    document.undoManager?.removeAllActions()
    ActionRecorder.shared.start()
    var deposits = 0
    if airbrush {
        let point = CGPoint(x: 48.5, y: 30.5)
        try send(editor.canvas, .leftMouseDown, point)
        let first = editor.canvas.nativeStrokeSession?.dabs.count ?? 0
        try require(first == 4, "A byte airbrush press did not open a dab session")
        let start = Date()
        while Date().timeIntervalSince(start) < 0.28 { pump() }
        deposits = (editor.canvas.nativeStrokeSession?.dabs.count ?? 0) / 4
        try require(deposits >= 4, "Held real airbrush Timer did not add at least three byte deposits")
        try send(editor.canvas, .leftMouseUp, point)
    } else {
        try send(editor.canvas, .leftMouseDown, CGPoint(x: 20.5, y: 28.5), pressure: 0.25)
        try require(editor.canvas.nativeStrokeSession != nil,
            "A byte pressure press did not open a dab session")
        try send(editor.canvas, .leftMouseDragged, CGPoint(x: 38.5, y: 29.5), pressure: 0.5)
        try send(editor.canvas, .leftMouseDragged, CGPoint(x: 62.5, y: 30.5), pressure: 0.875)
        try screenshot(editor.view, output.appendingPathComponent("\(name).png"))
        try send(editor.canvas, .leftMouseUp, CGPoint(x: 62.5, y: 30.5), pressure: 0.875)
    }
    let steps = ActionRecorder.shared.stop()
    guard steps.count == 1, steps[0].tool == "brush_stroke",
          let canonical = steps[0].arguments["native_stroke"] as? [String: Any],
          let result = document.doc else {
        throw DoctoredCoreError(message: "\(name) failed to record one canonical byte stroke")
    }
    for key in ["stroke_version", "pressures", "times", "duration", "pressure_size",
                "pressure_opacity", "airbrush"] {
        try require(steps[0].arguments[key] == nil, "Recorded byte step carried generation field \(key)")
    }
    try require(editor.canvas.nativeStrokeSession == nil, "The byte dab session outlived its stroke")
    let dabs = try decodeDabs(canonical)
    let diameters = stride(from: 2, to: dabs.count, by: 4).map { dabs[$0] }
    if airbrush {
        try require(dabs.count / 4 == deposits, "The recorded dabs are not the deposits the timer made")
    } else {
        try require(Set(diameters).count > 3, "Changing tablet pressure did not change recorded diameters")
    }
    let after = try flatBytes(result)
    let baseline = try flatBytes(source)
    try require(result !== source && after != baseline, "The byte stroke published nothing")
    // One Undo restores the exact pre-stroke owner.
    document.undoManager?.undo(); pump()
    try require(document.doc === source && !document.isDocumentEdited,
        "One Undo did not restore the saved byte owner")
    document.undoManager?.redo(); pump()
    try require(document.doc === result, "Redo did not restore the exact byte owner")
    // …and the recorded step reproduces it on an IDENTICAL FRESH document.
    let (fresh, _, freshSource) = try openByteDocument()
    defer { fresh.close(); pump() }
    let freshBaseline = try flatBytes(freshSource)
    try require(freshBaseline == baseline, "The fresh 8-bit fixture is not identical to the recorded one")
    let reply = try replayBinding(steps[0], document: fresh)
    let refused = try responseIsError(reply)
    try require(!refused, "Byte canonical replay refused: \(reply)")
    guard let replayed = fresh.doc else { throw DoctoredCoreError(message: "Replay lost the document") }
    let replayedBytes = try flatBytes(replayed)
    try require(replayedBytes == after,
        "Byte canonical replay is not byte-identical: \(hashBytes(replayedBytes)) vs \(hashBytes(after))")
    fresh.undoManager?.undo(); pump()
    let undone = try flatBytes(fresh.doc ?? replayed)
    try require(undone == freshBaseline, "One Undo did not undo the whole replayed byte stroke")
    return ["name": name, "passed": true, "dabs": dabs.count / 4,
            "distinct_diameters": Set(diameters).count,
            "samples_sha256": hashBytes(after), "replay_sha256": hashBytes(replayedBytes),
            "action": steps[0].arguments]
}

/// move-record-ui: what a Move DRAG records for one layer and for a set.
///
/// Both halves record the RELATIVE `move_layers` step since Phase 12B: one
/// layer names itself by reference, a set uses the `$layers` symbol.
func moveRecordUI() throws -> [String: Any] {
    let previousView = ToolOptionsStore.shared.view, previousMove = ToolOptionsStore.shared.move
    defer { ToolOptionsStore.shared.view = previousView; ToolOptionsStore.shared.move = previousMove }
    // A snapped drag is a different measurement; this case is about what the
    // gesture records, so the snap engine and Auto-Select are both off.
    ToolOptionsStore.shared.view.snapEnabled = false
    ToolOptionsStore.shared.move.autoSelect = false
    func twoLayerDocument() throws -> (ImageDocument, EditorViewController) {
        let (document, editor, _) = try openByteDocument()
        _ = try call("duplicate_layer", ["layers": [0]], document: document)
        pump()
        try require(document.doc?.layerCount == 2, "The owned fixture did not gain a second layer")
        editor.selectTool(.move); pump()
        return (document, editor)
    }
    func drag(_ editor: EditorViewController, dx: CGFloat) throws {
        let start = CGPoint(x: 30.5, y: 30.5)
        try send(editor.canvas, .leftMouseDown, start)
        try send(editor.canvas, .leftMouseDragged, CGPoint(x: start.x + dx, y: start.y))
        try send(editor.canvas, .leftMouseUp, CGPoint(x: start.x + dx, y: start.y))
        pump()
    }
    let (document, editor) = try twoLayerDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }

    // --- ONE layer: the RELATIVE move_layers form, naming that one layer.
    //
    // Phase 12B changed this half. It used to record an ABSOLUTE
    // `set_layer_properties` offset, which replayed as a snap to a fixed
    // canvas position instead of a nudge by the amount dragged; both sizes
    // now record the same relative step (`MultiLayerEdit.moveRecord`), so
    // what is asserted here is the DELTA and not the post-drag offset.
    _ = try call("set_selected_layers", ["layers": [1]], document: document)
    pump()
    guard let before = document.doc?.layerInfo(1)?.offsetX else {
        throw DoctoredCoreError(message: "Owned layer offset unreadable")
    }
    document.undoManager?.removeAllActions()
    ActionRecorder.shared.start()
    try drag(editor, dx: 40)
    let single = ActionRecorder.shared.stop()
    guard single.count == 1, single[0].tool == "move_layers",
          let recorded = single[0].arguments["dx"] as? Int,
          let recordedDY = single[0].arguments["dy"] as? Int,
          let singleLayers = single[0].arguments["layers"] as? [Any], singleLayers.count == 1,
          let after = document.doc?.layerInfo(1)?.offsetX else {
        throw DoctoredCoreError(
            message: "A one-layer Move drag did not record one move_layers step")
    }
    try require(after == before + 40,
        "The one-layer drag did not move the layer by 40: \(before) -> \(after)")
    try require(recorded == 40 && recordedDY == 0,
        "The recorded one-layer delta is not the drag's: \(recorded), \(recordedDY)")
    // Named by REFERENCE — the {"$layer": {name, id, session}} pair — so the
    // step aims at the layer that was dragged rather than at whatever the
    // replay document has selected.
    guard let singleSymbol = singleLayers[0] as? [String: Any],
          singleSymbol["$layer"] is [String: Any] else {
        throw DoctoredCoreError(
            message: "The one-layer move step does not name its layer by reference")
    }

    // --- A SET: the relative move_layers form over the $layers symbol.
    _ = try call("set_selected_layers", ["layers": [0, 1]], document: document)
    pump()
    let offsetsBefore = (0..<2).compactMap { document.doc?.layerInfo($0)?.offsetX }
    ActionRecorder.shared.start()
    try drag(editor, dx: 40)
    let set = ActionRecorder.shared.stop()
    guard set.count == 1, set[0].tool == "move_layers",
          let dx = set[0].arguments["dx"] as? Int, let dy = set[0].arguments["dy"] as? Int,
          let symbol = set[0].arguments["layers"] as? [String: Any] else {
        throw DoctoredCoreError(message: "A set Move drag did not record one move_layers step")
    }
    try require(dx == 40 && dy == 0, "The recorded set delta is not the drag's: \(dx), \(dy)")
    try require(symbol["$layers"] as? String == "selected" && symbol.count == 1,
        "The recorded set is not the $layers symbol")
    let offsetsAfter = (0..<2).compactMap { document.doc?.layerInfo($0)?.offsetX }
    try require(offsetsAfter == offsetsBefore.map { $0 + 40 },
        "The set drag did not move both layers by 40: \(offsetsBefore) -> \(offsetsAfter)")

    // --- …and the recorded step reproduces those offsets on a fresh document.
    let (fresh, _) = try twoLayerDocument()
    defer { fresh.close(); pump() }
    _ = try call("set_selected_layers", ["layers": [0, 1]], document: fresh)
    pump()
    let freshBefore = (0..<2).compactMap { fresh.doc?.layerInfo($0)?.offsetX }
    let reply = try replayBinding(set[0], document: fresh)
    let refused = try responseIsError(reply)
    try require(!refused, "move_layers replay refused: \(reply)")
    let replayed = (0..<2).compactMap { fresh.doc?.layerInfo($0)?.offsetX }
    // The step is RELATIVE, so the replay moves the fresh document's own
    // offsets by the recorded delta — the whole point of the new form.
    try require(replayed == freshBefore.map { $0 + 40 },
        "Replayed move_layers offsets differ: \(replayed) vs \(freshBefore.map { $0 + 40 })")
    fresh.undoManager?.undo(); pump()
    try require((0..<2).compactMap { fresh.doc?.layerInfo($0)?.offsetX } == freshBefore,
        "One Undo did not undo the whole replayed move")
    return ["name": "move-record-ui", "passed": true, "single_recorded_dx": recorded,
            "single_actual_offset": after, "set_dx": dx, "set_dy": dy,
            "set_offsets": offsetsAfter, "replayed_offsets": replayed]
}
