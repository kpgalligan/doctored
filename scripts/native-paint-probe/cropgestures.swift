import AppKit

private func cropHandleTrial(angle: Double, sampler: DrResizeFilter) throws -> [String: Any] {
    let (document, editor, _) = try openDocument(width: 54, height: 42)
    let previousCrop = ToolOptionsStore.shared.crop
    defer {
        ToolOptionsStore.shared.crop = previousCrop
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
    _ = try geometryCall("add_guide", ["orientation": "vertical", "position": 9.0], document)
    _ = try geometryCall("set_ruler_origin", ["x": 3.0, "y": 4.0], document)
    _ = try geometryCall("select_rect", ["x": 2, "y": 2, "width": 12, "height": 9], document)
    _ = try geometryCall("save_selection", ["name": "Native crop channel"], document)
    if angle == 0 {
        _ = try geometryCall("add_text_layer", ["text": "Crop", "x": 7,
                                                 "y": 10, "size": 12], document)
    }
    guard let source = document.doc else { throw DoctoredCoreError(message: "Crop source missing") }
    let sourceDescription = angle == 0 ? source.artworkIndices.compactMap {
        source.layerMeta($0)
    } : []
    editor.transformSampler = sampler
    ToolOptionsStore.shared.crop.ratioIndex = 1 // Free: edge drag changes one axis.
    editor.selectTool(.crop); pump()
    guard let owner = editor.cropSession?.precisionGesture else {
        throw DoctoredCoreError(message: "Crop tool did not capture a native session")
    }
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    // Shrink a real corner, then a real edge; do not assign session.rect.
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 0, y: 0))
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 5, y: 4),
             modifiers: .control)
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 5, y: 4))
    guard let first = editor.cropSession?.rect else {
        throw DoctoredCoreError(message: "Corner drag lost Crop box")
    }
    try require(first.minX > 0 && first.minY > 0, "Real corner did not move Crop origin")
    let edge = CGPoint(x: first.maxX, y: first.midY)
    try send(editor.canvas, .leftMouseDown, edge)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: edge.x - 7, y: edge.y),
             modifiers: .control)
    try send(editor.canvas, .leftMouseUp, CGPoint(x: edge.x - 7, y: edge.y))
    editor.cropStraightenDegrees = angle
    guard let final = editor.cropSession,
          final.rect.maxX < first.maxX else {
        throw DoctoredCoreError(message: "Real edge did not shrink Crop window")
    }
    let window = CropSession.roundedWindow(final.rect,
        canvas: CGSize(width: source.width, height: source.height))
    try waitUntil({ !owner.isPending && owner.image?.width == Int(window.width)
        && owner.image?.height == Int(window.height) },
        "Final native Crop preview did not settle")
    guard let preview = owner.image else { throw DoctoredCoreError(message: "Crop preview missing") }
    let expected: RasterDocument?
    if angle == 0 {
        expected = try source.cropped(x: Int(window.minX), y: Int(window.minY),
                                  w: Int(window.width), h: Int(window.height))
    } else {
        let center = CGPoint(x: window.midX, y: window.midY)
        let matrix = CGAffineTransform(translationX: center.x, y: center.y)
            .rotated(by: CGFloat(-angle) * .pi / 180)
            .translatedBy(x: -center.x, y: -center.y)
        expected = try source.straightenedCrop(matrix, sampler: sampler,
            x: Int(window.minX), y: Int(window.minY),
            w: Int(window.width), h: Int(window.height), described: [])?.document
    }
    guard let expected else { throw DoctoredCoreError(message: "Numeric Crop refused") }
    try geometryKey(editor.canvas, code: 36)
    let recorded = ActionRecorder.shared.stop()
    guard let after = document.doc,
          let display = PreparedDocumentEdit(after).preview else {
        throw DoctoredCoreError(message: "Crop commit/display missing")
    }
    let digest = try geometryDigest(after)
    try require(digest == geometryDigest(expected),
                "Crop handles differ from independent numeric crop/straighten")
    try require(try geometryRaw(after, layer: 0) == geometryRaw(expected, layer: 0),
                "Crop native raster samples differ")
    try require(try geometryMask(after, layer: 0) == geometryMask(expected, layer: 0),
                "Crop native mask samples differ")
    let afterChannel = try PrecisionScalarOperations.read(after, target: "channel", index: 0).samples
    let expectedChannel = try PrecisionScalarOperations.read(expected, target: "channel", index: 0).samples
    try require(afterChannel == expectedChannel, "Crop channel samples differ")
    try require(after.guideCount == expected.guideCount
        && after.rulerOrigin == expected.rulerOrigin,
        "Crop chrome differs from numeric result")
    try require(after.width == expected.width && after.height == expected.height
        && after.layerInfo(0)?.offsetX == expected.layerInfo(0)?.offsetX
        && after.layerInfo(0)?.offsetY == expected.layerInfo(0)?.offsetY,
        "Crop canvas or placement differs")
    if angle == 0 {
        let descriptions = after.artworkIndices.compactMap { after.layerMeta($0) }
        try require(descriptions == sourceDescription && !descriptions.isEmpty,
                    "Zero-angle Crop did not preserve artwork description")
    }
    let delta = try geometryMaxDelta(preview, display)
    try require(delta <= 1, "Crop preview/commit differed by \(delta) byte codes")
    try require(window.width < CGFloat(source.width) && window.height < CGFloat(source.height),
                "Crop fixture did not change geometry")
    try require(document.history.entries.count == history + 1,
                "Crop gesture did not create one history step")
    try require(recorded.count == 1 && recorded[0].tool == "crop",
                "Crop gesture did not record one crop action")
    document.undoManager?.undo(); pump()
    try require(document.doc === source, "Crop Undo did not restore source")
    document.undoManager?.redo(); pump()
    guard let redone = document.doc else { throw DoctoredCoreError(message: "Crop Redo lost result") }
    try require(try geometryDigest(redone) == digest, "Crop Redo changed native result")
    _ = try geometryReplay(recorded, on: document, expectedDigest: digest)
    return ["angle": angle, "sampler": EditorViewController.samplerAgentName(sampler),
            "crop_window": [window.minX, window.minY, window.width, window.height],
            "native_digest": digest, "preview_max_delta": delta,
            "history_steps": 1, "recorded_steps": recorded.count,
            "channel_equal": true, "chrome_equal": true,
            "description_preserved": angle == 0,
            "run_action_equal": true]
}

