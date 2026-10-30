import AppKit

// Phase 12D-2: the UI-SOURCE half of view-only rotation and the synchronized
// second view. An MCP drive takes DOCUMENT coordinates, so it can never prove
// that a pointer lands where the user aimed under a rotated view, that an
// ⌥-drag turns the picture without touching a pixel, or that a second real
// NSWindow on the same NSDocument redraws, keeps its own zoom and leaves the
// document open and clean when it closes. Those are driven here, through the
// real controllers and the real canvas handlers, on the `workspace.swift`
// precedent.

/// A mouse event at an exact WINDOW point, rather than at a point in the
/// receiving view's own space (`send`'s contract).
///
/// It exists for the rotated-view case: the whole question there is where a
/// window-space press lands in DOCUMENT space, so the case has to choose the
/// window point itself and must not go through the very conversion it is
/// measuring.
@discardableResult
func sendWindow(_ view: NSView, _ kind: NSEvent.EventType, windowPoint: CGPoint,
                modifiers: NSEvent.ModifierFlags = []) throws -> NSEvent {
    guard let window = view.window,
          let event = NSEvent.mouseEvent(
            with: kind, location: windowPoint, modifierFlags: modifiers,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
    else { throw DoctoredCoreError(message: "Owned window-space mouse event unavailable") }
    try require(hypot(event.locationInWindow.x - windowPoint.x,
                      event.locationInWindow.y - windowPoint.y) < 0.0001,
        "Synthetic event moved the window point to \(event.locationInWindow)")
    switch kind {
    case .leftMouseDown: view.mouseDown(with: event)
    case .leftMouseDragged: view.mouseDragged(with: event)
    case .leftMouseUp: view.mouseUp(with: event)
    case .mouseMoved: view.mouseMoved(with: event)
    default: throw DoctoredCoreError(message: "Unsupported probe event")
    }
    return event
}

/// A fully TRANSPARENT 8-bit document: every alpha byte zero, so the flattened
/// composite of the document after one dab is the dab and nothing else, and an
/// alpha-weighted centroid is the dab's centre.
private func openTransparentByteDocument(width: Int, height: Int)
    throws -> (ImageDocument, EditorViewController)
{
    let pixels = [UInt8](repeating: 0, count: width * height * 4)
    guard let image = RasterImage.from(rgba: pixels, width: width, height: height),
          let source = RasterDocument.from(image: image) else {
        throw DoctoredCoreError(message: "Owned transparent fixture unavailable")
    }
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(
        document: source, selection: SelectionHistoryState(), layerSelection: .single(0),
        paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 8, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    document.showWindows()
    guard let editor = document.windowControllers.first?
        .contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Actual editor missing")
    }
    _ = editor.view
    editor.view.window?.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    pump()
    return (document, editor)
}

/// The alpha-weighted centroid of a flattened RGBA8 composite, over PIXEL
/// CENTRES — so a dab centred on an integer canvas coordinate answers that
/// coordinate exactly.
private func alphaCentroid(_ bytes: [UInt8], width: Int, height: Int) throws -> CGPoint {
    var weight = 0.0, sumX = 0.0, sumY = 0.0
    for y in 0..<height {
        for x in 0..<width {
            let alpha = Double(bytes[(y * width + x) * 4 + 3])
            guard alpha > 0 else { continue }
            weight += alpha
            sumX += alpha * (Double(x) + 0.5)
            sumY += alpha * (Double(y) + 0.5)
        }
    }
    try require(weight > 0, "No pixel carries coverage")
    return CGPoint(x: sumX / weight, y: sumY / weight)
}

/// The case's own oracle, in the flipped host space: H = hostCentre +
/// R(θ)·(P − canvasCentre) with R = [[cos, −sin], [sin, cos]], a clockwise
/// turn on screen, converted to a WINDOW point.
private func rotatedWindowPoint(editor: EditorViewController, canvasSize: CGSize,
                                target: CGPoint, degrees: Double) -> CGPoint {
    let host = editor.canvasHost.frame.size
    let radians = degrees * Double.pi / 180
    let dx = Double(target.x) - Double(canvasSize.width) / 2
    let dy = Double(target.y) - Double(canvasSize.height) / 2
    let inHost = CGPoint(x: Double(host.width) / 2 + dx * cos(radians) - dy * sin(radians),
                         y: Double(host.height) / 2 + dx * sin(radians) + dy * cos(radians))
    return editor.canvasHost.convert(inHost, to: nil)
}

/// `rotatedWindowPoint`'s exact inverse: a window point into the host, then
/// R(−θ) about the host's centre back into canvas space.
private func rotatedCanvasPoint(editor: EditorViewController, canvasSize: CGSize,
                                windowPoint: CGPoint, degrees: Double) -> CGPoint {
    let host = editor.canvasHost.frame.size
    let radians = degrees * Double.pi / 180
    let inHost = editor.canvasHost.convert(windowPoint, from: nil)
    let hx = Double(inHost.x) - Double(host.width) / 2
    let hy = Double(inHost.y) - Double(host.height) / 2
    return CGPoint(x: Double(canvasSize.width) / 2 + hx * cos(radians) + hy * sin(radians),
                   y: Double(canvasSize.height) / 2 - hx * sin(radians) + hy * cos(radians))
}

/// rotate-view-stroke-ui.
///
/// A 400 x 300 byte document, the view turned 30°. Rotating changes NO pixel
/// (the same flattened bytes, nothing to undo, not edited) and HIDES the ruler
/// strips. Then the case computes, on its own, where document point (120, 90)
/// must be on screen — H = hostCentre + R(30°)·(P − canvasCentre) with
/// R = [[cos, −sin], [sin, cos]] in the flipped host space, which is a
/// clockwise turn on screen — converts that to a window point, requires the
/// canvas to map it back to P, and then presses the brush THERE. The dab's
/// alpha-weighted centroid must be P.
func rotateViewStrokeUI(output: URL) throws -> [String: Any] {
    let previousChrome = ToolOptionsStore.shared.view
    defer { ToolOptionsStore.shared.view = previousChrome }
    ToolOptionsStore.shared.view.rulers = true
    let width = 400, height = 300
    let (document, editor) = try openTransparentByteDocument(width: width, height: height)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    editor.updateRulerVisibility()
    editor.view.layoutSubtreeIfNeeded()
    pump()
    try require(!editor.hRuler.isHidden, "The rulers are off, so hiding them proves nothing")

    configure(editor, size: 24, hardness: 1.0, opacity: 100, flow: 100)
    _ = try editor.applyViewState(zoom: 1, center: nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned document empty")
    }
    let beforeHash = hashBytes(try flatBytes(before))
    document.undoManager?.removeAllActions()

    editor.setViewRotation(30)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    try require(abs(editor.viewRotation - 30) < 1e-9,
        "The view rotation is \(editor.viewRotation), expected 30")
    try require(editor.hRuler.isHidden && editor.vRuler.isHidden,
        "The ruler strips stayed visible under a rotated view")
    guard let rotated = document.doc else {
        throw DoctoredCoreError(message: "Owned document empty")
    }
    try require(ObjectIdentifier(rotated) == ObjectIdentifier(before),
        "Rotating the view replaced the document handle")
    try require(hashBytes(try flatBytes(rotated)) == beforeHash,
        "Rotating the view changed the composite")
    try require(document.undoManager?.canUndo != true,
        "Rotating the view registered an undo step")
    try require(!document.isDocumentEdited, "Rotating the view marked the document edited")
    try screenshot(editor.view, output.appendingPathComponent("rotate-view-stroke-ui.png"))

    // The oracle: this case's own rotation matrix, in the flipped host space.
    let canvasSize = CGSize(width: Double(width), height: Double(height))
    let host = editor.canvasHost.frame.size
    let target = CGPoint(x: 120, y: 90)
    let inWindow = rotatedWindowPoint(editor: editor, canvasSize: canvasSize, target: target,
                                      degrees: 30)
    let mapped = editor.canvas.convert(inWindow, from: nil)
    try require(hypot(mapped.x - target.x, mapped.y - target.y) <= 0.01,
        "A window point the case computed maps to \(mapped), expected \(target)")

    try sendWindow(editor.canvas, .leftMouseDown, windowPoint: inWindow)
    try sendWindow(editor.canvas, .leftMouseUp, windowPoint: inWindow)
    pump()
    guard let painted = document.doc else {
        throw DoctoredCoreError(message: "Owned document empty")
    }
    let centroid = try alphaCentroid(try flatBytes(painted), width: width, height: height)
    try require(hypot(centroid.x - target.x, centroid.y - target.y) <= 0.5,
        "The rotated-view dab landed at \(centroid), expected \(target)")

    return ["name": "rotate-view-stroke-ui", "passed": true, "rotation": editor.viewRotation,
            "host_size": NSStringFromSize(host), "window_point": NSStringFromPoint(inWindow),
            "mapped_back": NSStringFromPoint(mapped), "target": NSStringFromPoint(target),
            "centroid": NSStringFromPoint(centroid), "composite_sha256": beforeHash,
            "rulers_hidden": editor.hRuler.isHidden]
}

/// rotate-view-hover-ui.
///
/// The brush ring's hover point under a rotated view. H0: a press at the
/// oracle's window point reaches the canvas, and one over an exposed host
/// corner reaches the host. H1: a mouse-moved there latches P and the Info
/// readout reads P's pixel. H2: after a 60° turn the latched point is what
/// the oracle derives. H3: a turn that does NOT scroll (30° → −30° at 50 %,
/// the host smaller than the well) must re-derive the latched point from
/// the pointer's real location instead of leaving the ring where it was.
/// H4: none of it touches the document.
func rotateViewHoverUI(output: URL) throws -> [String: Any] {
    let width = 400, height = 300
    let (document, editor) = try openTransparentByteDocument(width: width, height: height)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    let canvas = editor.canvas
    configure(editor, size: 24)
    _ = try editor.applyViewState(zoom: 1, center: nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    editor.showInfo(nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    try require(editor.layersPanelVisible && editor.panelTab == 3,
        "The Info tab is not showing: visible \(editor.layersPanelVisible), tab \(editor.panelTab)")
    guard let before = document.doc, let window = canvas.window,
          let content = window.contentView else {
        throw DoctoredCoreError(message: "Owned document or window missing")
    }
    let beforeHash = hashBytes(try flatBytes(before))
    document.undoManager?.removeAllActions()
    let canvasSize = CGSize(width: Double(width), height: Double(height))
    let target = CGPoint(x: 120.5, y: 90.5)
    func near(_ a: CGPoint?, _ b: CGPoint?) -> Bool {
        guard let a, let b else { return a == nil && b == nil }
        return hypot(a.x - b.x, a.y - b.y) <= 0.01
    }
    func describe(_ point: CGPoint?) -> String { point.map { NSStringFromPoint($0) } ?? "nil" }
    /// What `refreshHoverPoint` derives from a window location.
    func derived(_ windowLocation: CGPoint) -> CGPoint? {
        let local = canvas.convert(windowLocation, from: nil)
        guard canvas.visibleRect.contains(local) else { return nil }
        return CGPoint(x: min(max(local.x, 0), canvas.bounds.width),
                       y: min(max(local.y, 0), canvas.bounds.height))
    }
    func hitView(_ windowPoint: CGPoint) -> NSView? {
        content.hitTest(content.superview?.convert(windowPoint, from: nil) ?? windowPoint)
    }

    // H0: hit-testing under a 30° view.
    editor.setViewRotation(30)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    let w = rotatedWindowPoint(editor: editor, canvasSize: canvasSize, target: target, degrees: 30)
    try require(hitView(w) === canvas,
        "At 30° the window point \(NSStringFromPoint(w)) hit \(String(describing: hitView(w)))")
    // The first on-screen host point, walking in from the visible corners,
    // that the turned canvas does not cover.
    let shown = editor.canvasHost.visibleRect.insetBy(dx: 2, dy: 2)
    var cornerInHost: CGPoint?
    for step in 0..<25 where cornerInHost == nil {
        let t = CGFloat(step) * 0.02
        let dx = t * shown.width, dy = t * shown.height
        let candidates = [CGPoint(x: shown.minX + dx, y: shown.minY + dy),
                          CGPoint(x: shown.maxX - dx, y: shown.minY + dy),
                          CGPoint(x: shown.minX + dx, y: shown.maxY - dy),
                          CGPoint(x: shown.maxX - dx, y: shown.maxY - dy)]
        let covered = canvas.bounds.insetBy(dx: -2, dy: -2)
        cornerInHost = candidates.first {
            !covered.contains(canvas.convert($0, from: editor.canvasHost))
        }
    }
    guard let cornerInHost else {
        throw DoctoredCoreError(message: "No exposed host corner is on screen in "
            + NSStringFromRect(shown))
    }
    let corner = editor.canvasHost.convert(cornerInHost, to: nil)
    try require(hitView(corner) === editor.canvasHost,
        "An exposed host corner hit \(String(describing: hitView(corner)))")

    // H1: a mouse-moved at W latches P and reads P's pixel.
    try sendWindow(canvas, .mouseMoved, windowPoint: w)
    try require(near(canvas.hoverPoint, target),
        "At 30° the hover point is \(describe(canvas.hoverPoint)), expected \(target)")
    try require(editor.lastCursorPixel?.x == 120 && editor.lastCursorPixel?.y == 90,
        "The Info readout read \(String(describing: editor.lastCursorPixel)), expected (120, 90)")

    // H2: a 60° turn, then the same window location re-derived.
    editor.setViewRotation(60)
    editor.view.layoutSubtreeIfNeeded()
    canvas.refreshHoverPoint(windowLocation: w)
    let at60 = rotatedCanvasPoint(editor: editor, canvasSize: canvasSize, windowPoint: w,
                                  degrees: 60)
    let local60 = canvas.convert(w, from: nil)
    let expected60: CGPoint? = canvas.visibleRect.contains(local60)
        ? CGPoint(x: min(max(at60.x, 0), canvas.bounds.width),
                  y: min(max(at60.y, 0), canvas.bounds.height)) : nil
    try require(expected60 != nil, "At 60° the window point \(NSStringFromPoint(w)) is off the canvas")
    try require(near(canvas.hoverPoint, expected60),
        "At 60° the hover point is \(describe(canvas.hoverPoint)), expected \(describe(expected60))")

    // H3: a turn that does not scroll leaves no stale ring.
    _ = try editor.applyViewState(zoom: 0.5, center: nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    for _ in 0..<2 {
        editor.setViewRotation(30)
        editor.view.layoutSubtreeIfNeeded()
        pump()
    }
    let clip = editor.scrollView.contentView
    let unsettled = clip.bounds.origin
    // A turn keeps the canvas point under the well's centre, and after the
    // turns above that point is not the canvas's centre (measured: the origin
    // {-169.81, -88.32} against a centred {-169.79, -88.10} at 30° and 50 %),
    // so the next turn would scroll. Centre the host in the well first: the
    // canvas centre is the one point every turn leaves where it is. A scroll
    // from an unaligned origin can land a few ulps off, and one from a whole
    // point lands exactly (both measured), so it starts from a whole point.
    let centred = CGPoint(x: editor.canvasHost.frame.width / 2 - clip.bounds.width / 2,
                          y: editor.canvasHost.frame.height / 2 - clip.bounds.height / 2)
    clip.setBoundsOrigin(CGPoint(x: centred.x.rounded(.down), y: centred.y.rounded(.down)))
    clip.scroll(to: centred)
    editor.scrollView.reflectScrolledClipView(clip)
    pump()
    let hostSize = editor.canvasHost.frame.size
    try require(hostSize.width < clip.bounds.width && hostSize.height < clip.bounds.height,
        "The host \(NSStringFromSize(hostSize)) is not smaller than the well "
            + NSStringFromSize(clip.bounds.size))
    let origin = clip.bounds.origin
    let w30 = rotatedWindowPoint(editor: editor, canvasSize: canvasSize, target: target, degrees: 30)
    try sendWindow(canvas, .mouseMoved, windowPoint: w30)
    try require(near(canvas.hoverPoint, target),
        "At 30° and 50 % the hover point is \(describe(canvas.hoverPoint)), expected \(target)")
    editor.setViewRotation(-30)
    try require(clip.bounds.origin == origin,
        "The −30° turn scrolled the well \(NSStringFromPoint(origin)) → "
            + NSStringFromPoint(clip.bounds.origin) + ", so it proves nothing")
    let pointer = window.mouseLocationOutsideOfEventStream
    let expectedStale = derived(pointer)
    try require(!near(canvas.hoverPoint, target),
        "A −30° turn left the ring at \(describe(canvas.hoverPoint)), where the 30° view put it")
    try require(near(canvas.hoverPoint, expectedStale),
        "After the −30° turn the hover point is \(describe(canvas.hoverPoint)), the pointer at "
            + "\(NSStringFromPoint(pointer)) derives \(describe(expectedStale))")
    let afterTurn = canvas.hoverPoint

    // H4: nothing written.
    editor.setViewRotation(45)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    let w45 = rotatedWindowPoint(editor: editor, canvasSize: canvasSize, target: target, degrees: 45)
    try sendWindow(canvas, .mouseMoved, windowPoint: w45)
    try require(near(canvas.hoverPoint, target),
        "At 45° the hover point is \(describe(canvas.hoverPoint)), expected \(target)")
    try screenshot(editor.view, output.appendingPathComponent("rotate-view-hover-ui.png"))
    guard let after = document.doc else { throw DoctoredCoreError(message: "Owned document empty") }
    try require(ObjectIdentifier(after) == ObjectIdentifier(before),
        "Hovering under a turned view replaced the document handle")
    try require(hashBytes(try flatBytes(after)) == beforeHash,
        "Hovering under a turned view changed the composite")
    try require(document.undoManager?.canUndo != true,
        "Hovering under a turned view registered an undo step")
    try require(!document.isDocumentEdited, "Hovering under a turned view marked the document edited")

    return ["name": "rotate-view-hover-ui", "passed": true, "target": NSStringFromPoint(target),
            "window_point_30": NSStringFromPoint(w), "host_corner": NSStringFromPoint(corner),
            "expected_60": describe(expected60), "clip_origin": NSStringFromPoint(origin),
            "clip_origin_unsettled": NSStringFromPoint(unsettled),
            "clip_origin_centred": NSStringFromPoint(centred),
            "pointer_after_turn": NSStringFromPoint(pointer),
            "hover_after_turn": describe(afterTurn),
            "composite_sha256": beforeHash]
}

/// rotate-view-gesture-ui.
///
/// The Hand tool's ⌥-drag. The angle the editor ends up at must equal the
/// atan2 delta this case computes about the well's own centre — clockwise
/// positive — and the gesture must change no pixel, register no undo step and
/// leave the document clean. Reset then puts the rulers back where the
/// PREFERENCE says, which is what "the preference is untouched" means.
func rotateViewGestureUI() throws -> [String: Any] {
    let previousChrome = ToolOptionsStore.shared.view
    defer { ToolOptionsStore.shared.view = previousChrome }
    ToolOptionsStore.shared.view.rulers = true
    let (document, editor, _) = try openByteDocument(width: 400, height: 300)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    editor.updateRulerVisibility()
    editor.view.layoutSubtreeIfNeeded()
    pump()
    editor.selectTool(.hand)
    pump()
    try require(editor.currentTool == .hand, "The Hand tool did not become current")

    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned document empty")
    }
    let beforeHash = hashBytes(try flatBytes(before))
    document.undoManager?.removeAllActions()

    let clip = editor.scrollView.contentView
    let center = clip.convert(CGPoint(x: clip.bounds.midX, y: clip.bounds.midY), to: nil)
    let start = CGPoint(x: center.x + 120, y: center.y)
    let end = CGPoint(x: center.x + 100, y: center.y - 60)
    // Clockwise-positive screen angle: a window counts y UPWARD, so the y
    // difference is negated.
    func screenAngle(_ point: CGPoint) -> Double {
        atan2(Double(center.y - point.y), Double(point.x - center.x)) * 180 / Double.pi
    }
    let expected = screenAngle(end) - screenAngle(start)
    try require(expected > 1, "The case's own drag is not a real turn: \(expected)°")

    try sendWindow(editor.canvas, .leftMouseDown, windowPoint: start, modifiers: [.option])
    try require(editor.canvas.rotateViewSession != nil,
        "An ⌥-press with the Hand tool did not open a rotate-view session")
    try sendWindow(editor.canvas, .leftMouseDragged, windowPoint: end, modifiers: [.option])
    try sendWindow(editor.canvas, .leftMouseUp, windowPoint: end, modifiers: [.option])
    pump()
    try require(editor.canvas.rotateViewSession == nil,
        "The rotate-view session survived mouse-up")
    try require(abs(editor.viewRotation - expected) <= 0.01,
        "The gesture turned the view \(editor.viewRotation)°, the case computed \(expected)°")
    try require(editor.hRuler.isHidden, "The rulers stayed visible under the turned view")

    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document empty")
    }
    try require(ObjectIdentifier(after) == ObjectIdentifier(before),
        "The rotate gesture replaced the document handle")
    try require(hashBytes(try flatBytes(after)) == beforeHash,
        "The rotate gesture changed the composite")
    try require(document.undoManager?.canUndo != true,
        "The rotate gesture registered an undo step")
    try require(!document.isDocumentEdited, "The rotate gesture marked the document edited")

    editor.resetViewRotation(nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    try require(editor.viewRotation == 0,
        "Reset View Rotation left \(editor.viewRotation)°")
    try require(!editor.hRuler.isHidden && !editor.vRuler.isHidden,
        "Reset View Rotation did not bring the rulers back to the preference")

    return ["name": "rotate-view-gesture-ui", "passed": true, "expected_degrees": expected,
            "actual_degrees": editor.viewRotation, "composite_sha256": beforeHash,
            "center_in_window": NSStringFromPoint(center),
            "start": NSStringFromPoint(start), "end": NSStringFromPoint(end)]
}

/// second-view-ui.
///
/// Two real windows on one real NSDocument: the titles, live redraw through
/// one shared preview, mirrored layer/paint targets, independent zoom, one
/// shared undo stack, a close that neither closes nor dirties the document,
/// a closed view that does NOT come back to life on the next edit, and Sync
/// Pan & Zoom.
func secondViewUI(output: URL) throws -> [String: Any] {
    let (document, editor, _) = try openByteDocument(width: 320, height: 240)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.syncPanZoom = false
        document.close(); pump()
    }
    _ = try call("new_layer", [:], document: document)
    pump()
    try require(document.doc?.layerCount == 2,
        "The fixture has \(document.doc?.layerCount ?? 0) layers, expected 2")
    _ = try call("add_layer_mask", ["index": 1], document: document)
    pump()
    try require(document.doc?.layerHasMask(1) == true, "Layer 1 has no mask")

    let name = document.displayName ?? "Untitled"
    guard let secondEditor = document.openSecondView() else {
        throw DoctoredCoreError(message: "New View opened no second editor")
    }
    _ = secondEditor.view
    secondEditor.view.layoutSubtreeIfNeeded()
    pump()
    try require(document.windowControllers.count == 2,
        "The document has \(document.windowControllers.count) window controllers, expected 2")
    let titles = document.windowControllers.compactMap { $0.window?.title }
    try require(titles == ["\(name):1", "\(name):2"],
        "The window titles are \(titles), expected [\(name):1, \(name):2]")
    try screenshot(secondEditor.view, output.appendingPathComponent("second-view-ui.png"))

    // An edit made through window 1 is the SAME picture in window 2.
    document.setLayerSelection(.single(0))
    editor.activeLayerDidChange()
    configure(editor, size: 18, hardness: 1.0, opacity: 100, flow: 100)
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 60.5, y: 40.5))
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 60.5, y: 40.5))
    pump()
    try require(editor.canvas.image != nil, "Window 1 lost its picture")
    try require(editor.canvas.image === secondEditor.canvas.image,
        "The two views show different CGImages after an edit in window 1")
    try require(editor.canvas.image === document.previewImage,
        "The canvas is not showing the document's preview")

    // Targets are DOCUMENT state and are mirrored into the other view.
    editor.setSelectedLayers(.single(1))
    pump()
    editor.setPaintTarget(.mask)
    pump()
    try require(secondEditor.paintTarget == .mask,
        "View 2's paint target is \(secondEditor.paintTarget), expected mask")
    let selectedRow = secondEditor.layersPanel.tableView.selectedRow
    try require(selectedRow >= 0 && selectedRow < secondEditor.layersPanel.rows.count,
        "View 2's layers panel selected row \(selectedRow)")
    try require(secondEditor.layersPanel.rows[selectedRow].index == 1,
        "View 2's layers panel selected layer "
            + "\(secondEditor.layersPanel.rows[selectedRow].index), expected 1")

    // Zoom is per VIEW.
    _ = try editor.applyViewState(zoom: 2, center: nil)
    _ = try secondEditor.applyViewState(zoom: 0.5, center: nil)
    pump()
    try require(abs(editor.viewState.zoom - 2) < 1e-6 &&
                abs(secondEditor.viewState.zoom - 0.5) < 1e-6,
        "The two views report \(editor.viewState.zoom) and \(secondEditor.viewState.zoom)")

    // One shared undo stack, driven from window 2.
    let cursorBefore = document.history.cursor
    secondEditor.undo(nil)
    pump()
    try require(document.history.cursor == cursorBefore - 1,
        "Undo through view 2 moved the cursor \(cursorBefore) -> \(document.history.cursor)")
    try require(editor.canvas.image === document.previewImage
                && secondEditor.canvas.image === document.previewImage,
        "A view did not redraw the undone picture")

    // Closing view 2 neither closes nor dirties the document, and the editor
    // it leaves behind stays detached.
    let editedBefore = document.isDocumentEdited
    guard let secondWindow = secondEditor.view.window else {
        throw DoctoredCoreError(message: "View 2 has no window")
    }
    secondWindow.performClose(nil)
    try waitUntil({ document.windowControllers.count == 1 },
        "View 2's window did not close: \(document.windowControllers.count) controllers")
    pump()
    try require(!document.hasClosed, "Closing view 2 closed the document")
    try require(document.isDocumentEdited == editedBefore,
        "Closing view 2 changed the dirty flag \(editedBefore) -> \(document.isDocumentEdited)")
    try require(document.windowControllers.first?.window?.title == name,
        "The surviving window is still called "
            + "\(document.windowControllers.first?.window?.title ?? "—")")

    // The strong reference is the point: a closed view AppKit still retains
    // must not answer the next edit.
    try require(secondEditor.canvas.image == nil, "The closed view kept its picture")
    document.setLayerSelection(.single(0))
    editor.activeLayerDidChange()
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 100.5, y: 70.5))
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 100.5, y: 70.5))
    pump()
    try require(document.previewImage != nil, "The document lost its preview")
    try require(secondEditor.canvas.image == nil,
        "The closed view re-acquired the preview on the next edit")

    // Sync Pan & Zoom, on a freshly reopened second view.
    document.syncPanZoom = true
    guard let thirdEditor = document.openSecondView() else {
        throw DoctoredCoreError(message: "New View opened no third editor")
    }
    _ = thirdEditor.view
    thirdEditor.view.layoutSubtreeIfNeeded()
    pump()
    _ = try editor.applyViewState(zoom: 3, center: nil)
    pump()
    try require(abs(thirdEditor.viewState.zoom - 3) < 1e-6,
        "With Sync Pan & Zoom on, view 2 reports \(thirdEditor.viewState.zoom), expected 3")
    let ordinals = document.windowControllers
        .compactMap { ($0 as? EditorWindowController)?.viewOrdinal }
    try require(ordinals == [1, 3], "The view ordinals are \(ordinals), expected [1, 3]")

    return ["name": "second-view-ui", "passed": true, "titles": titles,
            "selected_row_layer": secondEditor.layersPanel.rows[selectedRow].index,
            "history_cursor_before": cursorBefore, "history_cursor_after": document.history.cursor,
            "edited_before_close": editedBefore, "edited_after_close": document.isDocumentEdited,
            "ordinals_after_reopen": ordinals,
            "synced_zoom": thirdEditor.viewState.zoom]
}

