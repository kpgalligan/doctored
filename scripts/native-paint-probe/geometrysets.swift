import AppKit

/// Actual Free Transform handles on an isolated group, compared with the
/// numeric set request captured from the same released session.
func geometrySetHandlesUI() throws -> [String: Any] {
    var trials: [[String: Any]] = []
    for typed in [false, true] {
        for fixture in ["group", "adjustment", "multi"] {
        for perspective in [false, true] {
            let (document, editor, _) = typed
                ? try openDocument(width: 52, height: 40)
                : try openByteDocument(width: 52, height: 40)
            defer { document.close(); pump() }
            let roots: [Int]
            switch fixture {
            case "group":
                _ = try geometryCall("duplicate_layer", ["index": 0], document)
                _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
                _ = try geometryCall("group_layers", ["layers": [0, 1], "name": "Geometry Set"], document)
                guard let source = document.doc,
                      let group = (0..<source.layerCount).first(where: source.layerIsGroup) else {
                    throw DoctoredCoreError(message: "Owned group fixture unavailable")
                }
                roots = [group]
            case "adjustment":
                _ = try geometryCall("add_adjustment_layer", ["op": "invert", "name": "Fx"], document)
                guard let source = document.doc,
                      let adjustment = (0..<source.layerCount).first(where: source.layerIsAdjustment) else {
                    throw DoctoredCoreError(message: "Owned adjustment fixture unavailable")
                }
                _ = try geometryCall("add_layer_mask", ["layer": adjustment,
                    "kind": "reveal_all"], document)
                roots = [adjustment]
            default:
                _ = try geometryCall("duplicate_layer", ["index": 0], document)
                _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
                roots = [0, 1]
            }
            _ = try geometryCall("set_selected_layers", ["layers": roots], document)
            guard let source = document.doc else {
                throw DoctoredCoreError(message: "Owned set source unavailable")
            }
            let scope = OperationScope.refusalReason(.layerGeometry,
                target: editor.paintTarget, doc: source,
                layerIndex: document.activeLayerIndex)
            editor.transformSampler = DR_FILTER_NEAREST
            editor.freeTransform(nil); pump()
            guard let session = editor.transformSession else {
                throw DoctoredCoreError(message: "\(fixture) Free Transform handles did not open: "
                    + "roots \(roots), selected \(document.selectedLayerIndices), "
                    + "info \(String(describing: source.layerInfo(roots[0]))), "
                    + "mask \(String(describing: source.layerMaskProperties(roots[0]))), "
                    + "moving \(source.movingSet(roots)), "
                    + "target \(editor.paintTarget), scope \(String(describing: scope)), "
                    + "rect \(String(describing: editor.transformPixelSourceRect(source, layers: source.movingSet(roots))))")
            }
            if typed {
                try require(session.precisionGesture != nil,
                            "Typed \(fixture) opened display-only rather than native handles")
            }
            let corner = session.transform.warpedHandlePoint(
                perspective ? .topLeft : .bottomRight, of: session.sourceRect)
            let end = perspective
                ? CGPoint(x: corner.x + 9, y: corner.y + 5)
                : CGPoint(x: corner.x - 12, y: corner.y - 8)
            let modifiers: NSEvent.ModifierFlags = perspective ? .command : []
            let steps = document.history.entries.count
            try send(editor.canvas, .leftMouseDown, corner, modifiers: modifiers)
            try send(editor.canvas, .leftMouseDragged, end, modifiers: modifiers)
            try send(editor.canvas, .leftMouseUp, end, modifiers: modifiers)
            guard let released = editor.transformSession else {
                throw DoctoredCoreError(message: "Group transform session vanished on release")
            }
            if let gesture = released.precisionGesture {
                try waitUntil({ !gesture.isPending }, "Set preview did not settle")
            }
            let request: GeometrySetRequest
            if perspective {
                try require(released.transform.hasCornerOffsets,
                            "Perspective handle did not create a quad")
                request = .perspective(layers: roots, sourceRect: released.sourceRect,
                    quad: released.transform.warpedQuad(of: released.sourceRect),
                    sampler: DR_FILTER_NEAREST)
            } else {
                request = .affine(layers: roots, matrix: released.transform.matrix,
                                  sampler: DR_FILTER_NEAREST)
            }
            guard let numeric = try source.geometrySet(request) else {
                throw DoctoredCoreError(message: "\(fixture) numeric comparison was a no-op at depth "
                    + "\(typed ? 16 : 8), perspective \(perspective), matrix "
                    + "\(released.transform.matrix), rect \(released.sourceRect)")
            }
            let preview: CGImage?
            if let gesture = released.precisionGesture {
                try waitUntil({ !gesture.isPending && gesture.image != nil },
                              "\(fixture) native set preview did not settle")
                preview = gesture.image
            } else if let corePreview = released.corePreview {
                try waitUntil({ !corePreview.isPending && corePreview.image != nil },
                              "\(fixture) byte set preview did not settle")
                preview = corePreview.image
            } else {
                throw DoctoredCoreError(message: "\(fixture) set has no core preview")
            }
            try geometryKey(editor.canvas, code: 36)
            guard let after = document.doc,
                  let display = PreparedDocumentEdit(after).preview,
                  let preview else {
                throw DoctoredCoreError(message: "Set handle commit lost the document")
            }
            try require(try geometryDigest(after) == geometryDigest(numeric),
                        "\(fixture) handle differs from the native set request")
            let previewDelta = try geometryMaxDelta(preview, display)
            try require(previewDelta <= 1,
                        "\(fixture) preview/release differs by \(previewDelta) codes")
            if fixture == "adjustment" {
                try require(after.layerIsAdjustment(roots[0])
                    && after.layerMaskProperties(roots[0])?.offsetX
                        == numeric.layerMaskProperties(roots[0])?.offsetX,
                    "Adjustment handle did not carry its linked mask")
            }
            try require(document.history.entries.count == steps + 1,
                        "Set handle did not publish exactly one history step")
            document.undoManager?.undo(); pump()
            try require(document.doc === source, "Set handle Undo lost its captured source")
            trials.append(["depth": typed ? 16 : 8, "fixture": fixture,
                           "mapping": perspective ? "perspective" : "affine",
                           "roots": roots, "expanded_count": source.movingSet(roots).count,
                           "one_undo": true, "numeric_equal": true,
                           "preview_delta": previewDelta])
        }
        }
    }
    // The host worker must honor an explicit rectangle even for one root.
    // It deliberately differs from that raster's own bounds.
    let (rectDocument, _, _) = try openByteDocument(width: 52, height: 40)
    defer { rectDocument.close(); pump() }
    guard let rectSource = rectDocument.doc else {
        throw DoctoredCoreError(message: "Explicit rectangle source missing")
    }
    let explicit = CGRect(x: 3, y: 2, width: 39, height: 31)
    let quad = [CGPoint(x: 3, y: 2), CGPoint(x: 41, y: 3),
                CGPoint(x: 39, y: 33), CGPoint(x: 2, y: 32)]
    let requested = GeometrySetRequest.perspective(layers: [0], sourceRect: explicit,
        quad: quad, sampler: DR_FILTER_NEAREST)
    guard let numeric = try rectSource.geometrySet(requested),
          let worked = try TransformWork(source: rectSource, layers: [0], quad: quad,
              matrix: .identity, sampler: DR_FILTER_NEAREST,
              droppingDescriptions: [], sourceRect: explicit).transformedDocumentChecked()
    else { throw DoctoredCoreError(message: "Explicit rectangle transform was a no-op") }
    try require(try geometryDigest(numeric) == geometryDigest(worked),
                "Single-root TransformWork replaced the supplied source rectangle")
    return ["case": "geometry-set-handles-ui", "trials": trials,
            "cancellation": try geometrySetCancellationUI()]
}

