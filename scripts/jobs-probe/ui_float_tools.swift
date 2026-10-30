import AppKit

// Six canvas tools in a 32-bit float document (4dbba7db) through the
// canvas's own entry points — the Paint Bucket's `onFillClick`, the Dodge and
// Sponge strokes' `beginStroke(at:)` / `continueStroke(to:pressure:)` /
// `endStroke()`, the Magic Wand's `onWandClick`, Quick Selection's
// `beginSelectionStroke(at:event:)` … `endSelectionStroke(at:)` and Select ▸
// Color Range…'s own sheet — never the MCP handlers. The shared picture is
// four 40-px neutral bands 0.25 | 1.0 | 2.0 | 8.0 over 160 × 120; the R1 display
// copy the bucket and the selections compare reads 1.0, 2.0 and 8.0 as white.

let floatToolBands: [Double] = [0.25, 1, 2, 8]
private let floatToolInk: [Double] = [4, 0.5, 0.25, 1]

/// The centre of band `index` on row `y`.
private func floatBandCentre(_ index: Int, y: Int = 60) -> (x: Int, y: Int) { (index * 40 + 20, y) }

/// `floatFixture()` cut to 160 × 120 and filled rect by rect over MCP, then
/// deselected with the undo stack cleared.
func floatToolDocument(_ fills: [(rect: [Int], rgba: [Double])]) throws -> ImageDocument {
    let document = try floatFixture()
    do {
        _ = try agentOK("canvas_size", document, ["width": 160, "height": 120, "anchor": "top-left"])
        for fill in fills {
            _ = try agentOK("select_rect", document, ["x": fill.rect[0], "y": fill.rect[1],
                                                      "width": fill.rect[2], "height": fill.rect[3]])
            _ = try agentOK("fill_selection", document, ["layer": 0, "rgba32": fill.rgba])
        }
        _ = try agentOK("deselect", document)
        document.undoManager?.removeAllActions()
    } catch {
        document.close()
        throw error
    }
    return document
}

/// The four bands.
private func floatBandsDocument() throws -> ImageDocument {
    try floatToolDocument(floatToolBands.enumerated().map {
        index, value -> (rect: [Int], rgba: [Double]) in
        ([index * 40, 0, 40, 120], [value, value, value, 1])
    })
}

/// The composite's straight linear RGBA at one pixel.
func floatToolPixel(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Double] {
    guard let sample = try FloatReadout.at((x: x, y: y), document: document) else {
        throw ProbeFailure(description: "No float sample at (\(x), \(y))")
    }
    return sample.values
}

/// Layer `index`'s own straight linear RGBA at one pixel.
func floatToolLayerPixel(_ doc: RasterDocument?, _ index: Int, _ x: Int, _ y: Int) throws -> [Double] {
    guard let source = doc?.precision?.layerSource(index),
          let sample = try FloatReadout.at((x: x, y: y), projection: source, reach: 0) else {
        throw ProbeFailure(description: "No layer \(index) float sample at (\(x), \(y))")
    }
    return sample.values
}

/// The selection's u16 coverage at one pixel (0 with no selection).
func floatToolCoverage(_ document: ImageDocument, _ x: Int, _ y: Int) -> UInt16 {
    guard let selection = document.canvasSelection, let width = document.doc?.width else { return 0 }
    return selection.maskSamples16()[y * width + x]
}

/// Every band reads its own value.
private func floatBandsIntact(_ document: ImageDocument, _ tag: String) throws {
    for (index, value) in floatToolBands.enumerated() {
        let centre = floatBandCentre(index, y: 110)
        let got = try floatToolPixel(document, centre.x, centre.y)
        try require(got == [value, value, value, 1],
                    "\(tag) band \(index) reads \(value) at (\(centre.x), \(centre.y)): \(got)")
    }
}

/// The job a stroke started, pumped to its end, or nothing when none ran.
func floatToolSettle() throws {
    if let job = DocumentJobs.active { try pumpUntil({ job.state != "running" }, seconds: 60) }
    try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
}

/// One stroke through the canvas: press, one drag, release.
private func floatToolStroke(_ canvas: ImageCanvasView, from: CGPoint, to: CGPoint,
                             _ tag: String) throws {
    canvas.beginStroke(at: from)
    try require(canvas.strokeActive, "\(tag) the \(canvas.tool) stroke began")
    canvas.continueStroke(to: to, pressure: nil)
    canvas.endStroke()
}

