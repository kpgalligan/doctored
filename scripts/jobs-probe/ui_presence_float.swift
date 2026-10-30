import AppKit

// Filters ▸ Presence… and Filters ▸ Develop Filter… in a 32-bit float
// document (3cf8a8aa, 8d1f952b) through their own sliders and Apply.
// The picture: the left half P = (4, 0.5, 0.25), with Q = P / 4 at one pixel
// and a 20-px block H = (8, 1, 0.5); the right half an in-range ramp in 10-px
// stripes. Each slider's stated rule on a value above 1.0 — D: Dehaze returns
// a component above 1 as read; H: Highlights and Shadows hold a pixel whose
// luma is at least 1; V: Vibrance and Saturation scale about white, so
// out(P) = 4·out(Q).
//
// Rule H's luma is the display-encoded copy's (`ShadowsHighlights::apply_float`
// in `adjust_tone.rs`, the `# 32 bits` table of `sampling_develop_filter.rs`), not
// the linear value's: P's linear luma is 1.23 but its encoded luma about 0.95,
// so P is not held. H's encoded luma is about 1.3, so it is.

private let presenceP: [Double] = [4, 0.5, 0.25, 1]
private let presenceQ: [Double] = [1, 0.125, 0.0625, 1]
private let presenceAtP = (x: 60, y: 60)
private let presenceAtQ = (x: 20, y: 20)
private let presenceH: [Double] = [8, 1, 0.5, 1]
private let presenceAtH = (x: 70, y: 100)
private let presenceRamp = (x: 115, y: 60)

private func presenceFloatDocument() throws -> ImageDocument {
    var fills: [(rect: [Int], rgba: [Double])] = [([0, 0, 80, 120], presenceP),
                                                  ([presenceAtQ.x, presenceAtQ.y, 1, 1], presenceQ),
                                                  ([60, 90, 20, 20], presenceH)]
    for stripe in 0..<8 {
        let v = 0.1 + 0.1 * Double(stripe)
        fills.append(([80 + stripe * 10, 0, 10, 120], [v, 0.4, 0.2 + 0.05 * Double(stripe), 1]))
    }
    return try floatToolDocument(fills)
}

/// The slider sheet `open` just presented on `controller`.
private func presenceFloatSheet(_ controller: EditorViewController, _ tag: String,
                                _ open: () -> Void) throws -> SliderSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    open()
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no sheet") }
    return sheet
}

private func presenceFloatApply(_ sheet: SliderSheetController, _ document: ImageDocument,
                                _ tag: String) throws {
    let base = document.doc
    guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
        throw ProbeFailure(description: "\(tag) the sheet has no Apply")
    }
    try uiSend(apply, "\(tag) Apply")
    try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                    && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
}

/// One step (named `name` when one is given), one recorded `apply_filter`
/// `filter` step whose arguments hold `values`, no alert, and one undo back to
/// `base`.
private func presenceFloatOneStep(_ document: ImageDocument, base: RasterDocument, entries: Int,
                                  alerts: Int, steps: [ActionStep], filter: String, name: String?,
                                  values: [String: Double], _ tag: String) throws {
    try require(document.history.cursor == entries + 1
                && (name == nil || document.undoManager?.undoActionName == name),
                "\(tag) one \"\(name ?? "-")\" step: \(document.history.entries.map(\.name))")
    let arguments = steps.first?.arguments ?? [:]
    try require(steps.count == 1 && steps[0].tool == "apply_filter"
                && arguments["filter"] as? String == filter
                && values.allSatisfy { (arguments[$0.key] as? NSNumber)?.doubleValue == $0.value },
                "\(tag) one recorded apply_filter \(filter) step with \(values): "
                + "\(steps.map { "\($0.tool) \($0.arguments)" })")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the document")
}

/// "presence float": Dehaze 40 returns a component above 1 as read.
func verifyPresenceFloat() throws {
    let tag = "[presence float]"
    let document = try presenceFloatDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) the float picture")
    }
    let p = try floatToolPixel(document, presenceAtP.x, presenceAtP.y)
    let ramp = try floatToolPixel(document, presenceRamp.x, presenceRamp.y)
    try require(p == presenceP, "\(tag) the picture holds P: \(p)")
    try require(uiValidates(controller, "showPresence:"), "\(tag) Presence… validates")
    let entries = document.history.cursor
    let alerts = AutomationAlerts.recorded.count
    let steps = try uiRecorded {
        let sheet = try presenceFloatSheet(controller, tag) { controller.showPresence(nil) }
        let sliders = uiViews(sheet.view, as: NSSlider.self)
        try require(sliders.count == 3 && sliders.allSatisfy(\.isEnabled),
                    "\(tag) three enabled sliders: \(sliders.count)")
        sliders[2].doubleValue = 40
        try uiSend(sliders[2], "\(tag) the Dehaze slider")
        try presenceFloatApply(sheet, document, tag)
    }
    let dehazed = try floatToolPixel(document, presenceAtP.x, presenceAtP.y)
    let rampAfter = try floatToolPixel(document, presenceRamp.x, presenceRamp.y)
    try require(dehazed[0] == 4, "\(tag) P's red above white reads 4 exactly (rule D): \(dehazed)")
    try require(rampAfter != ramp, "\(tag) the ramp pixel changed: \(ramp) → \(rampAfter)")
    try presenceFloatOneStep(document, base: base, entries: entries, alerts: alerts, steps: steps,
                             filter: "presence", name: "Presence", values: ["dehaze": 40], tag)
    print("PASS: presence float — in a 32-bit float document Filters ▸ Presence… opens three enabled "
        + "sliders; Dehaze 40 and Apply keep P's red at exactly 4 (rule D) and change the in-range "
        + "ramp, as one \"Presence\" step recorded apply_filter presence dehaze 40, one undo")
}