private func setGestureFixture() throws -> (ImageDocument, EditorViewController, RasterDocument) {
    let (document, editor, _) = try openDocument(width: 512, height: 384)
    _ = try geometryCall("duplicate_layer", ["index": 0], document)
    _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
    _ = try geometryCall("group_layers", ["layers": [0, 1], "name": "Cancellation set"], document)
    guard let source = document.doc,
          let group = (0..<source.layerCount).first(where: source.layerIsGroup) else {
        throw DoctoredCoreError(message: "Cancellation group fixture unavailable")
    }
    _ = try geometryCall("add_layer_mask", ["layer": group, "kind": "reveal_all"], document)
    _ = try geometryCall("set_selected_layers", ["layers": [group]], document)
    editor.transformSampler = DR_FILTER_LANCZOS3
    guard let masked = document.doc else {
        throw DoctoredCoreError(message: "Cancellation group mask unavailable")
    }
    return (document, editor, masked)
}

private func startSetDrag(_ editor: EditorViewController) throws
    -> (PrecisionGeometryGesture, EditorViewController.TransformSession, CGPoint) {
    editor.freeTransform(nil); pump()
    guard let session = editor.transformSession,
          let owner = session.precisionGesture else {
        throw DoctoredCoreError(message: "Cancellation set handles did not open")
    }
    let corner = session.transform.warpedHandlePoint(.topLeft, of: session.sourceRect)
    let moved = CGPoint(x: corner.x + 15, y: corner.y + 11)
    try send(editor.canvas, .leftMouseDown, corner, modifiers: .control)
    try send(editor.canvas, .leftMouseDragged, moved, modifiers: .control)
    try require(owner.previewReservationsIssued > 0,
                "Set drag created no owner preview reservation")
    return (owner, session, moved)
}