/// A hard 10-px tip with no smoothing or pressure.
private func floatToolTip(_ options: PaintToolOptions) -> PaintToolOptions {
    var tip = options
    tip.size = 10
    tip.hardness = 100
    tip.smoothing = 0
    tip.pressureSize = false
    tip.pressureOpacity = false
    tip.blendIndex = 0
    return tip
}

/// One step: the history grew by one, `steps` holds one `tool` step, no alert,
/// and one undo returns `document` to `base` with no selection.
private func floatToolOneStep(_ document: ImageDocument, base: RasterDocument, entries: Int,
                              alerts: Int, steps: [ActionStep], tool: String,
                              name: String? = nil, _ tag: String) throws {
    try require(document.history.cursor == entries + 1,
                "\(tag) one step: \(document.history.entries.map(\.name))")
    if let name {
        try require(document.undoManager?.undoActionName == name,
                    "\(tag) the step is \"\(name)\": \(document.undoManager?.undoActionName ?? "-")")
    }
    try require(steps.count == 1 && steps[0].tool == tool,
                "\(tag) one recorded \(tool) step: \(steps.map(\.tool))")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base && document.canvasSelection == nil
                && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
}

private func floatToolNear(_ a: Double, _ b: Double, _ relative: Double) -> Bool {
    abs(a - b) <= relative * max(abs(a), abs(b))
}

/// "float paint bucket": a click on the 2.0 band fills the three white-reading
/// bands with an ink above white.
func verifyFloatPaintBucket() throws {
    let tag = "[float paint bucket]"
    let document = try floatBandsDocument()
    let store = ToolOptionsStore.shared
    let saved = store.fill
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        store.fill = saved
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let colors = (controller.paintColor, controller.backgroundColor)
    defer { (controller.paintColor, controller.backgroundColor) = colors }
    guard let base = document.doc, base.isFloat,
          let hdr = ExtendedRangeColor.linearColor(floatToolInk, in: base) else {
        throw ProbeFailure(description: "\(tag) the float picture and its ink")
    }
    try floatBandsIntact(document, tag)
    var fill = FillToolOptions()
    fill.tolerance = 0
    fill.contiguous = true
    store.fill = fill
    controller.selectTool(.fill)
    try require(controller.currentTool == .fill, "\(tag) the Paint Bucket is selected")
    controller.paintColor = hdr
    canvas.paintColor = hdr
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    let steps = try uiRecorded {
        guard let click = canvas.onFillClick else {
            throw ProbeFailure(description: "\(tag) the canvas has no fill click")
        }
        click(CGPoint(x: 100, y: 60))
        try floatToolSettle()
    }
    for index in 1...3 {
        let centre = floatBandCentre(index)
        let got = try floatToolPixel(document, centre.x, centre.y)
        try require(got == floatToolInk, "\(tag) band \(floatToolBands[index]) reads the ink: \(got)")
    }
    let dim = try floatToolPixel(document, 20, 60)
    try require(dim == [0.25, 0.25, 0.25, 1], "\(tag) the 0.25 band is unchanged: \(dim)")
    let recorded = steps.first?.arguments["rgba32"] as? [Double] ?? []
    try require(recorded == floatToolInk && steps.first?.arguments["fill_mode"] as? String == "color_match",
                "\(tag) the step records rgba32 and fill_mode color_match: "
                + "\(steps.map { "\($0.tool) \($0.arguments)" })")
    try floatToolOneStep(document, base: base, entries: entries, alerts: alerts, steps: steps,
                         tool: steps.first?.tool ?? "-", name: "Fill", tag)
    print("PASS: float paint bucket — in a 32-bit float document a Paint Bucket click (tolerance 0, "
        + "contiguous) on the 2.0 band fills the 1.0, 2.0 and 8.0 bands with (4, 0.5, 0.25) exactly "
        + "and leaves the 0.25 band, as one \"Fill\" step recorded \(steps.first?.tool ?? "-") "
        + "with rgba32 and fill_mode color_match, one undo (R1)")
}

