import AppKit

/// "apply image typed scale": Image ▸ Apply Image…'s Scale and Offset as typed, with Apply clicked
/// before Return. A value typed out of range counts, clamped into it, so Apply blends at the clamped
/// pair and records it, instead of handing the core a pair it refuses and leaving the layer as it was.

private func applyImageMember(_ sheet: ApplyImageSheetController, _ label: String) throws -> Any {
    guard let value = Mirror(reflecting: sheet).children.first(where: { $0.label == label })?.value else {
        throw ProbeFailure(description: "Apply Image sheet member \(label) is missing")
    }
    return value
}

func verifyApplyImageTypedScale() throws {
    let tag = "[apply image typed scale]"
    // (Scale typed, Offset typed, the 8-bit result of one channel code b): the source is the layer
    // itself inverted (Merged, Invert), so Subtract's difference is b − (255 − b) = 2b − 255.
    let cases: [(scale: String, offset: String, expected: (Double) -> Double)] = [
        // Scale 3 clamps to 2 and Offset 40 is in range: (2b − 255) / 2 + 40.
        ("3", "40", { b in (2 * b - 255) / 2 + 40 }),
        // A cleared Scale reads 0 and clamps to 1; Offset 300 clamps to 255: 2b − 255 + 255.
        ("", "300", { b in 2 * b }),
    ]
    for (scaleText, offsetText, expected) in cases {
        let label = "\(tag) Scale \"\(scaleText)\", Offset \"\(offsetText)\""
        let document = try fixture(width: 32, height: 24)
        defer { document.close() }
        let controller = try editor(document)
        guard let original = document.doc, let before = original.layerPixels(0) else {
            throw ProbeFailure(description: "\(label): fixture pixels")
        }
        controller.applyImageSheet(nil)
        try pumpUntil({ controller.presentedViewControllers?.contains { $0 is ApplyImageSheetController } == true },
                      seconds: 5)
        guard let sheet = controller.presentedViewControllers?.first(where: { $0 is ApplyImageSheetController })
            as? ApplyImageSheetController else { throw ProbeFailure(description: "\(label): no Apply Image sheet") }
        _ = sheet.view
        guard let blend = try applyImageMember(sheet, "blendPopup") as? NSPopUpButton,
              let invert = try applyImageMember(sheet, "invertCheckbox") as? NSButton,
              let scale = try applyImageMember(sheet, "scaleField") as? NSTextField,
              let offset = try applyImageMember(sheet, "offsetField") as? NSTextField,
              let blendAction = blend.action else {
            throw ProbeFailure(description: "\(label): the sheet's controls")
        }
        try require(blend.selectItem(withTag: Int(DR_BLEND_SUBTRACT.rawValue)), "\(label): no Subtract item")
        invert.state = .on
        blend.sendAction(blendAction, to: blend.target)
        try require(scale.isEnabled && offset.isEnabled, "\(label): Subtract enables Scale and Offset")
        // Typed, never committed: no Return, so the fields' own action never runs.
        scale.stringValue = scaleText
        offset.stringValue = offsetText
        guard let apply = buttons(sheet.view).first(where: { $0.title == "Apply" }) else {
            throw ProbeFailure(description: "\(label): no Apply button")
        }
        ActionRecorder.shared.start()
        apply.performClick(nil)
        try pumpUntil({ controller.view.window?.attachedSheet == nil && !BackgroundWork.occupied }, seconds: 10)
        let steps = ActionRecorder.shared.stop()
        guard let after = document.doc?.layerPixels(0) else {
            throw ProbeFailure(description: "\(label): result pixels")
        }
        try require(after != before, "\(label): Apply left the layer as it was")
        var worst = 0.0
        for i in 0..<(after.count / 4) {
            for c in 0..<3 {
                let exact = min(max(expected(Double(before[i * 4 + c])), 0), 255)
                worst = max(worst, abs(Double(after[i * 4 + c]) - exact))
            }
            try require(after[i * 4 + 3] == before[i * 4 + 3], "\(label): alpha changed at \(i)")
        }
        try require(worst <= 1, "\(label): result off the clamped pair's oracle by \(worst)")
        let clampedScale = scaleText == "3" ? 2.0 : 1.0
        let clampedOffset = offsetText == "300" ? 255.0 : 40.0
        try require(steps.count == 1 && steps[0].tool == "apply_image"
                        && (steps[0].arguments["scale"] as? Double) == clampedScale
                        && (steps[0].arguments["offset"] as? Double) == clampedOffset,
                    "\(label): the recorded step carries the clamped pair: \(steps.map(\.arguments))")
        print("PASS: \(label) applied at Scale \(clampedScale), Offset \(clampedOffset) (worst \(worst))")
    }
}
