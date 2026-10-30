import AppKit

private func bytePaintCapTrial(_ tool: EditorTool, target: PaintTarget = .layer,
                               quickMask: Bool = false) throws -> [String: Any] {
    let (document, editor, _) = try openBytePaintDocument(opaque: tool == .heal || tool == .spotHeal)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: tool, variant: "soft", held: false)
    if quickMask {
        document.setQuickMaskActive(true); editor.syncSelectionHistory(); pump()
    } else {
        try setupBytePaintTarget(document, editor, target: target, tool: tool)
    }
    let before = try bytePaintSnapshot(document)
    let owner = document.doc
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let point = CGPoint(x: 31.5, y: 27.5)
    try send(editor.canvas, .leftMouseDown, point)
    guard var session = editor.canvas.nativeStrokeSession else {
        throw DoctoredCoreError(message: "Cap case opened no real byte session")
    }
    let prefix = session.dabs.count / 4
    try require(prefix > 0, "Cap case had no visible/deferred prefix deposit")
    if !quickMask && !target.isCoverage && tool != .spotHeal {
        try require(document.doc !== owner, "Cap case had no live prefix edit")
    }
    for _ in prefix..<NativeBrushStroke.maximumDabs {
        session.stamp(at: point, diameter: 23, pressure: 1)
    }
    try require(session.dabs.count / 4 == NativeBrushStroke.maximumDabs && session.acceptsInk,
                "Normal stamp admission missed the exact 50,000-dab limit")
    _ = try session.resolved()
    // A canvas session stops laying ink at the cap instead of refusing
    // the whole stroke; mouse-up commits the 50,000 dabs already laid.
    session.stamp(at: point, diameter: 23, pressure: 1)
    try require(session.isFull && session.refusal == nil
        && session.dabs.count / 4 == NativeBrushStroke.maximumDabs,
        "The 50,001st canvas stamp was not dropped with the session left whole")
    editor.canvas.nativeStrokeSession = session
    let alerts = AutomationAlerts.recorded.count
    try send(editor.canvas, .leftMouseUp, point)
    if tool == .spotHeal || tool == .heal {
        try waitUntil({ !BackgroundWork.occupied }, "Capped healing worker did not finish")
    }
    let steps = ActionRecorder.shared.stop()
    try require(AutomationAlerts.recorded.count == alerts,
                "Capped stroke presented an alert or error")
    try require(editor.canvas.nativeStrokeSession == nil
        && editor.canvas.preparedByteStroke == nil && editor.canvas.strokeCloneSource == nil
        && member(editor.canvas, "airbrushTimer") == nil,
        "Capped stroke retained a session, source or timer")
    try require(steps.count == 1 && !steps[0].isUnrecorded,
                "Capped stroke did not record exactly one Action: \(steps.map(\.tool))")
    guard let canonical = steps[0].arguments["native_stroke"] as? [String: Any] else {
        throw DoctoredCoreError(message: "Capped stroke's Action lacks canonical RZBR dabs")
    }
    try require(try decodeDabs(canonical).count / 4 == NativeBrushStroke.maximumDabs,
                "Capped stroke's Action does not carry the 50,000 laid dabs")
    try require((try bytePaintSnapshot(document)) != before,
                "Capped stroke committed no bytes or coverage")
    if !quickMask {
        try require(document.history.entries.count == history + 1 && !document.isLiveEditing,
            "Capped stroke is not one history step: "
            + "\(document.history.entries.count - history) entries")
    }
    return ["tool": tool.displayName, "target": target.agentName(in: document.doc),
        "quick_mask": quickMask, "prefix": prefix, "admitted": 50_000,
        "dropped_at": 50_001, "committed_dabs": 50_000, "recorded_steps": steps.count]
}

