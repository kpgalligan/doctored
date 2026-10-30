import AppKit

/// The remaining 8-bit composite readers — the Info readout and
/// `sample_pixel`, Copy Merged, the eyedropper and `sample_color`, Print,
/// `SelectionCapture` (Color Range) — read `byteComposite(of:)`, and a render
/// failure is the core's sentence; the typed Evaluation readout reads the
/// retained typed projection.

private func storedValue(_ subject: Any, _ label: String) -> Any? {
    Mirror(reflecting: subject).children.first(where: { $0.label == label })?.value
}

private func footnoteText(_ panel: InfoPanelViewController) throws -> String {
    guard let footnote = storedValue(panel, "footnote") as? NSTextField else {
        throw ProbeFailure(description: "Info panel footnote")
    }
    return footnote.stringValue
}

/// Chooses the Info panel's source popup item `tag` the way a click does.
private func chooseInfoSource(_ panel: InfoPanelViewController, tag: Int) throws {
    guard let popup = storedValue(panel, "sourcePopup") as? NSPopUpButton,
          let action = popup.action else {
        throw ProbeFailure(description: "Info panel source popup")
    }
    popup.selectItem(withTag: tag)
    NSApp.sendAction(action, to: popup.target, from: popup)
}

private func maxAbsOf(_ difference: [String: Any]) -> Int? {
    (difference["max_abs"] as? NSNumber)?.intValue
}

