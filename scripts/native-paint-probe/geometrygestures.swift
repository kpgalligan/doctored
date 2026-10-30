import AppKit
import CryptoKit

func geometryCall(_ tool: String, _ fields: [String: Any], _ document: ImageDocument) throws -> [String: Any] {
    let reply = try call(tool, fields, document: document)
    try require(!(try responseIsError(reply)), "\(tool) refused: \(reply)")
    guard let envelope = try JSONSerialization.jsonObject(with: Data(reply.utf8)) as? [String: Any],
          let rows = envelope["content"] as? [[String: Any]],
          let text = rows.first?["text"] as? String,
          let result = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any]
    else { throw DoctoredCoreError(message: "Invalid \(tool) result") }
    return result
}

func geometryKey(_ view: NSView, code: UInt16,
                 modifiers: NSEvent.ModifierFlags = []) throws {
    guard let window = view.window,
          let event = NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: modifiers,
            timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil,
            characters: "", charactersIgnoringModifiers: "",
            isARepeat: false, keyCode: code)
    else { throw DoctoredCoreError(message: "Owned geometry key event unavailable") }
    view.keyDown(with: event)
}

func geometryDigest(_ document: RasterDocument) throws -> String {
    let base = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_PAINT_UI_OUTPUT"]
        ?? NSTemporaryDirectory())
    let file = base.appendingPathComponent("geometry-\(UUID().uuidString).rz")
    try document.saveNative(to: file)
    let bytes = try Data(contentsOf: file)
    try FileManager.default.removeItem(at: file)
    return SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
}

func geometryBytes(_ image: CGImage) throws -> [UInt8] {
    var bytes = [UInt8](repeating: 0, count: image.width * image.height * 4)
    let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
        guard let base = buffer.baseAddress,
              let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: base, width: image.width, height: image.height,
                bitsPerComponent: 8, bytesPerRow: image.width * 4,
                space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return false }
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return true
    }
    try require(drawn, "Could not read owned geometry preview")
    return bytes
}

func geometryMaxDelta(_ a: CGImage, _ b: CGImage) throws -> Int {
    try require(a.width == b.width && a.height == b.height, "Preview dimensions differ")
    let lhs = try geometryBytes(a), rhs = try geometryBytes(b)
    return zip(lhs, rhs).map { abs(Int($0) - Int($1)) }.max() ?? 0
}

func geometryStorage(_ document: RasterDocument, layer: Int) throws -> UInt64 {
    guard let image = document.precision?.layerSource(layer) else {
        throw DoctoredCoreError(message: "Native layer source is unavailable")
    }
    let identity = image.storageIdentity
    guard identity != 0 else {
        throw DoctoredCoreError(message: "Native layer storage identity is unavailable")
    }
    return identity
}

func geometryRaw(_ document: RasterDocument, layer: Int) throws -> [UInt16] {
    guard let image = document.precision?.layerSource(layer) else {
        throw DoctoredCoreError(message: "Native layer source is unavailable")
    }
    return try image.withRGBA16 { Array($0) }
}

func geometryMask(_ document: RasterDocument, layer: Int) throws -> [UInt16] {
    try PrecisionScalarOperations.read(document, target: "mask", index: layer).samples
}

func geometryReplay(_ steps: [ActionStep], on document: ImageDocument,
                    expectedDigest: String) throws -> Bool {
    try require(steps.count == 1, "Geometry gesture recorded \(steps.count) steps")
    let name = "geometry-probe-\(UUID().uuidString)"
    _ = try geometryCall("save_action", ["name": name, "steps": steps.map { $0.jsonObject() }], document)
    document.undoManager?.undo(); pump()
    let report = try geometryCall("run_action", ["name": name], document)
    try require(report["ok"] as? Bool == true
        && (report["steps_run"] as? NSNumber)?.intValue == 1,
        "run_action did not run one geometry step: \(report)")
    guard let replayed = document.doc else { throw DoctoredCoreError(message: "Replay lost document") }
    let actualDigest = try geometryDigest(replayed)
    try require(actualDigest == expectedDigest,
                "run_action did not reproduce native geometry: \(report), "
                    + "actual \(actualDigest), expected \(expectedDigest)")
    return true
}

