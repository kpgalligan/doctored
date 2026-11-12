import AppKit

// The Pencil (`EditorTool.pencil`): the Brush's stroke with an aliased tip
// (`BrushTip.aliased`, the core's half-coverage cut). The canvas cases call
// `beginStroke` / `continueStroke` / `endStroke` directly, so none needs a key
// window.
//
// (a) The tool's facts: it strokes as the Brush (`strokeFamily`), has the
//     aliased tip, shares the Brush's rail slot and B key, and its stored tip
//     stays pinned whatever is written to it.
// (b) Every tool gate answers for the Pencil what it answers for the Brush
//     at 8 bits, 16 bits and 32-bit float (`DocumentPrecision.allows`).
// (c) A session with the aliased tip snaps each dab (odd diameters on a pixel
//     centre, even ones on a corner) and drops a dab that repeats the last.
// (d) `BytePaintRasterizer.pixels` of an aliased stroke holds alpha 0 and 255
//     only, and its canonical form carries `aliased` back.
// (e) The canvas, 8-bit: a Pencil drag leaves each pixel the base or the ink,
//     every pixel the live preview painted is painted the same at mouse-up,
//     and one undo restores the base. 16-bit: the stroke's job leaves each
//     sample the base or the ink.
// (f) A Pencil dab the core cannot build refuses the stroke at mouse-down
//     with its error, rather than starting a drag whose commit would hold only
//     its first dab.

func verifyPencil() throws {
    try verifyPencilFacts()
    try verifyPencilGates()
    try verifyPencilSession()
    try verifyPencilByteRasterizer()
    print("PASS: the Pencil's facts, gates, snapped session and two-valued byte overlay")
}

func verifyPencilCanvases() throws {
    try verifyPencilCanvas(sixteen: false)
    try verifyPencilCanvas(sixteen: true)
    try verifyPencilDabRefusal()
    print("PASS: a Pencil drag on the canvas paints the ink or keeps the base at 8 and 16 bits;"
          + " a dab the core cannot build refuses at mouse-down")
}

private func verifyPencilFacts() throws {
    let tag = "[pencil facts]"
    try require(EditorTool.pencil.strokeFamily == .brush && EditorTool.brush.strokeFamily == .brush
                && EditorTool.eraser.strokeFamily == .eraser, "\(tag) the Pencil strokes as the Brush")
    try require(EditorTool.pencil.aliasedTip && !EditorTool.brush.aliasedTip, "\(tag) the aliased tip")
    let slots = EditorTool.railGroups.flatMap { $0 }
    try require(slots.count == EditorTool.allCases.count && Set(slots).count == slots.count,
                "\(tag) the rail lists every tool once")
    try require(EditorTool.pencil.railGroupIndex == EditorTool.brush.railGroupIndex,
                "\(tag) the Pencil shares the Brush's slot")
    try require(EditorTool.tool(forKey: "b", current: .brush, last: nil) == .pencil
                && EditorTool.tool(forKey: "b", current: .pencil, last: nil) == .brush,
                "\(tag) B cycles the Brush and the Pencil")
    let store = ToolOptionsStore.shared
    let saved = store.pencil
    defer { store.pencil = saved }
    var bent = PaintToolOptions(size: 12, hardness: 30, opacity: 70, flow: 40, spacing: 90,
                                angle: 45, roundness: 20)
    bent.airbrush = true
    store.setPaintOptions(bent, for: .pencil)
    guard let read = store.paintOptions(for: .pencil) else {
        throw ProbeFailure(description: "\(tag) the Pencil has paint options")
    }
    try require(read.size == 12 && read.opacity == 70 && read.hardness == 100 && read.flow == 100
                && read.spacing == Double(BrushTip.aliasedSpacingPercent) && read.angle == 0
                && read.roundness == 100 && !read.airbrush,
                "\(tag) size and opacity keep, the tip stays pinned: \(read)")
}

private func verifyPencilGates() throws {
    let tag = "[pencil gates]"
    let documents = [("8-bit", try fixture(width: 64, height: 48)),
                     ("16-bit", try precisionFixture(width: 64, height: 48)),
                     ("float", try floatFixture())]
    defer { documents.forEach { $0.1.close() } }
    for (name, document) in documents {
        let brush = DocumentPrecision.allows(.brush, document: document)
        try require(brush && DocumentPrecision.allows(.pencil, document: document) == brush,
                    "\(tag) \(name): the Pencil is offered where the Brush is")
    }
}

