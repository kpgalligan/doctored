import AppKit

// The Brush, Eraser and Gradient in a 32-bit float document, through
// the canvas's own `beginStroke(at:)` / `continueStroke(to:pressure:)` /
// `endStroke()` and the Gradient tool's commit, so the case needs no key
// window. A layer paints in linear light (a foreground above white as its own
// numbers, recorded as `rgba32` / `start_rgba32`); its mask is u16 coverage,
// through the routes the 16-bit tools use.

private let floatInk: [Double] = [4, 0.5, 0.25, 1]

/// One float stroke through the canvas: press, one drag, release.
private func floatStroke(_ controller: EditorViewController, from: CGPoint, to: CGPoint) throws {
    let canvas: ImageCanvasView = controller.canvas
    canvas.beginStroke(at: from)
    try require(canvas.strokeActive, "The float \(canvas.tool) stroke began")
    canvas.continueStroke(to: to, pressure: nil)
    canvas.endStroke()
}

/// The job the last stroke started, pumped to its end.
private func finishedStrokeJob(_ what: String) throws -> DocumentJob {
    guard let job = DocumentJobs.active else {
        throw ProbeFailure(description: "\(what) started no job")
    }
    try pumpUntil({ job.state != "running" })
    return job
}

/// The composite's straight linear RGBA at one pixel.
private func floatPixel(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Double] {
    guard let sample = try FloatReadout.at((x: x, y: y), document: document) else {
        throw ProbeFailure(description: "No float sample at (\(x), \(y))")
    }
    return sample.values
}

private func near(_ got: [Double], _ want: [Double]) -> Bool {
    got.count == want.count
        && zip(got, want).allSatisfy { abs($0 - $1) <= 1e-4 * max(1, abs($1)) }
}

private func maskPlane(_ document: ImageDocument) throws -> [UInt16] {
    guard let doc = document.doc else { throw ProbeFailure(description: "No float document") }
    return try PrecisionScalarOperations.read(doc, target: "mask", index: 0).samples
}

/// The brush, eraser and gradient stores for one case, restored afterwards.
private func withFloatPaintOptions(_ body: () throws -> Void) throws {
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.eraser, store.gradient)
    defer { (store.brush, store.eraser, store.gradient) = saved }
    var tip = PaintToolOptions()
    tip.smoothing = 0
    tip.pressureSize = false
    store.brush = tip
    store.eraser = tip
    store.gradient = GradientToolOptions()
    try body()
}