private func bytePaintCancellation(_ event: String) throws -> [String: Any] {
    let (document, editor, _) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: .brush, variant: "soft", held: true)
    let before = try bytePaintSnapshot(document)
    let owner = document.doc
    let history = document.history.entries.count
    let dirty = document.isDocumentEdited
    ActionRecorder.shared.start()
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 29.5, y: 25.5))
    try require(editor.canvas.nativeStrokeSession != nil, "Cancellation opened no byte stroke")
    switch event {
    case "escape": try geometryKey(editor.canvas, code: 53)
    case "tool": editor.selectTool(.move)
    case "close": editor.view.window?.close(); pump()
    default: throw DoctoredCoreError(message: "Unknown byte cancellation event")
    }
    let steps = ActionRecorder.shared.stop()
    try require(steps.isEmpty && editor.canvas.nativeStrokeSession == nil
        && editor.canvas.preparedByteStroke == nil && editor.canvas.strokeCloneSource == nil,
        "Cancelled byte stroke retained Action/session/source")
    try require(document.doc === owner && document.history.entries.count == history
        && document.isDocumentEdited == dirty && (try bytePaintSnapshot(document)) == before,
        "Cancelled byte stroke published partial edit")
    return ["event": event, "rollback_exact": true]
}

/// A later owner may replace the document during a live drag. Cancelling the
/// old gesture must release its session without enrolling that foreign edit.
private func bytePaintExternalEditCancellation() throws -> [String: Any] {
    let (document, editor, owner) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: .brush, variant: "soft", held: false)
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    try send(editor.canvas, .leftMouseDown, start)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5))
    try require(document.doc !== owner && document.isLiveEditing,
                "External edit case had no owned live prefix")
    guard let foreign = try? owner.withLayerName(0, "Foreign edit") else {
        throw DoctoredCoreError(message: "Foreign edit fixture unavailable")
    }
    document.doc = foreign
    try geometryKey(editor.canvas, code: 53)
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === foreign && document.isDocumentEdited
        && !document.isLiveEditing && document.history.entries.count == history
        && steps.isEmpty && editor.canvas.nativeStrokeSession == nil,
        "Byte cancellation changed or recorded a foreign document")
    return ["foreign_owner_preserved": true, "history_unchanged": true]
}

/// Reject through the real core painting call after a visible prefix: the
/// captured destination is made pixel-locked before final raster publication.
private func bytePaintFinalCoreRefusal() throws -> [String: Any] {
    let (document, editor, owner) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: .brush, variant: "soft", held: false)
    let before = try bytePaintSnapshot(document)
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    try send(editor.canvas, .leftMouseDown, start)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5))
    try require(document.doc !== owner && editor.canvas.nativeStrokeSession != nil,
                "Core refusal case had no visible live prefix")
    guard let locked = try? owner.withLayerLocks(0, DR_LOCK_PIXELS.rawValue) else {
        throw DoctoredCoreError(message: "Locked paint destination unavailable")
    }
    editor.strokeDestination = locked
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 59.5, y: 36.5))
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === owner && !document.isLiveEditing
        && document.history.entries.count == history && !document.isDocumentEdited
        && (try bytePaintSnapshot(document)) == before && steps.isEmpty
        && editor.canvas.preparedByteStroke == nil,
        "Final core refusal retained a painted prefix or complete Action")
    return ["core_rejected_final": true, "rollback_exact": true]
}

private func bytePaintSerializationRefusal() throws -> [String: Any] {
    let (document, editor, owner) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: .brush, variant: "soft", held: false)
    let before = try bytePaintSnapshot(document)
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    try send(editor.canvas, .leftMouseDown, start)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5))
    try require(document.doc !== owner, "Serialization case had no live prefix")
    guard var session = editor.canvas.nativeStrokeSession else {
        throw DoctoredCoreError(message: "Serialization case has no session")
    }
    session.stamp(at: CGPoint(x: CGFloat.nan, y: 25), diameter: 23, pressure: 1)
    editor.canvas.nativeStrokeSession = session
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 59.5, y: 36.5))
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === owner && !document.isLiveEditing
        && document.history.entries.count == history && !document.isDocumentEdited
        && (try bytePaintSnapshot(document)) == before && steps.isEmpty
        && editor.canvas.nativeStrokeSession == nil,
        "Invalid canonical serialization retained a prefix or Action")
    return ["invalid_dab_refused": true, "rollback_exact": true]
}

