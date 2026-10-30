import AppKit

// Image ▸ Mode ▸ 32 Bits/Channel… and Convert Precision… (698b2d10)
// through the Convert Precision sheet's own controls: 8 → float → 8 and
// 16 → float → 16 are exact round trips, a Screen layer blocks the float plan
// by name in the status line with nothing changed, and the status line
// follows the mode both leaving 32-bit (4dbba7db) and entering it (this
// batch's `choiceChanged(_:)` fix).

/// One private field of the sheet, by reflection (`v1_fixes.swift`'s
/// `sheetControl`).
private func convertField<T>(_ sheet: ConvertPrecisionSheetController, _ label: String,
                             as type: T.Type) throws -> T {
    guard let value = Mirror(reflecting: sheet).children.first(where: { $0.label == label })?.value,
          let typed = value as? T else {
        throw ProbeFailure(description: "Convert Precision sheet field \(label) is missing")
    }
    return typed
}

/// The sheet `open` just presented on `controller`.
private func convertSheet(_ controller: EditorViewController, _ tag: String,
                          _ open: () -> Void) throws -> ConvertPrecisionSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    open()
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? ConvertPrecisionSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else {
        throw ProbeFailure(description: "\(tag) no Convert Precision sheet")
    }
    return sheet
}

private func convertButton(_ sheet: ConvertPrecisionSheetController, _ title: String,
                           _ tag: String) throws -> NSButton {
    guard let button = buttons(sheet.view).first(where: { $0.title == title }) else {
        throw ProbeFailure(description: "\(tag) the sheet has no \(title) button")
    }
    return button
}

/// Chooses Mode popup item `index` through the popup's own action.
private func convertChoose(_ sheet: ConvertPrecisionSheetController, _ index: Int,
                           _ tag: String) throws {
    let popup = try convertField(sheet, "modePopup", as: NSPopUpButton.self)
    popup.selectItem(at: index)
    try uiSend(popup, "\(tag) the Mode popup")
}

private func convertCancel(_ sheet: ConvertPrecisionSheetController, _ tag: String) throws {
    guard let cancel = buttons(sheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }) else {
        throw ProbeFailure(description: "\(tag) the sheet has no Cancel")
    }
    try uiSend(cancel, "\(tag) Cancel")
    try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 30)
}

/// Prepare Preview, then Apply Conversion; answers the recorded steps.
private func convertApply(_ sheet: ConvertPrecisionSheetController, _ document: ImageDocument,
                          _ tag: String) throws -> [ActionStep] {
    try uiRecorded {
        let base = document.doc
        try uiSend(try convertButton(sheet, "Prepare Preview", tag), "\(tag) Prepare Preview")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 60)
        let apply = try convertButton(sheet, "Apply Conversion", tag)
        let status = try convertField(sheet, "statusLabel", as: NSTextField.self)
        try require(apply.isEnabled, "\(tag) Prepare enables Apply: \"\(status.stringValue)\"")
        try uiSend(apply, "\(tag) Apply Conversion")
        try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
}

private func convertRecorded(_ steps: [ActionStep], _ want: [String: String],
                             _ tag: String) throws {
    let same = steps.count == 1 && steps[0].tool == "convert_precision"
        && Set(steps[0].arguments.keys) == Set(want.keys)
        && want.allSatisfy { steps[0].arguments[$0.key] as? String == $0.value }
    try require(same, "\(tag) one recorded convert_precision step \(want): "
                + "\(steps.map { "\($0.tool) \($0.arguments)" })")
}