/// rotate-view-sheet-ui.
///
/// The Rotate View sheet's ANGLE FIELD, which an MCP drive cannot reach at
/// all. A number TYPED into the field and then Done clicked — no Return, no
/// slider touched — must be the angle the sheet previews and commits. That is
/// exactly the case a sheet reading its SLIDER back at Done gets wrong, since
/// a typed number sits in the field EDITOR until the editing session ends.
func rotateViewSheetUI() throws -> [String: Any] {
    let typed = 45.0, typedText = "45"
    var previews: [Double] = []
    var problem: String?
    var typedIntoField = false, clickedDone = false
    var fieldTextBeforeDone = ""
    // The sheet runs a modal session, so the drive is a timer in the modal
    // run loop modes — `quickmask.swift`'s precedent, `isVisible` included:
    // a sheet an earlier case closed stays in `NSApp.windows`.
    let drive = Timer(timeInterval: 0.05, repeats: true) { timer in
        guard let window = NSApp.windows.first(where: {
                  $0.title == "Rotate View" && $0.isVisible
              }), let card = window.contentView else { return }
        let identified = descendants(card).filter { $0.accessibilityIdentifier() != "" }
        guard let field = identified.compactMap({ $0 as? NSTextField })
                  .first(where: { $0.accessibilityIdentifier() == "rotate-view-angle" }),
              let done = identified.compactMap({ $0 as? NSButton })
                  .first(where: { $0.accessibilityIdentifier() == "rotate-view-done" }) else {
            problem = "The Rotate View sheet has no angle field or Done button"
            timer.invalidate(); NSApp.stopModal(); return
        }
        guard typedIntoField else {
            window.makeFirstResponder(field)
            guard let editor = field.currentEditor() as? NSTextView else {
                problem = "Making the angle field first responder opened no field editor"
                timer.invalidate(); NSApp.stopModal(); return
            }
            // TYPE, exactly as the user leaves it: the characters go into the
            // field editor and no Return follows them.
            editor.insertText(typedText, replacementRange:
                NSRange(location: 0, length: (editor.string as NSString).length))
            fieldTextBeforeDone = editor.string
            typedIntoField = true
            return
        }
        clickedDone = true
        timer.invalidate()
        done.performClick(nil)
    }
    // A sheet that never appears must not hang the whole probe run.
    let safety = Timer(timeInterval: 20, repeats: false) { _ in
        drive.invalidate()
        problem = problem ?? "The Rotate View sheet never came up"
        NSApp.stopModal()
    }
    for timer in [drive, safety] {
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
    }
    let committed = RotateViewSheetController.run(current: 0) { previews.append($0) }
    drive.invalidate(); safety.invalidate(); pump()
    if let problem { throw DoctoredCoreError(message: problem) }
    try require(typedIntoField && clickedDone,
        "The case did not reach the sheet: typed=\(typedIntoField) done=\(clickedDone)")
    try require(fieldTextBeforeDone == typedText,
        "The field editor held \(fieldTextBeforeDone) before Done, expected \(typedText)")
    try require(committed != nil, "Done returned no angle at all")
    try require(abs((committed ?? .nan) - typed) < 1e-9,
        "Done committed \(committed ?? .nan)°, the field said \(typed)°")
    try require(previews.last == typed,
        "The sheet's live preview last applied \(previews.last ?? .nan)°, expected \(typed)°")

    return ["name": "rotate-view-sheet-ui", "passed": true, "typed": typedText,
            "field_text_before_done": fieldTextBeforeDone, "committed": committed ?? .nan,
            "previewed": previews, "return_pressed": false, "slider_touched": false]
}

