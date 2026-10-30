import AppKit

// The 32-bit float VIEW surfaces, through the sheets' own controls:
// View ▸ 32-bit Exposure…, Image ▸ Tone Map…'s live preview and File ▸
// Export…'s float rows. None of the three is an edit until Tone Map's Apply.

private func floatViewBytes(_ image: CGImage?) -> Data? { image?.dataProvider?.data as Data? }

private func floatViewSize(_ image: CGImage?) -> NSSize? {
    image.map { NSSize(width: $0.width, height: $0.height) }
}

/// The app-modal exposure card's Cancel, by its key equivalent.
private func floatViewPress(_ root: NSView, key: String, _ what: String) throws {
    guard let button = buttons(root).first(where: { $0.keyEquivalent == key }) else {
        throw ProbeFailure(description: "\(what): the button is missing")
    }
    try uiSend(button, what)
}

/// "view exposure sheet": the exposure card previews live through the
/// display-only proof slot, Cancel puts the opening exposure back, Done keeps
/// the slider's, and none of it is an edit; a held image of another size is
/// never on screen across a canvas resize.
func verifyViewExposureSheet() throws {
    let tag = "[view exposure sheet]"
    let byte = try fixture(width: 64, height: 48)
    let onByte: Bool
    do {
        defer { byte.close() }
        onByte = uiValidates(try editor(byte), "showViewExposure:")
    }
    try require(!onByte, "\(tag) 32-bit Exposure… does not validate on an 8-bit document")

    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    try pumpUntil({ document.retainedPrecisionProjection != nil && canvas.image != nil },
                  seconds: 20)
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) a float fixture")
    }
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count
    try require(uiValidates(controller, "showViewExposure:"),
                "\(tag) 32-bit Exposure… validates on a float document")
    try require(!uiValidates(controller, "resetViewExposure:"),
                "\(tag) Reset 32-bit Exposure does not validate at 0 stops")
    try require(canvas.proofImage == nil && !controller.viewExposureDisplayed,
                "\(tag) the slot is empty at 0 stops")

    // Cancel: −2 typed in the field previews, and the opening exposure comes back.
    var previewed = false
    let cancelled = ModalSteps(title: "32-bit Exposure", steps: [
        ("type -2", { window in
            guard let card = window.contentView else { return false }
            let field = try uiControl(card, id: "view-exposure-stops", as: NSTextField.self)
            field.stringValue = "-2"
            try uiSend(field, "\(tag) the stops field")
            try require(controller.viewExposure.stops == -2,
                        "\(tag) the typed stops reach the window: \(controller.viewExposure.stops)")
            return true
        }),
        ("the preview lands, then Cancel", { window in
            guard controller.viewExposureDisplayed, let proof = canvas.proofImage else {
                return false
            }
            try require(floatViewSize(proof) == floatViewSize(canvas.image),
                        "\(tag) the exposed image is the canvas image's size")
            try require(floatViewBytes(proof) != floatViewBytes(canvas.image),
                        "\(tag) the image at -2 stops differs from the document's display")
            previewed = true
            guard let card = window.contentView else { return false }
            try floatViewPress(card, key: "\u{1b}", "\(tag) Cancel")
            return true
        }),
    ])
    let steps = try uiRecorded {
        try cancelled.run { controller.showViewExposure(nil) }
        try require(previewed, "\(tag) the -2 preview was seen")
        try require(controller.viewExposure.stops == 0 && canvas.proofImage == nil
                    && controller.viewExposure.shown == nil,
                    "\(tag) Cancel puts 0 stops back and empties the slot: "
                    + "\(controller.viewExposure.stops)")

        // Done: the slider at −3 is kept.
        let kept = ModalSteps(title: "32-bit Exposure", steps: [
            ("slider at -3", { window in
                guard let card = window.contentView else { return false }
                let slider = try uiControl(card, id: "view-exposure-slider", as: NSSlider.self)
                slider.doubleValue = -3
                try uiSend(slider, "\(tag) the slider")
                let field = try uiControl(card, id: "view-exposure-stops", as: NSTextField.self)
                try require(field.stringValue == ViewExposure.text(-3),
                            "\(tag) the field follows the slider: \(field.stringValue)")
                return true
            }),
            ("the preview lands, then Done", { window in
                guard controller.viewExposureDisplayed, canvas.proofImage != nil,
                      let card = window.contentView else { return false }
                let done = try uiControl(card, id: "view-exposure-done", as: NSButton.self)
                try uiSend(done, "\(tag) Done")
                return true
            }),
        ])
        try kept.run { controller.showViewExposure(nil) }
    }
    try require(controller.viewExposure.stops == -3 && controller.viewExposureStops == -3,
                "\(tag) Done keeps -3 stops: \(controller.viewExposure.stops)")
    try require(controller.viewExposureStatusText == "Exposure \(ViewExposure.text(-3)) EV"
                && controller.viewExposureStatusText == "Exposure -3.00 EV",
                "\(tag) the status segment: \(controller.viewExposureStatusText ?? "nil")")
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "\(tag) the exposure is no edit: no step, no undo, not dirty")
    try require(steps.isEmpty, "\(tag) no Action step: \(steps.map(\.tool))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    try require(uiValidates(controller, "resetViewExposure:"),
                "\(tag) Reset 32-bit Exposure validates at -3 stops")
    try pumpUntil({ controller.viewExposureDisplayed && canvas.proofImage != nil }, seconds: 20)

    // A resize: the held image of the old size is never up over the new canvas.
    let (width, height) = (base.width + 16, base.height + 8)
    _ = try agentOK("canvas_size", document,
                    ["width": width, "height": height, "anchor": "top-left"])
    var ticks = 0
    let deadline = Date().addingTimeInterval(30)
    while !(canvas.image?.width == width && canvas.image?.height == height
            && controller.viewExposureDisplayed && canvas.proofImage != nil) {
        if let proof = canvas.proofImage {
            try require(floatViewSize(proof) == floatViewSize(canvas.image),
                        "\(tag) tick \(ticks): a \(proof.width) × \(proof.height) exposed image "
                        + "over a \(canvas.image?.width ?? 0) × \(canvas.image?.height ?? 0) canvas")
        }
        try require(Date() < deadline, "\(tag) the exposure is displayed again after the resize")
        ticks += 1
        RunLoop.main.run(until: Date().addingTimeInterval(0.002))
    }
    try require(floatViewSize(canvas.proofImage) == NSSize(width: width, height: height),
                "\(tag) the exposed image is redrawn at \(width) × \(height)")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) the resize undoes to the base document")
    try pumpUntil({ canvas.image?.width == base.width && controller.viewExposureDisplayed },
                  seconds: 30)

    // A render that fails: the slot is cleared, not left under the new name.
    _ = try RasterResources.shared.report(evict: true)
    let free = try RasterResources.shared.admit(scratch: 0)
    var failed = false
    var outcome = "no ballast"
    if free > 1 {
        let ballast = try PrecisionEditPayload.admitted(bytes: free - 1)
        try withExtendedLifetime(ballast) {
            try controller.setViewExposure(-1)
            let end = Date().addingTimeInterval(10)
            while Date() < end {
                if canvas.proofImage == nil && controller.viewExposure.shown == nil {
                    failed = true
                    break
                }
                if controller.viewExposureDisplayed { break }
                RunLoop.main.run(until: Date().addingTimeInterval(0.005))
            }
            outcome = failed ? "refused" : controller.viewExposureDisplayed ? "rendered" : "pending"
        }
    }
    try require(failed, "\(tag) with \(free) B free held as ballast the -1 render fails and "
                + "clears the slot: \(outcome)")
    try require(controller.viewExposure.stops == -1 && !controller.viewExposureDisplayed,
                "\(tag) the failed render leaves -1 stops stored and nothing displayed")
    try controller.setViewExposure(-1)
    try pumpUntil({ controller.viewExposureDisplayed && canvas.proofImage != nil }, seconds: 20)

    controller.resetViewExposure(nil)
    try require(controller.viewExposure.stops == 0 && canvas.proofImage == nil
                && !uiValidates(controller, "resetViewExposure:"),
                "\(tag) Reset 32-bit Exposure returns to 0 stops and empties the slot")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: view exposure sheet — 32-bit Exposure… validates on a float document only; the "
        + "card's field at -2 previews in the proof slot and Cancel puts 0 back; the slider at -3 "
        + "and Done keep \"Exposure -3.00 EV\" with no step, undo, dirty flag, Action or alert; "
        + "across a canvas resize (\(ticks) ticks) no exposed image of another size is up; a "
        + "render refused for memory clears the slot; Reset returns to 0")
}