private func bytePaintMissingSource(_ tool: EditorTool, afterBegin: Bool) throws -> [String: Any] {
    let (document, editor, owner) = try openBytePaintDocument(opaque: tool == .heal)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: tool, variant: "soft", held: false)
    let before = try bytePaintSnapshot(document)
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    if !afterBegin { editor.canvas.cloneSource = nil }
    try send(editor.canvas, .leftMouseDown, start)
    if afterBegin {
        try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5))
        try require(document.doc !== owner, "Source loss case had no live prefix")
        editor.canvas.cloneSnapshot = nil
        try send(editor.canvas, .leftMouseUp, CGPoint(x: 59.5, y: 36.5))
    }
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === owner && !document.isLiveEditing
        && document.history.entries.count == history && !document.isDocumentEdited
        && (try bytePaintSnapshot(document)) == before && steps.isEmpty,
        "Missing source published partial clone or Action")
    return ["tool": tool.displayName, "after_begin": afterBegin, "rollback_exact": true]
}

private func bytePaintNoOp(_ zeroOpacity: Bool) throws -> [String: Any] {
    let (document, editor, owner) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: .brush, variant: "soft", held: false)
    if zeroOpacity {
        var options = ToolOptionsStore.shared.brush
        options.opacity = 0
        ToolOptionsStore.shared.brush = options
        editor.syncCanvasPaintState(); pump()
    } else {
        var remote = [UInt8](repeating: 0, count: 96 * 64)
        for y in 55..<62 { for x in 84..<94 { remote[y * 96 + x] = 255 } }
        document.setCanvasSelection(CanvasSelection(shape: .mask(remote),
            canvasWidth: 96, canvasHeight: 64))
    }
    let before = try bytePaintSnapshot(document)
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    try send(editor.canvas, .leftMouseDown, start)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5))
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 59.5, y: 36.5))
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === owner && !document.isLiveEditing
        && document.history.entries.count == history && !document.isDocumentEdited
        && (try bytePaintSnapshot(document)) == before && steps.isEmpty,
        "No-deposit byte gesture (zero opacity \(zeroOpacity)) created pixels/history/Action: "
        + "owner \(document.doc === owner), live \(document.isLiveEditing), "
        + "history \(document.history.entries.count - history), steps \(steps.count)")
    return ["zero_opacity": zeroOpacity, "exact_noop": true]
}

private func bytePaintZeroPressure() throws -> [String: Any] {
    let (document, editor, owner) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: .brush, variant: "pressure", held: false)
    let before = try bytePaintSnapshot(document)
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    try send(editor.canvas, .leftMouseDown, start, pressure: 0)
    try require((editor.canvas.nativeStrokeSession?.dabs.count ?? 0) >= 4,
        "Zero-pressure tablet gesture opened no canonical dab session")
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5), pressure: 0)
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 59.5, y: 36.5), pressure: 0)
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === owner && !document.isLiveEditing
        && document.history.entries.count == history && !document.isDocumentEdited
        && (try bytePaintSnapshot(document)) == before && steps.isEmpty,
        "Zero-pressure byte gesture published a deposit or Action")
    return ["tablet_pressure": 0, "exact_noop": true]
}

/// Reserve the remaining *accounted* working set without allocating it.
/// Mouse-up then reaches the real final canonical/raster admission and must
/// refuse the complete gesture after its already-visible live prefix.
private func bytePaintResourceRefusal() throws -> [String: Any] {
    let (document, editor, owner) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: .brush, variant: "soft", held: false)
    let before = try bytePaintSnapshot(document)
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    try send(editor.canvas, .leftMouseDown, start)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5))
    try require(document.doc !== owner, "Resource refusal had no visible live prefix")
    _ = try RasterResources.shared.report(evict: true)
    let available = try RasterResources.shared.admit(scratch: 0)
    try require(available > 40_000, "Resource fixture had no budget to reserve")
    let lease = try PrecisionEditPayload.admitted(bytes: available - 20_000)
    let errorsBefore = (NSApp as? ProbeApplication)?.errors.count ?? 0
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 59.5, y: 36.5))
    let errors = Array(((NSApp as? ProbeApplication)?.errors ?? []).dropFirst(errorsBefore))
    let steps = ActionRecorder.shared.stop()
    try require(lease.bytes > 0 && errors.contains(where: { $0.contains("working memory") })
        && document.doc === owner && !document.isLiveEditing
        && document.history.entries.count == history && !document.isDocumentEdited
        && (try bytePaintSnapshot(document)) == before && steps.isEmpty,
        "Real resource admission did not refuse and roll back the whole byte gesture")
    return ["registered_reservation_bytes": lease.bytes,
            "resource_error": errors.last ?? "", "rollback_exact": true]
}