/// view-items-at-depth-ui.
///
/// The view-only View/Window items and the depth-independent window items
/// (Layer Comps, the Workspaces items, Navigator, Assistant) must VALIDATE and
/// WORK in a 16-bit and in a 32-bit float document, resolved through the real
/// responder chain, while an item the depth refuses stays disabled.
func viewItemsAtDepthUI() throws -> [String: Any] {
    let u16 = try viewItemsAtDepth(label: "rgba16", negative: "showToneMap:") {
        try openDocument()
    }
    let float = try viewItemsAtDepth(label: "float", negative: "resizeImage:") {
        try openFloatDocument()
    }
    return ["case": "view-items-at-depth-ui", "passed": true, "rgba16": u16, "float": float]
}

/// A nil-target menu item resolved through AppKit's responder chain; the
/// resolved target must be this editor, and its validation must answer.
private func viewItemValidates(_ editor: EditorViewController, _ name: String,
                               tag: Int = 0, label: String) throws -> Bool {
    let item = NSMenuItem(title: "Item", action: NSSelectorFromString(name), keyEquivalent: "")
    item.target = nil
    item.tag = tag
    guard let resolved = NSApp.target(forAction: NSSelectorFromString(name), to: nil, from: item)
            as? EditorViewController else {
        throw DoctoredCoreError(message:
            "\(label): \(name) does not resolve to an editor through the responder chain")
    }
    try require(resolved === editor, "\(label): \(name) resolves to another editor")
    return resolved.validateUserInterfaceItem(item)
}

