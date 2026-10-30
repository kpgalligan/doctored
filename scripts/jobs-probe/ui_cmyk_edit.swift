import AppKit

// The CMYK document's phase-2 in-process UI cases (#1): the Color Picker's C/M/Y/K fields — in
// a CMYK document they set the foreground's ink exactly, beside an RGB document they show the
// working CMYK equivalent and convert back to sRGB with no ink kept — the Levels and Curves
// dialogs' channel popups, which list CMYK, Cyan, Magenta, Yellow and Black on a CMYK document,
// and the Hue/Saturation sheet and menus on a CMYK document (owner option (b)).
// The CMYK profile is macOS's Generic CMYK Profile, read from the system.

private let genericCMYKProfile = URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc")

/// A fresh 8-bit fixture converted to CMYK through Image ▸ Mode's commit.
private func cmykFixture(_ controller: EditorViewController, _ document: ImageDocument, _ tag: String) throws {
    controller.commitColorMode(.cmyk, profile: try Data(contentsOf: genericCMYKProfile), destination: [:],
                               options: CmykConversionOptions(), document: document)
    guard document.doc?.isCmyk == true else { throw ProbeFailure(description: "\(tag) not CMYK") }
}

/// Opens the foreground Color Picker and answers it.
private func openPicker(_ controller: EditorViewController, _ tag: String) throws -> ColorPickerSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.presentColorPicker(.foreground)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? ColorPickerSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Color Picker") }
    return sheet
}

/// Types `value` into the picker field `id` and commits it, as Return does.
private func enter(_ sheet: ColorPickerSheetController, _ id: String, _ value: Int, _ tag: String) throws {
    let field = try uiControl(sheet.view, id: id, as: NSTextField.self)
    field.integerValue = value
    try uiSend(field, "\(tag) \(id) = \(value)")
}

private func okButton(_ sheet: ColorPickerSheetController, _ tag: String) throws -> NSButton {
    guard let ok = buttons(sheet.view).first(where: { $0.title == "OK" }) else {
        throw ProbeFailure(description: "\(tag) no OK button")
    }
    return ok
}