/// "float dodge": a Dodge stroke's gain is the same above white as at white,
/// and on a mask it raises the u16 coverage under the stroke alone.
func verifyFloatDodge() throws {
    let tag = "[float dodge]"
    let document = try floatBandsDocument()
    let store = ToolOptionsStore.shared
    let saved = store.dodge
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        store.dodge = saved
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) the float picture")
    }
    var tip = floatToolTip(store.dodge)
    tip.burn = false
    store.dodge = tip
    controller.selectTool(.dodge)
    try require(controller.currentTool == .dodge, "\(tag) Dodge is selected")
    let from = CGPoint(x: 30.5, y: 60.5), to = CGPoint(x: 130.5, y: 60.5)
    var entries = document.history.cursor
    var alerts = AutomationAlerts.recorded.count
    var jobs = 0
    let steps = try uiRecorded {
        try floatToolStroke(canvas, from: from, to: to, tag)
        if DocumentJobs.active != nil { jobs += 1 }
        try floatToolSettle()
    }
    let gain = { (x: Int, value: Double) throws -> Double in
        try floatToolPixel(document, x, 60)[0] / value
    }
    let white = try gain(60, 1), two = try gain(100, 2), eight = try gain(125, 8)
    try require(floatToolNear(two, white, 1e-5) && floatToolNear(eight, white, 1e-5),
                "\(tag) the gain at 2.0 and 8.0 is the gain at white: \(white) \(two) \(eight)")
    let dim = try floatToolPixel(document, 35, 60)
    try require(dim[0] != 0.25, "\(tag) the 0.25 band changed: \(dim)")
    try require(jobs == 1, "\(tag) the stroke ran as one job")
    try floatToolOneStep(document, base: base, entries: entries, alerts: alerts, steps: steps,
                         tool: "dodge_burn", tag)

    // The mask, seeded to 32896 (`verify_phase11f_float.py` F46 (b)): a
    // reveal-all mask is already 65535 and a dodge there changes nothing.
    _ = try agentOK("add_layer_mask", document, ["layer": 0])
    _ = try agentOK("fill", document, ["layer": 0, "target": "mask", "x": 5, "y": 5,
                                       "tolerance": 0, "color": "#808080",
                                       "color_space": "document"])
    document.undoManager?.removeAllActions()
    guard let masked = document.doc else { throw ProbeFailure(description: "\(tag) no mask") }
    let seeded = try PrecisionScalarOperations.read(masked, target: "mask", index: 0).samples
    try require(seeded.count == 160 * 120 && seeded.allSatisfy { $0 == 32896 },
                "\(tag) the mask is seeded to 32896")
    let layerBefore = try (0..<4).map { try floatToolLayerPixel(masked, 0, $0 * 40 + 20, 60) }
    controller.setPaintTarget(.mask)
    try require(document.planeEditTarget == .mask, "\(tag) the mask is the target")
    entries = document.history.cursor
    alerts = AutomationAlerts.recorded.count
    let maskSteps = try uiRecorded {
        try floatToolStroke(canvas, from: from, to: to, tag)
        try floatToolSettle()
    }
    guard let dodged = document.doc, dodged !== masked else {
        throw ProbeFailure(description: "\(tag) the mask stroke changed nothing: \(uiAlerts())")
    }
    let mask = try PrecisionScalarOperations.read(dodged, target: "mask", index: 0).samples
    for x in [40, 60, 100, 125] {
        try require(mask[60 * 160 + x] > 32896,
                    "\(tag) the mask under the stroke rose at x \(x): \(mask[60 * 160 + x])")
    }
    var off = 0
    for y in 0..<120 {
        for x in 0..<160 {
            let dx = Double(x) + 0.5 < from.x ? from.x - Double(x) - 0.5
                : Double(x) + 0.5 > to.x ? Double(x) + 0.5 - to.x : 0
            let dy = Double(y) + 0.5 - from.y
            guard (dx * dx + dy * dy).squareRoot() >= 25 else { continue }
            off += 1
            try require(mask[y * 160 + x] == 32896,
                        "\(tag) the mask 20 px off the stroke is untouched at (\(x), \(y)): "
                        + "\(mask[y * 160 + x])")
        }
    }
    let layerAfter = try (0..<4).map { try floatToolLayerPixel(dodged, 0, $0 * 40 + 20, 60) }
    try require(layerAfter == layerBefore, "\(tag) the layer's float pixels are unchanged (R4m)")
    try require(maskSteps.first?.arguments["target"] as? String == "mask",
                "\(tag) the mask stroke records target mask: "
                + "\(maskSteps.map { "\($0.tool) \($0.arguments)" })")
    try floatToolOneStep(document, base: masked, entries: entries, alerts: alerts, steps: maskSteps,
                         tool: "dodge_burn", tag)
    controller.setPaintTarget(.layer)
    print("PASS: float dodge — in a 32-bit float document a Dodge stroke across the bands is one job "
        + "and one step recorded dodge_burn whose gain at 2.0 and 8.0 is the gain at white ("
        + String(format: "%.6f", white) + ", R4) and which changes the 0.25 band; on a mask seeded "
        + "to 32896 the stroke raises the coverage under it, leaves \(off) samples 20 px off it at "
        + "32896 and no layer pixel, as one step recorded dodge_burn target mask (R4m)")
}

