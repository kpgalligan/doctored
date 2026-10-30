import AppKit

// The retouch batch's in-process UI cases (#547): the canvas retouch tools a CMYK document admits
// at each depth (`CmykRetouch`'s table, take1's record), Image ▸ Adjustments ▸
// Equalize's prompt on CMYK and its menu validation at every depth, and Dodge/Burn's Protect Tones
// checkbox. The CMYK profile is macOS's Generic CMYK Profile, read from the system.

private let retouchGenericCMYK = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh fixture converted to CMYK through Image ▸ Mode's commit.
private func retouchDocument(_ tag: String) throws -> (ImageDocument, EditorViewController) {
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: retouchGenericCMYK), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else {
        document.close()
        throw ProbeFailure(description: "\(tag) not CMYK")
    }
    return (document, controller)
}

/// The composite's ink16 at `(x, y)` of a CMYK document.
private func retouchInk(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Int] {
    guard let precision = document.doc?.precision else { throw ProbeFailure(description: "no precision") }
    let context = try PrecisionWorkContext(source: precision)
    let sample = try precision.cmykSample(x: x, y: y, reach: 0, workingLimit: context.limit,
                                          otherRetained: context.otherRetained)
    return (sample["ink16"] as? [NSNumber])?.map(\.intValue) ?? []
}

/// "cmyk retouch tools": at 16 and 8 bits every canvas retouch tool is admitted exactly as
/// `CmykRetouch`'s table answers (Dodge/Burn, Sponge, Blur, Sharpen, Smudge, the History Brush, the
/// Clone Stamp, the healing tools, Patch, Remove and Content-Aware Move admitted; Red Eye not), and
/// Edit ▸ Content-Aware Fill… validates on a CMYK layer with a selection, as `content_aware_fill` runs.
func verifyCmykRetouchTools() throws {
    let tag = "[cmyk retouch tools]"
    let (document, controller) = try retouchDocument(tag)
    defer { document.close() }
    let first = CmykAdjustFilter.depth(of: document.doc)
    for depth in [first, first == 8 ? 16 : 8] {
        if depth != first { _ = try agentOK("convert_precision", document, ["mode": "cmyk\(depth)"]) }
        try require(CmykAdjustFilter.depth(of: document.doc) == depth, "\(tag) the document is \(depth)-bit")
        for (tool, _) in CmykRetouch.tools {
            let want = CmykRetouch.allows(tool, depth: depth)
            try require(DocumentPrecision.allows(tool, document: document) == want,
                        "\(tag) \(tool.displayName) admitted \(want) at \(depth) bits")
        }
        for tool in [EditorTool.dodge, .sponge, .clone, .heal, .spotHeal, .smudge, .historyBrush] {
            try require(DocumentPrecision.allows(tool, document: document),
                        "\(tag) \(tool.displayName) runs on ink at \(depth) bits")
        }
        try require(!DocumentPrecision.allows(.redEye, document: document),
                    "\(tag) Red Eye stays refused at \(depth) bits")
        _ = try agentOK("select_rect", document, ["x": 8, "y": 8, "width": 24, "height": 16])
        try require(uiValidates(controller, "contentAwareFill:"),
                    "\(tag) Content-Aware Fill… validates with a selection at \(depth) bits")
        _ = try agentOK("deselect", document)
    }
    print("PASS: \(tag) the canvas retouch tools follow the CMYK table at 16 and 8 bits")
}

/// "cmyk equalize prompt": with a selection, Image ▸ Adjustments ▸ Equalize raises its
/// prompt; automation answers Selected Area Only, which commits ONE "Equalize" step that changes
/// the ink inside the selection, keeps the ink outside it exactly and keeps the document CMYK.
func verifyCmykEqualizePrompt() throws {
    let tag = "[cmyk equalize prompt]"
    let (document, controller) = try retouchDocument(tag)
    defer { document.close() }
    _ = try agentOK("select_rect", document, ["x": 0, "y": 0, "width": 32, "height": 48])
    let inside = [(8, 8), (24, 40)], outside = [(48, 8), (56, 40)]
    let before = try (inside + outside).map { try retouchInk(document, $0.0, $0.1) }
    let entries = document.history.entries.count
    AutomationAlerts.reset()
    controller.equalize(nil)
    try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 30)
    let alerts = AutomationAlerts.recorded
    try require(alerts.contains { $0.messageText == "Equalize" && $0.answer == "Selected Area Only"
                    && $0.buttonTitles.contains("Entire Image Based on Selected Area") },
                "\(tag) the prompt appeared and took Selected Area Only: \(alerts)")
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Equalize",
                "\(tag) one \"Equalize\" step: \(document.history.entries.map(\.name))")
    let after = try (inside + outside).map { try retouchInk(document, $0.0, $0.1) }
    try require(after[0] != before[0] || after[1] != before[1], "\(tag) the selected ink changes")
    try require(Array(after[2...]) == Array(before[2...]), "\(tag) the ink outside is unchanged")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    print("PASS: \(tag) the prompt's Selected Area Only equalizes the selection as one step on ink")
}

