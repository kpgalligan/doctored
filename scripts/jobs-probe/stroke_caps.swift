import AppKit

// A CANVAS stroke that reaches a stroke cap stops laying ink and commits
// what was laid (`NativeBrushStrokeSession.Overflow.stopInk`); it no longer
// refuses the whole stroke. This case stands for the UI path of a Brush drag at
// 8 and 16 bits — the session `ImageCanvasView.beginStroke` constructs at
// mouse-down, its stamps during `continueStroke`, and the commit `endStroke`
// starts — and calls those methods directly, so none needs a key window.
//
// (a) Session only, a 12500 × 8000 extent: 5,000 px dabs at the centre until
//     the session is full, soft and hard. The laid count is the charge
//     formula's (71 soft, 64 hard), one more stamp adds nothing, and the laid
//     dabs resolve and plan in the core (`dr_brush_u16_plan`).
// (b) Session only, 64 × 64: the 50,001st 23 px dab is dropped.
// (c) Canvas, 16-bit and 8-bit: a stroke whose session is filled to the dab cap
//     commits as one history entry with changed pixels, and one undo restores
//     the base.
// (d) Session only: a SMUDGE session also stops ink at Smudge's own
//     256-million bound (`NativeBrushStroke.maximumSmudgeWork`), far below the
//     brush cap; what it laid plans in the core, and a stroke twice as long
//     is refused there in Smudge's own sentence.

private let workCap: UInt64 = 2_000_000_000

/// The session's charge for one dab, written out independently: `(⌈d⌉ + 2)²`,
/// plus `64·⌈4πaρ + πρ²⌉` for a hard tip (`a = d/2`, `ρ = √2 + 2⁻¹⁶·a`).
private func dabCharge(diameter: Double, hard: Bool) -> UInt64 {
    let side = UInt64(diameter.rounded(.up)) + 2
    guard hard else { return side * side }
    let a = diameter / 2
    let rho = 2.0.squareRoot() + a / 65536
    return side * side + 64 * UInt64((4 * Double.pi * a * rho + Double.pi * rho * rho).rounded(.up))
}

private func verifySessionStopsAtWorkCap(hardness: CGFloat, expected: Int) throws {
    let tag = "[stroke cap work, hardness \(hardness)]"
    let (width, height) = (12500, 8000)
    let charge = dabCharge(diameter: 5000, hard: hardness >= 1)
    let count = Int((workCap - 2 * UInt64(width * height)) / charge)
    try require(count == expected, "\(tag) the formula lays \(expected) dabs: \(count) at \(charge) per dab")
    var session = NativeBrushStrokeSession(tip: BrushTip(hardness: hardness), size: 5000,
        pressureOpacity: false, overflow: .stopInk(canvasPixels: width * height))
    let centre = CGPoint(x: 6250.5, y: 4000.5)
    var stamps = 0
    while !session.isFull, stamps < 1000 {
        session.stamp(at: centre, diameter: 5000, pressure: 1)
        stamps += 1
    }
    try require(session.isFull && session.refusal == nil && !session.acceptsInk,
                "\(tag) the session is full, not refused: \(session.refusal ?? "-")")
    try require(session.dabs.count / 4 == count && stamps == count + 1,
                "\(tag) laid \(session.dabs.count / 4) dabs in \(stamps) stamps, expected \(count)")
    session.stamp(at: centre, diameter: 5000, pressure: 1)
    try require(session.dabs.count / 4 == count && session.refusal == nil,
                "\(tag) one more stamp adds nothing")
    let stroke = try session.resolved()
    let scratch = try stroke.footprintPlan(width: width, height: height)
    try require(stroke.dabs.count / 4 == count && scratch > 0,
                "\(tag) the laid dabs resolve and plan: scratch \(scratch)")
}