/// "float sponge": Saturate pushes every channel away from Rec. 709 luma by
/// the same multiple, and red past 4.
func verifyFloatSponge() throws {
    let tag = "[float sponge]"
    let p: [Double] = [4, 0.5, 0.25, 1]
    let document = try floatToolDocument([([0, 0, 160, 120], p)])
    let store = ToolOptionsStore.shared
    let saved = store.sponge
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        store.sponge = saved
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) the float picture")
    }
    let before = try floatToolPixel(document, 80, 60)
    try require(before == p, "\(tag) the picture holds P: \(before)")
    var tip = floatToolTip(store.sponge)
    tip.saturate = true
    store.sponge = tip
    controller.selectTool(.sponge)
    try require(controller.currentTool == .sponge, "\(tag) the Sponge is selected")
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    let steps = try uiRecorded {
        try floatToolStroke(canvas, from: CGPoint(x: 30.5, y: 60.5), to: CGPoint(x: 130.5, y: 60.5), tag)
        try floatToolSettle()
    }
    let out = try floatToolPixel(document, 80, 60)
    let luma = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
    let multiples = (0..<3).map { (out[$0] - p[$0]) / (p[$0] - luma) }
    try require(multiples.allSatisfy { abs($0 - multiples[0]) <= 1e-4 * max(1, abs(multiples[0])) }
                && multiples[0] > 0 && out[0] > 4,
                "\(tag) each channel moves away from luma by one multiple, red past 4: \(out) "
                + "(multiples \(multiples))")
    try floatToolOneStep(document, base: base, entries: entries, alerts: alerts, steps: steps,
                         tool: "sponge_stroke", tag)
    print("PASS: float sponge — in a 32-bit float document a Saturate Sponge stroke over (4, 0.5, "
        + "0.25) moves every channel away from its Rec. 709 luma by the same multiple ("
        + String(format: "%.4f", multiples[0]) + ") with red at " + String(format: "%.4f", out[0])
        + ", as one step recorded sponge_stroke, one undo")
}

/// The selection options for one case, restored afterwards.
private func withFloatSelectOptions(_ body: () throws -> Void) throws {
    let store = ToolOptionsStore.shared
    let saved = store.select
    defer { store.select = saved }
    var options = SelectToolOptions()
    options.tolerance = 0
    options.contiguous = true
    store.select = options
    try body()
}

/// The three white-reading bands are selected and the 0.25 band is not.
private func floatBandsSelected(_ document: ImageDocument, _ tag: String) throws {
    for index in 1...3 {
        let centre = floatBandCentre(index)
        let coverage = floatToolCoverage(document, centre.x, centre.y)
        try require(coverage == 65535, "\(tag) band \(floatToolBands[index]) is selected: \(coverage)")
    }
    let dim = floatToolCoverage(document, 20, 60)
    try require(dim == 0, "\(tag) the 0.25 band is not selected: \(dim)")
}

