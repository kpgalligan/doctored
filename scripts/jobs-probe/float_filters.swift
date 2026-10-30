import AppKit

// The twenty-four filters and Remove Red Eye on a 32-bit float
// layer. The Filter menu's own
// validation on the layer, on its mask target and in Quick Mask; one slider
// filter (Gaussian Blur) through the session its sheet uses, preview, commit,
// recorded step and Repeat Last Filter; and the Liquify workspace through its
// own stroke entry points and its Apply button. The pixel arithmetic is the
// core's (`core/tests/float_filters*.rs`); this case is about the app's path
// to it.

/// The twenty-four Filter menu items a float layer runs, by selector name:
/// eleven in linear light, Add Noise and Pixelate in linear light too, and
/// the eleven with a display-referred parameter on the display encoding.
private let floatFilterItems = [
    "showBlur:", "showBoxBlur:", "showMotionBlur:", "showRadialBlur:", "showLensBlur:",
    "showTiltShift:", "showOffset:", "showLensCorrection:", "showLiquify:", "showUnsharpMask:",
    "showHighPass:",
    "showSurfaceBlur:", "showDustAndScratches:", "showReduceNoise:", "showAddNoise:",
    "showAddGrain:", "applyClouds:", "applyDifferenceClouds:", "applyEmboss:", "applyEdgeDetect:",
    "showPixelate:", "applySharpen:", "showPresence:", "showDevelopFilter:",
]

private func validates(_ controller: EditorViewController, _ name: String) -> Bool {
    controller.validateUserInterfaceItem(
        NSMenuItem(title: "", action: NSSelectorFromString(name), keyEquivalent: ""))
}

/// The composite's straight linear RGBA at one pixel.
private func linearPixel(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Double] {
    guard let sample = try FloatReadout.at((x: x, y: y), document: document) else {
        throw ProbeFailure(description: "No float sample at (\(x), \(y))")
    }
    return sample.values
}

private func subviews(_ view: NSView) -> [NSView] {
    [view] + view.subviews.flatMap(subviews)
}

private func liquifyControl<T: NSView>(_ sheet: LiquifySheetController, _ id: String,
                                       as type: T.Type) throws -> T {
    guard let value = subviews(sheet.view)
        .first(where: { $0.identifier?.rawValue == "liquify.\(id)" }) as? T else {
        throw ProbeFailure(description: "[float filters] missing liquify control \(id)")
    }
    return value
}

private func frameData(_ image: CGImage?) -> Data? { image?.dataProvider?.data as Data? }

