import AppKit

// The committed selection's marching ants. `cacheDisplay`
// never captures a layer's own content, so no screenshot can see the ants;
// this case reads the real `CAShapeLayer` and its animation instead, through
// the real canvas and the editor's own selection path.

/// The ants' view, which a committed selection must have created.
private func antsView(_ canvas: ImageCanvasView) throws -> MarchingAntsView {
    guard let view = canvas.marchingAntsView else {
        throw DoctoredCoreError(message: "A committed selection made no marching-ants view")
    }
    return view
}

private func antsLayer(_ canvas: ImageCanvasView) throws -> CAShapeLayer {
    guard let layer = try antsView(canvas).shapeLayer else {
        throw DoctoredCoreError(message: "The marching-ants view is not backed by a CAShapeLayer")
    }
    return layer
}

/// The precondition the animation assertions stand on: the app is active and
/// this window is key and on screen. An unmet one is an ACTIVATION problem
/// (rerun), never a failed animation, so it says so in its message.
private func activate(_ window: NSWindow) throws {
    for _ in 0..<3 {
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline,
              !(NSApp.isActive && window.isKeyWindow
                && window.occlusionState.contains(.visible)) {
            pump()
        }
        if NSApp.isActive && window.isKeyWindow && window.occlusionState.contains(.visible) {
            return
        }
    }
    throw DoctoredCoreError(message: "not run (activation): active \(NSApp.isActive), key "
        + "\(window.isKeyWindow), visible \(window.occlusionState.contains(.visible))")
}

/// A canvas-sized mask of isolated single pixels, one every `step` pixels
/// in both directions.
private func gridMask(width: Int, height: Int, step: Int) -> [UInt8] {
    var mask = [UInt8](repeating: 0, count: width * height)
    for y in stride(from: 0, to: height, by: step) {
        for x in stride(from: 0, to: width, by: step) {
            mask[y * width + x] = 255
        }
    }
    return mask
}