/// One round trip from `document` into float and back to its own depth.
private func convertRoundTrip(_ document: ImageDocument, deep: Bool, _ tag: String) throws {
    let controller = try editor(document)
    guard let base = document.doc, base.isHighDepth == deep else {
        throw ProbeFailure(description: "\(tag) the fixture's depth")
    }
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count

    // Into float through Image ▸ Mode ▸ 32 Bits/Channel….
    let into = try convertSheet(controller, tag) { controller.convertToFloatPrecision(nil) }
    let popup = try convertField(into, "modePopup", as: NSPopUpButton.self)
    try require(popup.titleOfSelectedItem == "To 32-bit float",
                "\(tag) 32 Bits/Channel… opens on To 32-bit float: \(popup.titleOfSelectedItem ?? "-")")
    let intoSteps = try convertApply(into, document, "\(tag) into float")
    guard let float = document.doc, float.isFloat else {
        throw ProbeFailure(description: "\(tag) Apply made no float document")
    }
    try require(document.history.cursor == entries + 1
                && document.undoManager?.undoActionName == "Convert to 32-bit Float (Preserve Layers)",
                "\(tag) one \"Convert to 32-bit Float (Preserve Layers)\" step: "
                + "\(document.history.entries.map(\.name))")
    try convertRecorded(intoSteps, ["mode": "rgba32", "structure": "preserve"], "\(tag) into float")
    if !deep {
        let bytes = base.layerPixels(0) ?? []
        for (x, y) in [(0, 0), (63, 0), (17, 29), (63, 47)] {
            let i = (y * base.width + x) * 4
            guard bytes.count >= i + 4,
                  let got = try FloatReadout.at((x: x, y: y), document: document)?.values else {
                throw ProbeFailure(description: "\(tag) no pixel at (\(x), \(y))")
            }
            let want = (0..<3).map { uiLinearSRGB(Int(bytes[i + $0])) } + [Double(bytes[i + 3]) / 255]
            try require(zip(got, want).allSatisfy { abs($0 - $1) <= uiLinearSRGBTolerance },
                        "\(tag) (\(x), \(y)) reads the linear sRGB decode: \(got) vs \(want)")
        }
    }

    // Back through Image ▸ Mode ▸ Convert Precision….
    let back = try convertSheet(controller, tag) { controller.convertDocumentPrecision(nil) }
    try convertChoose(back, deep ? 0 : 1, tag)
    let preserve = try convertField(back, "preserveRadio", as: NSButton.self)
    let dither = try convertField(back, "ditherCheck", as: NSButton.self)
    try require(preserve.state == .on && dither.state == .off,
                "\(tag) the way back is Preserve Layers with dithering off")
    let backSteps = try convertApply(back, document, "\(tag) back")
    guard let returned = document.doc, !returned.isFloat else {
        throw ProbeFailure(description: "\(tag) Apply left the float document")
    }
    try require(document.history.cursor == entries + 2,
                "\(tag) one more step: \(document.history.entries.map(\.name))")
    if deep {
        guard let was = base.precision?.layerSource(0), let now = returned.precision?.layerSource(0)
        else { throw ProbeFailure(description: "\(tag) no 16-bit layer source") }
        let before = try was.withRGBA16 { Array($0) }, after = try now.withRGBA16 { Array($0) }
        try require(returned.isHighDepth && before == after,
                    "\(tag) every 16-bit sample is the original")
        try convertRecorded(backSteps, ["mode": "rgba16", "structure": "preserve"], "\(tag) back")
    } else {
        try require(!returned.isHighDepth && returned.layerPixels(0) == base.layerPixels(0),
                    "\(tag) the 8-bit layer is the original bytes")
        try convertRecorded(backSteps, ["mode": "rgba8", "structure": "preserve", "dither": "none"],
                            "\(tag) back")
    }
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === float, "\(tag) one undo returns to the float document")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) a second undo restores the original")
}