/// "cmyk picker": a CMYK document's picker shows C/M/Y/K; typing them sets the foreground's ink
/// EXACTLY (kept in the shared state, its sRGB the swatch), an RGB-side edit clears it, swap
/// carries it to the background and reset clears both; beside an RGB document the fields show
/// the working CMYK equivalent, editing them converts the colour, and no ink is kept.
func verifyCmykPicker() throws {
    let tag = "[cmyk picker]"
    WorkingCMYK.install()
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    controller.resetColors()
    // RGB document: the working-CMYK equivalent, converted back.
    let rgb = try openPicker(controller, tag)
    let cyanRGB = try uiControl(rgb.view, id: "picker-cmyk-c", as: NSTextField.self)
    try require(cyanRGB.stringValue != "", "\(tag) an RGB document's picker shows the working CMYK C")
    for (id, value) in [("picker-cmyk-c", 100), ("picker-cmyk-m", 0), ("picker-cmyk-y", 0),
                        ("picker-cmyk-k", 0)] {
        try enter(rgb, id, value, tag)
    }
    try uiSend(try okButton(rgb, tag), "\(tag) OK on RGB")
    try pumpUntil({ rgb.view.window?.isVisible != true }, seconds: 10)
    try require(ToolOptionsStore.shared.sharedState.foregroundInk == nil,
                "\(tag) an RGB document keeps no ink")
    let srgb = controller.paintColor.usingColorSpace(.sRGB)
    try require((srgb?.redComponent ?? 1) < 0.5 && (srgb?.blueComponent ?? 0) > 0.5,
                "\(tag) C 100 K 0 converts to a cyan colour: \(String(describing: srgb))")
    // CMYK document: the ink exactly.
    try cmykFixture(controller, document, tag)
    controller.resetColors()
    let sheet = try openPicker(controller, tag)
    for (id, value) in [("picker-cmyk-c", 37), ("picker-cmyk-m", 12), ("picker-cmyk-y", 80),
                        ("picker-cmyk-k", 5)] {
        try enter(sheet, id, value, tag)
    }
    try uiSend(try okButton(sheet, tag), "\(tag) OK on CMYK")
    try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 10)
    let picked = ToolOptionsStore.shared.sharedState.foregroundInk
    try require(picked == CmykInk(c: 37, m: 12, y: 80, k: 5),
                "\(tag) the foreground's ink is the typed ink: \(String(describing: picked))")
    let display = try RasterPrecisionDocument.srgb(fromInk: CmykInk(c: 37, m: 12, y: 80, k: 5),
                                                    document: document.doc?.precision)
    let swatch = controller.paintColor.usingColorSpace(.sRGB)
    try require(abs((swatch?.greenComponent ?? 0) - display[1]) < 2.0 / 255,
                "\(tag) the swatch is the ink's sRGB: \(String(describing: swatch)) vs \(display)")
    // An RGB-side edit clears the ink.
    let again = try openPicker(controller, tag)
    let cyan = try uiControl(again.view, id: "picker-cmyk-c", as: NSTextField.self)
    try require(cyan.integerValue == 37, "\(tag) the picker reopens on the stored ink's C: \(cyan.stringValue)")
    guard let hex = uiViews(again.view, as: NSTextField.self).first(where: {
        $0.isEditable && $0.stringValue.count == 6 && $0.stringValue.allSatisfy(\.isHexDigit)
    }) else { throw ProbeFailure(description: "\(tag) no hex field") }
    hex.stringValue = "FF0000"
    try uiSend(hex, "\(tag) hex FF0000")
    try uiSend(try okButton(again, tag), "\(tag) OK after an RGB edit")
    try pumpUntil({ again.view.window?.isVisible != true }, seconds: 10)
    try require(ToolOptionsStore.shared.sharedState.foregroundInk == nil,
                "\(tag) an RGB-side edit clears the ink")
    // Swap carries a stored ink; reset clears both.
    controller.setPaintInk(CmykInk(c: 10, m: 20, y: 30, k: 40), color: .gray)
    controller.swapColors()
    try require(ToolOptionsStore.shared.sharedState.backgroundInk == CmykInk(c: 10, m: 20, y: 30, k: 40)
                && ToolOptionsStore.shared.sharedState.foregroundInk == nil, "\(tag) swap swaps the inks")
    controller.resetColors()
    try require(ToolOptionsStore.shared.sharedState.backgroundInk == nil, "\(tag) reset clears the inks")
    // A reversed two-colour ramp: each end takes the ink of the swatch its colour came from.
    guard let precision = document.doc?.precision else { throw ProbeFailure(description: "\(tag) no precision") }
    let front = CmykInk(c: 90, m: 10, y: 0, k: 0), back = CmykInk(c: 0, m: 0, y: 70, k: 20)
    for reverse in [false, true] {
        var options = GradientToolOptions()
        options.reverse = reverse
        let ramp = GradientRamp(options: options, foreground: .blue, background: .yellow)
        let stops = try CmykPaint.stops(ramp.stops.map { ($0.position, $0.color) }, foreground: front,
                                        background: back, reversed: reverse, in: precision)
        let ends = [stops[0], stops[stops.count - 1]].map { ($0["ink"] as? [Double] ?? []).prefix(4).map { $0 } }
        let want = reverse ? [back.unit, front.unit] : [front.unit, back.unit]
        try require(ends == want, "\(tag) reverse \(reverse): the ramp's ends are \(ends), not \(want)")
    }
    print("PASS: \(tag) C/M/Y/K set the ink on CMYK and convert beside RGB")
}

/// "cmyk tone channels": on a CMYK document Levels and Curves list CMYK, Cyan, Magenta, Yellow
/// and Black (and validate), and a Curves Apply on the Cyan channel is one "Curves" step that
/// changes only the layer's ink.
func verifyCmykToneChannels() throws {
    let tag = "[cmyk tone channels]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    try cmykFixture(controller, document, tag)
    let expected = ["CMYK", "Cyan", "Magenta", "Yellow", "Black"]
    for selector in ["showLevels:", "showCurves:", "newAdjustmentLayerLevels:", "newAdjustmentLayerCurves:"] {
        try require(uiValidates(controller, selector), "\(tag) \(selector) validates on CMYK")
    }
    // Levels.
    let knownLevels = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showLevels(nil)
    let levels = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? LevelsAdjustmentEditor }
            .last { !knownLevels.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ levels()?.view.window?.isVisible == true }, seconds: 20)
    guard let levelsSheet = levels() else { throw ProbeFailure(description: "\(tag) no Levels sheet") }
    let levelsPopup = try uiControl(levelsSheet.view, id: "levels-channel", as: NSPopUpButton.self)
    try require(levelsPopup.itemTitles == expected, "\(tag) Levels lists \(levelsPopup.itemTitles)")
    if let cancel = buttons(levelsSheet.view).first(where: { $0.keyEquivalent == "\u{1b}" }) {
        try uiSend(cancel, "\(tag) Levels Cancel")
    } else { levelsSheet.dismiss(nil) }
    try pumpUntil({ levelsSheet.view.window?.isVisible != true && !BackgroundWork.occupied }, seconds: 20)
    // Curves: the Cyan curve, applied.
    let base = document.doc
    let entries = document.history.entries.count
    let knownCurves = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showCurves(nil)
    let curves = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? CurvesAdjustmentSheetController }
            .last { !knownCurves.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ curves()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = curves() else { throw ProbeFailure(description: "\(tag) no Curves sheet") }
    let popup = try uiControl(sheet.view, id: "curves-channel", as: NSPopUpButton.self)
    try require(popup.itemTitles == expected, "\(tag) Curves lists \(popup.itemTitles)")
    popup.selectItem(at: 1)
    try uiSend(popup, "\(tag) Cyan")
    guard let curve = uiViews(sheet.view, as: CurveEditorView.self).first else {
        throw ProbeFailure(description: "\(tag) no curve")
    }
    try require(curve.inkAxes, "\(tag) the curve's axes are ink")
    let points = [CurvePoint(x: 0, y: 0), CurvePoint(x: 128, y: 200), CurvePoint(x: 255, y: 255)]
    curve.points = points
    curve.onPointsChanged?(points)
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no Apply button")
    }
    try uiSend(apply, "\(tag) Apply")
    try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                    && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Curves",
                "\(tag) one \"Curves\" step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    print("PASS: \(tag) Levels and Curves list the inks, and a Cyan curve applies as one step")
}