/// selection-marching-ants-ui.
func selectionMarchingAntsUI(output: URL) throws -> [String: Any] {
    let previousReduceMotion = MarchingAnts.reduceMotion
    let previousLowPower = MarchingAnts.lowPower
    let defaults = UserDefaults.standard
    let previousEnabled = defaults.object(forKey: MarchingAnts.enabledDefaultsKey)
    defer {
        MarchingAnts.reduceMotion = previousReduceMotion
        MarchingAnts.lowPower = previousLowPower
        if let previousEnabled {
            defaults.set(previousEnabled, forKey: MarchingAnts.enabledDefaultsKey)
        } else {
            defaults.removeObject(forKey: MarchingAnts.enabledDefaultsKey)
        }
    }
    defaults.removeObject(forKey: MarchingAnts.enabledDefaultsKey)
    let (document, editor, source) = try openByteDocument(width: 96, height: 64)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    let canvas = editor.canvas
    editor.view.layoutSubtreeIfNeeded()
    pump()
    guard let window = canvas.window else {
        throw DoctoredCoreError(message: "Owned editor window missing")
    }
    try require(canvas.marchingAntsView?.isHidden ?? true,
        "With no selection the marching ants are showing")

    // Geometry, on an asymmetric rectangle.
    let rect = CGRect(x: 10, y: 6, width: 30, height: 14)
    guard let selection = CanvasSelection(shape: .rect(rect), canvasWidth: source.width,
                                          canvasHeight: source.height) else {
        throw DoctoredCoreError(message: "Owned rectangle selection unavailable")
    }
    editor.agentSetSelection(selection)
    pump()
    let view = try antsView(canvas)
    let layer = try antsLayer(canvas)
    try require(!view.isHidden, "A committed selection's ants are hidden")
    try require(view.superview === canvas && canvas.subviews.first === view,
        "The ants view is not the canvas's bottom subview: \(canvas.subviews)")
    try require(view.frame == canvas.bounds,
        "The ants view frame is \(NSStringFromRect(view.frame)), the canvas bounds "
            + NSStringFromRect(canvas.bounds))
    try require(layer.path?.boundingBox == rect,
        "The ants path bounds are \(String(describing: layer.path?.boundingBox)), expected \(rect)")
    let mag = canvas.magnification
    func strokeMatches(_ scale: CGFloat) -> Bool {
        let dash = (layer.lineDashPattern ?? []).map { CGFloat($0.doubleValue) }
        return abs(layer.lineWidth * scale - 2) <= 1e-9 && dash.count == 2
            && abs(dash[0] * scale - 5) <= 1e-9 && abs(dash[1] * scale - 4) <= 1e-9
    }
    try require(strokeMatches(mag),
        "At zoom \(mag) the ants are \(layer.lineWidth) wide dashed "
            + "\(String(describing: layer.lineDashPattern))")
    let probePoint = CGPoint(x: 10, y: 6)
    let inCanvasLayer = layer.convert(probePoint, to: canvas.layer)
    try require(inCanvasLayer == probePoint,
        "The ants layer maps \(probePoint) to \(inCanvasLayer) in the canvas layer")
    try require(layer.contentsAreFlipped() == canvas.layer?.contentsAreFlipped(),
        "The ants layer's flip \(layer.contentsAreFlipped()) differs from the canvas layer's")

    editor.setViewRotation(90)
    _ = try editor.applyViewState(zoom: 2, center: nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    let viewPoint = view.convert(probePoint, to: nil)
    let canvasPoint = canvas.convert(probePoint, to: nil)
    try require(hypot(viewPoint.x - canvasPoint.x, viewPoint.y - canvasPoint.y) <= 1e-9,
        "At 90° and 200 % the ants put \(probePoint) at \(viewPoint), the canvas at \(canvasPoint)")
    try require(strokeMatches(2),
        "At 200 % the ants are \(layer.lineWidth) wide dashed "
            + "\(String(describing: layer.lineDashPattern))")
    editor.setViewRotation(0)
    _ = try editor.applyViewState(zoom: 1, center: nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()

    // The march, and every condition that stops it.
    MarchingAnts.reduceMotion = { false }
    MarchingAnts.lowPower = { false }
    try activate(window)
    view.restartAnimation()
    try require(view.isAnimating, "Active, key and visible, the ants do not march")
    guard let march = layer.animation(forKey: MarchingAnts.animationKey)
            as? CAKeyframeAnimation else {
        throw DoctoredCoreError(message: "The march is not a keyframe animation")
    }
    try require(march.keyPath == "lineDashPhase" && march.values?.count == 9
                && march.calculationMode == .discrete && march.duration == 0.6
                && march.repeatCount == .infinity,
        "The march is \(String(describing: march.keyPath)) \(march.values?.count ?? 0) values "
            + "\(march.calculationMode.rawValue) \(march.duration)s x\(march.repeatCount)")

    let other = NSWindow(contentRect: CGRect(x: 40, y: 40, width: 200, height: 120),
                         styleMask: [.titled], backing: .buffered, defer: false)
    other.isReleasedWhenClosed = false
    defer { other.close() }
    other.makeKeyAndOrderFront(nil)
    try waitUntil({ other.isKeyWindow && !window.isKeyWindow },
        "not run (activation): the throwaway window did not become key")
    pump()
    try require(!view.isAnimating, "The ants still march in a window that is not key")
    try activate(window)
    pump()
    try require(view.isAnimating, "Re-keying the editor did not restart the march")
    other.orderOut(nil)
    pump()

    MarchingAnts.reduceMotion = { true }
    view.restartAnimation()
    try require(!view.isAnimating, "The ants march under Reduce Motion")
    MarchingAnts.reduceMotion = { false }
    view.restartAnimation()
    try require(view.isAnimating, "Clearing Reduce Motion did not restart the march")
    MarchingAnts.lowPower = { true }
    view.restartAnimation()
    try require(!view.isAnimating, "The ants march in Low Power Mode")
    MarchingAnts.lowPower = { false }
    view.restartAnimation()
    try require(view.isAnimating, "Leaving Low Power Mode did not restart the march")
    defaults.set(false, forKey: MarchingAnts.enabledDefaultsKey)
    view.restartAnimation()
    try require(!view.isAnimating, "The ants march with \(MarchingAnts.enabledDefaultsKey) off")
    defaults.removeObject(forKey: MarchingAnts.enabledDefaultsKey)
    view.restartAnimation()
    try require(view.isAnimating, "Clearing the kill switch did not restart the march")

    // Hidden while Free Transform and Move Selected own the outline.
    editor.freeTransform(nil)
    pump()
    try require(canvas.isTransforming, "Free Transform did not open on the selection")
    try require(view.isHidden && !view.isAnimating, "The ants show during Free Transform")
    editor.endTransformSession()
    pump()
    try require(!canvas.isTransforming, "Cancelling Free Transform left the preview up")
    try require(!view.isHidden && view.isAnimating,
        "The ants did not come back after Free Transform was cancelled")
    canvas.moveSelectionOffset = CGPoint(x: 3, y: 2)
    try require(view.isHidden && !view.isAnimating, "The ants show while the selection moves")
    canvas.moveSelectionOffset = nil
    try require(!view.isHidden && view.isAnimating,
        "The ants did not come back after the move ended")

    try require(canvas.layer?.shadowPath?.boundingBox == canvas.bounds,
        "The canvas shadow path is \(String(describing: canvas.layer?.shadowPath?.boundingBox)), "
            + "the bounds \(NSStringFromRect(canvas.bounds))")

    editor.agentSetSelection(nil)
    pump()
    try require(view.isHidden && !view.isAnimating, "Deselect left the ants showing")

    // The element cap and the simplified contour need a canvas large enough
    // to reach them.
    let (large, largeEditor, largeSource) = try openByteDocument(width: 1024, height: 1024)
    defer { large.close(); pump() }
    let largeCanvas = largeEditor.canvas
    largeEditor.view.layoutSubtreeIfNeeded()
    pump()
    guard let largeWindow = largeCanvas.window else {
        throw DoctoredCoreError(message: "Owned large editor window missing")
    }
    // Every 8 px: 16,384 isolated pixels, 65,536 contour corners (under the
    // 200,000 simplification limit) and five path elements each.
    guard let sparse = CanvasSelection(
        shape: .mask(gridMask(width: largeSource.width, height: largeSource.height, step: 8)),
        canvasWidth: largeSource.width, canvasHeight: largeSource.height),
          let sparsePath = sparse.marqueePath else {
        throw DoctoredCoreError(message: "Owned sparse grid selection unavailable")
    }
    try require(!sparse.contourIsSimplified, "The 8 px grid's contour was simplified")
    try require(sparsePath.elementCount > MarchingAnts.maxAnimatedElements,
        "The 8 px grid's contour has \(sparsePath.elementCount) elements, not over "
            + "\(MarchingAnts.maxAnimatedElements)")
    try activate(largeWindow)
    largeEditor.agentSetSelection(sparse)
    pump()
    let largeView = try antsView(largeCanvas)
    try require(!largeView.isHidden && !largeView.eligible && !largeView.isAnimating,
        "Over the element cap the ants are hidden \(largeView.isHidden), eligible "
            + "\(largeView.eligible), animating \(largeView.isAnimating)")
    // Every 4 px: 65,536 pixels and 262,144 corners, over the limit, so the
    // marquee falls back to the bounds and marches.
    guard let dense = CanvasSelection(
        shape: .mask(gridMask(width: largeSource.width, height: largeSource.height, step: 4)),
        canvasWidth: largeSource.width, canvasHeight: largeSource.height) else {
        throw DoctoredCoreError(message: "Owned dense grid selection unavailable")
    }
    try require(dense.contourIsSimplified, "The 4 px grid's contour was not simplified")
    largeEditor.agentSetSelection(dense)
    pump()
    try activate(largeWindow)
    largeView.restartAnimation()
    try require(!largeView.isHidden && largeView.eligible && largeView.isAnimating,
        "A simplified contour's ants are hidden \(largeView.isHidden), eligible "
            + "\(largeView.eligible), animating \(largeView.isAnimating)")

    return ["name": "selection-marching-ants-ui", "passed": true,
            "rect": NSStringFromRect(rect), "values": march.values?.count ?? 0,
            "duration": march.duration, "sparse_elements": sparsePath.elementCount,
            "dense_simplified": dense.contourIsSimplified]
}
