import AppKit

// The CMYK document's phase-4c in-process UI case (#1): with a selection, Cut, Copy, Copy Merged,
// Layer Via Copy / Cut and Duplicate to Document… validate on a CMYK document, and after ⌘C so do
// Paste, Paste in Place, Paste Into and Paste Outside; ⌘V lands ONE "Paste Layer" step whose ink
// is exactly the copied ink (the composite at the copied spot is unchanged) and the document stays
// CMYK; Stamp Selected validates with two layers selected; ⌘E on a group reads "Merge Group" and
// makes one "Merge Group" step; and the Duplicate to Document sheet lists an open RGB document,
// whose window takes the layers as one "Paste Layers (CMYK → RGB)" step. The runner's private
// pasteboard (`DR_PASTEBOARD_NAME`) is the clipboard. The CMYK profile is macOS's Generic CMYK
// Profile, read from the system.

private let clipboardGenericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// The composite's ink16 and alpha16 at one point.
private func compositeInk(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Int] {
    guard let precision = document.doc?.precision else {
        throw ProbeFailure(description: "[cmyk clipboard] no typed document")
    }
    let context = try PrecisionWorkContext(source: precision)
    let sample = try precision.cmykSample(x: x, y: y, reach: 0, workingLimit: context.limit,
                                          otherRetained: context.otherRetained)
    let ink = ((sample["ink16"] as? [NSNumber]) ?? []).map(\.intValue)
    return ink + [(sample["alpha16"] as? NSNumber)?.intValue ?? -1]
}

/// "cmyk clipboard" (module doc).
func verifyCmykClipboard() throws {
    let tag = "[cmyk clipboard]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: clipboardGenericCMYK),
                               destination: [:], options: CmykConversionOptions(), document: document)
    try require(document.doc?.isCmyk == true, "\(tag) the fixture is CMYK")
    _ = try select(document, side: 16)
    for selector in ["cut:", "copy:", "copyMerged:", "layerViaCopy:", "layerViaCut:",
                     "duplicateToNewDocument:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }
    let before = try compositeInk(document, 32, 24)

    // ⌘C writes a CMYK payload; the four paste items validate.
    controller.copy(nil)
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
    try require(TypedClipboardPayload.isAvailable, "\(tag) ⌘C wrote a typed payload")
    for selector in ["paste:", "pasteInPlace:", "pasteInto:", "pasteOutside:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }

    // ⌘V: one "Paste Layer" step, the copied ink exactly, the document still CMYK.
    var entries = document.history.entries.count
    controller.paste(nil)
    try pumpUntil({ document.history.entries.count == entries + 1 && !BackgroundWork.occupied },
                  seconds: 20)
    try require(document.undoManager?.undoActionName == "Paste Layer",
                "\(tag) one \"Paste Layer\" step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true && document.doc?.layerCount == 2,
                "\(tag) a second CMYK layer")
    let after = try compositeInk(document, 32, 24)
    try require(after == before, "\(tag) the pasted ink is the copied ink exactly: \(after) vs \(before)")

    // Stamp Selected validates with two layers selected.
    document.setLayerSelection(LayerSelection(primary: 1, others: [0, 1]))
    try require(uiValidates(controller, "stampSelected:"), "\(tag) stampSelected: validates")

    // ⌘E on a group reads "Merge Group" and makes one step.
    document.setLayerSelection(LayerSelection(primary: 1, others: [1]))
    controller.setActiveLayer(1)
    entries = document.history.entries.count
    controller.groupLayers(nil)
    try pumpUntil({ document.history.entries.count == entries + 1 && !BackgroundWork.occupied },
                  seconds: 20)
    try require(document.doc?.layerIsGroup(document.activeLayerIndex) == true, "\(tag) a CMYK group")
    let item = NSMenuItem(title: "", action: #selector(EditorViewController.mergeDown(_:)),
                          keyEquivalent: "")
    try require(controller.validateUserInterfaceItem(item) && item.title == "Merge Group",
                "\(tag) ⌘E reads \"Merge Group\" on a group: \(item.title)")
    entries = document.history.entries.count
    controller.mergeDown(nil)
    try pumpUntil({ document.history.entries.count == entries + 1 && !BackgroundWork.occupied },
                  seconds: 20)
    try require(document.undoManager?.undoActionName == "Merge Group",
                "\(tag) one \"Merge Group\" step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true && document.doc?.layerIsGroup(document.activeLayerIndex) == false,
                "\(tag) the group became one CMYK layer")

    // Duplicate to Document…: the sheet lists an open RGB document and Apply pastes there.
    let rgb = try fixture(width: 64, height: 48)
    defer { rgb.close() }
    _ = try editor(rgb)
    document.setLayerSelection(LayerSelection(primary: 1, others: [0, 1]))
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.duplicateToNewDocument(nil)
    let opened = {
        (controller.presentedViewControllers ?? [])
            .compactMap { $0 as? DuplicateToNewDocumentSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened(),
          let popup = uiViews(sheet.view, as: NSPopUpButton.self).first(where: {
              $0.itemTitles.first == "New Document"
          })
    else { throw ProbeFailure(description: "\(tag) no Destination popup") }
    try require(popup.itemTitles.contains(rgb.displayName),
                "\(tag) the Destination lists the open RGB document: \(popup.itemTitles)")
    popup.selectItem(withTitle: rgb.displayName)
    try uiSend(popup, "\(tag) Destination")
    let rgbEntries = rgb.history.entries.count
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no Apply button")
    }
    try uiSend(apply, "\(tag) Duplicate to Document Apply")
    try pumpUntil({ rgb.history.entries.count == rgbEntries + 1 && !BackgroundWork.occupied },
                  seconds: 30)
    try require(rgb.undoManager?.undoActionName.hasPrefix("Paste Layers (CMYK → RGB)") == true,
                "\(tag) one \"Paste Layers (CMYK → RGB)\" step: \(rgb.history.entries.map(\.name))")
    try require(rgb.doc?.isCmyk == false, "\(tag) the destination stays RGB")
    print("PASS: \(tag) Edit and Layer menus copy and paste on CMYK, Merge Group and Duplicate "
        + "to Document work")
}
