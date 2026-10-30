import AppKit

// Filters ▸ Presence… through its own sheet at 8 and 16 bits: the
// all-zero Apply is no step, the sliders preview, and Apply is one step whose
// recorded Action carries the three integers.

/// The Presence sheet `showPresence(_:)` just presented on `controller`.
private func presenceSheet(_ controller: EditorViewController,
                           _ tag: String) throws -> SliderSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showPresence(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Presence sheet") }
    return sheet
}

private func presencePress(_ sheet: SliderSheetController, key: String, _ what: String) throws {
    guard let button = buttons(sheet.view).first(where: { $0.keyEquivalent == key }) else {
        throw ProbeFailure(description: "\(what): the button is missing")
    }
    try uiSend(button, what)
}

private func verifyPresenceSheet(depth: Int) throws {
    let tag = "[presence sheet \(depth)-bit]"
    let document = depth == 8
        ? try fixture(width: 320, height: 240) : try precisionFixture(width: 320, height: 240)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isHighDepth == (depth == 16) else {
        throw ProbeFailure(description: "\(tag) the fixture's depth")
    }
    try require(uiValidates(controller, "showPresence:"), "\(tag) Presence… validates")
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    // All three at 0: Apply is the exact bypass.
    let bypass = try uiRecorded {
        let sheet = try presenceSheet(controller, tag)
        let sliders = uiViews(sheet.view, as: NSSlider.self)
        try require(sliders.count == 3 && sliders.allSatisfy { $0.doubleValue == 0 },
                    "\(tag) three sliders at 0: \(sliders.map(\.doubleValue))")
        try presencePress(sheet, key: "\r", "\(tag) Apply at 0/0/0")
        try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied
                        && DocumentJobs.active == nil }, seconds: 30)
    }
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "\(tag) Apply at 0/0/0 is no step")
    try require(bypass.isEmpty, "\(tag) Apply at 0/0/0 records nothing: \(bypass.map(\.tool))")

    // 30 / 20 / 10: a preview, then one step.
    let values = [30, 20, 10]
    let recorded = try uiRecorded {
        let sheet = try presenceSheet(controller, tag)
        let sliders = uiViews(sheet.view, as: NSSlider.self)
        try require(sliders.count == 3, "\(tag) three sliders: \(sliders.count)")
        for (slider, value) in zip(sliders, values) {
            slider.doubleValue = Double(value)
            try uiSend(slider, "\(tag) a slider")
        }
        try pumpUntil({ canvas.previewImage != nil }, seconds: 60)
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) the preview leaves the document untouched")
        try presencePress(sheet, key: "\r", "\(tag) Apply")
        try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    try require(canvas.previewImage == nil, "\(tag) Apply leaves no preview")
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Presence",
                "\(tag) one \"Presence\" step: \(document.history.entries.map(\.name))")
    let integer = { (step: ActionStep, key: String) in (step.arguments[key] as? NSNumber)?.intValue }
    try require(recorded.count == 1 && recorded[0].tool == "apply_filter"
                && recorded[0].arguments["filter"] as? String == "presence"
                && integer(recorded[0], "texture") == 30 && integer(recorded[0], "clarity") == 20
                && integer(recorded[0], "dehaze") == 10,
                "\(tag) one recorded apply_filter presence step with 30/20/10: "
                + "\(recorded.map { "\($0.tool) \($0.arguments)" })")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
}

/// "presence sheet": Filters ▸ Presence… at 8 and 16 bits, and its menu item
/// on in a 32-bit float document (all three controls run there).
func verifyPresenceSheet() throws {
    try verifyPresenceSheet(depth: 8)
    try verifyPresenceSheet(depth: 16)
    let float = try floatFixture()
    defer { float.close() }
    let floatController = try editor(float)
    try require(uiValidates(floatController, "showPresence:"),
                "[presence sheet] Presence… validates in a 32-bit float document")
    print("PASS: presence sheet — at 8 and 16 bits Filters ▸ Presence… opens three sliders; Apply "
        + "at 0/0/0 is no step and records nothing; 30/20/10 previews with the document "
        + "untouched and Apply is one \"Presence\" step, one recorded apply_filter presence step "
        + "carrying the three integers and one undo; the item validates in a float document")
}