func precisionMoveHandlesUI() throws -> [String: Any] {
    let (document, editor, _) = try openDocument(width: 64, height: 48)
    let previousOptions = ToolOptionsStore.shared.move
    defer {
        ToolOptionsStore.shared.move = previousOptions
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
    ToolOptionsStore.shared.move.autoSelect = false
    ToolOptionsStore.shared.move.snapToLayers = false
    editor.selectTool(.move); pump()
    guard let source = document.doc else { throw DoctoredCoreError(message: "Move source missing") }
    let pointer = try geometryStorage(source, layer: 0)
    let maskBefore = source.layerMaskProperties(0)?.offsetX
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let origin = CGPoint(x: 18, y: 18)
    try send(editor.canvas, .leftMouseDown, origin)
    guard let moveOwner = editor.precisionMoveGesture else {
        throw DoctoredCoreError(message: "Native Move owner did not open")
    }
    var previewDigests: [String] = []
    for (dx, dy) in [(4, 2), (7, 3), (11, 5)] {
        try send(editor.canvas, .leftMouseDragged,
                 CGPoint(x: origin.x + CGFloat(dx), y: origin.y + CGFloat(dy)),
                 modifiers: .control)
        try waitUntil({ editor.precisionMoveGesture?.isPending == false
            && editor.canvas.previewImage != nil }, "Move preview did not settle")
        try require(moveOwner.status == "Preview current",
                    "Move preview tick \(dx),\(dy) ended: \(moveOwner.status)")
        if let image = editor.canvas.previewImage {
            previewDigests.append(hashBytes(try geometryBytes(image)))
        }
    }
    try require(Set(previewDigests).count == 3,
                "Three distinct Move previews did not land: \(previewDigests)")
    // An EMPTY window fails (a window past the canvas is a crop past it).
    moveOwner.request(.crop(rect: CGRect(x: 1, y: 1, width: 0, height: 0),
                            angle: 0, sampler: DR_FILTER_NEAREST))
    try require(!moveOwner.isPending && moveOwner.image == nil
        && editor.canvas.previewImage == nil
        && moveOwner.status.hasPrefix("Preview unavailable:"),
        "Failed current preview kept the prior Move image")
    moveOwner.request(.move(layers: moveOwner.capture.selectedLayers, dx: 11, dy: 5))
    try waitUntil({ !moveOwner.isPending && moveOwner.image != nil },
                  "Move preview did not recover after a failed request")
    try send(editor.canvas, .leftMouseUp, CGPoint(x: origin.x + 11, y: origin.y + 5))
    let recorded = ActionRecorder.shared.stop()
    guard let after = document.doc,
          let numeric = try source.moveLayers([0], dx: 11, dy: 5) else {
        throw DoctoredCoreError(message: "Move release or numeric move failed")
    }
    let digest = try geometryDigest(after)
    try require(digest == geometryDigest(numeric), "Move differs from numeric move_layers")
    try require(try geometryStorage(after, layer: 0) == pointer,
                "Integer Move copied its native raster buffer")
    try require(try geometryRaw(after, layer: 0) == geometryRaw(source, layer: 0),
                "Integer Move changed native raster samples")
    guard let originalInfo = source.layerInfo(0) else {
        throw DoctoredCoreError(message: "Original Move layer info missing")
    }
    try require(after.layerInfo(0)?.offsetX == originalInfo.offsetX + 11
        && after.layerInfo(0)?.offsetY == originalInfo.offsetY + 5,
        "Move offsets are not the snapped integer total")
    try require(after.layerMaskProperties(0)?.offsetX == (maskBefore ?? 0) + 11,
                "Linked mask did not move with raster")
    try require(document.history.entries.count == history + 1,
                "Move did not create exactly one history step")
    try require(recorded.count == 1 && recorded[0].tool == "move_layers",
                "Move did not record one move_layers action")
    document.undoManager?.undo(); pump()
    try require(document.doc === source, "One Undo did not restore Move source")
    document.undoManager?.redo(); pump()
    guard let redone = document.doc else { throw DoctoredCoreError(message: "Redo lost Move result") }
    try require(try geometryDigest(redone) == digest, "Redo changed Move result")
    _ = try geometryReplay(recorded, on: document, expectedDigest: digest)

    // An unlinked mask stays in place while the raw raster storage remains shared.
    _ = try geometryCall("set_mask_properties", ["layer": 0, "linked": false], document)
    guard let unlinked = document.doc else { throw DoctoredCoreError(message: "Unlinked source missing") }
    let unlinkedMask = unlinked.layerMaskProperties(0)?.offsetX
    let nudgeHistory = document.history.entries.count
    ActionRecorder.shared.start()
    try geometryKey(editor.canvas, code: 124)
    try geometryKey(editor.canvas, code: 125, modifiers: .shift)
    let nudges = ActionRecorder.shared.stop()
    guard let nudged = document.doc,
          let numericNudge = try unlinked.moveLayers([0], dx: 1, dy: 10) else {
        throw DoctoredCoreError(message: "Arrow nudge failed")
    }
    try require(try geometryDigest(nudged) == geometryDigest(numericNudge),
                "Arrow/Shift-arrow result differs from numeric Move")
    try require(nudged.layerMaskProperties(0)?.offsetX == unlinkedMask,
                "Unlinked mask moved with arrow nudge")
    try require(try geometryStorage(nudged, layer: 0) == geometryStorage(unlinked, layer: 0),
                "Arrow nudge copied the raster")
    try require(document.history.entries.count == nudgeHistory + 2
        && nudges.count == 2 && nudges.allSatisfy { $0.tool == "move_layers" },
        "Arrow presses did not each create one history/action step")
    let zeroHistory = document.history.entries.count
    ActionRecorder.shared.start()
    try send(editor.canvas, .leftMouseDown, origin)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: origin.x + 3, y: origin.y))
    try send(editor.canvas, .leftMouseDragged, origin)
    try send(editor.canvas, .leftMouseUp, origin)
    let zeroSteps = ActionRecorder.shared.stop()
    try require(document.history.entries.count == zeroHistory && zeroSteps.isEmpty,
                "Net-zero Move created history or an Action")
    // A one-row selection still moves its linked partner as one expanded set.
    let (linkedDocument, linkedEditor, _) = try openDocument(width: 36, height: 28)
    defer { linkedDocument.close(); pump() }
    _ = try geometryCall("new_layer", ["name": "Partner"], linkedDocument)
    _ = try geometryCall("link_layers", ["layers": [0, 1]], linkedDocument)
    linkedEditor.setSelectedLayers(.single(0))
    linkedEditor.selectTool(.move); pump()
    guard let linkedSource = linkedDocument.doc,
          linkedSource.movingSet([0]).count == 2 else {
        throw DoctoredCoreError(message: "Linked-set control did not expand")
    }
    let linkedHistory = linkedDocument.history.entries.count
    let linkedOrigin = CGPoint(x: 12, y: 12)
    try send(linkedEditor.canvas, .leftMouseDown, linkedOrigin)
    try send(linkedEditor.canvas, .leftMouseDragged,
             CGPoint(x: 16, y: 15), modifiers: .control)
    try send(linkedEditor.canvas, .leftMouseUp, CGPoint(x: 16, y: 15))
    guard let linkedAfter = linkedDocument.doc,
          let linkedNumeric = try linkedSource.moveLayers([0], dx: 4, dy: 3) else {
        throw DoctoredCoreError(message: "Linked-set Move failed")
    }
    try require(try geometryDigest(linkedAfter) == geometryDigest(linkedNumeric)
        && linkedDocument.history.entries.count == linkedHistory + 1,
        "Linked-set drag differs from numeric expanded Move")
    try require(try geometryStorage(linkedAfter, layer: 0)
        == geometryStorage(linkedSource, layer: 0),
        "Linked-set Move copied first native raster")
    return ["case": "precision-move-handles-ui", "drag_delta": [11, 5],
            "preview_ticks": previewDigests.count, "history_steps": 1,
            "nudge_history_steps": 2, "recorded_steps": recorded.count + nudges.count,
            "native_digest": digest, "storage_equal": true,
            "linked_mask_delta_x": 11, "unlinked_mask_delta_x": 0,
            "linked_set_size": 2, "run_action_equal": true,
            "zero_steps": zeroSteps.count]
}

