import AppKit

// "fade sheet" (v1, `designs/v1-shortcuts-fade.md`): Edit ▸ Fade… through its own item and sheet
// at 8 and 16 bits — after Image ▸ Adjustments ▸ Invert the item validates titled "Fade Invert…",
// the sheet at 50 % previews with the document untouched, Apply is ONE "Fade Invert" step in place
// of "Invert" (the history keeps its entry count) recorded as `fade`, and one undo restores the
// original; a selection change makes the item unavailable. At 32 bits the Mode popup lists only
// the modes a float document admits (no Screen).

/// The Fade sheet `showFade(_:)` just presented on `controller`.
private func fadeSheet(_ controller: EditorViewController, _ tag: String) throws
    -> FadeSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showFade(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? FadeSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Fade sheet") }
    return sheet
}

/// The Edit ▸ Fade… item as the editor validates it: (enabled, title).
private func fadeItem(_ controller: EditorViewController) -> (Bool, String) {
    let item = NSMenuItem(title: "Fade…", action: #selector(EditorViewController.showFade(_:)),
                          keyEquivalent: "")
    let enabled = controller.validateUserInterfaceItem(item)
    return (enabled, item.title)
}

private func verifyFadeSheet(depth: Int) throws {
    let tag = "[fade sheet \(depth)-bit]"
    let document = depth == 8
        ? try fixture(width: 160, height: 120) : try precisionFixture(width: 160, height: 120)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isHighDepth == (depth == 16) else {
        throw ProbeFailure(description: "\(tag) the fixture's depth")
    }
    try require(!fadeItem(controller).0 && fadeItem(controller).1 == "Fade…",
                "\(tag) nothing to fade on a fresh document: \(fadeItem(controller))")

    // Image ▸ Adjustments ▸ Invert, through its own action.
    controller.applyInvert(nil)
    try pumpUntil({ document.doc !== base && !BackgroundWork.occupied
                    && DocumentJobs.active == nil }, seconds: 60)
    guard let inverted = document.doc else { throw ProbeFailure(description: "\(tag) no result") }
    try require(document.undoManager?.undoActionName == "Invert", "\(tag) one Invert step")
    let (enabled, title) = fadeItem(controller)
    try require(enabled && title == "Fade Invert…", "\(tag) the item after Invert: \(enabled) \(title)")
    let entries = document.history.entries.count

    let recorded = try uiRecorded {
        let sheet = try fadeSheet(controller, tag)
        let slider = try uiControl(sheet.view, id: "fade.opacity", as: NSSlider.self)
        try require(slider.doubleValue == 100, "\(tag) Opacity opens at 100: \(slider.doubleValue)")
        slider.doubleValue = 50
        try uiSend(slider, "\(tag) the Opacity slider")
        try pumpUntil({ canvas.previewImage != nil }, seconds: 60)
        try require(document.doc === inverted && document.history.entries.count == entries,
                    "\(tag) the preview leaves the document untouched")
        guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
            throw ProbeFailure(description: "\(tag) no Apply button")
        }
        try uiSend(apply, "\(tag) Apply")
        try pumpUntil({ document.doc !== inverted && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    try require(canvas.previewImage == nil, "\(tag) Apply leaves no preview")
    try require(document.history.entries.count == entries
                && document.undoManager?.undoActionName == "Fade Invert",
                "\(tag) one \"Fade Invert\" step in place of Invert: "
                + "\(document.history.entries.map(\.name))")
    try require(!document.history.entries.contains { $0.name == "Invert" },
                "\(tag) the Invert step was replaced: \(document.history.entries.map(\.name))")
    try require(recorded.map(\.tool) == ["fade"]
                && (recorded.first?.arguments["opacity"] as? Double) == 0.5
                && (recorded.first?.arguments["mode"] as? String) == "Normal",
                "\(tag) the recorded step: \(recorded.map { "\($0.tool) \($0.arguments)" })")
    // Halfway between a sample and its inverse is mid-grey: every byte of the
    // 8-bit layer lands within one code of 127.5.
    if depth == 8, let pixels = document.doc?.layerPixels(0) {
        var worst = 0.0
        for i in stride(from: 0, to: pixels.count, by: 4) {
            for c in 0..<3 { worst = max(worst, abs(Double(pixels[i + c]) - 127.5)) }
        }
        try require(worst <= 1.0, "\(tag) the 50 % fade of Invert is mid-grey: worst \(worst)")
    }
    try require(!fadeItem(controller).0, "\(tag) a Fade is not itself fadeable")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the original")

    // A selection change makes the item unavailable.
    controller.applyInvert(nil)
    try pumpUntil({ document.doc !== base && !BackgroundWork.occupied
                    && DocumentJobs.active == nil }, seconds: 60)
    try require(fadeItem(controller).0, "\(tag) the second Invert is fadeable")
    _ = try select(document, side: 40)
    try require(!fadeItem(controller).0, "\(tag) a selection change makes Fade unavailable")
    print("PASS: \(tag) Fade Invert at 50 % previews, replaces the step, records `fade` and undoes")
}

/// At 32 bits the Mode popup offers only what a float document admits.
private func verifyFadeSheetFloat() throws {
    let tag = "[fade sheet 32-bit]"
    let document = try floatFixture()
    defer { document.close() }
    let controller = try editor(document)
    guard let doc = document.doc, doc.isFloat else {
        throw ProbeFailure(description: "\(tag) the fixture is not a float document")
    }
    // The candidate a float filter leaves, installed directly: the popup is
    // the case's subject, not the filter.
    let candidate = FadeCandidate(
        name: "Gaussian Blur", kind: .region, layer: 0, before: doc, after: doc,
        selection: document.selectionHistory, layerSelection: document.layerSelection,
        historyID: document.history.currentID)
    let sheet = FadeSheetController(editor: controller, candidate: candidate)
    _ = sheet.view
    let popup = try uiControl(sheet.view, id: "fade.mode", as: NSPopUpButton.self)
    let titles = popup.itemArray.filter { !$0.isSeparatorItem }.map(\.title)
    try require(!titles.contains("Screen") && titles.contains("Multiply")
                && titles.contains("Normal") && popup.titleOfSelectedItem == "Normal",
                "\(tag) the float modes: \(titles)")
    try require(titles.count == NativeLayerBlending.floatModes.count,
                "\(tag) every admitted mode and no other: \(titles)")
    print("PASS: \(tag) the Mode popup lists the \(titles.count) float modes, Screen omitted")
}

func verifyFadeSheets() throws {
    try verifyFadeSheet(depth: 8)
    try verifyFadeSheet(depth: 16)
    try verifyFadeSheetFloat()
}
