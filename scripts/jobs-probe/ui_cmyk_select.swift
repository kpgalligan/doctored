import AppKit

// The CMYK document's selection-batch in-process UI cases (#547): the Select, Edit and Layer
// menu items a CMYK document now enables (Color Range, Select and Mask, Subject / Objects /
// People / Face Parts, Stroke…, Apply Layer Mask), the four canvas tools it admits (the channels
// batch's Select items and tools are `ui_cmyk_channels.swift`'s), and Edit ▸ Stroke…'s sheet, whose Apply is one
// "Stroke" step recorded in ink. The CMYK profile is macOS's Generic CMYK Profile.

private let genericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh fixture converted to CMYK through Image ▸ Mode's commit.
private func cmykDocument(_ tag: String) throws -> (ImageDocument, EditorViewController) {
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: genericCMYK), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else {
        document.close()
        throw ProbeFailure(description: "\(tag) not CMYK")
    }
    return (document, controller)
}

/// "cmyk selection menus": with a selection and a layer mask, the batch's items validate on a
/// CMYK document, and the Magic Wand, Quick Selection, Object Selection and Select Subject tools
/// are selectable.
func verifyCmykSelectionMenus() throws {
    let tag = "[cmyk selection menus]"
    let (document, controller) = try cmykDocument(tag)
    defer { document.close() }
    _ = try agentOK("select_rect", document, ["x": 8, "y": 8, "width": 30, "height": 20])
    _ = try agentOK("add_layer_mask", document, ["layer": 0, "kind": "reveal_all"])
    for selector in ["showColorRange:", "refineSelection:", "selectSubject:", "showSubjectTargets:",
                     "showPeopleTargets:", "selectPeople:", "showFaceParts:", "strokeSelection:",
                     "applyLayerMask:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }
    for tool in [EditorTool.wand, .quickSelect, .objectSelect, .subject] {
        try require(DocumentPrecision.allows(tool, document: document),
                    "\(tag) \(tool.displayName) is selectable on CMYK")
    }
    print("PASS: \(tag) the selection items and tools validate on CMYK")
}

/// "cmyk stroke sheet": Edit ▸ Stroke… on a CMYK document's selection opens the sheet, and its
/// Apply is one "Stroke" step that keeps the document CMYK, changes the layer's ink and records
/// `stroke_selection` with `ink` (C, M, Y, K percent) and no colour.
func verifyCmykStrokeSheet() throws {
    let tag = "[cmyk stroke sheet]"
    let (document, controller) = try cmykDocument(tag)
    defer { document.close() }
    controller.setPaintInk(CmykInk(c: 0, m: 0, y: 0, k: 100), color: .black)
    _ = try agentOK("select_rect", document, ["x": 10, "y": 10, "width": 30, "height": 20])
    try require(uiValidates(controller, "strokeSelection:"), "\(tag) Stroke… validates")
    let base = document.doc
    let entries = document.history.entries.count
    _ = try agentOK("start_recording", document, ["discard": true])
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.strokeSelection(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? StrokeSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Stroke sheet") }
    guard let ok = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no OK button")
    }
    try uiSend(ok, "\(tag) OK")
    try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                    && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    let stopped = try agentOK("stop_recording", document)
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Stroke",
                "\(tag) one \"Stroke\" step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    let steps = (stopped["steps"] as? [[String: Any]]) ?? []
    let strokes = steps.filter { $0["tool"] as? String == "stroke_selection" }
    guard strokes.count == 1, let arguments = strokes[0]["arguments"] as? [String: Any] else {
        throw ProbeFailure(description: "\(tag) one stroke_selection step: \(steps)")
    }
    let ink = (arguments["ink"] as? [NSNumber])?.map(\.doubleValue) ?? []
    try require(ink.count == 4 && arguments["color"] == nil,
                "\(tag) the step records ink, not a colour: \(arguments)")
    print("PASS: \(tag) the Stroke sheet applies one \"Stroke\" step recorded in ink")
}