/// The Tone Map sheet now presented on `controller`.
private func floatViewToneMapSheet(_ controller: EditorViewController,
                                   _ tag: String) throws -> ToneMapSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showToneMap(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? ToneMapSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Tone Map sheet") }
    return sheet
}

/// "tone map sheet preview": the sheet's popup sets a candidate map the window
/// draws in the proof slot; Cancel clears it, Apply commits one step.
func verifyToneMapSheetPreview() throws {
    let tag = "[tone map sheet preview]"
    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    try pumpUntil({ document.retainedPrecisionProjection != nil && canvas.image != nil },
                  seconds: 20)
    guard let base = document.doc, let current = base.toneMap,
          let other = ToneMapOperator.allCases.first(where: { $0 != current.operator }),
          let index = ToneMapOperator.allCases.firstIndex(of: other) else {
        throw ProbeFailure(description: "\(tag) a float fixture with a tone map")
    }
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count
    var wanted = current
    wanted.operator = other

    func choose(_ sheet: ToneMapSheetController) throws {
        guard let popup = uiViews(sheet.view, as: NSPopUpButton.self).first else {
            throw ProbeFailure(description: "\(tag) the sheet has no Map popup")
        }
        try require(popup.itemTitles == ToneMapOperator.allCases.map(\.displayName),
                    "\(tag) the popup lists the operators: \(popup.itemTitles)")
        popup.selectItem(at: index)
        try uiSend(popup, "\(tag) the Map popup")
        try require(controller.viewExposure.candidateToneMap == wanted,
                    "\(tag) the candidate is \(other.rawValue): "
                    + "\(String(describing: controller.viewExposure.candidateToneMap))")
        try pumpUntil({ controller.viewExposureDisplayed && canvas.proofImage != nil },
                      seconds: 20)
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) the candidate preview leaves the document untouched")
    }

    let steps = try uiRecorded {
        let first = try floatViewToneMapSheet(controller, tag)
        try choose(first)
        try floatViewPress(first.view, key: "\u{1b}", "\(tag) Cancel")
        try pumpUntil({ first.view.window?.isVisible != true }, seconds: 30)
        try require(controller.viewExposure.candidateToneMap == nil && canvas.proofImage == nil
                    && controller.viewExposure.shown == nil,
                    "\(tag) Cancel clears the candidate and empties the slot")
        try require(document.doc === base && document.history.entries.count == entries
                    && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                    "\(tag) Cancel changed nothing")
    }
    try require(steps.isEmpty, "\(tag) Cancel recorded nothing: \(steps.map(\.tool))")

    let applied = try uiRecorded {
        let second = try floatViewToneMapSheet(controller, tag)
        try choose(second)
        try floatViewPress(second.view, key: "\r", "\(tag) Apply")
        try pumpUntil({ second.view.window?.isVisible != true }, seconds: 30)
    }
    try require(document.doc !== base && document.doc?.toneMap == wanted,
                "\(tag) Apply set the map: \(String(describing: document.doc?.toneMap))")
    try require(controller.viewExposure.candidateToneMap == nil,
                "\(tag) the candidate is cleared after Apply")
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Tone Map",
                "\(tag) one \"Tone Map\" step: \(document.undoManager?.undoActionName ?? "-")")
    try require(applied.count == 1 && applied[0].tool == "set_tone_map",
                "\(tag) one recorded set_tone_map step: "
                + "\(applied.map { "\($0.tool) \($0.arguments)" })")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
    try pumpUntil({ canvas.proofImage == nil }, seconds: 10)
    print("PASS: tone map sheet preview — choosing \(other.displayName) in the Tone Map sheet "
        + "sets a candidate the window draws in the proof slot with the document untouched; "
        + "Cancel clears it with no step; Apply sets the map as one \"Tone Map\" undo step and one "
        + "recorded set_tone_map step, and one undo restores")
}

