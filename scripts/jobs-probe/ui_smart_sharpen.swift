import AppKit

// Filter ▸ Sharpen ▸ Smart Sharpen… through its own sheet at 8 and 16
// bits: ten sliders and the Remove popup at their defaults, a preview that
// leaves the document untouched, Apply as one step whose recorded Action
// carries the settings, Repeat Last Filter (⌃F) as one more step, and undo.

/// The Smart Sharpen sheet `showSmartSharpen(_:)` just presented on `controller`.
private func smartSharpenSheet(_ controller: EditorViewController,
                               _ tag: String) throws -> SliderSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showSmartSharpen(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Smart Sharpen sheet") }
    return sheet
}

private func verifySmartSharpenSheet(depth: Int) throws {
    let tag = "[smart sharpen sheet \(depth)-bit]"
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
    try require(uiValidates(controller, "showSmartSharpen:"), "\(tag) Smart Sharpen… validates")
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    // Amount 150, Radius 2, Reduce Noise 20, Shadows Fade 30, Remove Gaussian
    // Blur: a preview, then one step.
    let recorded = try uiRecorded {
        let sheet = try smartSharpenSheet(controller, tag)
        let sliders = uiViews(sheet.view, as: NSSlider.self)
        let defaults = [200.0, 1.0, 10.0, 0.0, 0.0, 50.0, 1.0, 0.0, 50.0, 1.0]
        try require(sliders.count == 10 && zip(sliders, defaults).allSatisfy {
            abs($0.doubleValue - $1) < 1e-9
        }, "\(tag) ten sliders at their defaults: \(sliders.map(\.doubleValue))")
        let popups = uiViews(sheet.view, as: NSPopUpButton.self)
        try require(popups.count == 1 && popups[0].titleOfSelectedItem == "Lens Blur",
                    "\(tag) the Remove popup reads Lens Blur: \(popups.map(\.titleOfSelectedItem))")
        for (index, value) in [(0, 150.0), (1, 2.0), (2, 20.0), (4, 30.0)] {
            sliders[index].doubleValue = value
            try uiSend(sliders[index], "\(tag) slider \(index)")
        }
        popups[0].selectItem(withTitle: "Gaussian Blur")
        try uiSend(popups[0], "\(tag) the Remove popup")
        try pumpUntil({ canvas.previewImage != nil }, seconds: 60)
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) the preview leaves the document untouched")
        guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
            throw ProbeFailure(description: "\(tag) the Apply button is missing")
        }
        try uiSend(apply, "\(tag) Apply")
        try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    try require(canvas.previewImage == nil, "\(tag) Apply leaves no preview")
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Smart Sharpen",
                "\(tag) one \"Smart Sharpen\" step: \(document.history.entries.map(\.name))")
    let number = { (step: ActionStep, key: String) in (step.arguments[key] as? NSNumber)?.doubleValue }
    try require(recorded.count == 1 && recorded[0].tool == "apply_filter"
                && recorded[0].arguments["filter"] as? String == "smart_sharpen"
                && number(recorded[0], "amount") == 150 && number(recorded[0], "radius") == 2
                && number(recorded[0], "noise") == 20 && number(recorded[0], "shadow_fade") == 30
                && number(recorded[0], "shadow_tonal_width") == 50
                && number(recorded[0], "highlight_radius") == 1
                && recorded[0].arguments["remove"] as? String == "gaussian",
                "\(tag) one recorded apply_filter smart_sharpen step with the settings: "
                + "\(recorded.map { "\($0.tool) \($0.arguments)" })")
    guard let sharpened = document.doc else { throw ProbeFailure(description: "\(tag) no document") }

    // ⌃F repeats it as one more step.
    try require(uiValidates(controller, "repeatLastFilter:"),
                "\(tag) Repeat Last Filter validates after Smart Sharpen")
    controller.repeatLastFilter(nil)
    try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    try require(document.doc !== sharpened && document.history.entries.count == entries + 2,
                "\(tag) Repeat Last Filter is one more step: \(document.history.entries.map(\.name))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === sharpened, "\(tag) the repeat is one undo step")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
}

/// "smart sharpen sheet": Filter ▸ Sharpen ▸ Smart Sharpen… at 8 and 16 bits,
/// and its menu item on in a 32-bit float document (the whole filter runs on
/// the display encoding there).
func verifySmartSharpenSheet() throws {
    try verifySmartSharpenSheet(depth: 8)
    try verifySmartSharpenSheet(depth: 16)
    let float = try floatFixture()
    defer { float.close() }
    let floatController = try editor(float)
    try require(uiValidates(floatController, "showSmartSharpen:"),
                "[smart sharpen sheet] Smart Sharpen… validates in a 32-bit float document")
    print("PASS: smart sharpen sheet — at 8 and 16 bits Filter ▸ Sharpen ▸ Smart Sharpen… opens "
        + "ten sliders and the Remove popup at their defaults; amount 150, radius 2, noise 20, "
        + "shadows fade 30 and Gaussian Blur preview with the document untouched; Apply is one "
        + "\"Smart Sharpen\" step and one recorded apply_filter smart_sharpen step carrying the "
        + "settings; ⌃F repeats it as one more step; two undos restore the document; the item "
        + "validates in a float document")
}
