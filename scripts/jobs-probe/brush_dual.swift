import AppKit

// Brush dynamics' dual brush (`designs/brush-dynamics.md`, slice 4), off screen, one call from
// `verifyBrushDynamics()`. Each row runs on its own and every failing row is reported, so one
// run shows each red.
//
// (a) The stored options: the Brush and the Eraser keep `dual`, the Pencil's options drop it
//     (keeping the rest), Dodge keeps none; the value round-trips through its JSON; a dual preset
//     round-trips at schema 2 and is absent from the Pencil's list while the Brush's lists it.
// (b) The Dynamics… sheet shows the Dual Brush rows with the options' values for the Brush and
//     none for the Pencil.
// (c) The tick rule: the same nominal points stamped with and without interleaved airbrush
//     ticks (`tick: true`, at points off the path) leave equal second dabs and anchors, while the
//     ticked session's first list is longer.
// (d) The size-1 charge: in a stop-ink session a hard 1 px second tip's one dab raises the work
//     by `workCharge(diameter: 1, tip: hard, size: 2)` = 1033, at least the core's 256 for its
//     2 × 2 box (`min(64 · box, box + 64 · band_bound(1))`).
// (e) The preview ceiling: `dualPreviewFigure` for one 2,000 px box under one 2,000 px first dab
//     and 200 coincident 2,000 px second dabs is over `dualPreviewWork`; for one 40 px dab and
//     one 10 px second dab it is under. The scan is bounded: it charges one unit per dab
//     examined and stops once its limit is passed, so 100,000 dabs far from the box cost the
//     limit plus one.
// (f) A dual Brush canvas stroke at 8 and 16 bits records a `native_stroke` holding `dual`, and
//     its commit equals the replay of that recorded step and differs from the replay without the
//     second tip.

func verifyBrushDual() throws {
    var failures: [String] = []
    let rows: [(String, () throws -> Void)] = [
        ("options", verifyDualOptions), ("sheet", verifyDualSheet),
        ("tick rule", verifyDualTicks), ("size-1 charge", verifyDualCharge),
        ("preview figure", verifyDualPreviewFigure),
        ("canvas 8-bit", { try verifyDualCanvas(sixteen: false) }),
        ("canvas 16-bit", { try verifyDualCanvas(sixteen: true) }),
    ]
    for (name, row) in rows {
        do { try row() } catch { failures.append("\(name): \(error)") }
    }
    guard failures.isEmpty else {
        throw ProbeFailure(description: failures.joined(separator: "\n"))
    }
    print("PASS: the dual brush's options, preset, sheet, tick rule, size-1 charge, preview "
          + "figure and canvas commits")
}

private let dualOnly: BrushDynamics = {
    var d = BrushDynamics()
    d.dual = BrushDynamics.Dual(size: 6, hardness: 1, roundness: 1, angleDegrees: 0, spacing: 100,
                                scatter: 0.5, bothAxes: false, count: 2, countJitter: 0)
    return d
}()

private func verifyDualOptions() throws {
    let tag = "[dual options]"
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.eraser, store.pencil, store.dodge)
    defer { (store.brush, store.eraser, store.pencil, store.dodge) = saved }
    var both = dualOnly
    both.scatter = 2
    var options = PaintToolOptions(size: 9, smoothing: 0)
    options.dynamics = both
    for tool in [EditorTool.brush, .eraser, .pencil, .dodge] {
        store.setPaintOptions(options, for: tool)
    }
    try require(store.paintOptions(for: .brush)?.dynamics == both
                && store.paintOptions(for: .eraser)?.dynamics == both,
                "\(tag) the Brush and the Eraser keep the dual brush")
    let pencil = store.paintOptions(for: .pencil)?.dynamics
    try require(pencil?.dual == nil && pencil?.scatter == 2,
                "\(tag) the Pencil drops the dual brush and keeps the rest: \(String(describing: pencil))")
    try require(store.paintOptions(for: .dodge)?.dynamics == nil, "\(tag) Dodge keeps none")
    let roundTrip = try BrushDynamics(json: dualOnly.json)
    try require(roundTrip == dualOnly, "\(tag) the dual brush round-trips through its JSON")
    let blob = try JSONDecoder().decode(PaintToolOptions.self,
                                        from: JSONEncoder().encode(options))
    try require(blob.dynamics == both, "\(tag) the options blob keeps the dual brush")
    let mine = BrushPreset(name: "Dual", hardness: 100, flow: 100, spacing: 25, angle: 0,
                           roundness: 100, airbrush: false, dynamics: dualOnly)
    let decoded = try BrushPreset.decode(mine.json)
    try require(mine.json["schema_version"] as? Int == 2 && decoded.dynamics == dualOnly,
                "\(tag) a dual preset round-trips at schema 2")
    try BrushPresetLibrary.save(name: "zz Dual Probe", values: EditorViewController.brushPresetValues(
        BrushPreset(name: "zz Dual Probe", hardness: 65, flow: 100, spacing: 25, angle: 0,
                    roundness: 100, airbrush: false, dynamics: dualOnly).applied(to: PaintToolOptions())),
        replace: true)
    defer { try? BrushPresetLibrary.delete(name: "zz Dual Probe") }
    BrushPresetLibrary.reload()
    try require(!BrushPreset.all(dynamics: true, shapes: EditorTool.pencil.takesTipShapeDynamics)
                    .contains { $0.name == "zz Dual Probe" }
                && BrushPreset.all().contains { $0.name == "zz Dual Probe" },
                "\(tag) the Pencil's list leaves a dual preset out; the Brush's lists it")
}