private func bytePaintStaleTarget(_ change: String) throws -> [String: Any] {
    let (document, editor, owner) = try openByteDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    if change == "layer" {
        _ = try call("add_adjustment_layer", ["op": "invert", "name": "Other"], document: document)
        document.activeLayerIndex = 0
    }
    try configureBytePaint(editor, tool: .brush, variant: "soft", held: false)
    let original = document.doc ?? owner
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let start = CGPoint(x: 29.5, y: 25.5)
    try send(editor.canvas, .leftMouseDown, start)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 43.5, y: 31.5))
    try require(document.doc !== original, "Stale target had no live prefix")
    switch change {
    case "target": editor.setPaintTarget(.plane(.red))
    case "layer": document.activeLayerIndex = 1
    case "selection":
        document.setCanvasSelection(CanvasSelection(shape: .mask(
            [UInt8](repeating: 93, count: 96 * 64)), canvasWidth: 96, canvasHeight: 64))
    default: throw DoctoredCoreError(message: "Unknown stale target")
    }
    let historyAfterExternalChange = document.history.entries.count
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 59.5, y: 36.5))
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === original && !document.isLiveEditing
        && document.history.entries.count == historyAfterExternalChange && steps.isEmpty,
        "Stale byte gesture (\(change)) redirected or recorded pixels: "
        + "owner \(document.doc === original), live \(document.isLiveEditing), "
        + "history after external change \(document.history.entries.count - historyAfterExternalChange), "
        + "steps \(steps.count)")
    return ["change": change, "owned_pixels_restored": true,
            "external_history_entries": historyAfterExternalChange - history]
}

func byteStrokeContractsUI() throws -> [String: Any] {
    var caps: [[String: Any]] = []
    for tool in [EditorTool.brush, .eraser, .clone, .heal, .spotHeal, .dodge] {
        caps.append(try bytePaintCapTrial(tool))
    }
    for target in [PaintTarget.mask, .plane(.red), .channel(0)] {
        caps.append(try bytePaintCapTrial(.brush, target: target))
        caps.append(try bytePaintCapTrial(.eraser, target: target))
    }
    caps.append(try bytePaintCapTrial(.brush, quickMask: true))
    caps.append(try bytePaintCapTrial(.eraser, quickMask: true))
    let cancellations = try [bytePaintCancellation("escape"),
                             bytePaintCancellation("tool"),
                             bytePaintCancellation("close")]
    return ["case": "byte-stroke-contracts-ui", "caps": caps,
            "cancellations": cancellations,
            "external_edit": try bytePaintExternalEditCancellation(),
            "core_refusal": try bytePaintFinalCoreRefusal(),
            "serialization_refusal": try bytePaintSerializationRefusal(),
            "source_refusals": try [bytePaintMissingSource(.clone, afterBegin: false),
                bytePaintMissingSource(.clone, afterBegin: true),
                bytePaintMissingSource(.heal, afterBegin: false),
                bytePaintMissingSource(.heal, afterBegin: true)],
            "noops": try [bytePaintNoOp(false), bytePaintNoOp(true)],
            "zero_pressure": try bytePaintZeroPressure(),
            "resource_refusal": try bytePaintResourceRefusal(),
            "stale_targets": try ["target", "layer", "selection"].map(bytePaintStaleTarget)]
}
