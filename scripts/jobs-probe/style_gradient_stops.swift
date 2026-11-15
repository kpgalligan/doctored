import AppKit

// The Layer Style panes' gradient stops (#18): the Gradient Overlay, the Outer Glow's gradient Fill
// and the Stroke's gradient fill each hold the shared stop editor (`GradientStopsPane`), which edits
// EVERY stop. On an 8-bit and a 16-bit document, for each effect: the sheet opened on it shows the
// three stops the layer's style holds; recolouring the inner stop and pressing OK stores that stop
// (the read-back is the style `get_document` reports, `AgentServer.layerStyleFields`) with the
// gradient's other keys unchanged; a second sheet's library preset stores its four stops, and on the
// Gradient Overlay and the Stroke its Smoothness. A colour glow switched to Fill: Gradient shows the
// two seeded stops. On a CMYK document a recoloured glow stop carries the new colour's ink, and the
// stop left alone keeps its ink exactly (`LayerStyle.mappingColourSlots`).

private let gradientStopsCMYKProfile =
    URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

func verifyStyleGradientStops() throws {
    for preset in try GradientPresetLibrary.listing().entries {
        try GradientPresetLibrary.delete(name: preset.item.name)
    }
    let four = [
        GradientStop(position: 0, color: "#000000", opacity: 1),
        GradientStop(position: 0.3, color: "#ff0000", opacity: 1),
        GradientStop(position: 0.7, color: "#00ff00", opacity: 0.5),
        GradientStop(position: 1, color: "#0000ff", opacity: 1),
    ]
    let preset = try GradientPresetLibrary.save(
        name: "Probe Four", stops: four, smoothness: 0.4, replace: false)
    defer { try? GradientPresetLibrary.delete(name: "Probe Four") }
    // Every part runs, so one run names each part that fails.
    var parts: [() throws -> Void] = []
    for sixteen in [false, true] {
        for kind in [LayerStyleEffectKind.gradientOverlay, .outerGlow, .stroke] {
            parts.append { try styleStopsCase(kind, sixteen: sixteen, preset: preset) }
        }
        parts.append { try styleStopsGlowSeed(sixteen: sixteen) }
    }
    parts.append(styleStopsCmykGlow)
    var failures: [String] = []
    for part in parts {
        do { try part() } catch { failures.append(String(describing: error)) }
        // A failed part may leave a preview worker running; let it end first.
        try? pumpUntil({ !BackgroundWork.occupied }, seconds: 10)
    }
    try require(failures.isEmpty, failures.joined(separator: " ;; "))
    print("PASS: the Gradient Overlay, glow and Stroke panes edit every stop at 8 and 16 bits, and a "
        + "CMYK glow stop takes its new ink")
}

private let threeStops = [
    GradientStop(position: 0, color: "#102030", opacity: 1),
    GradientStop(position: 0.4, color: "#c04080", opacity: 0.8),
    GradientStop(position: 1, color: "#f0e0a0", opacity: 1),
]

/// A style whose `kind` effect holds the three stops and gradient keys off their defaults.
private func threeStopStyle(_ kind: LayerStyleEffectKind) -> LayerStyle {
    var gradient = GradientFill()
    gradient.stops = threeStops
    var style = LayerStyle()
    switch kind {
    case .gradientOverlay:
        gradient.style = "radial"
        gradient.angle = 30
        gradient.scale = 0.8
        gradient.reverse = true
        var effect = GradientOverlayEffect()
        effect.gradient = gradient
        style.gradientOverlays = [effect]
    case .stroke:
        gradient.angle = 45
        gradient.scale = 1.2
        var effect = StrokeEffect()
        effect.fillType = "gradient"
        effect.size = 4
        effect.gradient = gradient
        style.strokes = [effect]
    default:
        var effect = OuterGlowEffect()
        effect.fillType = "gradient"
        effect.size = 6
        effect.gradient = gradient
        style.outerGlow = effect
    }
    return style
}

/// `kind`'s gradient in `style`.
private func effectGradient(_ style: LayerStyle?, _ kind: LayerStyleEffectKind) -> GradientFill? {
    switch kind {
    case .gradientOverlay: return style?.gradientOverlays.first?.gradient
    case .stroke: return style?.strokes.first?.gradient
    default: return style?.outerGlow?.gradient
    }
}

/// The document a case runs on: an 8-bit or a 16-bit RGB one, 48 × 40, with a second layer
/// holding an opaque square.
private func stopsDocument(sixteen: Bool) throws -> ImageDocument {
    let document = sixteen ? try precisionFixture(width: 48, height: 40) : try fixture(width: 48, height: 40)
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Styled") }
    document.undoManager?.removeAllActions()
    return document
}