/// "float magic wand": a tolerance-0 click on the 2.0 band.
func verifyFloatMagicWand() throws {
    let tag = "[float magic wand]"
    let document = try floatBandsDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) the float picture")
    }
    var steps: [ActionStep] = []
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    try withFloatSelectOptions {
        controller.selectTool(.wand)
        try require(controller.currentTool == .wand, "\(tag) the Magic Wand is selected")
        steps = try uiRecorded {
            guard let click = canvas.onWandClick else {
                throw ProbeFailure(description: "\(tag) the canvas has no wand click")
            }
            click(CGPoint(x: 100, y: 60), .replace)
            try floatToolSettle()
        }
    }
    try floatBandsSelected(document, tag)
    try floatToolOneStep(document, base: base, entries: entries, alerts: alerts, steps: steps,
                         tool: steps.first?.tool ?? "-", name: "Magic Wand", tag)
    print("PASS: float magic wand — in a 32-bit float document a tolerance-0 contiguous Magic Wand "
        + "click on the 2.0 band selects the 1.0, 2.0 and 8.0 bands at 65535 and not the 0.25 band, "
        + "as one \"Magic Wand\" step recorded \(steps.first?.tool ?? "-"), one undo (R1)")
}

/// "float quick selection": a drag inside the 2.0 band.
func verifyFloatQuickSelection() throws {
    let tag = "[float quick selection]"
    let document = try floatBandsDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isFloat, let window = canvas.window else {
        throw ProbeFailure(description: "\(tag) the float picture and its window")
    }
    var steps: [ActionStep] = []
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    try withFloatSelectOptions {
        controller.selectTool(.quickSelect)
        try require(controller.currentTool == .quickSelect, "\(tag) Quick Selection is selected")
        guard let down = NSEvent.mouseEvent(
            with: .leftMouseDown, location: .zero, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
            throw ProbeFailure(description: "\(tag) could not create the mouse-down")
        }
        steps = try uiRecorded {
            canvas.beginSelectionStroke(at: CGPoint(x: 95, y: 60), event: down)
            canvas.continueSelectionStroke(at: CGPoint(x: 100, y: 60))
            canvas.endSelectionStroke(at: CGPoint(x: 105, y: 60))
            try floatToolSettle()
        }
    }
    let far = floatToolCoverage(document, 140, 60), dim = floatToolCoverage(document, 20, 60)
    try require(far >= 32768 && dim == 0,
                "\(tag) the 8.0 band's centre is selected (\(far)) and the 0.25 band's is not (\(dim))")
    try floatToolOneStep(document, base: base, entries: entries, alerts: alerts, steps: steps,
                         tool: "quick_select", tag)
    print("PASS: float quick selection — in a 32-bit float document a Quick Selection drag from "
        + "(95, 60) to (105, 60) on the 2.0 band selects the 8.0 band's centre (\(far)) and not the "
        + "0.25 band's, as one step recorded quick_select, one undo (R1)")
}

/// "float color range sheet": one sample on the 2.0 band, through the sheet.
func verifyFloatColorRangeSheet() throws {
    let tag = "[float color range sheet]"
    let document = try floatBandsDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) the float picture")
    }
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    let steps = try uiRecorded {
        let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
        controller.showColorRange(nil)
        let opened = {
            (controller.presentedViewControllers ?? []).compactMap { $0 as? ColorRangeSheetController }
                .last { !known.contains(ObjectIdentifier($0)) }
        }
        try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
        guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Color Range sheet") }
        guard let preview = uiViews(sheet.view, as: SelectionPreviewView.self).first,
              let click = preview.onPoint else {
            throw ProbeFailure(description: "\(tag) the sheet has no clickable preview")
        }
        click(CGPoint(x: 100, y: 60))
        guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
            throw ProbeFailure(description: "\(tag) the sheet has no Apply")
        }
        try uiSend(apply, "\(tag) Apply")
        try pumpUntil({ sheet.view.window?.isVisible != true }, seconds: 30)
        try floatToolSettle()
    }
    try floatBandsSelected(document, tag)
    try floatToolOneStep(document, base: base, entries: entries, alerts: alerts, steps: steps,
                         tool: "select_color_range", tag)
    print("PASS: float color range sheet — in a 32-bit float document Select ▸ Color Range…'s "
        + "preview click on the 2.0 band and Apply select the 1.0, 2.0 and 8.0 bands and not the "
        + "0.25 band, as one step recorded select_color_range, one undo (R1)")
}
