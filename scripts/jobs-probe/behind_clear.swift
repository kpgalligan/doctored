import AppKit

// Behind and Clear in the UI (`PaintOnlyBlend`, the core's `paint_mode` rule). The canvas cases
// call `beginStroke` / `continueStroke` / `endStroke` directly, so none needs a key window.
//
// (a) The Mode lists: the Brush (and the Pencil, which strokes as it) offers Behind and Clear on
//     an RGB layer, the Clone Stamp does not, and a coverage target refuses them by name.
// (b) Edit ▸ Fill…'s sheet lists them only when asked (a layer target), and choosing either
//     turns Preserve Transparency off and disables it; another mode enables it again.
// (c) The 8-bit canvas: a Clear stroke lowers alpha and keeps every colour byte; a Behind stroke
//     then raises the cleared alpha and leaves every pixel the Clear left opaque; the recorded
//     steps name the modes. On a transparency-locked layer the Behind stroke refuses at mouse-down
//     by name and nothing changes.
// (d) A 32-bit float layer: the Brush begins and commits a Clear stroke (the float mode gate
//     admits the two), keeping the linear colour and halving alpha at opacity 0.5.

func verifyBehindClear() throws {
    try verifyBehindClearLists()
    try verifyBehindClearSheet()
    try verifyBehindClearByteCanvas()
    try verifyBehindClearFloatBrush()
    print("PASS: Behind and Clear in the Mode lists, the Fill sheet, an 8-bit and a float Brush")
}

/// The Brush store for one case, restored afterwards; `mode` is stored as its raw value.
private func withBrushMode(_ mode: DrBlendMode, _ body: () throws -> Void) throws {
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved }
    var options = PaintToolOptions()
    options.smoothing = 0
    options.pressureSize = false
    options.blendIndex = Int(mode.rawValue)
    store.brush = options
    try body()
}

private func verifyBehindClearLists() throws {
    let tag = "[behind clear lists]"
    let document = try fixture(width: 40, height: 30)
    defer { document.close() }
    let doc = document.doc
    let brush = EditorViewController.paintBlendChoices(for: doc, tool: .brush).map(\.0)
    try require(brush.contains(DR_BLEND_BEHIND) && brush.contains(DR_BLEND_CLEAR),
                "\(tag) the Brush lists Behind and Clear")
    let clone = EditorViewController.paintBlendChoices(for: doc, tool: .clone).map(\.0)
    try require(!clone.contains(DR_BLEND_BEHIND) && !clone.contains(DR_BLEND_CLEAR),
                "\(tag) the Clone Stamp keeps the layer modes")
    let coverage = PaintOnlyBlend.refusal(DR_BLEND_CLEAR, doc: doc, layer: 0, coverage: true,
                                          preserve: false) ?? ""
    try require(coverage.hasPrefix("The Clear blend mode paints a layer's transparency"),
                "\(tag) a coverage target refuses by name: \(coverage)")
}

/// Every view under `view`, depth first.
private func descendants(_ view: NSView) -> [NSView] {
    view.subviews.flatMap { [$0] + descendants($0) }
}

private func verifyBehindClearSheet() throws {
    let tag = "[behind clear sheet]"
    for offered in [true, false] {
        let sheet = FillSheetController(
            coverage: false, quickMask: false, options: FillDialogOptions(),
            paintOnly: offered) { _ in }
        let views = descendants(sheet.view)
        guard let popup = views.compactMap({ $0 as? NSPopUpButton })
                .first(where: { $0.itemTitles.contains("Multiply") }),
              let preserve = views.compactMap({ $0 as? NSButton })
                .first(where: { $0.title == "Preserve Transparency" })
        else { throw ProbeFailure(description: "\(tag) the Mode popup and the checkbox") }
        let listed = popup.itemTitles.contains("Behind") && popup.itemTitles.contains("Clear")
        try require(listed == offered, "\(tag) offered \(offered): \(popup.itemTitles)")
        guard offered else { continue }
        preserve.state = .on
        popup.selectItem(withTitle: "Behind")
        _ = popup.sendAction(popup.action, to: popup.target)
        try require(!preserve.isEnabled && preserve.state == .off,
                    "\(tag) Behind turns Preserve Transparency off")
        popup.selectItem(withTitle: "Normal")
        _ = popup.sendAction(popup.action, to: popup.target)
        try require(preserve.isEnabled, "\(tag) Normal enables it again")
    }
}

/// One stroke across the canvas through the editor's own canvas.
private func stroke(_ canvas: ImageCanvasView) {
    canvas.beginStroke(at: CGPoint(x: 6.5, y: 15.5))
    canvas.continueStroke(to: CGPoint(x: 30.5, y: 15.5), pressure: nil)
    canvas.endStroke()
}