func precisionCropHandlesUI() throws -> [String: Any] {
    let zero = try cropHandleTrial(angle: 0, sampler: DR_FILTER_BILINEAR)
    let bilinear = try cropHandleTrial(angle: 7.5, sampler: DR_FILTER_BILINEAR)
    let bicubic = try cropHandleTrial(angle: 7.5, sampler: DR_FILTER_CATMULL_ROM)
    return ["case": "precision-crop-handles-ui", "trials": [zero, bilinear, bicubic]]
}

private func staleGeometryTrial(_ kind: String) throws -> [String: Any] {
    let (document, editor, _) = try openDocument(width: 48, height: 36)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Probe error presenter unavailable")
    }
    app.errors.removeAll()
    let before = document.history.entries.count
    ActionRecorder.shared.start()
    switch kind {
    case "move":
        editor.selectTool(.move); pump()
        try send(editor.canvas, .leftMouseDown, CGPoint(x: 14, y: 14))
        try send(editor.canvas, .leftMouseDragged, CGPoint(x: 20, y: 17))
    case "crop":
        editor.selectTool(.crop); pump()
        try send(editor.canvas, .leftMouseDown, CGPoint(x: 0, y: 0))
        try send(editor.canvas, .leftMouseDragged, CGPoint(x: 5, y: 4),
                 modifiers: .control)
        try send(editor.canvas, .leftMouseUp, CGPoint(x: 5, y: 4))
    default:
        editor.freeTransform(nil); pump()
        guard let session = editor.transformSession else {
            throw DoctoredCoreError(message: "Stale \(kind) session did not open")
        }
        let corner = session.transform.warpedHandlePoint(.topLeft, of: session.sourceRect)
        let flags: NSEvent.ModifierFlags = kind == "perspective" ? .command : []
        try send(editor.canvas, .leftMouseDown, corner, modifiers: flags)
        try send(editor.canvas, .leftMouseDragged,
                 CGPoint(x: corner.x + 6, y: corner.y + 4), modifiers: flags)
        try send(editor.canvas, .leftMouseUp,
                 CGPoint(x: corner.x + 6, y: corner.y + 4), modifiers: flags)
    }
    let owner = kind == "move" ? editor.precisionMoveGesture
        : kind == "crop" ? editor.cropSession?.precisionGesture
        : editor.transformSession?.precisionGesture
    guard let owner, owner.previewReservationsIssued > 0 else {
        throw DoctoredCoreError(message: "Stale \(kind) had no captured preview reservation")
    }
    // This is a real foreign document edit while the captured gesture is open.
    _ = try geometryCall("add_guide", ["orientation": "vertical", "position": 8.5], document)
    guard let foreign = document.doc else { throw DoctoredCoreError(message: "Foreign edit lost document") }
    let foreignDigest = try geometryDigest(foreign)
    let activeAtCancellation = owner.activeReservationsAtTerminalCancellation
    try require(activeAtCancellation > 0,
                "Stale \(kind) cancelled no active owner reservation")
    pump()
    switch kind {
    case "move":
        try send(editor.canvas, .leftMouseUp, CGPoint(x: 20, y: 17))
        try require(editor.canvas.previewImage == nil, "Stale Move preview survived")
    case "crop":
        try geometryKey(editor.canvas, code: 36)
        try require(editor.canvas.precisionCropPreview == nil, "Stale Crop preview survived")
    default:
        try geometryKey(editor.canvas, code: 36)
        try require(editor.canvas.transformPreview?.composite == nil,
                    "Stale \(kind) preview survived")
    }
    pump()
    let recorded = ActionRecorder.shared.stop()
    try require(document.doc === foreign
        && document.history.entries.count == before + 1,
        "Stale \(kind) overwrote the foreign edit or added history")
    try require(recorded.count == 1 && recorded[0].tool == "add_guide",
                "Stale \(kind) added an Action beside the foreign edit")
    try require(app.errors.contains { $0.contains(
        "The document, target or selection changed while the 16-bit edit was running.") },
        "Stale \(kind) sentence missed presenter: \(app.errors)")
    try waitUntil({ owner.outstandingTerminalCancelledReservations == 0
        && owner.outstandingPreviewReservations == 0 },
                  "Stale \(kind) preview reservation did not drain")
    try require(try geometryDigest(document.doc ?? foreign) == foreignDigest,
                "Late \(kind) worker changed foreign result")
    return ["kind": kind, "foreign_digest": foreignDigest,
            "history_steps": 1, "recorded_steps": recorded.count,
            "stale_sentence_presented": true, "reservations_drained": true,
            "reservations_issued": owner.previewReservationsIssued,
            "active_at_cancellation": activeAtCancellation,
            "cancelled_ids_remaining": owner.outstandingTerminalCancelledReservations]
}