private func verifyPencilSession() throws {
    let tag = "[pencil session]"
    let tip = BrushTip(hardness: 1, flow: 1, spacingPercent: BrushTip.aliasedSpacingPercent,
                       aliased: true)
    var odd = NativeBrushStrokeSession(tip: tip, size: 7, pressureOpacity: false)
    odd.stamp(at: CGPoint(x: 10.3, y: 5.8), diameter: 7, pressure: 1)
    odd.stamp(at: CGPoint(x: 10.9, y: 5.1), diameter: 7.2, pressure: 1)
    odd.stamp(at: CGPoint(x: 11.2, y: 5.5), diameter: 7, pressure: 1)
    try require(odd.dabs == [10.5, 5.5, 7, 1, 11.5, 5.5, 7, 1],
                "\(tag) odd dabs land on pixel centres and a repeat is dropped: \(odd.dabs)")
    var even = NativeBrushStrokeSession(tip: tip, size: 6, pressureOpacity: false)
    even.stamp(at: CGPoint(x: 10.6, y: 5.4), diameter: 6.4, pressure: 1)
    try require(even.dabs == [11, 5, 6, 1], "\(tag) an even dab lands on a pixel corner: \(even.dabs)")
    var soft = tip
    soft.hardness = 0.5
    var refusal = ""
    do { _ = try NativeBrushStroke(tip: soft, pixel: false, dabs: [4, 4, 5, 1]) }
    catch { refusal = error.localizedDescription }
    try require(refusal == NativeBrushStroke.aliasedHardnessRefusal,
                "\(tag) a soft aliased tip refuses by name: \(refusal)")
}

private func verifyPencilByteRasterizer() throws {
    let tag = "[pencil byte rasterizer]"
    let tip = BrushTip(hardness: 1, flow: 1, spacingPercent: BrushTip.aliasedSpacingPercent,
                       aliased: true)
    let stroke = try NativeBrushStroke(tip: tip, pixel: false,
                                       dabs: [10.5, 8.5, 9, 1, 14, 9, 10, 1, 19.5, 10.5, 7, 0.6])
    let ink = NSColor(srgbRed: 0.8, green: 0.2, blue: 0.4, alpha: 1)
    let pixels = try BytePaintRasterizer.pixels(stroke: stroke, size: 9, ink: .color(ink),
        width: 32, height: 20, space: ColorProfile.sRGB, selection: nil, coverageOpacity: nil)
    let alphas = Set(stride(from: 3, to: pixels.count, by: 4).map { pixels[$0] })
    // The 0.6 deposit alone is round(round(0.6 · 65535) · 255 / 65535) = 153.
    try require(alphas == [0, 153, 255],
                "\(tag) the overlay's alpha is the deposits' levels only: \(alphas.sorted())")
    let full = try BytePaintRasterizer.pixels(
        stroke: try NativeBrushStroke(tip: tip, pixel: false, dabs: [10.5, 8.5, 9, 1, 14, 9, 10, 1]),
        size: 9, ink: .color(ink), width: 32, height: 20, space: ColorProfile.sRGB,
        selection: nil, coverageOpacity: nil)
    let fullAlphas = Set(stride(from: 3, to: full.count, by: 4).map { full[$0] })
    try require(fullAlphas == [0, 255], "\(tag) a deposit-1 overlay is two-valued: \(fullAlphas.sorted())")
    let canonical = try stroke.canonical
    let back = try NativeBrushStroke(canonical: canonical)
    try require(canonical["aliased"] as? Bool == true && back.tip.aliased,
                "\(tag) the canonical stroke carries aliased back")
    var plain = canonical
    plain.removeValue(forKey: "aliased")
    let brush = try NativeBrushStroke(canonical: plain)
    try require(!brush.tip.aliased, "\(tag) a canonical stroke without the key is the Brush's")
}