/// "convert precision float": the round trips, the blocked Screen layer, and
/// the status line leaving and entering 32-bit.
func verifyConvertPrecisionFloat() throws {
    let tag = "[convert precision float]"
    do {
        let document = try fixture(width: 64, height: 48)
        defer { document.close() }
        try convertRoundTrip(document, deep: false, "\(tag) 8-bit")
    }
    do {
        let document = try precisionFixture(width: 64, height: 48)
        defer { document.close() }
        try convertRoundTrip(document, deep: true, "\(tag) 16-bit")
    }

    // A Screen layer: the float plan refuses it by name.
    let document = try fixture(width: 64, height: 48)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let added = try agentOK("new_layer", document)
    guard let layer = (added["new_layer_index"] as? NSNumber)?.intValue,
          let name = added["name"] as? String else {
        throw ProbeFailure(description: "\(tag) new_layer answered no layer: \(added)")
    }
    _ = try agentOK("set_layer_properties", document, ["layer": layer, "blend_mode": "screen"])
    document.undoManager?.removeAllActions()
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    let prefix = "Preserve Layers cannot convert this document: layer \(layer) \"\(name)\": "
    var atFloat = ""
    let blocked = try uiRecorded {
        let sheet = try convertSheet(controller, tag) { controller.convertToFloatPrecision(nil) }
        let status = try convertField(sheet, "statusLabel", as: NSTextField.self)
        let preserve = try convertField(sheet, "preserveRadio", as: NSButton.self)
        let flatten = try convertField(sheet, "flattenRadio", as: NSButton.self)
        let apply = try convertButton(sheet, "Apply Conversion", tag)
        atFloat = status.stringValue
        // The core's plan names the mode in lowercase ("the screen blend mode").
        try require(atFloat.hasPrefix(prefix)
                    && atFloat.localizedCaseInsensitiveContains("screen blend mode"),
                    "\(tag) the open-at-32 status names the Screen layer: \"\(atFloat)\"")
        try uiSend(try convertButton(sheet, "Prepare Preview", tag), "\(tag) Prepare Preview")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 60)
        try require(status.stringValue.localizedCaseInsensitiveContains("screen blend mode")
                    && !apply.isEnabled,
                    "\(tag) Prepare leaves the Screen refusal and Apply disabled: "
                    + "\"\(status.stringValue)\"")
        // Leaving 32 (4dbba7db): the 8→16 plan admits Screen.
        try convertChoose(sheet, 0, tag)
        try require(status.stringValue.isEmpty && preserve.state == .on && flatten.isEnabled,
                    "\(tag) To 16-bit: empty status, Preserve on, Flatten enabled: "
                    + "\"\(status.stringValue)\"")
        try convertChoose(sheet, 1, tag)
        try require(preserve.state == .on && !flatten.isEnabled && status.stringValue == atFloat,
                    "\(tag) back to float: Preserve on, Flatten disabled and the open-at-32 "
                    + "status: \"\(status.stringValue)\"")
        try convertCancel(sheet, tag)

        // Entering 32 from Convert Precision… (this batch's fix).
        let mode = try convertSheet(controller, tag) { controller.convertDocumentPrecision(nil) }
        let modeStatus = try convertField(mode, "statusLabel", as: NSTextField.self)
        let modePreserve = try convertField(mode, "preserveRadio", as: NSButton.self)
        let modeFlatten = try convertField(mode, "flattenRadio", as: NSButton.self)
        let modeApply = try convertButton(mode, "Apply Conversion", tag)
        let popup = try convertField(mode, "modePopup", as: NSPopUpButton.self)
        try require(popup.titleOfSelectedItem == "To 16-bit" && modePreserve.state == .on
                    && modeStatus.stringValue.isEmpty,
                    "\(tag) Convert Precision… opens on To 16-bit, Preserve on, no status: "
                    + "\(popup.titleOfSelectedItem ?? "-") \"\(modeStatus.stringValue)\"")
        try convertChoose(mode, 1, tag)
        try require(modeStatus.stringValue == atFloat && modePreserve.state == .on
                    && !modeFlatten.isEnabled && !modeApply.isEnabled,
                    "\(tag) choosing To 32-bit float shows the open-at-32 status with Preserve on, "
                    + "Flatten and Apply disabled: \"\(modeStatus.stringValue)\"")
        try convertCancel(mode, tag)
    }
    try require(document.doc === base && document.history.cursor == entries
                && document.undoManager?.canUndo != true && blocked.isEmpty,
                "\(tag) the blocked sheets changed and recorded nothing: "
                + "\(blocked.map(\.tool))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: convert precision float — 32 Bits/Channel… opens on To 32-bit float; Prepare and "
        + "Apply convert an 8-bit document to float as one \"Convert to 32-bit Float (Preserve "
        + "Layers)\" step recorded {rgba32, preserve} whose pixels are the linear sRGB decode, and "
        + "Convert Precision… To 8-bit (Preserve, no dither) returns the original bytes exactly; a "
        + "16-bit document returns every sample; a Screen layer's float plan says \"\(atFloat)\" in "
        + "the status, Prepare keeps it with Apply disabled, To 16-bit clears it and back to float "
        + "restores it, and Convert Precision… shows it on choosing To 32-bit float (the "
        + "choiceChanged fix); nothing blocked changes or records anything")
}
