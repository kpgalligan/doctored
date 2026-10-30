import AppKit

// Select ▸ Select and Mask… and the Magnetic Lasso in a 32-bit float document
// (8d1f952b) through their own controls: the workspace's edge stage
// (Edge Radius and Smart Radius) with each of the four outputs, Decontaminate
// Colors disabled at 32 bits, and the Magnetic Lasso's clicks and hovers
// through its canvas session. The picture is a 0.1 background with an 80 × 60
// square at (40, 30) above white.

private let maskSquare = [40, 30, 80, 60]

/// The Select and Mask workspace `refineSelection(_:)` just presented.
private func refineSheet(_ controller: EditorViewController,
                         _ tag: String) throws -> RefineSelectionSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.refineSelection(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? RefineSelectionSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Select and Mask sheet") }
    return sheet
}

/// "select and mask float": the edge stage with each output.
func verifySelectAndMaskFloat() throws {
    let tag = "[select and mask float]"
    let square: [Double] = [8, 4, 2, 1]
    let document = try floatToolDocument([([0, 0, 160, 120], [0.1, 0.1, 0.1, 1]), (maskSquare, square)])
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    _ = try agentOK("select_rect", document, ["x": 44, "y": 34, "width": 72, "height": 52])
    document.undoManager?.removeAllActions()
    guard let base = document.doc, base.isFloat, document.canvasSelection != nil else {
        throw ProbeFailure(description: "\(tag) the float picture and its selection")
    }
    let alerts = AutomationAlerts.recorded.count
    let entries = document.history.cursor
    var rows: [String] = []
    for (index, output) in RefineSelectionSheetController.outputs.enumerated() {
        let what = "\(tag) \(output.rawValue)"
        let steps = try uiRecorded {
            let sheet = try refineSheet(controller, what)
            let decontaminate = try uiControl(sheet.view, id: "refine.decontaminate", as: NSButton.self)
            let amount = try uiControl(sheet.view, id: "refine.decontaminateAmount", as: NSTextField.self)
            try require(!decontaminate.isEnabled && !amount.isEnabled,
                        "\(what) Decontaminate Colors and its Amount are disabled at 32 bits")
            let radius = try uiControl(sheet.view, id: "refine.edgeRadius", as: NSTextField.self)
            radius.stringValue = "4"
            try uiSend(radius, "\(what) refine.edgeRadius")
            let smart = try uiControl(sheet.view, id: "refine.smartRadius", as: NSButton.self)
            smart.state = .on
            try uiSend(smart, "\(what) refine.smartRadius")
            let popup = try uiControl(sheet.view, id: "refine.output", as: NSPopUpButton.self)
            popup.selectItem(at: index)
            try uiSend(popup, "\(what) refine.output")
            let apply = try uiControl(sheet.view, id: "refine.apply", as: NSButton.self)
            try pumpUntil({ apply.isEnabled }, seconds: 60)
            try uiSend(apply, "\(what) Apply")
            try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied
                            && DocumentJobs.active == nil }, seconds: 60)
        }
        try require(document.history.cursor == entries + 1,
                    "\(what) one step: \(document.history.entries.map(\.name)) \(uiAlerts())")
        let arguments = steps.first?.arguments ?? [:]
        try require(steps.count == 1 && steps[0].tool == "refine_selection"
                    && arguments["output"] as? String == output.rawValue
                    && (arguments["edge_radius"] as? NSNumber)?.doubleValue == 4
                    && arguments["smart_radius"] as? Bool == true,
                    "\(what) one recorded refine_selection step with output, edge_radius 4 and "
                    + "smart_radius: \(steps.map { "\($0.tool) \($0.arguments)" })")
        guard let doc = document.doc else { throw ProbeFailure(description: "\(what) no document") }
        switch output {
        case .selection:
            let inside = floatToolCoverage(document, 80, 60), outside = floatToolCoverage(document, 10, 10)
            try require(inside == 65535 && outside == 0,
                        "\(what) the selection covers (80, 60) and not (10, 10): \(inside) \(outside)")
        case .mask:
            let mask = try PrecisionScalarOperations.read(doc, target: "mask", index: 0).samples
            try require(mask[60 * 160 + 80] == 65535 && mask[10 * 160 + 10] == 0,
                        "\(what) the layer mask reveals (80, 60) and hides (10, 10): "
                        + "\(mask[60 * 160 + 80]) \(mask[10 * 160 + 10])")
        case .newLayer:
            let inside = try floatToolLayerPixel(doc, 1, 80, 60)
            let outside = try floatToolLayerPixel(doc, 1, 10, 10)
            try require(doc.layerCount == 2 && inside == square && outside[3] == 0,
                        "\(what) the new layer above keeps [8, 4, 2, 1] and is clear at (10, 10): "
                        + "\(inside) \(outside)")
        case .newLayerWithMask:
            let inside = try floatToolLayerPixel(doc, 1, 80, 60)
            let mask = try PrecisionScalarOperations.read(doc, target: "mask", index: 1).samples
            try require(doc.layerCount == 2 && Array(inside.prefix(3)) == [8, 4, 2]
                        && mask[60 * 160 + 80] == 65535,
                        "\(what) the new layer keeps [8, 4, 2] under a revealing mask: "
                        + "\(inside) \(mask[60 * 160 + 80])")
        }
        try require(AutomationAlerts.recorded.count == alerts, "\(what) no alert: \(uiAlerts())")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true
                    && document.canvasSelection != nil,
                    "\(what) one undo restores the document and its selection")
        rows.append(output.rawValue)
    }
    print("PASS: select and mask float — in a 32-bit float document Select and Mask opens with "
        + "Decontaminate Colors and its Amount disabled; Edge Radius 4 with Smart Radius and each "
        + "output (\(rows.joined(separator: ", "))) is one step recorded refine_selection with that "
        + "output, edge_radius 4 and smart_radius; the selection and the layer mask cover the "
        + "square's inside and not the background, New Layer keeps [8, 4, 2, 1] above white, New "
        + "Layer with Layer Mask keeps [8, 4, 2] under a revealing mask; one undo each")
}