func verifyMoreCompositeReaders() throws {
    let document = try fixture()
    let domain = UserDefaults.argumentDomain
    let savedArguments = UserDefaults.standard.volatileDomain(forName: domain)
    defer {
        UserDefaults.standard.setVolatileDomain(savedArguments, forName: domain)
        document.close()
    }
    let controller = try editor(document)
    guard let panel = controller.infoPanel else { throw ProbeFailure(description: "Info panel") }
    _ = panel.view
    _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 8, "y": 8])
    document.setLayerSelection(.single(0))
    guard let doc = document.doc, let projection = document.retainedByteProjection else {
        throw ProbeFailure(description: "No retained 8-bit projection")
    }

    // (a) Baseline: every reader gets the retained projection.
    let readout = try PixelReadout.at((x: 5, y: 5), in: document, reach: 0)
    let sampled = projection.sample(x: 5, y: 5, reach: 0)
    try require(readout != nil && readout?.r == sampled?.r && readout?.g == sampled?.g
                    && readout?.b == sampled?.b && readout?.a == sampled?.a,
                "(a) PixelReadout.at equals the projection sample")
    let offCanvas = try agentCall("sample_pixel", document, ["x": 5000, "y": 5000])
    try require(offCanvas.isError && offCanvas.text.contains("No part of the"),
                "(a) off-canvas sample_pixel: \(offCanvas.text)")
    try require(controller.mergedCopySource() === projection, "(a) mergedCopySource is retained")
    try require(controller.eyedropperComposite() === projection, "(a) eyedropperComposite is retained")
    let baselineCapture = try SelectionCapture(document: document)
    try require(baselineCapture.source === projection, "(a) SelectionCapture's source is retained")
    _ = try document.printOperation(withSettings: [:])

    // (b) Budget: every reader answers the core's sentence.
    var arguments = savedArguments
    arguments["RenderGroupPixelBudget"] = 1
    UserDefaults.standard.setVolatileDomain(arguments, forName: domain)
    let grouped = try agentOK("group_layers", document, ["layers": [0]])
    guard let group = grouped["group"] as? Int else {
        throw ProbeFailure(description: "(b) group_layers returned no group: \(grouped)")
    }
    _ = try agentOK("set_layer_properties", document, ["index": group, "blend_mode": "normal"])
    document.setLayerSelection(.single(0))
    guard let sentence = document.projectionError, sentence.contains("budget"),
          document.retainedByteProjection == nil, let failing = document.doc else {
        throw ProbeFailure(description: "(b) the projection failed on the budget: "
            + "\(document.projectionError ?? "nil")")
    }
    guard let text = (0..<failing.layerCount).first(where: { document.layerDescribesSource($0) })
    else { throw ProbeFailure(description: "(b) no text layer") }
    do {
        _ = try PixelReadout.at((x: 5, y: 5), in: document, reach: 0)
        throw ProbeFailure(description: "(b) PixelReadout.at did not throw")
    } catch let error as ProbeFailure {
        throw error
    } catch {
        try require(error.localizedDescription.contains(sentence),
                    "(b) PixelReadout.at throws S: \(error.localizedDescription)")
    }
    let mcpReads: [(String, [String: Any])] = [
        ("sample_pixel", ["x": 1, "y": 1]), ("sample_color", ["x": 1, "y": 1]),
        ("copy_merged", [:]), ("select_color_range", ["range": "highlights"]),
    ]
    for (tool, args) in mcpReads {
        let alerts = AutomationAlerts.recorded.count
        let reply = try agentCall(tool, document, args)
        try require(reply.isError && reply.text.contains(sentence),
                    "(b) MCP \(tool) answers S: \(reply.text)")
        try require(AutomationAlerts.recorded.count == alerts, "(b) MCP \(tool) presented no alert")
    }
    let changeCount = Clipboard.pasteboard.changeCount
    var alerts = AutomationAlerts.recorded.count
    controller.copyMerged(nil)
    var added = AutomationAlerts.recorded.suffix(from: alerts)
    try require(added.count == 1 && added.first?.messageText.contains(sentence) == true,
                "(b) UI Copy Merged presented S once: \(Array(added))")
    try require(Clipboard.pasteboard.changeCount == changeCount, "(b) UI Copy Merged wrote nothing")
    panel.lastPixel = (2, 2)
    panel.setSelectionState(bounds: nil, area: nil)
    try require(panel.readoutFailure?.contains(sentence) == true,
                "(b) the Info readout carries S: \(panel.readoutFailure ?? "nil")")
    try require(panel.valueLabels[.rgb]?.stringValue == "—", "(b) the rgb row reads —")
    let failedFootnote = try footnoteText(panel)
    try require(failedFootnote.contains(sentence), "(b) the footnote carries S: \(failedFootnote)")
    let sampleText = controller.lastSampleText
    alerts = AutomationAlerts.recorded.count
    controller.canvas.onEyedropper?(CGPoint(x: 2.5, y: 2.5))
    try require(controller.lastSampleText == sampleText && AutomationAlerts.recorded.count == alerts,
                "(b) the eyedropper sampled nothing and presented nothing")
    do {
        _ = try document.printOperation(withSettings: [:])
        throw ProbeFailure(description: "(b) printOperation did not throw")
    } catch let error as ProbeFailure {
        throw error
    } catch {
        try require(error.localizedDescription.contains(sentence),
                    "(b) printOperation throws S: \(error.localizedDescription)")
    }
    do {
        _ = try SelectionCapture(document: document)
        throw ProbeFailure(description: "(b) SelectionCapture did not throw")
    } catch let error as ProbeFailure {
        throw error
    } catch {
        try require(error.localizedDescription.contains(sentence),
                    "(b) SelectionCapture throws S: \(error.localizedDescription)")
    }
    // The failure footnote does not outlive a hand-off to the scoped readout.
    try chooseInfoSource(panel, tag: 1)
    let layerFootnote = try footnoteText(panel)
    try require(panel.readoutFailure == nil && !layerFootnote.contains(sentence),
                "(b) a layer source clears the composite failure footnote: \(layerFootnote)")

    // (c) Stale: the budget is lifted while the stored error is still set.
    UserDefaults.standard.setVolatileDomain(savedArguments, forName: domain)
    try require(document.projectionError != nil, "(c) the stored error is still set")
    let staleReadout = try PixelReadout.at((x: 5, y: 5), in: document, reach: 0)
    try require(staleReadout != nil, "(c) PixelReadout.at renders past the stale error")
    let staleCapture = try SelectionCapture(document: document)
    try require(staleCapture.source != nil, "(c) SelectionCapture renders past the stale error")
    try require(controller.eyedropperComposite() != nil, "(c) eyedropperComposite renders")
    _ = try document.printOperation(withSettings: [:])
    controller.canvas.onEyedropper?(CGPoint(x: 2.5, y: 2.5))
    try require(controller.lastSampleText != sampleText, "(c) the eyedropper sampled")
    try chooseInfoSource(panel, tag: 0)
    let compositeFootnote = try footnoteText(panel)
    try require(panel.readoutFailure == nil && !compositeFootnote.contains(sentence),
                "(c) the composite source's refresh clears the failure: \(compositeFootnote)")

    // (d) 16 bits: the typed readout reads the projection it is handed.
    let deep = try precisionFixture(width: 64, height: 48)
    defer { deep.close() }
    _ = try editor(deep)
    guard let deepDoc = deep.doc, let halved = try deepDoc.withLayerOpacity(0, 0.5) else {
        throw ProbeFailure(description: "(d) 16-bit opacity edit")
    }
    let handed = try AgentServer.projectionDifference(
        deepDoc, deepDoc, beforeProjection: nil,
        beforePrecisionProjection: try halved.renderPrecision())
    let rendered = try AgentServer.projectionDifference(
        deepDoc, deepDoc, beforeProjection: nil, beforePrecisionProjection: nil)
    try require((maxAbsOf(handed) ?? 0) > 0 && maxAbsOf(rendered) == 0,
                "(d) projectionDifference reads beforePrecisionProjection: \(handed), \(rendered)")
    let evaluation = try agentOK("get_evaluation", deep, ["preview": true])
    try require(evaluation["difference"] != nil, "(d) get_evaluation has a difference: \(evaluation)")
    _ = try deep.printOperation(withSettings: [:])

    // (e) Budget with a text-layer target: the composite fails before any
    // rasterize prompt, so the description survives.
    UserDefaults.standard.setVolatileDomain(arguments, forName: domain)
    alerts = AutomationAlerts.recorded.count
    let applied = try agentCall("apply_image", document,
                                ["layer": text, "source": "merged", "blend_mode": "multiply"])
    try require(applied.isError && applied.text.contains(sentence),
                "(e) MCP apply_image answers S: \(applied.text)")
    try require(AutomationAlerts.recorded.count == alerts && document.layerDescribesSource(text),
                "(e) MCP apply_image presented nothing and kept the description")
    var parameters = ApplyImageParameters()
    parameters.blend = DR_BLEND_MULTIPLY
    parameters.targetLayer = text
    controller.applyImage(parameters)
    added = AutomationAlerts.recorded.suffix(from: alerts)
    try require(added.count == 1 && added.first?.messageText.contains(sentence) == true,
                "(e) UI Apply Image presented S once and no prompt: \(Array(added))")
    try require(document.layerDescribesSource(text), "(e) UI Apply Image kept the description")
    print("PASS: more composite readers — Info readout, sample_pixel, Copy Merged, the eyedropper, "
        + "sample_color, Print and SelectionCapture read the composite and answer the core's "
        + "sentence; a stale stored error is ignored; the typed readout reads its handed "
        + "projection")
}