func precisionGeometryLifecycleUI() throws -> [String: Any] {
    let foreign = try ["move", "affine", "perspective", "crop"].map(staleGeometryTrial)
    let (document, editor, _) = try openDocument(width: 48, height: 36)
    defer { document.close(); pump() }
    _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
    guard let source = document.doc, let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Lifecycle source/presenter missing")
    }
    editor.freeTransform(nil); pump()
    guard editor.transformSession?.precisionGesture != nil else {
        throw DoctoredCoreError(message: "Target-stale transform did not open")
    }
    app.errors.removeAll()
    editor.setPaintTarget(.mask)
    try require(document.doc === source, "Changing target replaced document")
    try geometryKey(editor.canvas, code: 36)
    try require(app.errors.contains { $0.contains("target or selection changed") },
                "Target change did not refuse captured transform")
    editor.endTransformSession()

    editor.selectTool(.crop); pump()
    guard editor.cropSession?.precisionGesture != nil else {
        throw DoctoredCoreError(message: "Selection-stale Crop did not open")
    }
    app.errors.removeAll()
    _ = try geometryCall("select_rect", ["x": 2, "y": 2, "width": 8, "height": 8], document)
    try require(document.doc === source, "Changing selection replaced document")
    try geometryKey(editor.canvas, code: 36)
    try require(app.errors.contains { $0.contains("target or selection changed") },
                "Selection change did not refuse captured Crop")
    editor.endCropSession()
    _ = try geometryCall("deselect", [:], document)

    // Escape cancels without a model, history, dirty or redo change.
    let escapeHistory = document.history.entries.count
    let escapeDirty = document.isDocumentEdited
    editor.selectTool(.move); pump()
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 14, y: 14))
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 19, y: 17))
    guard let escapeOwner = editor.precisionMoveGesture,
          escapeOwner.previewReservationsIssued > 0 else {
        throw DoctoredCoreError(message: "Escape control had no captured preview reservation")
    }
    try geometryKey(editor.canvas, code: 53)
    let escapeActiveAtCancellation = escapeOwner.activeReservationsAtTerminalCancellation
    try require(escapeActiveAtCancellation > 0,
                "Escape cancelled no active owner reservation")
    try waitUntil({ escapeOwner.outstandingTerminalCancelledReservations == 0
        && escapeOwner.outstandingPreviewReservations == 0 },
                  "Escape preview reservation did not drain")
    try require(editor.precisionMoveGesture == nil
        && document.history.entries.count == escapeHistory
        && document.isDocumentEdited == escapeDirty,
        "Escape published or dirtied a native Move")

    // A rejected typed capture must not close another tool's live edit.
    app.errors.removeAll()
    document.beginLiveEdit()
    try require(document.isLiveEditing, "Could not open the control live edit")
    let refusedHistory = document.history.entries.count
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 14, y: 14))
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 14, y: 14))
    try require(document.isLiveEditing && editor.precisionMoveGesture == nil
        && document.history.entries.count == refusedHistory,
        "Rejected native Move capture closed an unrelated live edit")
    try require(app.errors.contains { $0.contains("Finish the current gesture") },
                "Rejected native Move capture did not name the live edit")
    document.endLiveEdit("Probe control", record: .notACommand)

    // Release immediately after the final tick, before its worker can land.
    editor.selectTool(.move); pump()
    let immediateSource = document.doc
    let immediateHistory = document.history.entries.count
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 14, y: 14))
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 21, y: 18),
             modifiers: .control)
    guard let immediateOwner = editor.precisionMoveGesture,
          immediateOwner.previewReservationsIssued > 0 else {
        throw DoctoredCoreError(message: "Immediate Move had no captured preview reservation")
    }
    try require(editor.moveAppliedDelta?.x == 7 && editor.moveAppliedDelta?.y == 4,
                "Immediate Move did not latch the requested 7,4 delta")
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 21, y: 18))
    let immediateActiveAtCancellation = immediateOwner.activeReservationsAtTerminalCancellation
    try require(immediateActiveAtCancellation > 0,
                "Immediate Move release cancelled no active owner reservation")
    guard let immediate = document.doc,
          let expected = try immediateSource?.moveLayers([0], dx: 7, dy: 4) else {
        throw DoctoredCoreError(message: "Immediate final tick did not commit")
    }
    try require(try geometryDigest(immediate) == geometryDigest(expected)
        && document.history.entries.count == immediateHistory + 1,
        "Unpumped final tick did not publish final parameters once")
    try waitUntil({ immediateOwner.outstandingTerminalCancelledReservations == 0
        && immediateOwner.outstandingPreviewReservations == 0 },
                  "Immediate final tick reservation did not drain")
    try require(editor.canvas.previewImage == nil, "Late Move preview returned after release")

    // Closing one actual view cancels only that view's owner.
    guard let second = document.openSecondView() else {
        throw DoctoredCoreError(message: "Lifecycle second view did not open")
    }
    _ = second.view; pump()
    editor.selectTool(.move); pump()
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 10, y: 10))
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 12, y: 11))
    guard let survivingOwner = editor.precisionMoveGesture,
          survivingOwner.previewReservationsIssued > 0 else {
        throw DoctoredCoreError(message: "Surviving view had no captured preview reservation")
    }
    second.selectTool(.move); pump()
    try send(second.canvas, .leftMouseDown, CGPoint(x: 10, y: 10))
    try send(second.canvas, .leftMouseDragged, CGPoint(x: 13, y: 12))
    guard let closingOwner = second.precisionMoveGesture,
          closingOwner.previewReservationsIssued > 0 else {
        throw DoctoredCoreError(message: "Closing view had no captured preview reservation")
    }
    guard let secondWindow = second.view.window else {
        throw DoctoredCoreError(message: "Second view window missing")
    }
    secondWindow.performClose(nil)
    let closeActiveAtCancellation = closingOwner.activeReservationsAtTerminalCancellation
    try require(closeActiveAtCancellation > 0,
                "Closing view cancelled no active owner reservation")
    try waitUntil({ document.windowControllers.count == 1 }, "Second view did not close")
    try require(second.precisionMoveGesture == nil && editor.precisionMoveGesture != nil,
                "Closing second view cancelled the surviving gesture or kept its own")
    editor.moveDidCancel()
    try waitUntil({ closingOwner.outstandingTerminalCancelledReservations == 0
        && closingOwner.outstandingPreviewReservations == 0
        && survivingOwner.outstandingPreviewReservations == 0 },
                  "View-close reservations did not drain")
    return ["case": "precision-geometry-lifecycle-ui", "foreign_trials": foreign,
            "target_stale": true, "selection_stale": true,
            "escape_clean": true, "immediate_final_digest": try geometryDigest(immediate),
            "escape_active_at_cancellation": escapeActiveAtCancellation,
            "escape_cancelled_ids_remaining": escapeOwner.outstandingTerminalCancelledReservations,
            "view_close_cancelled_only_own": true,
            "late_preview_absent": true, "reservations_drained": true,
            "immediate_active_at_cancellation": immediateActiveAtCancellation,
            "immediate_cancelled_ids_remaining": immediateOwner.outstandingTerminalCancelledReservations,
            "view_close_active_at_cancellation": closeActiveAtCancellation,
            "view_close_cancelled_ids_remaining": closingOwner.outstandingTerminalCancelledReservations]
}

