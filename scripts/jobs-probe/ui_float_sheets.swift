import AppKit

// Three sheets and panel paths of a 32-bit float document that "float
// filters" and "float retouch strokes" reach only through the session or the
// controller: Filters ▸ Lens Correction…'s own sheet, the Liquify workspace's
// Cancel and its recorded step, and the Paths panel's own Stroke Path button
// and row-menu item.

/// The composite's straight linear RGBA at one pixel.
private func floatSheetPixel(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Double] {
    guard let sample = try FloatReadout.at((x: x, y: y), document: document) else {
        throw ProbeFailure(description: "No float sample at (\(x), \(y))")
    }
    return sample.values
}

/// The largest red in the composite.
private func floatSheetBrightestRed(_ document: ImageDocument) throws -> Double {
    guard let doc = document.doc else { throw ProbeFailure(description: "no document") }
    var brightest = -Double.infinity
    for y in 0..<doc.height {
        for x in 0..<doc.width { brightest = max(brightest, try floatSheetPixel(document, x, y)[0]) }
    }
    return brightest
}

private func floatSheetPress(_ root: NSView, key: String, _ what: String) throws {
    guard let button = buttons(root).first(where: { $0.keyEquivalent == key }) else {
        throw ProbeFailure(description: "\(what): the button is missing")
    }
    try uiSend(button, what)
}

/// The Lens Correction sheet `showLensCorrection(_:)` just presented, with its
/// Distortion slider at 30 and the preview that follows on the canvas.
private func floatLensSheet(_ controller: EditorViewController,
                            _ tag: String) throws -> SliderSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showLensCorrection(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else {
        throw ProbeFailure(description: "\(tag) no Lens Correction sheet")
    }
    let sliders = uiViews(sheet.view, as: NSSlider.self)
    try require(sliders.count == 9, "\(tag) nine sliders: \(sliders.count)")
    sliders[0].doubleValue = 30
    try uiSend(sliders[0], "\(tag) the Distortion slider")
    try pumpUntil({ controller.canvas.previewImage != nil }, seconds: 60)
    return sheet
}

/// "float lens correction sheet": the sheet's own preview, Cancel and Apply.
func verifyFloatLensCorrectionSheet() throws {
    let tag = "[float lens correction sheet]"
    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) a float fixture")
    }
    try require(uiValidates(controller, "showLensCorrection:"),
                "\(tag) Lens Correction… validates on a float layer")
    let before = try floatSheetBrightestRed(document)
    try require(before > 1, "\(tag) the fixture holds a red above white: \(before)")
    let entry = SelectionWorkBudget.scratchBytes
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    let cancelled = try uiRecorded {
        let sheet = try floatLensSheet(controller, tag)
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) the preview leaves the document untouched")
        try floatSheetPress(sheet.view, key: "\u{1b}", "\(tag) Cancel")
        try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 30)
        try require(canvas.previewImage == nil, "\(tag) Cancel leaves no preview")
        // A render already on its way must not land after Cancel.
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        try require(canvas.previewImage == nil, "\(tag) no preview lands after Cancel")
    }
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && cancelled.isEmpty,
                "\(tag) Cancel changed nothing and recorded nothing")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)

    let steps = try uiRecorded {
        let sheet = try floatLensSheet(controller, tag)
        try floatSheetPress(sheet.view, key: "\r", "\(tag) Apply")
        try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    try require(canvas.previewImage == nil, "\(tag) Apply leaves no preview")
    try require(document.doc?.isFloat == true && document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Lens Correction",
                "\(tag) one \"Lens Correction\" step on the float document: "
                + "\(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == "apply_filter"
                && steps[0].arguments["filter"] as? String == "lens_correction"
                && (steps[0].arguments["distortion"] as? NSNumber)?.intValue == 30,
                "\(tag) one recorded apply_filter lens_correction step with distortion 30: "
                + "\(steps.map { "\($0.tool) \($0.arguments)" })")
    let after = try floatSheetBrightestRed(document)
    try require(after > 1, "\(tag) a red above white survives the correction: \(after)")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)
    print("PASS: float lens correction sheet — on a 32-bit float layer the sheet's Distortion "
        + "slider previews on the canvas with the document untouched; Cancel leaves no preview "
        + "and no step; Apply is one float \"Lens Correction\" step, one recorded apply_filter "
        + "lens_correction step and one undo, with a red above white kept (\(after)) and no "
        + "alert")
}