/// Opens layer 1's Layer Style sheet on `kind` and answers it with its pane's stop editor.
private func openSheet(_ controller: EditorViewController, _ kind: LayerStyleEffectKind, tag: String)
    throws -> (LayerStyleSheetController, GradientStopsPane)
{
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.editLayerStyle(1, initial: kind)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? LayerStyleSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Layer Style sheet") }
    let panes = uiViews(sheet.view, as: GradientStopsPane.self)
    guard panes.count == 1, let pane = panes.first else {
        throw ProbeFailure(description: "\(tag) the pane holds one stop editor: \(panes.count)")
    }
    try require(!pane.isHiddenOrHasHiddenAncestor, "\(tag) the stop editor is showing")
    return (sheet, pane)
}

/// Presses the sheet's OK and waits for the one Layer Style step.
private func pressOK(_ sheet: LayerStyleSheetController, _ document: ImageDocument, tag: String) throws {
    let entries = document.history.entries.count
    guard let ok = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no OK button")
    }
    try uiSend(ok, "\(tag) OK")
    try pumpUntil({
        document.history.entries.count == entries + 1 && sheet.view.window?.isVisible != true
            && !BackgroundWork.occupied
    }, seconds: 30)
}

private func styleStopsCase(_ kind: LayerStyleEffectKind, sixteen: Bool, preset: GradientPreset) throws {
    let tag = "[style gradient stops \(kind.rawValue) \(sixteen ? 16 : 8)]"
    let document = try stopsDocument(sixteen: sixteen)
    defer { document.close() }
    let controller = try editor(document)
    let style = threeStopStyle(kind)
    document.applyEdit("Style", record: .notACommand) { try $0.withLayerStylePayload(1, style) }
    document.undoManager?.removeAllActions()
    guard let before = effectGradient(document.doc?.layerStylePayload(1), kind) else {
        throw ProbeFailure(description: "\(tag) the style was not set")
    }

    // Pass 1: recolour the inner stop.
    let (sheet, pane) = try openSheet(controller, kind, tag: tag)
    try require(pane.fill.stops.map(\.color) == threeStops.map(\.color),
                "\(tag) the editor shows the three stops: \(pane.fill.stops.map(\.color))")
    pane.selectedStopIndex = 1
    pane.pick(NSColor(srgbRed: 0, green: 0.5, blue: 1, alpha: 1))
    try pressOK(sheet, document, tag: tag)
    let stored = document.doc?.layerStylePayload(1)
    guard let after = effectGradient(stored, kind) else {
        throw ProbeFailure(description: "\(tag) the effect is gone after OK")
    }
    try require(after.stops.count == 3 && after.stops[1].color == "#0080ff"
                && after.stops[0].color == threeStops[0].color
                && after.stops[2].color == threeStops[2].color
                && abs(after.stops[1].position - 0.4) < 1e-9 && abs(after.stops[1].opacity - 0.8) < 1e-9,
                "\(tag) OK stored the recoloured inner stop: \(after.stops)")
    var expected = before
    expected.stops = after.stops
    try require(after == expected, "\(tag) the gradient's other keys are unchanged: \(after)")
    // The agent's read of the same style.
    let fields = AgentServer.layerStyleFields(document.doc, 1)
    let agentStops = agentGradientStops(fields, kind)
    try require(agentStops.count == 3 && agentStops[1]["color"] as? String == "#0080ff",
                "\(tag) the agent's style read holds the recoloured stop: \(agentStops)")

    // Pass 2: a library preset's four stops (and Smoothness where the effect keeps one).
    let (second, presetPane) = try openSheet(controller, kind, tag: tag)
    presetPane.applyPreset(preset)
    try pressOK(second, document, tag: tag)
    guard let replaced = effectGradient(document.doc?.layerStylePayload(1), kind) else {
        throw ProbeFailure(description: "\(tag) the effect is gone after the preset")
    }
    try require(replaced.stops.map(\.color) == preset.stops.map(\.color),
                "\(tag) the preset's four stops are stored: \(replaced.stops.map(\.color))")
    if kind != .outerGlow {
        try require(abs((replaced.smoothness ?? 1) - preset.smoothness) < 1e-6,
                    "\(tag) the preset's Smoothness is stored: \(String(describing: replaced.smoothness))")
        try require(replaced.style == before.style && replaced.angle == before.angle
                    && replaced.reverse == before.reverse,
                    "\(tag) the preset left Style, Angle and Reverse: \(replaced)")
    }
}