private func verifySessionStopsAtDabCap() throws {
    let tag = "[stroke cap dabs]"
    var session = NativeBrushStrokeSession(tip: BrushTip(hardness: 0.5), size: 23,
        pressureOpacity: false, overflow: .stopInk(canvasPixels: 64 * 64))
    let point = CGPoint(x: 31.5, y: 27.5)
    for _ in 0..<NativeBrushStroke.maximumDabs { session.stamp(at: point, diameter: 23, pressure: 1) }
    try require(session.dabs.count / 4 == NativeBrushStroke.maximumDabs && session.acceptsInk,
                "\(tag) 50,000 dabs are laid and the session still accepts ink")
    session.stamp(at: point, diameter: 23, pressure: 1)
    try require(session.isFull && session.refusal == nil
                && session.dabs.count / 4 == NativeBrushStroke.maximumDabs,
                "\(tag) the 50,001st dab is dropped, not refused: \(session.refusal ?? "-")")
    let stroke = try session.resolved()
    let scratch = try stroke.footprintPlan(width: 64, height: 64)
    try require(scratch > 0, "\(tag) the 50,000 dabs resolve and plan")
    // The MCP walk's default still refuses the whole stroke by name.
    var refusing = NativeBrushStrokeSession(tip: BrushTip(hardness: 0.5), size: 23, pressureOpacity: false)
    for _ in 0...NativeBrushStroke.maximumDabs { refusing.stamp(at: point, diameter: 23, pressure: 1) }
    try require(refusing.refusal == NativeBrushStroke.dabLimitRefusal && !refusing.isFull,
                "\(tag) the default session refuses by name: \(refusing.refusal ?? "-")")
}

/// (d): soft 200 px dabs 50 px apart. Each move is walked in 50 one-pixel
/// sub-steps charged `(200 + 2)² + 1` = 40,805, written out independently of
/// the session, so 125 moves fit under 256 million (255,031,250) and the 126th
/// does not: 126 dabs are laid and the 127th stamp is dropped.
private func verifySmudgeSessionStopsAtItsBound() throws {
    let tag = "[stroke cap smudge]"
    let smudgeCap: UInt64 = 256_000_000
    let smudgeSentence = "smudge stroke exceeds 256 million work units; "
        + "use a shorter stroke or a smaller brush"
    let (width, height) = (8000, 600)
    let move = 50 * (dabCharge(diameter: 200, hard: false) + 1)
    let moves = Int(smudgeCap / move)
    try require(move == 2_040_250 && moves == 125,
                "\(tag) the formula lays 125 moves: \(moves) at \(move) per move")
    let point = { (index: Int) in CGPoint(x: 300.5 + Double(index) * 50, y: 300.5) }
    var session = NativeBrushStrokeSession(tip: BrushTip(hardness: 0.5), size: 200,
        pressureOpacity: false, overflow: .stopInk(canvasPixels: width * height), smudging: true)
    var stamps = 0
    while !session.isFull, stamps < 1000 {
        session.stamp(at: point(stamps), diameter: 200, pressure: 1)
        stamps += 1
    }
    try require(session.isFull && session.refusal == nil && !session.acceptsInk,
                "\(tag) the session is full, not refused: \(session.refusal ?? "-")")
    try require(session.dabs.count / 4 == moves + 1 && stamps == moves + 2,
                "\(tag) laid \(session.dabs.count / 4) dabs in \(stamps) stamps, expected \(moves + 1)")
    // The same walk as a plain brush stroke is nowhere near the brush cap.
    var plain = NativeBrushStrokeSession(tip: BrushTip(hardness: 0.5), size: 200,
        pressureOpacity: false, overflow: .stopInk(canvasPixels: width * height))
    for index in 0..<stamps { plain.stamp(at: point(index), diameter: 200, pressure: 1) }
    try require(!plain.isFull && plain.dabs.count / 4 == stamps,
                "\(tag) the same dabs do not fill a Brush session")
    // What the session laid is within the core's own bound.
    let stroke = try session.resolved()
    let blank = try RasterDocument.blankRGBA16(
        width: width, height: height, fill: [65535, 65535, 65535, 65535],
        profile: RasterProfile.builtin(.sRGB), layerName: "Background")
    guard let precision = blank.precision else {
        throw ProbeFailure(description: "\(tag) no 16-bit document to plan on")
    }
    let context = try PrecisionWorkContext(source: precision)
    let plan = try precision.smudgePlan(layer: 0, request: stroke.request, dabs: stroke.dabs,
        selection: false, workingLimit: context.limit, otherRetained: context.otherRetained)
    let scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
    try require(scratch > 0, "\(tag) the laid dabs plan in the core: \(plan)")
    // There and back over the same 6,250 px is 12,500 sub-steps, about twice
    // the bound: the core refuses it by name, before walking it.
    let far = point(moves)
    let long: [Double] = [point(0), far, point(0)].flatMap { (p: CGPoint) -> [Double] in
        [Double(p.x), Double(p.y), 200, 1]
    }
    var refusal = ""
    do {
        _ = try precision.smudgePlan(layer: 0, request: stroke.request, dabs: long,
            selection: false, workingLimit: context.limit, otherRetained: context.otherRetained)
    } catch { refusal = error.localizedDescription }
    try require(refusal == smudgeSentence, "\(tag) the core's refusal: \"\(refusal)\"")
}