private func viewItemsAtDepth(
    label: String, negative: String,
    open: () throws -> (ImageDocument, EditorViewController, RasterDocument)
) throws -> [String: Any] {
    let (document, editor, _) = try open()
    let workspaceName = "View Items At Depth"
    defer {
        try? WorkspaceLibrary.delete(name: workspaceName)
        document.syncPanZoom = false
        document.layerCompsWindowController?.close()
        document.layerCompsWindowController = nil
        document.close(); pump()
    }
    try require(document.doc?.isHighDepth == true, "\(label): the document is 8-bit")

    let mustValidate = ["rotateView:", "newView:", "toggleSyncPanZoom:", "setGridSpacing:",
        "setGridSubdivisions:", "snapToAll:", "snapToNone:", "showLayerComps:",
        "saveWorkspace:", "restoreWorkspace:", "deleteWorkspace:", "showNavigator:",
        "showAssistant:", "togglePanels:"]
    for name in mustValidate {
        try require(try viewItemValidates(editor, name, label: label),
            "\(label): \(name) does not validate")
    }
    editor.setViewRotation(30)
    pump()
    try require(try viewItemValidates(editor, "resetViewRotation:", label: label),
        "\(label): resetViewRotation: does not validate on a turned view")
    editor.setViewRotation(0)
    pump()
    try require(!(try viewItemValidates(editor, "resetViewRotation:", label: label)),
        "\(label): resetViewRotation: validates on an unturned view")
    try require(!(try viewItemValidates(editor, negative, label: label)),
        "\(label): the negative control \(negative) validates")

    // New View opens a second window on the same document.
    editor.newView(nil)
    pump()
    try require(document.windowControllers.count == 2,
        "\(label): New View left \(document.windowControllers.count) window controllers")
    guard let secondWindow = document.windowControllers.last?.window else {
        throw DoctoredCoreError(message: "\(label): the second view has no window")
    }
    secondWindow.performClose(nil)
    try waitUntil({ document.windowControllers.count == 1 },
        "\(label): the second view did not close")
    pump()

    // Sync Pan & Zoom flips the document's switch.
    let syncBefore = document.syncPanZoom
    editor.toggleSyncPanZoom(nil)
    try require(document.syncPanZoom != syncBefore, "\(label): Sync Pan & Zoom did not flip")
    editor.toggleSyncPanZoom(nil)
    try require(document.syncPanZoom == syncBefore, "\(label): Sync Pan & Zoom did not flip back")

    editor.showNavigator(nil)
    pump()
    try require(editor.panelTab == 4, "\(label): Navigator left panel tab \(editor.panelTab)")
    editor.showAssistant(nil)
    pump()
    try require(editor.panelTab == 2, "\(label): Assistant left panel tab \(editor.panelTab)")

    editor.showLayerComps(nil)
    pump()
    try require(document.layerCompsWindowController != nil,
        "\(label): Layer Comps opened no window controller")
    document.layerCompsWindowController?.close()
    pump()

    // The workspace actions' bodies, bypassing their modal error alerts.
    _ = try WorkspaceLibrary.save(editor.captureWorkspace(name: workspaceName), replace: true)
    editor.applyWorkspace(try WorkspaceLibrary.named(workspaceName))
    pump()
    try WorkspaceLibrary.delete(name: workspaceName)
    try require(!(try WorkspaceLibrary.entries()).contains { $0.name == workspaceName },
        "\(label): the deleted workspace is still listed")

    return ["validated": mustValidate + ["resetViewRotation:"], "negative": negative,
            "new_view": true, "sync_pan_zoom": true, "navigator_tab": 4, "assistant_tab": 2,
            "layer_comps": true, "workspace_round_trip": true]
}