/// "equalize menus": Image ▸ Adjustments ▸ Equalize validates on an RGB layer at 8, 16 and 32 bits,
/// on a CMYK layer at 16 and 8 bits and on an 8-bit layer mask target, and not on an adjustment
/// layer.
func verifyEqualizeMenus() throws {
    let tag = "[equalize menus]"
    let rgb = try fixture(width: 64, height: 48)
    defer { rgb.close() }
    let controller = try editor(rgb)
    try require(uiValidates(controller, "equalize:"), "\(tag) an 8-bit layer")
    _ = try agentOK("add_layer_mask", rgb, ["layer": 0])
    controller.setPaintTarget(.mask)
    try require(rgb.planeEditTarget == .mask, "\(tag) the panel targets the mask")
    try require(uiValidates(controller, "equalize:"), "\(tag) an 8-bit layer mask")
    controller.setPaintTarget(.layer)
    _ = try agentOK("add_adjustment_layer", rgb, ["op": "levels", "params": ["gamma": 1.2]])
    let top = (rgb.doc?.layerCount ?? 1) - 1
    _ = try agentOK("set_active_layer", rgb, ["index": top])
    try require(rgb.doc?.layerIsAdjustment(rgb.activeLayerIndex) == true, "\(tag) the adjustment is active")
    // An adjustment layer has no pixels: with its mask removed the panel targets the layer
    // itself, which refuses (a mask, which the panel targets while there is one, equalizes as
    // any mask does).
    _ = try agentOK("remove_layer_mask", rgb, ["layer": top])
    controller.setPaintTarget(.layer)
    try require(rgb.planeEditTarget == .layer, "\(tag) the panel targets the adjustment layer itself")
    try require(!uiValidates(controller, "equalize:"), "\(tag) not on an adjustment layer")
    for mode in ["rgba16", "rgba32"] {
        let document = try fixture(width: 64, height: 48)
        defer { document.close() }
        let typed = try editor(document)
        _ = try agentOK("convert_precision", document, ["mode": mode, "structure": "preserve"])
        try require(uiValidates(typed, "equalize:"), "\(tag) a \(mode) layer")
    }
    let (cmyk, cmykController) = try retouchDocument(tag)
    defer { cmyk.close() }
    let first = CmykAdjustFilter.depth(of: cmyk.doc)
    try require(uiValidates(cmykController, "equalize:"), "\(tag) a \(first)-bit CMYK layer")
    let other = first == 8 ? 16 : 8
    _ = try agentOK("convert_precision", cmyk, ["mode": "cmyk\(other)"])
    try require(CmykAdjustFilter.depth(of: cmyk.doc) == other, "\(tag) the document is \(other)-bit")
    try require(uiValidates(cmykController, "equalize:"), "\(tag) a \(other)-bit CMYK layer")
    print("PASS: \(tag) Equalize validates at RGB 8/16/32, CMYK 8/16 and on a mask, not on an adjustment layer")
}

/// "dodge protect tones option": the Dodge/Burn options show a Protect Tones checkbox; checking it
/// sets the stored option, which survives a tool switch and back.
func verifyDodgeProtectTonesOption() throws {
    let tag = "[dodge protect tones option]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    let saved = ToolOptionsStore.shared.dodge
    defer { ToolOptionsStore.shared.dodge = saved }
    var reset = saved
    reset.protectTones = false
    ToolOptionsStore.shared.dodge = reset
    guard let window = controller.view.window else { throw ProbeFailure(description: "\(tag) no window") }
    // Wide enough that the bar shows every Dodge/Burn cluster rather than folding one into More.
    window.setContentSize(NSSize(width: 2600, height: 700))
    controller.selectTool(.dodge)
    try require(controller.currentTool == .dodge, "\(tag) Dodge / Burn is selected")
    let checkbox = { () -> OptionCheckboxControl? in
        controller.view.layoutSubtreeIfNeeded()
        controller.optionsBar.layoutSubtreeIfNeeded()
        return uiViews(controller.optionsBar, as: OptionCheckboxControl.self)
            .first { $0.identifier?.rawValue == "paint.protectTones" }
    }
    guard let box = checkbox() else { throw ProbeFailure(description: "\(tag) no Protect Tones checkbox") }
    guard let click = NSEvent.mouseEvent(with: .leftMouseDown, location: .zero, modifierFlags: [],
                                         timestamp: 0, windowNumber: window.windowNumber, context: nil,
                                         eventNumber: 0, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "\(tag) no click event")
    }
    box.mouseDown(with: click)
    try require(ToolOptionsStore.shared.dodge.protectTones == true, "\(tag) a click turns the option on")
    controller.selectTool(.brush)
    controller.selectTool(.dodge)
    try require(ToolOptionsStore.shared.dodge.protectTones == true, "\(tag) the option survives a tool switch")
    guard let again = checkbox() else { throw ProbeFailure(description: "\(tag) the checkbox is gone") }
    // The checkbox toggles what it reads: a click on the rebuilt one turns the stored option off,
    // so it read the stored "on".
    again.mouseDown(with: click)
    try require(ToolOptionsStore.shared.dodge.protectTones == false,
                "\(tag) the rebuilt checkbox read the stored option")
    print("PASS: \(tag) the Protect Tones checkbox is shown and persists")
}
