import AppKit

// Filters ▸ Develop Filter…: the item sits directly above Lens
// Correction… in the Filters menu, and the sheet's Reset puts every control
// back to 0 with the preset popup on "Default", the entry those values are,
// not "Custom".

/// "develop filter sheet": the menu position, and Reset's preset title.
func verifyDevelopFilterSheet() throws {
    let tag = "[develop filter sheet]"
    let titles = AppDelegate().filtersMenu().items.map(\.title)
    guard let develop = titles.firstIndex(of: "Develop Filter…"),
          let lens = titles.firstIndex(of: "Lens Correction…")
    else { throw ProbeFailure(description: "\(tag) the Filters menu's items: \(titles)") }
    try require(develop + 1 == lens,
                "\(tag) Develop Filter… sits directly above Lens Correction…: \(titles)")

    let document = try fixture(width: 320, height: 240)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    try require(uiValidates(controller, "showDevelopFilter:"),
                "\(tag) Develop Filter… validates")
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    let recorded = try uiRecorded {
        let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
        controller.showDevelopFilter(nil)
        let opened = {
            (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
                .last { !known.contains(ObjectIdentifier($0)) }
        }
        try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
        guard let sheet = opened() else {
            throw ProbeFailure(description: "\(tag) no Develop Filter sheet")
        }
        let popup = try uiControl(sheet.view, id: "develop-filter-preset-popup",
                                  as: NSPopUpButton.self)
        try require(popup.titleOfSelectedItem == "Default",
                    "\(tag) the sheet opens on Default: \(popup.titleOfSelectedItem ?? "nil")")
        let row = try uiControl(sheet.view, id: "develop-filter-contrast", as: CardControlRow.self)
        guard let slider = uiViews(row, as: NSSlider.self).first else {
            throw ProbeFailure(description: "\(tag) the Contrast row has no slider")
        }
        slider.doubleValue = 40
        try uiSend(slider, "\(tag) the Contrast slider")
        try require(popup.titleOfSelectedItem == "Custom",
                    "\(tag) Contrast 40 reads Custom: \(popup.titleOfSelectedItem ?? "nil")")
        try pumpUntil({ canvas.previewImage != nil }, seconds: 60)
        guard let reset = buttons(sheet.view).first(where: { $0.title == "Reset" }) else {
            throw ProbeFailure(description: "\(tag) the Reset button is missing")
        }
        try uiSend(reset, "\(tag) Reset")
        try require(slider.doubleValue == 0, "\(tag) Reset puts Contrast at 0: \(slider.doubleValue)")
        try require(popup.titleOfSelectedItem == "Default",
                    "\(tag) after Reset the preset popup reads Default: "
                    + "\(popup.titleOfSelectedItem ?? "nil")")
        guard let cancel = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }) else {
            throw ProbeFailure(description: "\(tag) the Cancel button is missing")
        }
        try uiSend(cancel, "\(tag) Cancel")
        try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied
                        && DocumentJobs.active == nil }, seconds: 30)
    }
    try require(canvas.previewImage == nil, "\(tag) Cancel leaves no preview")
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && recorded.isEmpty,
                "\(tag) Cancel changed nothing and recorded nothing")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: develop filter sheet — Filters ▸ Develop Filter… sits directly above "
        + "Lens Correction…; the sheet opens on Default, Contrast 40 reads Custom, and Reset puts "
        + "Contrast back to 0 with the preset popup reading Default; Cancel changes and records "
        + "nothing")
}