/// The Liquify workspace `showLiquify(_:)` just presented, after one push.
private func floatLiquifySheet(_ controller: EditorViewController,
                               _ tag: String) throws -> LiquifySheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showLiquify(nil)
    guard let sheet = (controller.presentedViewControllers ?? [])
        .compactMap({ $0 as? LiquifySheetController })
        .last(where: { !known.contains(ObjectIdentifier($0)) }) else {
        throw ProbeFailure(description: "\(tag) Filters ▸ Liquify… did not present the workspace")
    }
    try pumpUntil({ sheet.view.window != nil }, seconds: 10)
    sheet.view.window?.animationBehavior = .none
    for (id, value) in [("size", "16"), ("pressure", "100"), ("density", "100")] {
        let field = try uiControl(sheet.view, id: "liquify.\(id)", as: NSTextField.self)
        field.stringValue = value
        try uiSend(field, "\(tag) the \(id) field")
    }
    let apply = try uiControl(sheet.view, id: "liquify.apply", as: NSButton.self)
    try require(!apply.isEnabled, "\(tag) Apply waits for a stroke")
    try require(sheet.strokeBegan(at: CGPoint(x: 20, y: 16), modifiers: []),
                "\(tag) a liquify stroke begins")
    sheet.strokeMoved(to: CGPoint(x: 26, y: 16))
    sheet.strokeEnded()
    try require(apply.isEnabled, "\(tag) the push enables Apply")
    return sheet
}

/// "float liquify sheet": a push then Cancel, and the Apply's recorded step.
func verifyFloatLiquifySheet() throws {
    let tag = "[float liquify sheet]"
    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) a float fixture")
    }
    let entry = SelectionWorkBudget.scratchBytes
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    let cancelled = try uiRecorded {
        let sheet = try floatLiquifySheet(controller, tag)
        let cancel = try uiControl(sheet.view, id: "liquify.cancel", as: NSButton.self)
        try uiSend(cancel, "\(tag) Cancel")
        try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 30)
    }
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "\(tag) a push then Cancel leaves the handle and the undo stack")
    try require(cancelled.isEmpty, "\(tag) Cancel records nothing: \(cancelled.map(\.tool))")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)

    let steps = try uiRecorded {
        let sheet = try floatLiquifySheet(controller, tag)
        let apply = try uiControl(sheet.view, id: "liquify.apply", as: NSButton.self)
        try uiSend(apply, "\(tag) Apply")
        try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied
                        && DocumentJobs.active == nil }, seconds: 60)
    }
    guard let pushed = document.doc, pushed !== base, pushed.isFloat else {
        throw ProbeFailure(description: "\(tag) Apply did not change the float layer")
    }
    try require(steps.count == 1 && steps[0].tool == "liquify",
                "\(tag) exactly one recorded liquify step: \(steps.map(\.tool))")
    try require(document.history.entries.count == entries + 1,
                "\(tag) one entry: \(document.history.entries.map(\.name))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
    try pumpUntil({ SelectionWorkBudget.scratchBytes <= entry }, seconds: 10)
    print("PASS: float liquify sheet — on a 32-bit float layer a push then the workspace's Cancel "
        + "leaves the handle, the history and the undo stack and records nothing; a second "
        + "workspace's push and Apply are one step and exactly one recorded liquify step, with no "
        + "alert")
}

