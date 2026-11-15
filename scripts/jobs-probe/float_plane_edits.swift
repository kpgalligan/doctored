import AppKit

// "float plane edits": Image ▸ Adjustments, the three Auto items, Invert and Invert Channel on a
// 32-bit float document's layer mask and alpha channel, which are u16 coverage edited as at 16
// bits (`FloatDocument.appliesAdjustment`, `appliesAuto`), and on one float colour plane; the
// same items on a 32-bit Grayscale document's mask and channel. The menu's own validation and
// Invert's own action; the arithmetic is the core's (`core/tests/float_plane_adjust.rs`).

private func validates(_ controller: EditorViewController, _ name: String, tag: Int = 0) -> Bool {
    let item = NSMenuItem(title: "", action: NSSelectorFromString(name), keyEquivalent: "")
    item.tag = tag
    return controller.validateUserInterfaceItem(item)
}

/// Shadows/Highlights' tag in Image ▸ Adjustments (`AdjustmentMenuOrder.newOps`).
private var shadowsHighlightsTag: Int {
    AdjustmentMenuOrder.newOps.firstIndex(of: .shadowsHighlights) ?? -1
}

/// The items every coverage target must enable, by selector name.
private let coverageItems = [
    "applyInvert:", "autoTone:", "autoContrast:", "showLevels:", "showCurves:", "showThreshold:",
]

/// Which of `names` validate on the current target.
private func enabled(_ controller: EditorViewController, _ names: [String]) -> [String] {
    names.filter { validates(controller, $0) }
}

/// A layer mask and an alpha channel ("Edge") made from a rect selection, then deselected.
private func addPlanes(_ document: ImageDocument, tag: String) throws {
    guard let doc = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    document.setCanvasSelection(CanvasSelection(
        shape: .rect(CGRect(x: 4, y: 3, width: doc.width / 2, height: doc.height / 2)),
        canvasWidth: doc.width, canvasHeight: doc.height))
    _ = try agentOK("add_layer_mask", document, ["layer": 0, "kind": "from_selection"])
    _ = try agentOK("save_selection", document, ["name": "Edge"])
    _ = try agentOK("deselect", document)
}