func verifyFloatPaintStrokes() throws {
    let document = try floatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "No float base") }
    let (width, height) = (base.width, base.height)
    let colors = (controller.paintColor, controller.backgroundColor)
    defer { (controller.paintColor, controller.backgroundColor) = colors }
    guard let hdr = ExtendedRangeColor.linearColor(floatInk, in: base) else {
        throw ProbeFailure(description: "The above-white foreground did not build")
    }

    // The tool rail: the three tools are admitted, and the
    // bucket and Dodge too.
    for tool in [EditorTool.brush, .eraser, .gradient, .fill, .dodge] {
        try require(DocumentPrecision.allows(tool, document: document),
                    "[float] \(tool) is admitted")
    }

    try withFloatPaintOptions {
        let from = CGPoint(x: 10.5, y: 16.5), to = CGPoint(x: 38.5, y: 16.5)
        let centre = (x: 24, y: 16)
        let before = try floatPixel(document, centre.x, centre.y)

        // (a) A Brush stroke with a foreground above white: one job, the
        // ink's own numbers, one undo, recorded as `rgba32`.
        controller.selectTool(.brush)
        try require(controller.currentTool == .brush, "[float] the Brush is selected")
        controller.paintColor = hdr
        canvas.paintColor = hdr
        canvas.brushSize = 8
        canvas.brushOpacity = 1
        ActionRecorder.shared.start()
        try floatStroke(controller, from: from, to: to)
        guard DocumentJobs.active?.title == "Brush Stroke" else {
            throw ProbeFailure(description: "[float] the Brush stroke started no Brush Stroke job: "
                + "\(DocumentJobs.active?.report ?? [:])")
        }
        let brush = try finishedStrokeJob("[float] the Brush stroke")
        let brushSteps = ActionRecorder.shared.stop()
        try require(brush.state == "completed" && document.doc !== base,
                    "[float] the Brush stroke completed: \(brush.report)")
        let painted = try floatPixel(document, centre.x, centre.y)
        try require(near(painted, floatInk),
                    "[float] the stroke's centre reads the ink above white: \(painted)")
        let strokes = brushSteps.filter { $0.tool == "brush_stroke" }
        let recorded = strokes.first?.arguments["rgba32"] as? [Double] ?? []
        try require(strokes.count == 1 && near(recorded, floatInk)
                    && ["color", "rgba", "color_space"].allSatisfy { strokes[0].arguments[$0] == nil },
                    "[float] one brush_stroke step carrying rgba32 alone: "
                    + "\(brushSteps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "[float] the Brush stroke is exactly one undo")

        // (b) An Eraser stroke lowers alpha and keeps the colour.
        controller.selectTool(.eraser)
        canvas.brushSize = 8
        canvas.brushOpacity = 0.5
        try floatStroke(controller, from: from, to: to)
        let eraser = try finishedStrokeJob("[float] the Eraser stroke")
        try require(eraser.state == "completed", "[float] the Eraser stroke completed: \(eraser.report)")
        let erased = try floatPixel(document, centre.x, centre.y)
        try require(Array(erased.prefix(3)) == Array(before.prefix(3))
                    && abs(erased[3] - before[3] * 0.5) <= 1e-6,
                    "[float] the Eraser halves alpha and keeps RGB: \(before) → \(erased)")
        document.undoManager?.undo()
        try require(document.doc === base, "[float] the Eraser stroke is one undo")

        // (c) A stroke cancelled before the first poll applies nothing.
        canvas.brushOpacity = 1
        try floatStroke(controller, from: from, to: to)
        guard let cancelled = DocumentJobs.active else {
            throw ProbeFailure(description: "[float] the cancel case started no job")
        }
        cancelled.control.cancel()
        try pumpUntil({ cancelled.state != "running" })
        try require(cancelled.state == "cancelled" && document.doc === base
                    && document.undoManager?.canUndo != true,
                    "[float] a cancelled stroke applies nothing: \(cancelled.report)")

        // (d) With the mask targeted the Eraser edits u16 coverage: one step.
        _ = try agentOK("add_layer_mask", document, ["layer": 0])
        document.undoManager?.removeAllActions()
        guard let masked = document.doc else { throw ProbeFailure(description: "No masked base") }
        let revealAll = try maskPlane(document)
        try require(revealAll.count == width * height && revealAll.allSatisfy { $0 == 65535 },
                    "[float] the fresh mask reveals all")
        controller.setPaintTarget(.mask)
        try require(document.planeEditTarget == .mask, "[float] the panel targets the mask")
        try floatStroke(controller, from: from, to: to)
        let maskJob = try finishedStrokeJob("[float] the mask Eraser stroke")
        try require(maskJob.state == "completed",
                    "[float] the mask Eraser stroke completed: \(maskJob.report)")
        let erasedMask = try maskPlane(document)
        try require(erasedMask != revealAll && erasedMask[centre.y * width + centre.x] == 0,
                    "[float] the Eraser zeroed the mask in the stroke's core")
        document.undoManager?.undo()
        try require(document.doc === masked && document.undoManager?.canUndo != true,
                    "[float] the mask stroke is exactly one undo")

        // (e) The Gradient on the mask: the coverage ramp, no layer pixel,
        // one step recorded with `target: mask` and no linear endpoint.
        controller.selectTool(.gradient)
        let black = NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 1)
        controller.paintColor = black
        controller.backgroundColor = NSColor(srgbRed: 1, green: 1, blue: 1, alpha: 1)
        let left = CGPoint(x: 0.5, y: 16), right = CGPoint(x: CGFloat(width) - 0.5, y: 16)
        ActionRecorder.shared.start()
        controller.gradientPrecisionColor(from: left, to: right)
        let maskSteps = ActionRecorder.shared.stop()
        let ramp = try maskPlane(document)
        try require(ramp != revealAll && ramp[16 * width] == 0 && ramp[16 * width + width - 1] == 65535,
                    "[float] the mask holds the black-to-white ramp: \(ramp[16 * width]) … "
                    + "\(ramp[16 * width + width - 1])")
        let maskGradients = maskSteps.filter { $0.tool == "gradient" }
        try require(maskGradients.count == 1
                    && maskGradients[0].arguments["target"] as? String == "mask"
                    && maskGradients[0].arguments["start_rgba32"] == nil,
                    "[float] one gradient step on the mask with no linear endpoint: "
                    + "\(maskSteps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
        // Mask off, the composite is the layer's own pixels: untouched.
        _ = try agentOK("set_layer_mask_enabled", document, ["layer": 0, "enabled": false])
        let unmasked = try floatPixel(document, centre.x, centre.y)
        try require(unmasked == before, "[float] the mask gradient changed no layer pixel: \(unmasked)")
        document.undoManager?.undo()
        document.undoManager?.undo()
        try require(document.doc === masked && document.undoManager?.canUndo != true,
                    "[float] the mask gradient is exactly one undo")

        // (f) The Gradient on the layer with that foreground: linear
        // endpoints, recorded as start_rgba32 / end_rgba32.
        controller.setPaintTarget(.layer)
        controller.paintColor = hdr
        controller.backgroundColor = black
        ActionRecorder.shared.start()
        controller.gradientPrecisionColor(from: left, to: right)
        let layerSteps = ActionRecorder.shared.stop()
        let gradients = layerSteps.filter { $0.tool == "gradient" }
        let start = gradients.first?.arguments["start_rgba32"] as? [Double] ?? []
        let end = gradients.first?.arguments["end_rgba32"] as? [Double] ?? []
        try require(gradients.count == 1 && near(start, floatInk) && near(end, [0, 0, 0, 1])
                    && ["start_rgba", "end_rgba", "color_space", "stops"]
                        .allSatisfy { gradients[0].arguments[$0] == nil },
                    "[float] one gradient step carrying the linear endpoints alone: "
                    + "\(layerSteps.map { "\($0.tool) \($0.arguments)" })")
        let first = try floatPixel(document, 0, 16), last = try floatPixel(document, width - 1, 16)
        try require(near(first, floatInk) && near(last, [0, 0, 0, 1]),
                    "[float] the ramp lands its endpoint values: \(first) … \(last)")
        document.undoManager?.undo()
        try require(document.doc === masked, "[float] the layer gradient is one undo")

        // (g) A Brush whose stored mode is Screen starts no stroke and says why.
        var screen = ToolOptionsStore.shared.brush
        screen.blendIndex = Int(DR_BLEND_SCREEN.rawValue)
        ToolOptionsStore.shared.brush = screen
        controller.selectTool(.brush)
        AutomationAlerts.reset()
        canvas.beginStroke(at: from)
        let started = canvas.strokeActive
        if started { canvas.endStroke() }
        try require(!started && DocumentJobs.active == nil && document.doc === masked,
                    "[float] a Screen brush starts no stroke")
        try require(AutomationAlerts.recorded.count == 1
                    && AutomationAlerts.recorded.contains {
                        ($0.messageText + " " + $0.informativeText).contains("Screen blend mode")
                    },
                    "[float] the refused mode is explained once: \(AutomationAlerts.recorded)")
    }
    print("PASS: float-paint-strokes — in a 32-bit float document the Brush, Eraser and Gradient "
        + "are admitted (the bucket and Dodge too); a Brush stroke with a "
        + "(4, 0.5, 0.25) foreground is one Brush Stroke job whose centre reads those numbers, one "
        + "undo, recorded as rgba32; an Eraser stroke halves alpha and keeps RGB; a cancelled "
        + "stroke applies nothing; with the mask targeted the Eraser and the Gradient edit the u16 "
        + "mask as one step each and no layer pixel; a layer Gradient records start_rgba32 / "
        + "end_rgba32 and lands them; a Screen brush starts no stroke and says why")
}