func verifyFloatFilters() throws {
    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let tag = "[float filters]"

    // (a) The menu: the twenty-four validate on the layer's own pixels, and
    // so does Remove Red Eye.
    try require(document.planeEditTarget == .layer && !document.quickMaskActive,
                "\(tag) the fixture opens on its layer")
    let enabled = floatFilterItems.filter { validates(controller, $0) }
    try require(enabled == floatFilterItems,
                "\(tag) the twenty-four filters validate on a float layer: "
                + "\(Set(floatFilterItems).subtracting(enabled))")
    try require(validates(controller, "removeRedEye:"),
                "\(tag) Remove Red Eye validates on a float layer")

    // (b) On the mask target none of the twenty-four validates.
    _ = try agentOK("add_layer_mask", document, ["layer": 0])
    controller.setPaintTarget(.mask)
    try require(document.planeEditTarget == .mask, "\(tag) the panel targets the mask")
    let onMask = floatFilterItems.filter { validates(controller, $0) }
    try require(onMask.isEmpty, "\(tag) no float filter validates on the mask: \(onMask)")
    controller.setPaintTarget(.layer)
    try require(document.planeEditTarget == .layer, "\(tag) the panel targets the layer again")

    // (c) In Quick Mask Gaussian Blur is the Quick Mask blur, live
    // while there is coverage to blur, and still validates; the other
    // twenty-three do not.
    guard let sized = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    document.setCanvasSelection(CanvasSelection(
        shape: .rect(CGRect(x: 8, y: 6, width: 20, height: 12)),
        canvasWidth: sized.width, canvasHeight: sized.height))
    controller.toggleQuickMask(nil)
    try require(document.quickMaskActive, "\(tag) Quick Mask is on")
    let inQuickMask = floatFilterItems.filter { validates(controller, $0) }
    try require(inQuickMask == ["showBlur:"],
                "\(tag) in Quick Mask only Gaussian Blur validates: \(inQuickMask)")
    controller.toggleQuickMask(nil)
    try require(!document.quickMaskActive, "\(tag) Quick Mask is off")
    // Whatever selection leaving Quick Mask made, the edits below run unselected.
    _ = try agentOK("deselect", document)
    document.undoManager?.removeAllActions()
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base") }

    // (d) Gaussian Blur through the session its sheet uses: a preview lands
    // on the canvas, and the commit is one undo step and one recorded
    // `apply_filter` `blur` step. The 8.0 block's edge keeps a red above 1.
    let params: [String: Any] = ["sigma": 2.0]
    guard let session = PrecisionAdjustmentSession(document, canvas: canvas, mode: .destructive)
    else { throw ProbeFailure(description: "\(tag) no adjustment session on a float layer") }
    session.request(filter: "blur", params: params)
    try pumpUntil({ canvas.previewImage != nil || !session.previewStatus.stringValue.isEmpty },
                  seconds: 20)
    try require(canvas.previewImage != nil,
                "\(tag) the blur preview lands on the canvas: \(session.previewStatus.stringValue)")
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) the preview leaves the document untouched")
    ActionRecorder.shared.start()
    session.commit(filter: "blur", params: params)
    let recorded = ActionRecorder.shared.stop()
    try require(!BackgroundWork.occupied && canvas.previewImage == nil,
                "\(tag) the commit's job has finished and the preview is gone")
    guard let blurred = document.doc, blurred !== base, blurred.isFloat else {
        throw ProbeFailure(description: "\(tag) the blur commit did not change the float layer")
    }
    let steps = recorded.filter { $0.tool == "apply_filter" }
    try require(recorded.count == 1 && steps.count == 1
                && steps[0].arguments["filter"] as? String == "blur",
                "\(tag) one recorded apply_filter blur step: "
                + "\(recorded.map { "\($0.tool) \($0.arguments)" })")
    let brightest = try (0..<base.width).map { try linearPixel(document, $0, 4)[0] }.max() ?? 0
    try require(brightest > 1, "\(tag) a red above white survives the blur: \(brightest)")

    // (e) Repeat Last Filter validates and repeats it as one more step.
    try require(validates(controller, "repeatLastFilter:"),
                "\(tag) Repeat Last Filter validates after a float blur")
    controller.repeatLastFilter(nil)
    try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 20)
    guard let twice = document.doc, twice !== blurred else {
        throw ProbeFailure(description: "\(tag) Repeat Last Filter did not repeat the blur")
    }
    document.undoManager?.undo()
    try require(document.doc === blurred, "\(tag) the repeat is one undo step")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) the blur commit is exactly one undo step")

    // (f) Filters ▸ Liquify… opens its workspace on the float layer; a push
    // through the sheet's own stroke entry points previews, and Apply moves
    // the layer's linear samples in one undo step.
    let from = CGPoint(x: 20, y: 16), to = CGPoint(x: 26, y: 16)
    let source = try linearPixel(document, 20, 16), target = try linearPixel(document, 26, 16)
    try require(source != target, "\(tag) the ramp differs across the push: \(source) \(target)")
    controller.showLiquify(nil)
    guard let sheet = controller.presentedViewControllers?.last as? LiquifySheetController else {
        throw ProbeFailure(description: "\(tag) Filters ▸ Liquify… did not present the workspace")
    }
    try pumpUntil({ sheet.view.window != nil }, seconds: 10)
    sheet.view.window?.animationBehavior = .none
    for (id, value) in [("size", "16"), ("pressure", "100"), ("density", "100")] {
        let field = try liquifyControl(sheet, id, as: NSTextField.self)
        field.stringValue = value
        _ = field.sendAction(field.action, to: field.target)
    }
    let apply = try liquifyControl(sheet, "apply", as: NSButton.self)
    let proxy = sheet.session.proxy
    let idle = sheet.canvasView.image
    try require(idle != nil && idle?.width == proxy.width && idle?.height == proxy.height
                && proxy.width == base.width && proxy.height == base.height && !apply.isEnabled,
                "\(tag) the workspace shows the float layer at the proxy's size and Apply waits")
    try require(sheet.strokeBegan(at: from, modifiers: []), "\(tag) a liquify stroke begins")
    sheet.strokeMoved(to: to)
    sheet.strokeEnded()
    try require(frameData(sheet.canvasView.image) != frameData(idle) && apply.isEnabled,
                "\(tag) the push changes the preview and enables Apply")
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) the liquify preview leaves the document untouched")
    apply.performClick(nil)
    guard let pushed = document.doc, pushed !== base, pushed.isFloat else {
        throw ProbeFailure(description: "\(tag) Apply did not change the float layer: "
            + sheet.status.stringValue)
    }
    let moved = try linearPixel(document, 26, 16)
    try require(zip(moved, source).allSatisfy { abs($0 - $1) <= 1e-4 * max(1, abs($1)) },
                "\(tag) pixel (26, 16) reads the linear sample from (20, 16): "
                + "\(moved) vs \(source)")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) the liquify Apply is exactly one undo step")
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, seconds: 10)

    print("PASS: float-filters — on a 32-bit float layer the twenty-four filters' menu items and "
        + "Remove Red Eye validate; none validates on the mask target, and in Quick Mask only "
        + "Gaussian Blur (the Quick Mask blur) does; a Gaussian Blur request lands a preview, its "
        + "commit is one undo step and one recorded apply_filter blur step with a red above white "
        + "kept, and Repeat Last Filter repeats it; Filters ▸ Liquify… opens its workspace at the "
        + "proxy's size, a push previews and enables Apply, and Apply moves the layer's linear "
        + "samples in one undo step")
}