/// "paths panel stroke float": the Paths panel's own Stroke Path controls on a
/// float layer. The setup is `float_retouch.swift`'s case (h): a 160 × 120
/// ramp, one open work path, the Brush and a foreground above white.
func verifyPathsPanelStrokeFloat() throws {
    let tag = "[paths panel stroke float]"
    let ink: [Double] = [4, 0.5, 0.25, 1]
    let (width, height) = (160, 120)
    let document = try floatFixture()
    let store = ToolOptionsStore.shared
    let savedBrush = store.brush
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        store.brush = savedBrush
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let colors = (controller.paintColor, controller.backgroundColor)
    defer { (controller.paintColor, controller.backgroundColor) = colors }
    var tip = PaintToolOptions()
    tip.smoothing = 0
    tip.pressureSize = false
    store.brush = tip

    _ = try agentOK("canvas_size", document,
                    ["width": width, "height": height, "anchor": "top-left"])
    _ = try agentOK("gradient", document, [
        "layer": 0, "start_rgba32": [0, 0.5, 0.25, 1], "end_rgba32": [8, 0.5, 0.25, 1],
        "x0": 0.5, "y0": 60, "x1": Double(width) - 0.5, "y1": 60,
    ])
    _ = try agentOK("set_path", document, ["subpaths": [["anchors": [[30, 100], [130, 100]]]]])
    document.undoManager?.removeAllActions()
    guard let pathed = document.doc, pathed.isFloat,
          let work = pathed.paths.entry(named: "Work Path")?.id,
          let hdr = ExtendedRangeColor.linearColor(ink, in: pathed) else {
        throw ProbeFailure(description: "\(tag) the float picture, its work path and the ink")
    }
    controller.showPathsTab()
    controller.setCurrentPath(.path(work))
    controller.selectTool(.brush)
    controller.paintColor = hdr
    canvas.paintColor = hdr
    guard let panel = controller.pathsPanel else {
        throw ProbeFailure(description: "\(tag) no Paths panel")
    }
    try require(panel.selectedRow?.target == .path(work), "\(tag) the panel selects the work path")
    let action = #selector(EditorViewController.strokeCurrentPath(_:))
    let near = { (got: [Double], want: [Double]) in
        got.count == want.count && zip(got, want).allSatisfy { abs($0 - $1) <= 1e-4 * max(1, abs($1)) }
    }

    /// One Stroke Path sent up the responder chain from `sender`'s view.
    func strokeFrom(_ view: NSView, sender: Any, _ what: String) throws {
        // The cursor, not the count: the button's undone stroke stays
        // retained as redo until the row menu's stroke replaces it.
        let cursor = document.history.cursor
        let alerts = AutomationAlerts.recorded.count
        var performed = false
        let steps = uiRecorded { performed = view.tryToPerform(action, with: sender) }
        try require(performed, "\(tag) \(what): the responder chain from the panel performs "
                    + "strokeCurrentPath:")
        try require(AutomationAlerts.recorded.count == alerts,
                    "\(tag) \(what): no alert: \(uiAlerts())")
        let strokes = steps.filter { $0.tool == "stroke_path" }
        let recorded = strokes.first?.arguments["rgba32"] as? [Double] ?? []
        try require(steps.count == 1 && strokes.count == 1 && near(recorded, ink)
                    && ["color", "color_space"].allSatisfy { strokes[0].arguments[$0] == nil },
                    "\(tag) \(what): one stroke_path step carrying rgba32 alone: "
                    + "\(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
        try require(document.history.cursor == cursor + 1
                    && document.history.entries.count == cursor + 2
                    && document.undoManager?.undoActionName == "Stroke Path",
                    "\(tag) \(what): one \"Stroke Path\" step: "
                    + "\(document.undoManager?.undoActionName ?? "-"), "
                    + "\(document.history.entries.map(\.name)) at \(document.history.cursor)")
        let stroked = try floatSheetPixel(document, 80, 100)
        try require(near(stroked, ink),
                    "\(tag) \(what): the path's line reads the ink above white: \(stroked)")
        document.undoManager?.undo()
        try require(document.doc === pathed && document.undoManager?.canUndo != true,
                    "\(tag) \(what): one undo restores the document")
    }

    // The footer button.
    guard let button = uiViews(panel.view, as: NSButton.self).first(where: { $0.action == action })
    else { throw ProbeFailure(description: "\(tag) the panel has no Stroke Path button") }
    try require(button.isEnabled && button.target == nil,
                "\(tag) the Stroke Path button is enabled, with a nil target")
    try strokeFrom(button, sender: button, "the footer button")

    // The row menu, built for a right-click on the work path's row.
    guard let table = uiViews(panel.view, as: NSTableView.self).first, table.selectedRow >= 0,
          let window = table.window else {
        throw ProbeFailure(description: "\(tag) the panel's table and its selected row")
    }
    let rowRect = table.rect(ofRow: table.selectedRow)
    let point = table.convert(NSPoint(x: rowRect.midX, y: rowRect.midY), to: nil)
    guard let click = NSEvent.mouseEvent(
        with: .rightMouseDown, location: point, modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 1, clickCount: 1, pressure: 1),
          let menu = table.menu(for: click) else {
        throw ProbeFailure(description: "\(tag) the row has no menu")
    }
    menu.delegate?.menuNeedsUpdate?(menu)
    guard let item = menu.items.first(where: { $0.title == "Stroke Path" }) else {
        throw ProbeFailure(description: "\(tag) the row menu has no Stroke Path item: "
            + "\(menu.items.map(\.title))")
    }
    try require(item.action == action && item.target == nil,
                "\(tag) the row menu's Stroke Path is the same nil-target action")
    try require(controller.validatePathsItem(item) == true,
                "\(tag) the row menu's Stroke Path validates enabled")
    try strokeFrom(table, sender: item, "the row menu's item")

    // MCP `gradient` with three stops on the float layer: recorded, not required.
    let stops: [[String: Any]] = [
        ["position": 0, "color": "#ff0000"], ["position": 0.5, "color": "#00ff00"],
        ["position": 1, "color": "#0000ff"],
    ]
    let gradient = try agentCall("gradient", document, [
        "layer": 0, "stops": stops, "x0": 0.5, "y0": 60, "x1": Double(width) - 0.5, "y1": 60,
    ])
    if !gradient.isError {
        try require(document.doc !== pathed && document.doc?.isFloat == true,
                    "\(tag) the three-stop gradient changed the float layer")
        document.undoManager?.undo()
        try require(document.doc === pathed, "\(tag) the gradient undoes")
    }
    let answer = gradient.isError ? "refused (\(gradient.text))" : "ok"
    print("PASS: paths panel stroke float — on a 32-bit float layer the Paths panel's Stroke Path "
        + "button is enabled and, sent up the responder chain from the button, is one \"Stroke "
        + "Path\" step recorded as rgba32 with the line above white, and so is the row menu's "
        + "Stroke Path item; MCP gradient with three stops on the float layer answers \(answer)")
}