/// The view under `root` whose accessibility identifier is `id`.
private func dualSubview(_ id: String, in root: NSView) -> NSView? {
    if root.accessibilityIdentifier() == id { return root }
    for child in root.subviews {
        if let found = dualSubview(id, in: child) { return found }
    }
    return nil
}

private func verifyDualSheet() throws {
    let tag = "[dual sheet]"
    var options = PaintToolOptions(size: 9, smoothing: 0)
    options.dynamics = dualOnly
    let brush = BrushDynamicsSheetController(options: options, shapes: true, onApply: { _ in })
    guard let enabled = dualSubview("brush-dynamics-dual", in: brush.view) as? NSButton,
          let size = dualSubview("brush-dynamics-dual-size", in: brush.view) as? NSTextField,
          let spacing = dualSubview("brush-dynamics-dual-spacing", in: brush.view) as? NSTextField,
          let count = dualSubview("brush-dynamics-dual-count", in: brush.view) as? NSTextField,
          dualSubview("brush-dynamics-dual-both-axes", in: brush.view) is NSButton else {
        throw ProbeFailure(description: "\(tag) the Brush's sheet has no Dual Brush rows")
    }
    try require(enabled.state == .on && size.stringValue == "6" && spacing.stringValue == "100"
                && count.stringValue == "2",
                "\(tag) the Brush's sheet shows the dual brush: \(enabled.state.rawValue) "
                + "\(size.stringValue) \(spacing.stringValue) \(count.stringValue)")
    let pencil = BrushDynamicsSheetController(options: options, shapes: false, onApply: { _ in })
    try require(dualSubview("brush-dynamics-dual", in: pencil.view) == nil
                && dualSubview("brush-dynamics-dual-size", in: pencil.view) == nil,
                "\(tag) the Pencil's sheet shows no Dual Brush rows")
}

/// A Brush session with `dynamics`' handles at `seed`, its dual brush attached.
private func dualSession(_ dynamics: BrushDynamics, tip: BrushTip, size: CGFloat, seed: UInt32,
                         overflow: NativeBrushStrokeSession.Overflow = .refuse) throws
    -> NativeBrushStrokeSession {
    var session = NativeBrushStrokeSession(tip: tip, size: size, pressureOpacity: false,
                                           overflow: overflow)
    session.dynamics = try BrushDynamics.handle(dynamics, size: size, seed: seed,
                                                pressureSize: false, pressureOpacity: false,
                                                tip: tip)
    session.attachDual(try BrushDynamics.dualHandle(dynamics, size: size, seed: seed))
    return session
}

private func verifyDualTicks() throws {
    let tag = "[dual tick rule]"
    var d = dualOnly
    d.dual?.spacing = 50
    let tip = BrushTip(hardness: 1)
    var walked = try dualSession(d, tip: tip, size: 20, seed: 4)
    var ticked = try dualSession(d, tip: tip, size: 20, seed: 4)
    let points = [CGPoint(x: 10, y: 10), CGPoint(x: 25, y: 12), CGPoint(x: 40, y: 18),
                  CGPoint(x: 55, y: 30)]
    for point in points {
        walked.stamp(at: point, pen: PenSample())
        ticked.stamp(at: point, pen: PenSample())
        // A held deposit off the path: it lays first-tip dabs and must not move the walk.
        ticked.stamp(at: CGPoint(x: point.x + 7, y: point.y - 9), pen: PenSample(), tick: true)
    }
    try require(!walked.dualDabs.isEmpty && ticked.dualDabs == walked.dualDabs
                && ticked.dualAnchor == walked.dualAnchor,
                "\(tag) ticks left the second walk alone: \(walked.dualDabs.count / 4) against "
                + "\(ticked.dualDabs.count / 4) second dabs")
    try require(ticked.dabs.count > walked.dabs.count, "\(tag) the ticks laid first-tip dabs")
}