/// The stops of `kind`'s gradient in the agent's style object.
private func agentGradientStops(_ fields: [String: Any]?, _ kind: LayerStyleEffectKind) -> [[String: Any]] {
    guard let effects = fields?["effects"] as? [[String: Any]],
          let effect = effects.first(where: { $0["type"] as? String == kind.rawValue }),
          let gradient = effect["gradient"] as? [String: Any] else { return [] }
    return gradient["stops"] as? [[String: Any]] ?? []
}

/// A colour Outer Glow switched to Fill: Gradient shows the two seeded stops in its colour.
private func styleStopsGlowSeed(sixteen: Bool) throws {
    let tag = "[style gradient stops glow seed \(sixteen ? 16 : 8)]"
    let document = try stopsDocument(sixteen: sixteen)
    defer { document.close() }
    let controller = try editor(document)
    var style = LayerStyle()
    var glow = OuterGlowEffect()
    glow.color = "#40c020"
    style.outerGlow = glow
    document.applyEdit("Style", record: .notACommand) { try $0.withLayerStylePayload(1, style) }
    document.undoManager?.removeAllActions()
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.editLayerStyle(1, initial: .outerGlow)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? LayerStyleSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened(), let pane = uiViews(sheet.view, as: GradientStopsPane.self).first,
          let fill = uiViews(sheet.view, as: NSPopUpButton.self)
            .first(where: { $0.itemTitles == ["Color", "Gradient"] })
    else { throw ProbeFailure(description: "\(tag) the glow pane's Fill popup and stop editor") }
    try require(pane.isHiddenOrHasHiddenAncestor, "\(tag) a colour glow hides the stop editor")
    fill.selectItem(withTitle: "Gradient")
    try uiSend(fill, "\(tag) Fill: Gradient")
    try require(!pane.isHiddenOrHasHiddenAncestor, "\(tag) Fill: Gradient shows the stop editor")
    try require(pane.fill.stops.map(\.color) == ["#40c020", "#40c020"]
                && pane.fill.stops.map(\.opacity) == [1, 0],
                "\(tag) the seed's two stops: \(pane.fill.stops)")
    try pressOK(sheet, document, tag: tag)
    let stored = document.doc?.layerStylePayload(1)?.outerGlow
    try require(stored?.fillType == "gradient" && stored?.gradient?.stops.count == 2,
                "\(tag) OK stored the seeded gradient: \(String(describing: stored))")
}

/// A CMYK glow whose stops carry inks: the recoloured stop takes the new colour's ink; the
/// others keep theirs exactly.
private func styleStopsCmykGlow() throws {
    let tag = "[style gradient stops cmyk glow]"
    let document = try fixture(width: 48, height: 40)
    defer { document.close() }
    let controller = try editor(document)
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: gradientStopsCMYKProfile),
                               destination: [:], options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else { throw ProbeFailure(description: "\(tag) not CMYK") }
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Styled") }
    let inks: [[Double]] = [[0.1, 0.2, 0.3, 0.05], [0.6, 0.1, 0.0, 0.0], [0.0, 0.7, 0.2, 0.1]]
    var style = threeStopStyle(.outerGlow)
    if var glow = style.outerGlow, var gradient = glow.gradient {
        for i in gradient.stops.indices { gradient.stops[i].ink = inks[i] }
        glow.gradient = gradient
        style.outerGlow = glow
    }
    document.applyEdit("Style", record: .notACommand) { try $0.withLayerStylePayload(1, style) }
    document.undoManager?.removeAllActions()
    let before = document.doc?.layerStylePayload(1)?.outerGlow?.gradient?.stops ?? []
    try require(before.map(\.ink) == inks.map { Optional($0) },
                "\(tag) the stops hold their inks: \(before.map(\.ink))")
    let (sheet, pane) = try openSheet(controller, .outerGlow, tag: tag)
    pane.selectedStopIndex = 1
    pane.pick(NSColor(srgbRed: 1, green: 1, blue: 0, alpha: 1))
    try pressOK(sheet, document, tag: tag)
    let after = document.doc?.layerStylePayload(1)?.outerGlow?.gradient?.stops ?? []
    try require(after.count == 3, "\(tag) three stops after OK: \(after)")
    try require(after[0].ink == inks[0] && after[2].ink == inks[2],
                "\(tag) the stops left alone keep their inks: \(after.map(\.ink))")
    guard let moved = after[1].ink else {
        throw ProbeFailure(description: "\(tag) the recoloured stop has no ink")
    }
    try require(moved != inks[1] && moved[2] > 0.5 && moved[0] < 0.2,
                "\(tag) the recoloured stop carries yellow's ink, not its old one: \(moved)")
}
