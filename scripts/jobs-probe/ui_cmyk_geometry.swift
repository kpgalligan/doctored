import AppKit

// The CMYK document's phase-4b in-process UI cases (#547): the geometry items of the Image, Edit,
// Layer and Filter menus validate on a CMYK document, with Transform Selection since the channels
// batch and Auto-Align and Reduce Noise from Stack too, ⌘T keeps its one-entry rule, and the
// Move, Crop, Perspective Crop and Ruler tools are selectable; one ⌘T session on a CMYK layer
// and one on its selected pixels each commit as a single undo step that keeps the document
// CMYK. The CMYK profile is macOS's Generic CMYK Profile, read from the system.

private let genericCMYKProfile = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh 8-bit fixture converted to CMYK through Image ▸ Mode's commit.
private func cmykGeometryFixture(_ tag: String) throws -> (ImageDocument, EditorViewController) {
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: genericCMYKProfile), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else { throw ProbeFailure(description: "\(tag) not CMYK") }
    return (document, controller)
}

/// "cmyk geometry menus" (module doc).
func verifyCmykGeometryMenus() throws {
    let tag = "[cmyk geometry menus]"
    let (document, controller) = try cmykGeometryFixture(tag)
    defer { document.close() }
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Two") }
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 1, name: "Three") }
    try require(document.doc?.layerCount == 3 && document.doc?.isCmyk == true, "\(tag) three CMYK layers")
    controller.setActiveLayer(0)
    _ = try agentOK("add_layer_mask", document, ["layer": 0, "kind": "reveal_all"])
    // A committed ⌘T gives Transform Again its memory.
    controller.freeTransform(nil)
    controller.transformScaleXPercent = 90
    controller.transformScaleYPercent = 90
    try require(controller.commitTransformSession(), "\(tag) a ⌘T on the CMYK layer commits")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
    controller.setActiveLayer(0)
    _ = try agentOK("select_rect", document, ["x": 8, "y": 8, "width": 30, "height": 20])
    for selector in ["resizeImage:", "showCanvasSize:", "cropToSelection:", "showTrim:", "revealAllCanvas:",
                     "rotateCW:", "rotateCCW:", "rotate180:", "rotateArbitrary:", "flipH:", "flipV:",
                     "freeTransform:", "freeTransformCopy:", "transformAgain:", "transformAgainWithCopy:",
                     "showPersistedWarp:", "transformMask:", "showPuppetWarp:", "showPerspectiveWarp:",
                     "showLiquify:"] {
        try require(CmykDocument.selectors.contains(selector), "\(tag) \(selector) is in the CMYK list")
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }
    try require(uiValidates(controller, "transformSelection:"), "\(tag) Transform Selection validates on CMYK")
    _ = try agentOK("set_selected_layers", document, ["layers": [0, 1]])
    for selector in ["alignLeft:", "alignCenterX:", "alignRight:", "alignTop:", "alignCenterY:", "alignBottom:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates with two layers")
    }
    try require(uiValidates(controller, "autoAlignLayers:"), "\(tag) Auto-Align validates on CMYK")
    try require(uiValidates(controller, "showReduceNoiseFromStack:"),
                "\(tag) Reduce Noise from Stack validates on CMYK")
    _ = try agentOK("set_selected_layers", document, ["layers": [0, 1, 2]])
    for selector in ["distributeHorizontally:", "distributeVertically:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates with three layers")
    }
    // ⌘T with two unrelated layers and no selection: the one-entry rule holds on CMYK.
    _ = try agentOK("deselect", document)
    _ = try agentOK("set_selected_layers", document, ["layers": [1, 2]])
    try require(!uiValidates(controller, "freeTransform:"), "\(tag) ⌘T needs one entry with no selection")
    for tool in [EditorTool.move, .crop, .perspectiveCrop, .ruler] {
        try require(DocumentPrecision.allows(tool, document: document),
                    "\(tag) \(tool.displayName) is selectable on CMYK")
    }
    print("PASS: \(tag) the geometry items and tools, Transform Selection, Auto-Align and Reduce "
        + "Noise from Stack validate on CMYK; ⌘T keeps its one-entry rule")
}

/// The composite's alpha at canvas `(x, y)` (`sample_color`'s ink reply).
private func inkAlpha(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> Int {
    let reply = try agentOK("sample_color", document, ["x": x, "y": y])
    guard let alpha = reply["alpha16"] as? Int else {
        throw ProbeFailure(description: "no alpha16 in \(reply)")
    }
    return alpha
}

/// "cmyk transform session" (module doc): (a) ⌘T on the layer, (b) ⌘T on selected pixels.
func verifyCmykTransformSession() throws {
    let tag = "[cmyk transform session]"
    let (document, controller) = try cmykGeometryFixture(tag)
    defer { document.close() }
    document.undoManager?.removeAllActions()

    // (a) The layer at 50 %: one step, bounds halved, still CMYK; undo restores.
    guard let before = document.doc?.layerBounds(0) else { throw ProbeFailure(description: "\(tag) bounds") }
    let entries = document.history.entries.count
    controller.freeTransform(nil)
    guard let session = controller.transformSession, session.floating == nil else {
        throw ProbeFailure(description: "\(tag) ⌘T opened no layer session")
    }
    controller.transformScaleXPercent = 50
    controller.transformScaleYPercent = 50
    try require(controller.commitTransformSession(), "\(tag) the layer ⌘T committed")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
    try require(document.history.entries.count == entries + 1, "\(tag) the layer ⌘T is one step")
    guard let after = document.doc?.layerBounds(0) else { throw ProbeFailure(description: "\(tag) bounds") }
    try require(document.doc?.isCmyk == true, "\(tag) still CMYK after the layer ⌘T")
    try require(after.width == before.width / 2 && after.height == before.height / 2,
                "\(tag) the bounds halved: \(before) → \(after)")
    document.undoManager?.undo()
    try require(document.doc?.layerBounds(0).map { $0 == before } == true,
                "\(tag) undo restores the bounds")

    // (b) The selected pixels at 50 %: the floating session, one step, still CMYK, the vacated
    // part of the selection transparent.
    _ = try select(document, side: 16)
    let steps = document.history.entries.count
    controller.freeTransform(nil)
    guard let floating = controller.transformSession, floating.floating != nil else {
        throw ProbeFailure(description: "\(tag) ⌘T opened no floating session")
    }
    controller.transformScaleXPercent = 50
    controller.transformScaleYPercent = 50
    try require(controller.commitTransformSession(), "\(tag) the selected-pixels ⌘T committed")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
    try require(document.history.entries.count == steps + 1, "\(tag) the selected-pixels ⌘T is one step")
    try require(document.undoManager?.undoActionName == TransformSelectedPixels.actionName,
                "\(tag) \"\(TransformSelectedPixels.actionName)\": \(document.undoManager?.undoActionName ?? "-")")
    try require(document.doc?.isCmyk == true, "\(tag) still CMYK after the selected-pixels ⌘T")
    // The 16 × 16 selection at (24, 16) lands as 8 × 8 at (28, 20): its corner is vacated.
    let vacated = try inkAlpha(document, 25, 17)
    let landed = try inkAlpha(document, 32, 24)
    try require(vacated == 0, "\(tag) the vacated corner is transparent: alpha \(vacated)")
    try require(landed > 0, "\(tag) the landing holds ink")
    print("PASS: \(tag) ⌘T on a CMYK layer and on its selected pixels is one step each and stays CMYK")
}