private func offsetAliases(_ document: ImageDocument, depth: String) throws -> [Int] {
    let info = try geometryCall("get_document", [:], document)
    guard let layers = info["layers"] as? [[String: Any]],
          let row = layers.first,
          let x = row["offset_x"] as? NSNumber,
          let y = row["offset_y"] as? NSNumber else {
        throw DoctoredCoreError(message: "\(depth) get_document omitted offset aliases")
    }
    if depth != "8" {
        guard let pair = row["offset"] as? [NSNumber], pair.count == 2 else {
            throw DoctoredCoreError(message: "\(depth) get_document lost existing offset pair")
        }
        try require(pair[0].intValue == x.intValue && pair[1].intValue == y.intValue,
                    "\(depth) offset aliases disagree with the retained pair")
    }
    return [x.intValue, y.intValue]
}

func precisionGeometryFloatUI() throws -> [String: Any] {
    let (document, editor, _) = try openFloatDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    let byte = try openByteDocument(width: 8, height: 6)
    defer { byte.0.close(); pump() }
    let byteOffsets = try offsetAliases(byte.0, depth: "8")
    try require(try offsetAliases(document, depth: "float").count == byteOffsets.count,
                "Float and byte offset shapes differ")
    let sixteen = try openDocument(width: 8, height: 6)
    defer { sixteen.0.close(); pump() }
    let sixteenOffsets = try offsetAliases(sixteen.0, depth: "16")
    try require(sixteenOffsets.count == byteOffsets.count,
                "16-bit and byte offset shapes differ")

    guard let source = document.doc, let initial = document.previewImage else {
        throw DoctoredCoreError(message: "Float Move source/display missing")
    }
    try require(DocumentPrecision.allows(.move, document: document),
                "Float Move tool is unavailable")
    editor.selectTool(.move); pump()
    let origin = CGPoint(x: min(CGFloat(source.width - 1), 1),
                         y: min(CGFloat(source.height - 1), 1))
    let before = document.history.entries.count
    ActionRecorder.shared.start()
    try send(editor.canvas, .leftMouseDown, origin)
    try send(editor.canvas, .leftMouseDragged,
             CGPoint(x: origin.x + 1, y: origin.y + 1), modifiers: .control)
    guard let owner = editor.precisionMoveGesture else {
        throw DoctoredCoreError(message: "Float Move did not capture a gesture")
    }
    try waitUntil({ !owner.isPending && owner.image != nil },
                  "Float tone-mapped Move preview did not settle")
    try require(owner.status == "Preview current",
                "Float Move preview ended: \(owner.status)")
    try require(editor.moveAppliedDelta?.x == 1 && editor.moveAppliedDelta?.y == 1,
                "Float Move snapped total is \(String(describing: editor.moveAppliedDelta))")
    guard let preview = owner.image else { throw DoctoredCoreError(message: "Float preview missing") }
    try send(editor.canvas, .leftMouseUp,
             CGPoint(x: origin.x + 1, y: origin.y + 1))
    let recorded = ActionRecorder.shared.stop()
    guard let after = document.doc,
          let numeric = try source.moveLayers([0], dx: 1, dy: 1),
          let committed = PreparedDocumentEdit(after).preview else {
        throw DoctoredCoreError(message: "Float Move/numeric/display missing")
    }
    let digest = try geometryDigest(after)
    try require(digest == geometryDigest(numeric),
                "Float real Move differs from exact numeric Move; actual offset "
                    + "\(String(describing: after.layerInfo(0)?.offsetX)),"
                    + "\(String(describing: after.layerInfo(0)?.offsetY))")
    let previewDelta = try geometryMaxDelta(preview, committed)
    try require(previewDelta <= 1, "Float tone-map preview differs by \(previewDelta) codes")
    try require(try geometryMaxDelta(initial, committed) >= 1,
                "Float Move fixture did not change the display")
    try require(document.history.entries.count == before + 1
        && recorded.count == 1 && recorded[0].tool == "move_layers",
        "Float Move did not make one history/action step")
    document.undoManager?.undo(); pump()
    try require(document.doc === source, "Float Move Undo did not restore source")
    document.undoManager?.redo(); pump()
    guard let redone = document.doc else { throw DoctoredCoreError(message: "Float Redo lost Move") }
    try require(try geometryDigest(redone) == digest, "Float Redo changed Move")
    _ = try geometryReplay(recorded, on: document, expectedDigest: digest)

    let nudgeSource = document.doc
    let nudgeHistory = document.history.entries.count
    ActionRecorder.shared.start()
    try geometryKey(editor.canvas, code: 124)
    let nudges = ActionRecorder.shared.stop()
    guard let nudgeAfter = document.doc,
          let nudgeExpected = try nudgeSource?.moveLayers([0], dx: 1, dy: 0) else {
        throw DoctoredCoreError(message: "Float arrow Move failed")
    }
    try require(try geometryDigest(nudgeAfter) == geometryDigest(nudgeExpected)
        && document.history.entries.count == nudgeHistory + 1
        && nudges.count == 1 && nudges[0].tool == "move_layers",
        "Float arrow Move differs from numeric or lost history/action")

    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Float refusal presenter missing")
    }
    let refusalHistory = document.history.entries.count
    let refusalSource = document.doc
    app.errors.removeAll()
    editor.freeTransform(nil)
    editor.beginCropSession()
    try require(app.errors.contains { $0.contains("Free Transform") }
        && app.errors.contains { $0.contains("Crop tool") },
        "Float direct entries did not name Free Transform and Crop tool")
    for (tool, arguments, named) in [
        ("transform_layer", ["layer": 0, "dx": 1] as [String: Any], "transform_layer"),
        ("distort_layer", ["layer": 0, "corners": [0, 0, 1, 0, 1, 1, 0, 1]], "distort_layer"),
        ("crop", ["x": 0, "y": 0, "width": source.width,
                  "height": source.height, "angle": 7.5], "Straightening"),
    ] {
        let reply = try call(tool, arguments, document: document)
        try require(try responseIsError(reply) && reply.contains(named),
                    "Float \(tool) did not refuse by name: \(reply)")
    }
    try require(document.doc === refusalSource
        && document.history.entries.count == refusalHistory,
        "Float refused geometry changed document/history")
    let cropWidth = max(source.width - 1, 1)
    let cropHeight = max(source.height - (source.width == 1 ? 1 : 0), 1)
    let exact = try geometryCall("crop", ["x": 0, "y": 0,
        "width": cropWidth, "height": cropHeight], document)
    try require(exact["ok"] as? Bool == true, "Exact numeric float Crop refused")
    return ["case": "precision-geometry-float-ui", "move_delta": [1, 1],
            "native_digest": digest, "preview_max_delta": previewDelta,
            "history_steps": 1, "recorded_steps": recorded.count,
            "nudge_steps": nudges.count, "offset_16": sixteenOffsets,
            "offset_float": try offsetAliases(document, depth: "float"),
            "offset_byte": byteOffsets, "run_action_equal": true,
            "named_refusals": 5, "exact_crop_ok": true]
}