private func verifyBehindClearByteCanvas() throws {
    let tag = "[behind clear 8-bit]"
    let document = try fixture(width: 40, height: 30)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, let original = base.layerPixels(0) else {
        throw ProbeFailure(description: "\(tag) no base")
    }
    ActionRecorder.shared.start()
    try withBrushMode(DR_BLEND_CLEAR) {
        controller.selectTool(.brush)
        canvas.brushSize = 8
        canvas.brushOpacity = 0.5
        stroke(canvas)
    }
    guard let cleared = document.doc?.layerPixels(0), document.doc !== base else {
        throw ProbeFailure(description: "\(tag) the Clear stroke published nothing")
    }
    let pixels = original.count / 4
    let px = { (buffer: [UInt8], i: Int) in Array(buffer[i * 4..<i * 4 + 4]) }
    let lowered = (0..<pixels).filter { px(cleared, $0) != px(original, $0) }
    try require(lowered.count > 40, "\(tag) the Clear stroke moved \(lowered.count) pixels")
    for i in lowered {
        let (b, a) = (px(original, i), px(cleared, i))
        try require(Array(a.prefix(3)) == Array(b.prefix(3)) && a[3] < b[3],
                    "\(tag) Clear keeps colour and lowers alpha at \(i): \(b) → \(a)")
    }
    try withBrushMode(DR_BLEND_BEHIND) {
        controller.selectTool(.brush)
        canvas.brushSize = 8
        canvas.brushOpacity = 1
        stroke(canvas)
    }
    guard let behind = document.doc?.layerPixels(0) else {
        throw ProbeFailure(description: "\(tag) no pixels after Behind")
    }
    var raised = 0
    for i in 0..<pixels {
        let (b, a) = (px(cleared, i), px(behind, i))
        if b[3] == 255 { try require(a == b, "\(tag) Behind left opaque pixel \(i): \(b) → \(a)") }
        try require(a[3] >= b[3], "\(tag) Behind never lowers alpha at \(i): \(b) → \(a)")
        raised += a[3] > b[3] ? 1 : 0
    }
    try require(raised > 20, "\(tag) Behind raised the cleared alpha at \(raised) pixels")
    let steps = ActionRecorder.shared.stop().filter { $0.tool == "brush_stroke" }
    let modes = steps.compactMap { $0.arguments["blend_mode"] as? String }
    try require(modes == ["Clear", "Behind"], "\(tag) the recorded steps name the modes: \(modes)")

    // A transparency-locked layer refuses the Behind stroke at mouse-down, by name.
    _ = try agentOK("set_layer_lock", document, ["layer": 0, "locks": ["transparency"]])
    let locked = document.doc
    let history = document.history.entries.count
    AutomationAlerts.reset()
    try withBrushMode(DR_BLEND_BEHIND) {
        controller.selectTool(.brush)
        canvas.beginStroke(at: CGPoint(x: 6.5, y: 15.5))
        let began = canvas.strokeActive
        if began { canvas.cancelStroke() }
        let said = AutomationAlerts.recorded.map(\.messageText)
        AutomationAlerts.reset()
        try require(!began && said.contains { $0.contains("transparency lock keeps") },
                    "\(tag) the locked stroke refuses by name: began \(began), \(said)")
    }
    try require(document.doc === locked && document.history.entries.count == history,
                "\(tag) the refusal changed nothing")
}

/// The composite's straight linear RGBA at one pixel.
private func floatSample(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Double] {
    guard let sample = try FloatReadout.at((x: x, y: y), document: document) else {
        throw ProbeFailure(description: "No float sample at (\(x), \(y))")
    }
    return sample.values
}

private func verifyBehindClearFloatBrush() throws {
    let tag = "[behind clear float]"
    let document = try floatFixture()
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let before = try floatSample(document, 18, 15)
    try withBrushMode(DR_BLEND_CLEAR) {
        controller.selectTool(.brush)
        canvas.brushSize = 8
        canvas.brushOpacity = 0.5
        canvas.beginStroke(at: CGPoint(x: 6.5, y: 15.5))
        try require(canvas.strokeActive, "\(tag) the float Clear stroke began")
        canvas.continueStroke(to: CGPoint(x: 30.5, y: 15.5), pressure: nil)
        canvas.endStroke()
    }
    guard let job = DocumentJobs.active else {
        throw ProbeFailure(description: "\(tag) the stroke started no job")
    }
    try pumpUntil({ job.state != "running" })
    try require(job.state == "completed", "\(tag) the stroke's job completed: \(job.report)")
    let after = try floatSample(document, 18, 15)
    try require(Array(after.prefix(3)) == Array(before.prefix(3))
                && abs(after[3] - before[3] * 0.5) <= 1e-6,
                "\(tag) Clear keeps the linear colour and halves alpha: \(before) → \(after)")
}