private func transformHandleTrial(_ kind: String, sampler: DrResizeFilter,
                                  linkedMask: Bool) throws -> [String: Any] {
    let (document, editor, _) = try openDocument(width: 52, height: 40)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
    if !linkedMask {
        _ = try geometryCall("set_mask_properties", ["layer": 0, "linked": false], document)
    }
    guard let source = document.doc, let initial = document.previewImage else {
        throw DoctoredCoreError(message: "Transform fixture missing")
    }
    let fixtureSamples = try geometryRaw(source, layer: 0)
    let fixturePixels = stride(from: 0, to: fixtureSamples.count, by: 4).map {
        Array(fixtureSamples[$0..<$0 + 4])
    }
    try require(fixturePixels.contains { $0[3] == 0 && $0[0] > 25_000 },
                "Transform fixture lacks loud hidden RGB")
    try require(zip(fixturePixels, fixturePixels.dropFirst()).contains {
        $0.0[3] == 0 && $0.1[3] > 0 && $0.1[3] < 65_535
    }, "Transform fixture lacks a translucent edge")
    try require(fixturePixels.contains { $0[3] > 0 && $0[3] < 65_535
        && $0[0] % 257 != 0 },
        "Transform fixture lacks a non-byte-aligned u16 sample")
    editor.transformSampler = sampler
    editor.freeTransform(nil); pump()
    guard let opened = editor.transformSession,
          let owner = opened.precisionGesture else {
        throw DoctoredCoreError(message: "Free Transform did not open native handles")
    }
    let corner = opened.transform.warpedHandlePoint(
        kind == "scale" ? .bottomRight : .topLeft, of: opened.sourceRect)
    let start: CGPoint
    let finish: CGPoint
    let modifiers: NSEvent.ModifierFlags
    switch kind {
    case "scale":
        start = corner
        finish = CGPoint(x: corner.x - 12, y: corner.y - 8)
        modifiers = []
    case "rotate":
        start = CGPoint(x: corner.x - 18, y: corner.y - 18)
        finish = CGPoint(x: corner.x - 27, y: corner.y + 17)
        modifiers = []
    default:
        start = corner
        finish = CGPoint(x: corner.x + 9, y: corner.y + 5)
        modifiers = .command
    }
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    try send(editor.canvas, .leftMouseDown, start, modifiers: modifiers)
    try send(editor.canvas, .leftMouseDragged, finish, modifiers: modifiers)
    try send(editor.canvas, .leftMouseUp, finish, modifiers: modifiers)
    guard let finalSession = editor.transformSession else {
        throw DoctoredCoreError(message: "Handle drag closed Free Transform")
    }
    let matrix = finalSession.transform.matrix
    let quad = finalSession.transform.hasCornerOffsets
        ? finalSession.transform.warpedQuad(of: finalSession.sourceRect) : nil
    try require(kind == "perspective" ? quad != nil : quad == nil,
                "\(kind) selected wrong native transform family")
    try waitUntil({ !owner.isPending && owner.image != nil },
                  "\(kind) final native preview did not settle")
    guard let preview = owner.image else {
        throw DoctoredCoreError(message: "Transform preview vanished")
    }
    let filter = try RasterDocument.precisionFilter(sampler)
    let numericCommand: PrecisionPixelCommand
    if let quad {
        numericCommand = try .perspective(layer: 0, target: .pixels,
                                          quad: quad, filter: filter)
    } else {
        numericCommand = try .affine(layer: 0, target: .pixels,
                                     matrix: matrix, filter: filter)
    }
    guard let numeric = try source.performingPrecisionPixelEdit(numericCommand) else {
        throw DoctoredCoreError(message: "Numeric \(kind) refused")
    }
    try geometryKey(editor.canvas, code: 36)
    let recorded = ActionRecorder.shared.stop()
    guard let after = document.doc,
          let committedDisplay = PreparedDocumentEdit(after).preview else {
        throw DoctoredCoreError(message: "\(kind) commit/display missing")
    }
    let digest = try geometryDigest(after)
    try require(digest == geometryDigest(numeric),
                "\(kind) handle result differs from numeric native op")
    try require(try geometryRaw(after, layer: 0) == geometryRaw(numeric, layer: 0),
                "\(kind) raw layer samples differ")
    try require(try geometryMask(after, layer: 0) == geometryMask(numeric, layer: 0),
                "\(kind) raw mask samples differ")
    try require(after.layerInfo(0)?.offsetX == numeric.layerInfo(0)?.offsetX
        && after.layerInfo(0)?.offsetY == numeric.layerInfo(0)?.offsetY,
        "\(kind) native placement differs")
    let previewDelta = try geometryMaxDelta(preview, committedDisplay)
    let changedDelta = try geometryMaxDelta(initial, committedDisplay)
    try require(previewDelta <= 1, "\(kind) preview/commit delta \(previewDelta) exceeds one code")
    try require(changedDelta >= 32, "\(kind) fixture did not visibly change")
    try require(document.history.entries.count == history + 1,
                "\(kind) did not create exactly one history step")
    try require(recorded.count == 1
        && recorded[0].tool == (quad == nil ? "transform_layer" : "distort_layer"),
        "\(kind) did not record its numeric MCP twin")
    document.undoManager?.undo(); pump()
    try require(document.doc === source, "\(kind) Undo did not restore source")
    document.undoManager?.redo(); pump()
    guard let redone = document.doc else { throw DoctoredCoreError(message: "\(kind) Redo lost result") }
    try require(try geometryDigest(redone) == digest, "\(kind) Redo changed result")
    _ = try geometryReplay(recorded, on: document, expectedDigest: digest)
    return ["kind": kind, "sampler": EditorViewController.samplerAgentName(sampler),
            "mask_linked": linkedMask, "native_digest": digest,
            "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty],
            "quad": quad?.flatMap { [$0.x, $0.y] } ?? [],
            "preview_max_delta": previewDelta, "changed_max_delta": changedDelta,
            "history_steps": 1, "recorded_steps": recorded.count,
            "run_action_equal": true]
}

func precisionTransformHandlesUI() throws -> [String: Any] {
    let scale = try transformHandleTrial("scale", sampler: DR_FILTER_BILINEAR,
                                         linkedMask: true)
    let rotation = try transformHandleTrial("rotate", sampler: DR_FILTER_CATMULL_ROM,
                                            linkedMask: false)
    let perspective = try transformHandleTrial("perspective", sampler: DR_FILTER_CATMULL_ROM,
                                               linkedMask: true)
    return ["case": "precision-transform-handles-ui",
            "trials": [scale, rotation, perspective]]
}