private func verifyCanvasCommitsCappedStroke(sixteen: Bool) throws {
    let tag = sixteen ? "[stroke cap canvas 16-bit]" : "[stroke cap canvas 8-bit]"
    let document = try sixteen ? precisionFixture(width: 300, height: 200)
        : fixture(width: 300, height: 200)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base document") }
    let baseSamples = sixteen ? try precisionLayerSamples(base) : []
    let basePixels = sixteen ? nil : base.layerPixels(0)
    controller.selectTool(.brush)
    canvas.paintColor = NSColor(srgbRed: 0.9, green: 0.1, blue: 0.1, alpha: 1)
    canvas.brushSize = 23
    canvas.brushHardness = 0.5
    canvas.brushOpacity = 1
    let history = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count
    let point = CGPoint(x: 150.5, y: 100.5)
    canvas.beginStroke(at: point)
    try require(canvas.strokeActive, "\(tag) the Brush stroke began")
    guard var session = canvas.nativeStrokeSession else {
        throw ProbeFailure(description: "\(tag) the stroke opened no dab session")
    }
    let prefix = session.dabs.count / 4
    try require(prefix > 0, "\(tag) mouse-down laid a dab")
    for _ in prefix..<NativeBrushStroke.maximumDabs { session.stamp(at: point, diameter: 23, pressure: 1) }
    try require(session.dabs.count / 4 == NativeBrushStroke.maximumDabs && session.acceptsInk,
                "\(tag) the canvas session took 50,000 dabs")
    session.stamp(at: point, diameter: 23, pressure: 1)
    try require(session.isFull && session.refusal == nil,
                "\(tag) the canvas session stops ink at the dab cap: \(session.refusal ?? "-")")
    canvas.nativeStrokeSession = session
    canvas.continueStroke(to: CGPoint(x: 220.5, y: 100.5), pressure: nil)
    try require(canvas.strokeActive && canvas.nativeStrokeSession?.refusal == nil
                && (canvas.nativeStrokeSession?.dabs.count ?? 0) / 4 == NativeBrushStroke.maximumDabs,
                "\(tag) a drag past the cap adds nothing and refuses nothing")
    canvas.endStroke()
    if sixteen {
        guard let job = DocumentJobs.active, job.title == "Brush Stroke" else {
            throw ProbeFailure(description: "\(tag) the capped stroke started no Brush Stroke job: "
                + "\(DocumentJobs.active?.report ?? [:])")
        }
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "\(tag) the capped stroke's job completed: \(job.report)")
    }
    try require(canvas.nativeStrokeSession == nil && !canvas.strokeActive,
                "\(tag) the session ended with the stroke")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert or error was presented: \(AutomationAlerts.recorded.suffix(2))")
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "\(tag) the capped stroke published nothing")
    }
    try require(document.history.entries.count == history + 1,
                "\(tag) one history entry: \(document.history.entries.count) vs \(history)")
    if sixteen {
        let paintedSamples = try precisionLayerSamples(painted)
        try require(paintedSamples != baseSamples, "\(tag) the samples changed")
    } else {
        try require(painted.layerPixels(0) != basePixels, "\(tag) the pixels changed")
    }
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the base")
}

func verifyStrokeStopsAtCap() throws {
    try verifySessionStopsAtWorkCap(hardness: 0.5, expected: 71)
    try verifySessionStopsAtWorkCap(hardness: 1, expected: 64)
    try verifySessionStopsAtDabCap()
    try verifySmudgeSessionStopsAtItsBound()
    try verifyCanvasCommitsCappedStroke(sixteen: true)
    try verifyCanvasCommitsCappedStroke(sixteen: false)
    print("PASS: a canvas stroke stops laying ink at a stroke cap and commits what was laid — "
        + "71 soft / 64 hard 5,000 px dabs on 12500 x 8000 under the 2 billion work cap, the "
        + "50,001st dab dropped, a Smudge session stopped after 125 moves of a soft 200 px tip "
        + "at its own 256 million bound, and a capped Brush stroke is one undo step at 16 and "
        + "8 bits")
}