private func drainedSetOwner(_ owner: PrecisionGeometryGesture, _ label: String) throws -> Int {
    let active = owner.activeReservationsAtTerminalCancellation
    try require(active > 0, "\(label) cancelled no current active reservation IDs")
    try waitUntil({ owner.outstandingTerminalCancelledReservations == 0
        && owner.outstandingPreviewReservations == 0 },
        "\(label) did not drain its cancelled reservation IDs")
    return active
}

private func geometrySetCancellationUI() throws -> [String: Any] {
    var receipts: [[String: Any]] = []
    for event in ["stale", "target_stale", "selection_stale", "escape", "immediate_release"] {
        let (document, editor, source) = try setGestureFixture()
        defer { document.close(); pump() }
        let history = document.history.entries.count
        let (owner, _, moved) = try startSetDrag(editor)
        let active: Int
        switch event {
        case "stale":
            _ = try geometryCall("add_guide", ["orientation": "vertical",
                "position": 12.5], document)
            let foreign = document.doc
            try geometryKey(editor.canvas, code: 36)
            active = try drainedSetOwner(owner, "Stale set")
            try require(document.doc === foreign && document.history.entries.count == history + 1,
                        "Stale set overwrote the foreign edit")
        case "target_stale":
            editor.setPaintTarget(.mask)
            try require(editor.paintTarget == .mask && document.planeEditTarget == .mask,
                        "Set target change did not land before cancellation")
            try geometryKey(editor.canvas, code: 36)
            active = try drainedSetOwner(owner, "Target-stale set")
            try require(document.doc === source && document.history.entries.count == history
                && editor.paintTarget == .mask && document.planeEditTarget == .mask,
                "Target-stale set published or changed the chosen mask view")
            editor.endTransformSession()
        case "selection_stale":
            _ = try geometryCall("select_rect", ["x": 3, "y": 4,
                "width": 19, "height": 17], document)
            try require(document.canvasSelection != nil,
                        "Set selection change did not land before cancellation")
            let selectionHistory = document.history.entries.count
            try geometryKey(editor.canvas, code: 36)
            active = try drainedSetOwner(owner, "Selection-stale set")
            try require(document.doc === source
                && document.history.entries.count == selectionHistory
                && document.canvasSelection != nil,
                "Selection-stale set published or erased the chosen selection")
            editor.endTransformSession()
        case "escape":
            try geometryKey(editor.canvas, code: 53)
            active = try drainedSetOwner(owner, "Escape set")
            try require(document.doc === source && document.history.entries.count == history,
                        "Escape set published geometry or history")
        default:
            try send(editor.canvas, .leftMouseUp, moved, modifiers: .control)
            guard let released = editor.transformSession else {
                throw DoctoredCoreError(message: "Immediate set session vanished")
            }
            let request = GeometrySetRequest.affine(layers: released.layers,
                matrix: released.transform.matrix, sampler: DR_FILTER_LANCZOS3)
            guard let numeric = try source.geometrySet(request) else {
                throw DoctoredCoreError(message: "Immediate set numeric result is a no-op: "
                    + "matrix \(released.transform.matrix), rect \(released.sourceRect), "
                    + "status \(owner.status)")
            }
            try geometryKey(editor.canvas, code: 36)
            active = try drainedSetOwner(owner, "Immediate set release")
            guard let after = document.doc else {
                throw DoctoredCoreError(message: "Immediate set lost document")
            }
            try require(try geometryDigest(after) == geometryDigest(numeric)
                && document.history.entries.count == history + 1,
                "Immediate set release did not publish its final parameters once")
        }
        receipts.append(["event": event, "active_at_cancellation": active,
                         "cancelled_ids_remaining": owner.outstandingTerminalCancelledReservations])
    }

    let (document, editor, _) = try setGestureFixture()
    defer { document.close(); pump() }
    guard let second = document.openSecondView() else {
        throw DoctoredCoreError(message: "Set view-close control did not open")
    }
    _ = second.view; pump()
    let (survivor, _, _) = try startSetDrag(editor)
    let (closing, _, _) = try startSetDrag(second)
    guard let window = second.view.window else {
        throw DoctoredCoreError(message: "Closing set view has no window")
    }
    window.performClose(nil)
    let active = try drainedSetOwner(closing, "Set view close")
    try require(second.transformSession == nil && editor.transformSession != nil,
                "Set view close cancelled the other view or kept its own session")
    editor.endTransformSession()
    try waitUntil({ survivor.outstandingPreviewReservations == 0 },
                  "Surviving set view did not drain")
    receipts.append(["event": "view_close", "active_at_cancellation": active,
                     "cancelled_ids_remaining": closing.outstandingTerminalCancelledReservations])
    return ["trials": receipts]
}