/// "magnetic lasso float": four clicks with hovers along the edges of a
/// square above white, closed by a double-click.
func verifyMagneticLassoFloat() throws {
    let tag = "[magnetic lasso float]"
    let document = try floatToolDocument([([0, 0, 160, 120], [0.1, 0.1, 0.1, 1]),
                                          (maskSquare, [4, 4, 4, 1])])
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isFloat, let window = canvas.window else {
        throw ProbeFailure(description: "\(tag) the float picture and its window")
    }
    controller.selectTool(.magneticLasso)
    try require(controller.currentTool == .magneticLasso, "\(tag) the Magnetic Lasso is selected")
    let width = MagneticLassoSettings.shared.options.width
    func click(_ point: CGPoint, count: Int = 1) throws {
        guard let event = NSEvent.mouseEvent(
            with: .leftMouseDown, location: .zero, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 1, clickCount: count, pressure: 1) else {
            throw ProbeFailure(description: "\(tag) could not create the click")
        }
        canvas.magneticLassoMouseDown(at: point, event: event)
    }
    func hover(from: CGPoint, to: CGPoint) {
        for step in 1...20 {
            let t = CGFloat(step) / 20
            canvas.moveMagneticLasso(to: CGPoint(x: from.x + (to.x - from.x) * t,
                                                 y: from.y + (to.y - from.y) * t))
        }
    }
    let corners = [CGPoint(x: 40, y: 30), CGPoint(x: 120, y: 30), CGPoint(x: 120, y: 90),
                   CGPoint(x: 40, y: 90)]
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    let steps = try uiRecorded {
        try click(corners[0])
        try require(canvas.hasMagneticLassoSession, "\(tag) the first click starts an outline")
        for index in 1...3 {
            hover(from: corners[index - 1], to: corners[index])
            try click(corners[index])
        }
        hover(from: corners[3], to: CGPoint(x: 40, y: 32))
        try click(corners[0], count: 2)
        try require(!canvas.hasMagneticLassoSession, "\(tag) the double-click closes the outline")
        try floatToolSettle()
    }
    guard let selection = document.canvasSelection else {
        throw ProbeFailure(description: "\(tag) the outline set no selection: \(uiAlerts())")
    }
    let coverage = selection.maskSamples16()
    var inside = 0, farthest = 0.0
    for y in 0..<120 {
        for x in 0..<160 {
            let value = coverage[y * 160 + x]
            let cx = Double(x) + 0.5, cy = Double(y) + 0.5
            if cx > 40, cx < 120, cy > 30, cy < 90 {
                if value >= 32768 { inside += 1 }
            } else if value > 0 {
                let dx = max(40 - cx, cx - 120, 0), dy = max(30 - cy, cy - 90, 0)
                farthest = max(farthest, (dx * dx + dy * dy).squareRoot())
            }
        }
    }
    let share = Double(inside) / Double(80 * 60)
    try require(share >= 0.95, "\(tag) at least 95% of the square is selected: \(share)")
    try require(farthest <= width, "\(tag) nothing selected lies farther than the Width "
                + "(\(width)) from the square: \(farthest)")
    try require(document.history.cursor == entries + 1
                && document.undoManager?.undoActionName == "Magnetic Lasso",
                "\(tag) one \"Magnetic Lasso\" step: \(document.history.entries.map(\.name))")
    try require(steps.count == 1 && steps[0].tool == "select_freehand",
                "\(tag) one recorded select_freehand step: \(steps.map(\.tool))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base && document.canvasSelection == nil
                && document.undoManager?.canUndo != true, "\(tag) one undo clears the selection")
    print("PASS: magnetic lasso float — in a 32-bit float document four Magnetic Lasso clicks with "
        + "hovers along a square above white, closed by a double-click, are one \"Magnetic Lasso\" "
        + "step recorded select_freehand selecting " + String(format: "%.1f%%", share * 100)
        + " of the square with nothing farther than " + String(format: "%.1f", farthest) + " px "
        + "(Width \(width)) from it, one undo")
}