/// The export sheet now presented on `controller`.
private func floatViewExportSheet(_ controller: EditorViewController,
                                  _ tag: String) throws -> ExportWorkspaceSheetController {
    controller.showExportWorkspace(nil)
    let opened = {
        (controller.presentedViewControllers ?? [])
            .compactMap { $0 as? ExportWorkspaceSheetController }.last
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Export sheet") }
    return sheet
}

/// The bytes of the result the sheet would write, by reflection over its
/// private `encoded` slot (`fillReview`'s way): no product-only API.
private func floatViewEncodedBytes(_ sheet: ExportWorkspaceSheetController) -> Int? {
    guard let slot = Mirror(reflecting: sheet).children.first(where: { $0.label == "encoded" })?
            .value,
          let result = Mirror(reflecting: slot).children.first?.value as? RasterExportResult,
          let info = try? result.info() else { return nil }
    return (info["bytes"] as? NSNumber)?.intValue
}

/// "float export rows": the export sheet offers OpenEXR and Radiance on a
/// float document only, pins their depth, and encodes a real preview.
func verifyFloatExportRows() throws {
    let tag = "[float export rows]"
    let floatOnly = ExportFormat.allCases.enumerated().filter { $0.element.isFloatOnly }
    try require(floatOnly.count == 2, "\(tag) two float-only formats: \(floatOnly.count)")
    let alerts = AutomationAlerts.recorded.count

    func press(_ sheet: ExportWorkspaceSheetController, key: String, _ what: String) throws {
        try floatViewPress(sheet.view, key: key, what)
        try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 30)
    }

    // 8 bits: the two rows are hidden.
    let byte = try fixture(width: 64, height: 48)
    do {
        defer { byte.close() }
        let sheet = try floatViewExportSheet(try editor(byte), tag)
        let format = try uiControl(sheet.view, id: "export.format", as: NSPopUpButton.self)
        try require(floatOnly.allSatisfy { format.item(at: $0.offset)?.isHidden == true },
                    "\(tag) OpenEXR and Radiance are hidden on an 8-bit document")
        try press(sheet, key: "\u{1b}", "\(tag) Cancel (8-bit)")
    }

    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) a float fixture")
    }
    let entries = document.history.entries.count
    var sizes: [String: Int] = [:]
    let steps = try uiRecorded {
        let sheet = try floatViewExportSheet(controller, tag)
        let format = try uiControl(sheet.view, id: "export.format", as: NSPopUpButton.self)
        let save = try uiControl(sheet.view, id: "export.save", as: NSButton.self)
        let popups = uiViews(sheet.view, as: NSPopUpButton.self)
        guard let depth = popups.first(where: { $0.itemTitles == ["8-bit", "16-bit", "32-bit float"] }),
              let samples = popups.first(where: { $0.itemTitles == ["32-bit float", "16-bit half"] })
        else { throw ProbeFailure(description: "\(tag) the depth and float-samples popups") }
        try require(floatOnly.allSatisfy { format.item(at: $0.offset)?.isHidden == false },
                    "\(tag) OpenEXR and Radiance are offered on a float document")
        let status = {
            uiViews(sheet.view, as: NSTextField.self).map(\.stringValue)
                .first { $0.hasSuffix(" encoded") }
        }
        /// Selects `title` in `popup`, sends its action and waits for the encode.
        func choose(_ popup: NSPopUpButton, _ title: String) throws -> (text: String, bytes: Int) {
            guard popup.itemTitles.contains(title) else {
                throw ProbeFailure(description: "\(tag) no \"\(title)\" row: \(popup.itemTitles)")
            }
            popup.selectItem(withTitle: title)
            try uiSend(popup, "\(tag) choosing \(title)")
            try require(!save.isEnabled, "\(tag) Export… waits for the \(title) encode")
            try pumpUntil({ save.isEnabled && status() != nil }, seconds: 60)
            guard let text = status(), let bytes = floatViewEncodedBytes(sheet) else {
                throw ProbeFailure(description: "\(tag) \(title): no encoded result")
            }
            return (text, bytes)
        }

        let exr = try choose(format, ExportFormat.exr.displayName)
        try require(depth.titleOfSelectedItem == "32-bit float" && !depth.isEnabled,
                    "\(tag) OpenEXR pins the depth popup to 32-bit float, disabled")
        try require(samples.isEnabled && samples.titleOfSelectedItem == "32-bit float",
                    "\(tag) OpenEXR enables the float-samples popup")
        try require(exr.text.contains("32-bit")
                    && exr.text.hasPrefix("\(base.width)×\(base.height) px"),
                    "\(tag) the OpenEXR status names the size and 32-bit: \(exr.text)")
        sizes["exr f32"] = exr.bytes
        let half = try choose(samples, "16-bit half")
        try require(half.bytes < exr.bytes,
                    "\(tag) half samples encode to fewer bytes: \(half.bytes) against \(exr.bytes)")
        sizes["exr f16"] = half.bytes

        let hdr = try choose(format, ExportFormat.hdr.displayName)
        try require(!samples.isEnabled && depth.titleOfSelectedItem == "32-bit float"
                    && !depth.isEnabled,
                    "\(tag) Radiance pins 32-bit and disables the float-samples popup")
        sizes["hdr"] = hdr.bytes

        let tiff = try choose(format, ExportFormat.tiff.displayName)
        try require(depth.isEnabled && depth.titleOfSelectedItem == "32-bit float"
                    && samples.isEnabled,
                    "\(tag) TIFF at 32 bits enables the depth and float-samples popups")
        sizes["tiff f32"] = tiff.bytes
        let tiffHalf = try choose(samples, "16-bit half")
        try require(samples.titleOfSelectedItem == "16-bit half" && tiffHalf.bytes < tiff.bytes,
                    "\(tag) a TIFF takes half samples: \(tiffHalf.bytes) against \(tiff.bytes)")
        sizes["tiff f16"] = tiffHalf.bytes
        try press(sheet, key: "\u{1b}", "\(tag) Cancel")
    }
    try require(steps.isEmpty, "\(tag) no Action step: \(steps.map(\.tool))")
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "\(tag) the sheet changed nothing")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    let report = sizes.sorted { $0.key < $1.key }.map { "\($0.key) \($0.value) B" }
        .joined(separator: ", ")
    print("PASS: float export rows — OpenEXR and Radiance are hidden on an 8-bit document and "
        + "offered on a float one; OpenEXR and Radiance pin the depth to 32-bit float, half "
        + "samples are an OpenEXR's and a TIFF's and encode smaller (\(report)); Cancel leaves no "
        + "step, dirty flag, Action or alert. The write behind the save panel is not reached")
}