private func verifyPencilCanvas(sixteen: Bool) throws {
    let tag = sixteen ? "[pencil canvas 16-bit]" : "[pencil canvas 8-bit]"
    let document = try sixteen ? precisionFixture(width: 80, height: 60) : fixture(width: 80, height: 60)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base document") }
    let store = ToolOptionsStore.shared
    let saved = store.pencil
    defer { store.pencil = saved }
    var options = PaintToolOptions(size: 9, smoothing: 0)
    options.pressureSize = false
    store.setPaintOptions(options, for: .pencil)
    controller.selectTool(.pencil)
    try require(controller.currentTool == .pencil && canvas.tool == .pencil, "\(tag) the Pencil is selected")
    canvas.paintColor = NSColor(srgbRed: 0.85, green: 0.15, blue: 0.3, alpha: 1)
    canvas.brushOpacity = 1
    try require(canvas.brushSize == 9 && canvas.brushHardness == 1 && canvas.brushFlow == 1,
                "\(tag) the canvas takes the Pencil's pinned tip")
    let history = document.history.entries.count
    canvas.beginStroke(at: CGPoint(x: 12.3, y: 20.7))
    try require(canvas.strokeActive && canvas.strokeTip.aliased, "\(tag) the stroke began aliased")
    for point in [CGPoint(x: 30.2, y: 24.1), CGPoint(x: 48.6, y: 31.9), CGPoint(x: 66.1, y: 38.4)] {
        canvas.continueStroke(to: point, pressure: nil)
    }
    let preview = sixteen ? nil : document.doc?.layerPixels(0)
    canvas.endStroke()
    if sixteen {
        guard let job = DocumentJobs.active else {
            throw ProbeFailure(description: "\(tag) the stroke started no job")
        }
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "\(tag) the stroke's job completed: \(job.report)")
    }
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "\(tag) the stroke published nothing")
    }
    try require(document.history.entries.count == history + 1, "\(tag) one history entry")
    if sixteen {
        let before = try precisionLayerSamples(base), after = try precisionLayerSamples(painted)
        let n = before.count / 2
        let changed = (0..<n).filter { before[$0] != after[$0] || before[n + $0] != after[n + $0] }
        let inks = Set(changed.map { [after[$0], after[n + $0]] })
        try require(changed.count > 200 && inks.count == 1,
                    "\(tag) \(changed.count) samples changed, to \(inks.count) values")
    } else {
        guard let before = base.layerPixels(0), let after = painted.layerPixels(0),
              let preview else {
            throw ProbeFailure(description: "\(tag) no layer pixels")
        }
        let pixels = before.count / 4
        let changed = (0..<pixels).filter { Array(before[$0 * 4..<$0 * 4 + 4]) != Array(after[$0 * 4..<$0 * 4 + 4]) }
        let inks = Set(changed.map { Array(after[$0 * 4..<$0 * 4 + 4]) })
        try require(changed.count > 200 && inks.count == 1,
                    "\(tag) \(changed.count) pixels changed, to \(inks.count) values")
        let previewed = (0..<pixels).filter { Array(before[$0 * 4..<$0 * 4 + 4]) != Array(preview[$0 * 4..<$0 * 4 + 4]) }
        let shifted = previewed.filter { Array(preview[$0 * 4..<$0 * 4 + 4]) != Array(after[$0 * 4..<$0 * 4 + 4]) }
        try require(previewed.count > 100 && shifted.isEmpty,
                    "\(tag) the preview's \(previewed.count) pixels hold at mouse-up; \(shifted.count) moved")
    }
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the base")
}

/// (f): the lever is a size past the stroke's own cap, which the options bar
/// never sets, so the canvas is set directly; the preview dab's
/// `NativeBrushStroke` then refuses, as a failing 1-dab overlay would.
private func verifyPencilDabRefusal() throws {
    let tag = "[pencil dab refusal]"
    let document = try fixture(width: 80, height: 60)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let store = ToolOptionsStore.shared
    let saved = store.pencil
    defer { store.pencil = saved }
    store.setPaintOptions(PaintToolOptions(size: 9, smoothing: 0), for: .pencil)
    controller.selectTool(.pencil)
    let base = document.doc
    let history = document.history.entries.count
    canvas.brushSize = CGFloat(NativeBrushStroke.maximumDiameter + 1)
    AutomationAlerts.reset()
    canvas.beginStroke(at: CGPoint(x: 20.3, y: 20.7))
    let began = canvas.strokeActive
    if began { canvas.cancelStroke() }
    let errors = AutomationAlerts.recorded.filter { $0.kind == .errorModal }
    AutomationAlerts.reset()
    try require(!began && errors.count == 1
                && errors[0].messageText.contains("diameter or opacity are outside"),
                "\(tag) mouse-down refuses with the dab's error: began \(began), "
                + "\(errors.map(\.messageText))")
    try require(document.doc === base && document.history.entries.count == history,
                "\(tag) the document and its history are unchanged")
}
