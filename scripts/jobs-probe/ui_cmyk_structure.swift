import AppKit

// The CMYK document's phase-4a in-process UI case (#1): the structure menus (groups, clipping,
// merges, Stamp Visible, Blending Options) validate on a CMYK document, the Layers panel's blend
// popup enables exactly `NativeLayerBlending.cmykModes`, Blending Options shows Fill Opacity and
// the knockout rows with every effect row available and Blend If on Gray and the four plates (the
// styles and Blend If batches, #1; the effects themselves are "cmyk layer style"'s) and previews
// a Fill change with no refusal, and ⌘G
// groups as one step. The CMYK profile is macOS's Generic CMYK
// Profile, read from the system.

private let genericCMYKProfile = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh 8-bit fixture converted to CMYK through Image ▸ Mode's commit.
private func cmykFixture(_ controller: EditorViewController, _ document: ImageDocument, _ tag: String) throws {
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: genericCMYKProfile), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else { throw ProbeFailure(description: "\(tag) not CMYK") }
}

/// "cmyk structure" (module doc).
func verifyCmykStructure() throws {
    let tag = "[cmyk structure]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    try cmykFixture(controller, document, tag)
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Two") }
    try require(document.doc?.layerCount == 2 && document.doc?.isCmyk == true,
                "\(tag) a second CMYK layer")
    controller.setActiveLayer(1)
    for selector in ["groupLayers:", "toggleClippingMask:", "mergeDown:", "mergeVisible:",
                     "stampVisible:", "layerStyle:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }

    // The Layers panel's blend popup enables exactly the CMYK modes.
    controller.layersPanel.updateHeaderControls()
    guard let popup = uiViews(controller.layersPanel.view, as: NSPopUpButton.self).first(where: {
        $0.itemArray.contains { $0.title == "Multiply" }
    }) else { throw ProbeFailure(description: "\(tag) no blend popup") }
    let enabled = Set(popup.itemArray.filter { !$0.isSeparatorItem && $0.isEnabled }.map(\.tag))
    let expected = Set(NativeLayerBlending.cmykModes.map { Int($0.rawValue) })
    try require(enabled == expected, "\(tag) the blend popup enables \(enabled.sorted()), not \(expected.sorted())")

    // Blending Options: Fill Opacity and knockout, every effect available, a Fill change previews.
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.layerStyle(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? LayerStyleSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Layer Style sheet") }
    // The checklist's effect checkboxes: titleless, each targeting its row.
    let effectBoxes = uiViews(sheet.view, as: NSButton.self).filter { box in
        box.title.isEmpty && box.target.map { String(describing: type(of: $0)) == "StyleListRow" } == true
    }
    // Every effect has its row on CMYK, Pattern Overlay's included.
    try require(effectBoxes.count == LayerStyleEffectKind.allCases.count,
                "\(tag) the ten effect checkboxes: \(effectBoxes.count)")
    try require(effectBoxes.allSatisfy(\.isEnabled), "\(tag) every effect row is available on CMYK")
    let identifiers = uiViews(sheet.view, as: NSView.self).compactMap(\.identifier)
    try require(identifiers.contains(LayerStyleKnockoutRows.knockoutIdentifier)
                && identifiers.contains(LayerStyleKnockoutRows.blendInteriorIdentifier),
                "\(tag) the knockout rows on CMYK")
    try requireCmykBlendIf(sheet, tag: tag)
    guard let fill = uiViews(sheet.view, as: NSSlider.self).first(where: { $0.minValue == 0 && $0.maxValue == 100 })
    else { throw ProbeFailure(description: "\(tag) no Fill Opacity slider") }
    controller.canvas.previewImage = nil
    fill.doubleValue = 50
    try uiSend(fill, "\(tag) Fill 50 %")
    try pumpUntil({ controller.canvas.previewImage != nil }, seconds: 30)
    let status = try uiControl(sheet.view, id: "layer-style.status", as: NSTextField.self)
    try require(status.stringValue.isEmpty, "\(tag) the Fill preview reports no refusal: \(status.stringValue)")
    if let cancel = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }) {
        try uiSend(cancel, "\(tag) Layer Style Cancel")
    } else { sheet.dismiss(nil) }
    try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied }, seconds: 20)

    // A Fill Opacity style: Clear Layer Style validates.
    var filled = LayerStyle()
    filled.fillOpacity = 0.5
    document.applyEdit("Fill", record: .notACommand) { try $0.withLayerStylePayload(1, filled) }
    try require(document.doc?.layerHasStyle(1) == true, "\(tag) a Fill Opacity style on CMYK")
    try require(uiValidates(controller, "clearLayerStyle:"), "\(tag) clearLayerStyle: validates on CMYK")

    // ⌘G groups as one step and the document stays CMYK.
    let entries = document.history.entries.count
    controller.setActiveLayer(1)
    controller.groupLayers(nil)
    try pumpUntil({ document.history.entries.count == entries + 1 && !BackgroundWork.occupied }, seconds: 20)
    try require(document.undoManager?.undoActionName == "Group Layers",
                "\(tag) one \"Group Layers\" step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true && document.doc?.layerIsGroup(document.activeLayerIndex) == true,
                "\(tag) a CMYK group")
    try require(uiValidates(controller, "ungroupLayers:"), "\(tag) ungroupLayers: validates on the group")
    print("PASS: \(tag) the structure menus, blend popup and Blending Options work on CMYK")
}