/// The float document's checks.
private func verifyFloatDocument(tag: String) throws {
    let document = try floatFixture()
    defer { document.close() }
    let controller = try editor(document)
    try addPlanes(document, tag: tag)

    // (a) On the layer: the adjustments the float layer applies, and no Auto or Invert.
    try require(document.planeEditTarget == .layer, "\(tag) the fixture opens on its layer")
    try require(validates(controller, "showLevels:"), "\(tag) Levels validates on the layer")
    let onLayer = enabled(controller, ["applyInvert:", "autoTone:", "autoContrast:",
                                       "autoColor:", "invertChannel:"])
    try require(onLayer.isEmpty, "\(tag) no Auto or Invert validates on the float layer: \(onLayer)")
    try require(!validates(controller, "showAdjustmentSheet:", tag: shadowsHighlightsTag),
                "\(tag) Shadows/Highlights stays off on the float layer")

    // (b) On the mask and on the channel: every coverage item and Shadows/Highlights; Invert
    // Channel on the channel.
    for (name, target) in [("mask", PaintTarget.mask), ("channel", PaintTarget.channel(0))] {
        controller.setPaintTarget(target)
        try require(document.planeEditTarget == target, "\(tag) the panel targets the \(name)")
        let on = enabled(controller, coverageItems + ["autoColor:"])
        try require(on == coverageItems + ["autoColor:"],
                    "\(tag) on the \(name) every coverage item validates: missing "
                    + "\(Set(coverageItems + ["autoColor:"]).subtracting(on))")
        try require(validates(controller, "showAdjustmentSheet:", tag: shadowsHighlightsTag),
                    "\(tag) Shadows/Highlights validates on the \(name)")
    }
    try require(validates(controller, "invertChannel:"), "\(tag) Invert Channel validates")

    // (b2) On Red the layer's applied adjustments validate, and Invert, Auto and
    // Shadows/Highlights do not; on the layer's Alpha every adjustment and Invert validate, and
    // Auto does not.
    controller.setPaintTarget(.plane(.red))
    try require(validates(controller, "showLevels:") && validates(controller, "showCurves:"),
                "\(tag) Levels and Curves validate on Red")
    let onRed = enabled(controller, ["applyInvert:", "autoTone:", "autoContrast:", "autoColor:"])
    try require(onRed.isEmpty && !validates(controller, "showAdjustmentSheet:",
                                            tag: shadowsHighlightsTag),
                "\(tag) Invert, Auto and Shadows/Highlights stay off on Red: \(onRed)")
    controller.setPaintTarget(.plane(.alpha))
    try require(validates(controller, "applyInvert:") && validates(controller, "showLevels:")
                && validates(controller, "showAdjustmentSheet:", tag: shadowsHighlightsTag),
                "\(tag) Invert, Levels and Shadows/Highlights validate on Alpha")
    try require(!validates(controller, "autoTone:"), "\(tag) Auto Tone stays off on Alpha")

    // (c) Invert on the mask: 65535 − v in one undo step.
    controller.setPaintTarget(.mask)
    document.undoManager?.removeAllActions()
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base") }
    let before = try PrecisionScalarOperations.read(base, target: "mask", index: 0).samples
    controller.applyInvert(nil)
    try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 20)
    guard let inverted = document.doc, inverted !== base else {
        throw ProbeFailure(description: "\(tag) Invert did not change the float mask")
    }
    let after = try PrecisionScalarOperations.read(inverted, target: "mask", index: 0).samples
    try require(after.count == before.count && zip(before, after).allSatisfy { $1 == 65535 - $0 },
                "\(tag) Invert on the mask is 65535 − v")
    try require(before.contains(0) && before.contains(65535), "\(tag) the mask has both ends")
    document.undoManager?.undo()
    try require(document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) Invert on the mask is one undo step")

    // (d) In Quick Mask the coverage items stay off.
    document.setCanvasSelection(CanvasSelection(
        shape: .rect(CGRect(x: 2, y: 2, width: 10, height: 8)),
        canvasWidth: base.width, canvasHeight: base.height))
    controller.toggleQuickMask(nil)
    try require(document.quickMaskActive, "\(tag) Quick Mask is on")
    let inQuickMask = enabled(controller, coverageItems + ["invertChannel:"])
    try require(inQuickMask.isEmpty, "\(tag) nothing validates in Quick Mask: \(inQuickMask)")
    controller.toggleQuickMask(nil)
    _ = try agentOK("deselect", document)
    controller.setPaintTarget(.layer)
}

/// The 32-bit Grayscale document's checks: its mask and channel answer as at 16 bits.
private func verifyGrayDocument(tag: String) throws {
    guard let row = colourModeRows().first(where: { $0.name == "Grayscale 32" }) else {
        throw ProbeFailure(description: "\(tag) no Grayscale 32 row")
    }
    let (document, controller, _) = try openColourModeRow(row, tag: tag)
    defer { document.close() }
    try addPlanes(document, tag: tag)
    try require(!validates(controller, "autoTone:"), "\(tag) Gray 32: Auto Tone stays off on the layer")
    for (name, target) in [("mask", PaintTarget.mask), ("channel", PaintTarget.channel(0))] {
        controller.setPaintTarget(target)
        let on = enabled(controller, coverageItems)
        try require(on == coverageItems, "\(tag) Gray 32 \(name): every coverage item validates: "
                    + "missing \(Set(coverageItems).subtracting(on))")
    }
    try require(validates(controller, "invertChannel:"), "\(tag) Gray 32: Invert Channel validates")
    controller.setPaintTarget(.layer)
}

func verifyFloatPlaneEdits() throws {
    let tag = "[float plane edits]"
    try verifyFloatDocument(tag: tag)
    try verifyGrayDocument(tag: tag)
    print("PASS: float-plane-edits — on a 32-bit float document's mask and alpha channel the "
        + "adjustment items, Auto Tone, Auto Contrast, Auto Color and Invert validate (Invert "
        + "Channel on the channel), none of Auto or Invert does on the layer, on Red Levels and "
        + "Curves do and Invert, Auto and Shadows/Highlights do not, on Alpha Invert, Levels and "
        + "Shadows/Highlights do and Auto does not, Invert on the mask "
        + "is 65535 − v in one undo step, nothing validates in Quick Mask; a 32-bit Grayscale "
        + "document's mask and channel answer as at 16 bits")
}