/// The composite's ink16 at `(x, y)` of a CMYK document.
private func cmykInk(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Int] {
    guard let precision = document.doc?.precision else { throw ProbeFailure(description: "no precision") }
    let context = try PrecisionWorkContext(source: precision)
    let sample = try precision.cmykSample(x: x, y: y, reach: 0, workingLimit: context.limit,
                                          otherRetained: context.otherRetained)
    return (sample["ink16"] as? [NSNumber])?.map(\.intValue) ?? []
}

/// "cmyk hue saturation": on a CMYK document Image ▸ Adjustments ▸ Hue/Saturation… and Layer ▸
/// New Adjustment Layer ▸ Hue/Saturation… validate (and White Balance's tag, RGB only, does not), Desaturate
/// validates, the sheet's Targeted Adjustment is disabled, and a master Hue Apply is one
/// "Hue/Saturation" step that keeps the document CMYK and changes the layer's ink.
func verifyCmykHueSaturation() throws {
    let tag = "[cmyk hue saturation]"
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    try cmykFixture(controller, document, tag)
    guard let hue = AdjustmentMenuOrder.newOps.firstIndex(of: .hueSaturation),
          let balance = AdjustmentMenuOrder.newOps.firstIndex(of: .whiteBalance) else {
        throw ProbeFailure(description: "\(tag) no Hue/Saturation or White Balance tag")
    }
    for selector in ["showAdjustmentSheet:", "newAdjustmentLayerOp:"] {
        try require(uiValidates(controller, selector, tag: hue), "\(tag) \(selector) Hue/Saturation validates")
        try require(!uiValidates(controller, selector, tag: balance),
                    "\(tag) \(selector) White Balance stays refused")
    }
    try require(uiValidates(controller, "applyDesaturate:"), "\(tag) Desaturate validates")
    let probes = [(8, 8), (32, 24), (56, 40), (16, 40), (48, 8)]
    let before = try probes.map { try cmykInk(document, $0.0, $0.1) }
    let base = document.doc
    let entries = document.history.entries.count
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    let item = NSMenuItem(title: "Hue/Saturation…", action: #selector(EditorViewController.showAdjustmentSheet(_:)),
                          keyEquivalent: "")
    item.tag = hue
    controller.showAdjustmentSheet(item)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? HueSaturationSheet }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 20)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Hue/Saturation sheet") }
    guard let targeted = buttons(sheet.view).first(where: { $0.title == "Targeted Adjustment" }) else {
        throw ProbeFailure(description: "\(tag) no Targeted Adjustment checkbox")
    }
    try require(!targeted.isEnabled, "\(tag) Targeted Adjustment is disabled on CMYK")
    guard let slider = uiViews(sheet.view, as: NSSlider.self).first(where: {
        $0.minValue == -180 && $0.maxValue == 180
    }) else { throw ProbeFailure(description: "\(tag) no master Hue slider") }
    slider.doubleValue = 40
    try uiSend(slider, "\(tag) Hue +40")
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) no Apply button")
    }
    try uiSend(apply, "\(tag) Apply")
    try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                    && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Hue/Saturation",
                "\(tag) one \"Hue/Saturation\" step: \(document.history.entries.map(\.name))")
    try require(document.doc?.isCmyk == true, "\(tag) the document stays CMYK")
    let after = try probes.map { try cmykInk(document, $0.0, $0.1) }
    try require(after != before, "\(tag) the layer's ink changes: \(before) -> \(after)")
    print("PASS: \(tag) Hue/Saturation validates on CMYK and a master Hue applies as one step")
}