private func verifyDualCharge() throws {
    let tag = "[dual size-1 charge]"
    var d = BrushDynamics()
    d.dual = BrushDynamics.Dual(size: 1)
    let tip = BrushTip(hardness: 1)
    var session = try dualSession(d, tip: tip, size: 20, seed: 0,
                                  overflow: .stopInk(canvasPixels: 100 * 100))
    let before = session.usedWork
    session.walkDual(to: CGPoint(x: 40.3, y: 30.6))
    let charged = session.usedWork - before
    let want = NativeBrushStrokeSession.workCharge(diameter: 1, tip: BrushTip(hardness: 1), size: 2)
    try require(session.dualDabs.count == 4 && want == 1033 && charged == want && charged >= 256,
                "\(tag) one hard 1 px second dab is charged \(charged), want \(want) (at least "
                + "the core's 256)")
}

private func verifyDualPreviewFigure() throws {
    let tag = "[dual preview figure]"
    let big = [1000.0, 1000, 2000, 1]
    let over = ImageCanvasView.dualPreviewFigure(box: (0, 0, 2000, 2000), dabs: big,
                                                 dualDabs: Array([[Double]](repeating: big,
                                                                            count: 200).joined()))
    try require(over > ImageCanvasView.dualPreviewWork,
                "\(tag) 200 coincident 2,000 px second dabs over one box: \(over)")
    let small = ImageCanvasView.dualPreviewFigure(box: (10, 2, 51, 43), dabs: [30.5, 22.5, 40, 1],
                                                  dualDabs: [30.5, 22.5, 10, 1])
    try require(small > 0 && small < ImageCanvasView.dualPreviewWork,
                "\(tag) one 40 px dab and one 10 px second dab: \(small)")
    let far = Array([[Double]](repeating: [5000.5, 5000.5, 10, 1], count: 100_000).joined())
    let bounded = ImageCanvasView.dualPreviewFigure(box: (0, 0, 40, 40), dabs: far, dualDabs: [],
                                                    limit: 1000)
    try require(bounded == 1001,
                "\(tag) 100,000 dabs far from the box cost \(bounded), want the limit plus one")
}

/// The first layer's red and green: bytes at 8 bits, u16 codes at 16.
private func dualRedGreen(_ doc: RasterDocument?, sixteen: Bool) throws -> [Int] {
    if sixteen {
        guard let doc else { throw ProbeFailure(description: "no 16-bit document") }
        return try [RasterPlane.red, .green].flatMap {
            try PrecisionScalarOperations.read(doc, target: $0.agentName, index: 0).samples.map(Int.init)
        }
    }
    guard let pixels = doc?.layerPixels(0) else { throw ProbeFailure(description: "no pixels") }
    return pixels.enumerated().filter { $0.offset % 4 < 2 }.map { Int($0.element) }
}

private func verifyDualCanvas(sixteen: Bool) throws {
    let tag = sixteen ? "[dual canvas 16-bit]" : "[dual canvas 8-bit]"
    let document = try sixteen ? precisionFixture(width: 80, height: 60) : fixture(width: 80, height: 60)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base document") }
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved }
    var options = PaintToolOptions(size: 15, hardness: 100, smoothing: 0)
    options.pressureSize = false
    options.pressureOpacity = false
    options.dynamics = dualOnly
    store.setPaintOptions(options, for: .brush)
    controller.selectTool(.brush)
    canvas.paintColor = NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 1)
    canvas.brushOpacity = 1
    let history = document.history.entries.count
    func settle() throws {
        guard let job = DocumentJobs.active else { return }
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "\(tag) the job completed: \(job.report)")
    }
    let steps = try uiRecorded {
        canvas.beginStroke(at: CGPoint(x: 12.3, y: 20.7))
        try require(canvas.nativeStrokeSession?.dual != nil, "\(tag) the session has the dual brush")
        for point in [CGPoint(x: 30.2, y: 24.1), CGPoint(x: 48.6, y: 31.9), CGPoint(x: 66.1, y: 38.4)] {
            canvas.continueStroke(to: point, pressure: nil)
        }
        canvas.endStroke()
        try settle()
    }
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "\(tag) the stroke published nothing")
    }
    try require(document.history.entries.count == history + 1, "\(tag) one history entry")
    guard steps.count == 1, steps[0].tool == "brush_stroke",
          let native = steps[0].arguments["native_stroke"] as? [String: Any],
          let dual = native["dual"] as? [String: Any], dual["dabs"] is String else {
        throw ProbeFailure(description: "\(tag) the stroke records no dual native_stroke: "
                           + "\(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
    }
    let committed = try dualRedGreen(painted, sixteen: sixteen)
    func replayed(_ arguments: [String: Any]) throws -> [Int] {
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) one undo restores the base")
        _ = try agentOK(steps[0].tool, document, arguments)
        try settle()
        return try dualRedGreen(document.doc, sixteen: sixteen)
    }
    let replay = try replayed(steps[0].arguments)
    try require(replay == committed,
                "\(tag) the commit equals the replay of its recorded step: "
                + "\(zip(replay, committed).filter { $0 != $1 }.count) samples differ")
    var bare = steps[0].arguments
    bare["native_stroke"] = native.filter { $0.key != "dual" }
    let single = try replayed(bare)
    try require(single != committed, "\(tag) the dual brush changed the commit")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the base")
}
