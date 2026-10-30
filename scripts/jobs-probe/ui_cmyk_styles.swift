import AppKit

// The CMYK layer-style in-process UI case (#1, the styles batch): on a CMYK document Layer ▸
// Layer Style's effect items and Copy / Paste Layer Style validate; the sheet opened on Drop
// Shadow has every effect checkbox and the knockout popup enabled, and its Blending Options pane
// a Blend If heading over the channels Gray, Cyan, Magenta, Yellow and Black (the Blend If batch);
// the Drop Shadow previews with no refusal; and OK is ONE undo step whose stored shadow carries an
// ink. The CMYK profile is macOS's Generic CMYK Profile, read from the system.

private let stylesCMYKProfile = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// "cmyk layer style" (module doc).
func verifyCmykLayerStyle() throws {
    let tag = "[cmyk layer style]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: stylesCMYKProfile), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else { throw ProbeFailure(description: "\(tag) not CMYK") }
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Two") }
    controller.setActiveLayer(1)
    // Copy Layer Style validates once the layer has a style (checked after OK below).
    for selector in ["layerStyle:", "layerStyleEffect:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }

    // Blending Options (Layer Style… on no effect) shows the knockout popup, enabled.
    let before = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.layerStyle(nil)
    let blending = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? LayerStyleSheetController }
            .last { !before.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ blending()?.view.window?.isVisible == true }, seconds: 20)
    guard let options = blending() else { throw ProbeFailure(description: "\(tag) no Blending Options sheet") }
    let knockout = uiViews(options.view, as: NSView.self).first {
        $0.identifier == LayerStyleKnockoutRows.knockoutIdentifier
    }
    let popup = (knockout as? NSPopUpButton) ?? knockout.flatMap { uiViews($0, as: NSPopUpButton.self).first }
    try require(popup?.isEnabled == true, "\(tag) the knockout popup is enabled on CMYK")
    if let cancel = buttons(options.view).first(where: { $0.keyEquivalent == "\u{1b}" }) {
        try uiSend(cancel, "\(tag) Blending Options Cancel")
    } else { options.dismiss(nil) }
    try pumpUntil({ options.view.window?.isVisible != true && !BackgroundWork.occupied }, seconds: 20)

    // Drop Shadow… opens the sheet on that effect, turned on.
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    let item = NSMenuItem(title: "Drop Shadow…", action: nil, keyEquivalent: "")
    item.tag = LayerStyleEffectKind.allCases.firstIndex(of: .dropShadow) ?? 0
    controller.layerStyleEffect(item)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? LayerStyleSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Layer Style sheet") }
    let effectBoxes = uiViews(sheet.view, as: NSButton.self).filter { box in
        box.title.isEmpty && box.target.map { String(describing: type(of: $0)) == "StyleListRow" } == true
    }
    // Every effect has its row on CMYK, Pattern Overlay's included.
    try require(effectBoxes.count == LayerStyleEffectKind.allCases.count && effectBoxes.allSatisfy(\.isEnabled),
                "\(tag) the ten effect checkboxes are enabled: \(effectBoxes.map(\.isEnabled))")
    try requireCmykBlendIf(sheet, tag: tag)

    // The Drop Shadow previews with no refusal.
    try pumpUntil({ controller.canvas.previewImage != nil }, seconds: 30)
    let status = try uiControl(sheet.view, id: "layer-style.status", as: NSTextField.self)
    try require(status.stringValue.isEmpty, "\(tag) the Drop Shadow preview reports no refusal: \(status.stringValue)")

    // OK: one "Layer Style" step whose drop shadow stores an ink.
    let entries = document.history.entries.count
    guard let ok = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no OK button")
    }
    try uiSend(ok, "\(tag) Layer Style OK")
    try pumpUntil({
        document.history.entries.count == entries + 1 && sheet.view.window?.isVisible != true
            && !BackgroundWork.occupied
    }, seconds: 30)
    try require(document.undoManager?.undoActionName == "Layer Style",
                "\(tag) one \"Layer Style\" step: \(document.history.entries.map(\.name))")
    let stored = document.doc?.layerStylePayload(1)
    try require(stored?.dropShadows.first?.enabled == true && stored?.dropShadows.first?.ink?.count == 4,
                "\(tag) the stored drop shadow carries an ink: \(String(describing: stored?.dropShadows))")
    try require(uiValidates(controller, "copyLayerStyle:") && uiValidates(controller, "clearLayerStyle:"),
                "\(tag) Copy and Clear Layer Style validate on a styled CMYK layer")
    controller.copyLayerStyle(nil)
    try require(uiValidates(controller, "pasteLayerStyle:"), "\(tag) pasteLayerStyle: validates after a copy")
    print("PASS: \(tag) the Layer Style sheet edits effects in ink on CMYK")
}

/// The sheet's Blending Options pane (selected through its checklist row when another pane is
/// showing) has the Blend If heading and a channel popup of Gray and the four plates.
func requireCmykBlendIf(_ sheet: NSViewController, tag: String) throws {
    let heading = { uiViews(sheet.view, as: NSTextField.self).contains { $0.stringValue == "Blend If" } }
    if !heading() {
        guard let row = uiViews(sheet.view, as: NSView.self)
                .first(where: { String(describing: type(of: $0)) == "StyleListRow" }),
              let click = NSEvent.mouseEvent(with: .leftMouseDown, location: .zero, modifierFlags: [],
                                             timestamp: 0, windowNumber: sheet.view.window?.windowNumber ?? 0,
                                             context: nil, eventNumber: 0, clickCount: 1, pressure: 1)
        else { throw ProbeFailure(description: "\(tag) no Blending Options row") }
        row.mouseDown(with: click)
        try pumpUntil(heading, seconds: 10)
    }
    try require(heading(), "\(tag) the Blend If heading on CMYK")
    let titles = uiViews(sheet.view, as: NSPopUpButton.self).map(\.itemTitles)
    try require(titles.contains(BlendIf.cmykTitles),
                "\(tag) the Blend If channels are \(BlendIf.cmykTitles): \(titles)")
}