/// "develop filter float": Highlights/Shadows, Saturation and Vibrance
/// enabled at 32 bits, each with its stated rule.
func verifyDevelopFilterFloat() throws {
    let tag = "[develop filter float]"
    let document = try presenceFloatDocument()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "\(tag) the float picture")
    }
    let ramp = try floatToolPixel(document, presenceRamp.x, presenceRamp.y)
    let q = try floatToolPixel(document, presenceAtQ.x, presenceAtQ.y)
    try require(q == presenceQ, "\(tag) the picture holds Q: \(q)")
    try require(uiValidates(controller, "showDevelopFilter:"), "\(tag) Develop Filter… validates")

    /// One sheet with `values` set through their sliders, then Apply.
    func run(_ values: [String: Double], _ what: String) throws -> [ActionStep] {
        try uiRecorded {
            let sheet = try presenceFloatSheet(controller, what) { controller.showDevelopFilter(nil) }
            for key in ["highlights", "shadows", "vibrance", "saturation"] {
                let row = try uiControl(sheet.view, id: "develop-filter-\(key)", as: CardControlRow.self)
                try require(row.isEnabled && row.slider?.isEnabled == true,
                            "\(what) the \(key) row and slider are enabled at 32 bits")
            }
            for (key, value) in values.sorted(by: { $0.key < $1.key }) {
                guard let slider = try uiControl(sheet.view, id: "develop-filter-\(key)",
                                                 as: CardControlRow.self).slider else {
                    throw ProbeFailure(description: "\(what) the \(key) row has no slider")
                }
                slider.doubleValue = value
                try uiSend(slider, "\(what) the \(key) slider")
            }
            try presenceFloatApply(sheet, document, what)
        }
    }

    // Rule H: Highlights −50 with Shadows +50 leaves H (encoded luma ≥ 1) as
    // it was.
    var entries = document.history.cursor
    var alerts = AutomationAlerts.recorded.count
    let tone: [String: Double] = ["highlights": -50, "shadows": 50]
    let toneSteps = try run(tone, "\(tag) Highlights/Shadows")
    let toned = try floatToolPixel(document, presenceAtH.x, presenceAtH.y)
    let tonedRamp = try floatToolPixel(document, presenceRamp.x, presenceRamp.y)
    try require(toned == presenceH, "\(tag) H is unchanged exactly (rule H): \(toned)")
    try require(tonedRamp != ramp, "\(tag) the ramp pixel changed: \(ramp) → \(tonedRamp)")
    try presenceFloatOneStep(document, base: base, entries: entries, alerts: alerts, steps: toneSteps,
                             filter: "develop_filter", name: "Develop Filter", values: tone,
                             "\(tag) Highlights/Shadows")

    // Rule V: Saturation, then Vibrance from a fresh base, scale about white.
    var ratios: [String] = []
    for key in ["saturation", "vibrance"] {
        entries = document.history.cursor
        alerts = AutomationAlerts.recorded.count
        let what = "\(tag) \(key.capitalized) +40"
        let steps = try run([key: 40], what)
        let outP = try floatToolPixel(document, presenceAtP.x, presenceAtP.y)
        let outQ = try floatToolPixel(document, presenceAtQ.x, presenceAtQ.y)
        try require(outP != presenceP, "\(what) changed P: \(outP)")
        try require((0..<3).allSatisfy { abs(outP[$0] - 4 * outQ[$0]) <= 1e-4 * max(abs(outP[$0]), 1e-9) },
                    "\(what) out(P) = 4·out(Q) within 1e-4 (rule V): \(outP) vs \(outQ)")
        ratios.append(String(format: "%.4f", outP[0] / max(outQ[0], 1e-12)))
        try presenceFloatOneStep(document, base: base, entries: entries, alerts: alerts,
                                 steps: steps, filter: "develop_filter", name: "Develop Filter",
                                 values: [key: 40], what)
    }
    print("PASS: develop filter float — in a 32-bit float document the Highlights, Shadows, "
        + "Vibrance and Saturation rows and sliders are enabled; Highlights −50 with Shadows +50 "
        + "leaves (8, 1, 0.5), whose encoded luma is above white, exactly (rule H) and changes the "
        + "ramp; Saturation +40 and Vibrance +40 each keep "
        + "out(P) = 4·out(Q) (red ratios \(ratios.joined(separator: ", ")), rule V); each Apply is "
        + "one \"Develop Filter\" step, the 8-bit sheet's name, recorded apply_filter "
        + "develop_filter with its values, one undo")
}
