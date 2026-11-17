import AppKit

// Brush dynamics' colour (`designs/brush-dynamics.md`, slice 5), off screen, one call from
// `verifyBrushDynamics()`. Each row runs on its own and every failing row is reported, so one
// run shows each red.
//
// (a) The stored options: the Brush keeps `color`, the Eraser's and the Pencil's options drop it
//     (keeping the rest); zero amounts are stored as none; the value round-trips through its
//     JSON; a colour preset round-trips at schema 2 and is absent from the Eraser's and the
//     Pencil's lists while the Brush's lists it.
// (b) The Dynamics… sheet shows the Colour rows with the options' values for the Brush and none
//     for the Eraser.
// (c) A coloured Brush canvas stroke at 8 and 16 bits records a `native_stroke` holding `inks`,
//     and its commit equals the replay of that recorded step and differs from the replay without
//     the inks.
// (d) The canvas limits: a layer mask, Quick Mask, a Multiply Brush, a CMYK document, a size-1
//     Brush and a float layer with a foreground above white each paint their one ink, recording
//     no `inks` (the float stroke keeps `rgba32`).
// (e) Lab: the canvas's colour handle for a stroke with hue and saturation set answers, dab for
//     dab, the hue-zero handle of the same seed.
// (f) A Lab 16 canvas stroke with hue and saturation set takes a colour handle, records inks and
//     commits; each recorded ink's a* and b* codes sit at one fraction t (0…mix) of the way from
//     the foreground's codes to the background's, and its L* code between that mix's and 1 −
//     brightness of it. Any seed holds it, so the canvas's fresh seed needs no hook; HSL on the
//     codes (the rgb model) moves a* and b* apart.

func verifyBrushColour() throws {
    var failures: [String] = []
    let rows: [(String, () throws -> Void)] = [
        ("options", verifyColourOptions), ("sheet", verifyColourSheet),
        ("canvas 8-bit", { try verifyColourCanvas(sixteen: false) }),
        ("canvas 16-bit", { try verifyColourCanvas(sixteen: true) }),
        ("limits", verifyColourLimits), ("lab", verifyColourLab),
        ("lab canvas", verifyColourLabCanvas),
    ]
    for (name, row) in rows {
        do { try row() } catch { failures.append("\(name): \(error)") }
    }
    guard failures.isEmpty else {
        throw ProbeFailure(description: failures.joined(separator: "\n"))
    }
    print("PASS: colour dynamics' options, preset, sheet, canvas commits, canvas limits, the "
          + "Lab reduction and a Lab canvas stroke's inks")
}

private let colourOnly: BrushDynamics = {
    var d = BrushDynamics()
    d.color = BrushDynamics.ColorDynamics(mix: 0.8, hue: 0.3, saturation: 0.5, brightness: 0.4)
    return d
}()

private func verifyColourOptions() throws {
    let tag = "[colour options]"
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.eraser, store.pencil)
    defer { (store.brush, store.eraser, store.pencil) = saved }
    var both = colourOnly
    both.scatter = 2
    var options = PaintToolOptions(size: 9, smoothing: 0)
    options.dynamics = both
    for tool in [EditorTool.brush, .eraser, .pencil] {
        store.setPaintOptions(options, for: tool)
    }
    try require(store.paintOptions(for: .brush)?.dynamics == both,
                "\(tag) the Brush keeps the colour")
    for tool in [EditorTool.eraser, .pencil] {
        let kept = store.paintOptions(for: tool)?.dynamics
        try require(kept?.color == nil && kept?.scatter == 2,
                    "\(tag) the \(tool) drops the colour and keeps the rest: \(String(describing: kept))")
    }
    var zero = BrushDynamics()
    zero.color = BrushDynamics.ColorDynamics()
    try require(zero.normalized == nil, "\(tag) zero amounts are stored as none")
    try require(colourOnly.normalized == colourOnly, "\(tag) a colour alone stays dynamics")
    let roundTrip = try BrushDynamics(json: colourOnly.json)
    try require(roundTrip == colourOnly, "\(tag) the colour round-trips through its JSON")
    let mine = BrushPreset(name: "Colour", hardness: 100, flow: 100, spacing: 25, angle: 0,
                           roundness: 100, airbrush: false, dynamics: colourOnly)
    let decoded = try BrushPreset.decode(mine.json)
    try require(mine.json["schema_version"] as? Int == 2 && decoded.dynamics == colourOnly,
                "\(tag) a colour preset round-trips at schema 2")
    try BrushPresetLibrary.save(name: "zz Colour Probe", values: EditorViewController.brushPresetValues(
        BrushPreset(name: "zz Colour Probe", hardness: 66, flow: 100, spacing: 25, angle: 0,
                    roundness: 100, airbrush: false, dynamics: colourOnly).applied(to: PaintToolOptions())),
        replace: true)
    defer { try? BrushPresetLibrary.delete(name: "zz Colour Probe") }
    BrushPresetLibrary.reload()
    func lists(_ tool: EditorTool) -> Bool {
        BrushPreset.all(dynamics: tool.takesBrushDynamics, shapes: tool.takesTipShapeDynamics,
                        colour: tool.takesColourDynamics).contains { $0.name == "zz Colour Probe" }
    }
    try require(lists(.brush) && !lists(.eraser) && !lists(.pencil),
                "\(tag) the Brush lists a colour preset; the Eraser and the Pencil leave it out")
}

/// The view under `root` whose accessibility identifier is `id`.
private func colourSubview(_ id: String, in root: NSView) -> NSView? {
    if root.accessibilityIdentifier() == id { return root }
    for child in root.subviews {
        if let found = colourSubview(id, in: child) { return found }
    }
    return nil
}

private func verifyColourSheet() throws {
    let tag = "[colour sheet]"
    var options = PaintToolOptions(size: 9, smoothing: 0)
    options.dynamics = colourOnly
    let brush = BrushDynamicsSheetController(options: options, shapes: true, colour: true,
                                             onApply: { _ in })
    let ids = ["mix", "hue", "saturation", "brightness"].map { "brush-dynamics-colour-\($0)" }
    let fields = ids.compactMap { colourSubview($0, in: brush.view) as? NSTextField }
    try require(fields.map(\.stringValue) == ["80", "30", "50", "40"],
                "\(tag) the Brush's sheet shows the colour: \(fields.map(\.stringValue))")
    let eraser = BrushDynamicsSheetController(options: options, shapes: true, colour: false,
                                              onApply: { _ in })
    try require(ids.allSatisfy { colourSubview($0, in: eraser.view) == nil },
                "\(tag) the Eraser's sheet shows no Colour rows")
}

/// The first layer's red, green and blue: bytes at 8 bits, u16 codes at 16.
private func colourChannels(_ doc: RasterDocument?, sixteen: Bool) throws -> [Int] {
    if sixteen {
        guard let doc else { throw ProbeFailure(description: "no 16-bit document") }
        return try [RasterPlane.red, .green, .blue].flatMap {
            try PrecisionScalarOperations.read(doc, target: $0.agentName, index: 0).samples.map(Int.init)
        }
    }
    guard let pixels = doc?.layerPixels(0) else { throw ProbeFailure(description: "no pixels") }
    return pixels.enumerated().filter { $0.offset % 4 < 3 }.map { Int($0.element) }
}

/// Runs a Brush drag on `controller`'s canvas with the store's Brush options, recorded, and
/// waits for its job; the steps it recorded. `coloured` is set to whether the stroke's session
/// held a colour handle at mouse-down.
private func colourDrag(_ controller: EditorViewController, tag: String,
                        coloured: UnsafeMutablePointer<Bool>? = nil) throws -> [ActionStep] {
    let canvas: ImageCanvasView = controller.canvas
    return try uiRecorded {
        canvas.beginStroke(at: CGPoint(x: 12.3, y: 20.7))
        try require(canvas.strokeActive, "\(tag) the stroke began")
        coloured?.pointee = canvas.nativeStrokeSession?.colour != nil
        for point in [CGPoint(x: 30.2, y: 24.1), CGPoint(x: 48.6, y: 31.9)] {
            canvas.continueStroke(to: point, pressure: nil)
        }
        canvas.endStroke()
        if let job = DocumentJobs.active {
            try pumpUntil({ job.state != "running" })
            try require(job.state == "completed", "\(tag) the job completed: \(job.report)")
        }
    }
}

/// The store's Brush options for the probe: size `size`, hard, colour dynamics, `blend`.
private func colourBrushOptions(size: CGFloat = 15, blend: DrBlendMode = DR_BLEND_NORMAL) {
    var options = PaintToolOptions(size: size, hardness: 100, smoothing: 0)
    options.pressureSize = false
    options.pressureOpacity = false
    options.blendIndex = Int(blend.rawValue)
    options.dynamics = colourOnly
    ToolOptionsStore.shared.setPaintOptions(options, for: .brush)
}

private func inked(_ step: ActionStep) -> Bool {
    (step.arguments["native_stroke"] as? [String: Any])?["inks"] != nil
}

private func verifyColourCanvas(sixteen: Bool) throws {
    let tag = sixteen ? "[colour canvas 16-bit]" : "[colour canvas 8-bit]"
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
    colourBrushOptions()
    controller.selectTool(.brush)
    canvas.paintColor = NSColor(srgbRed: 0.8, green: 0.2, blue: 0.1, alpha: 1)
    controller.backgroundColor = NSColor(srgbRed: 0.1, green: 0.4, blue: 0.9, alpha: 1)
    canvas.brushOpacity = 1
    let steps = try colourDrag(controller, tag: tag)
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "\(tag) the stroke published nothing")
    }
    guard steps.count == 1, steps[0].tool == "brush_stroke", inked(steps[0]),
          let native = steps[0].arguments["native_stroke"] as? [String: Any] else {
        throw ProbeFailure(description: "\(tag) the stroke records no inks: "
                           + "\(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
    }
    let committed = try colourChannels(painted, sixteen: sixteen)
    func replayed(_ arguments: [String: Any]) throws -> [Int] {
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) one undo restores the base")
        _ = try agentOK(steps[0].tool, document, arguments)
        if let job = DocumentJobs.active { try pumpUntil({ job.state != "running" }) }
        return try colourChannels(document.doc, sixteen: sixteen)
    }
    let replay = try replayed(steps[0].arguments)
    try require(replay == committed,
                "\(tag) the commit equals the replay of its recorded step: "
                + "\(zip(replay, committed).filter { $0 != $1 }.count) samples differ")
    var bare = steps[0].arguments
    bare["native_stroke"] = native.filter { $0.key != "inks" }
    let uncoloured = try replayed(bare)
    try require(uncoloured != committed, "\(tag) the inks changed the commit")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the base")
}

private func verifyColourLimits() throws {
    let tag = "[colour limits]"
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved }
    func run(_ name: String, _ document: ImageDocument, size: CGFloat = 15,
             blend: DrBlendMode = DR_BLEND_NORMAL,
             prepare: (EditorViewController) throws -> Void = { _ in }) throws -> [ActionStep] {
        defer {
            if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
            document.close()
        }
        let controller = try editor(document)
        colourBrushOptions(size: size, blend: blend)
        controller.selectTool(.brush)
        controller.canvas.paintColor = NSColor(srgbRed: 0.8, green: 0.2, blue: 0.1, alpha: 1)
        controller.backgroundColor = NSColor(srgbRed: 0.1, green: 0.4, blue: 0.9, alpha: 1)
        controller.canvas.brushOpacity = 1
        try prepare(controller)
        var coloured = true
        let steps = try colourDrag(controller, tag: "\(tag) \(name)", coloured: &coloured)
        try require(!coloured && !steps.contains(where: inked),
                    "\(tag) \(name) takes no colour handle (\(coloured)) and records no inks: "
                    + "\(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
        return steps
    }
    var failures: [String] = []
    func each(_ name: String, _ body: () throws -> Void) {
        do { try body() } catch { failures.append("\(name): \(error)") }
    }
    each("mask") {
        _ = try run("mask", try fixture(width: 80, height: 60)) { controller in
            guard let document = controller.document else { return }
            _ = try agentOK("add_layer_mask", document, ["layer": 0])
            controller.setPaintTarget(.mask)
        }
    }
    each("quick mask") {
        _ = try run("quick mask", try fixture(width: 80, height: 60)) { controller in
            guard let document = controller.document else { return }
            _ = try agentOK("set_quick_mask", document, ["active": true])
        }
    }
    each("multiply") {
        _ = try run("multiply", try fixture(width: 80, height: 60), blend: DR_BLEND_MULTIPLY)
    }
    each("size 1") { _ = try run("size 1", try fixture(width: 80, height: 60), size: 1) }
    each("cmyk") {
        guard let cmykRow = colourModeRows().first(where: { $0.name == "CMYK 8" }) else {
            throw ProbeFailure(description: "\(tag) no CMYK row")
        }
        _ = try run("cmyk", try openColourModeRow(cmykRow, tag: tag).document)
    }
    each("float above white") {
        let floatSteps = try run("float above white", try floatFixture()) { controller in
            guard let base = controller.document?.doc,
                  let hdr = ExtendedRangeColor.linearColor([2, 1.5, 1.2, 1], in: base) else {
                throw ProbeFailure(description: "\(tag) no above-white colour")
            }
            controller.paintColor = hdr
            controller.canvas.paintColor = hdr
        }
        try require(floatSteps.contains { $0.arguments["rgba32"] != nil },
                    "\(tag) the float stroke keeps its one linear ink: "
                    + "\(floatSteps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
    }
    guard failures.isEmpty else {
        throw ProbeFailure(description: failures.joined(separator: "\n"))
    }
}

private func verifyColourLab() throws {
    let tag = "[colour lab]"
    guard let row = colourModeRows().first(where: { $0.name == "Lab 16" }) else {
        throw ProbeFailure(description: "\(tag) no Lab row")
    }
    let lab = try openColourModeRow(row, tag: tag)
    defer { lab.document.close() }
    lab.controller.selectTool(.brush)
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved }
    colourBrushOptions()
    lab.controller.canvas.paintColor = NSColor(srgbRed: 0.8, green: 0.2, blue: 0.1, alpha: 1)
    lab.controller.backgroundColor = NSColor(srgbRed: 0.1, green: 0.4, blue: 0.9, alpha: 1)
    var flat = colourOnly.color ?? BrushDynamics.ColorDynamics()
    flat.hue = 0
    flat.saturation = 0
    guard let full = lab.controller.strokeColourHandle(colourOnly.color ?? flat, seed: 77),
          let reduced = lab.controller.strokeColourHandle(flat, seed: 77) else {
        throw ProbeFailure(description: "\(tag) a Lab layer stroke has no colour handle")
    }
    try require(full.lab && reduced.lab, "\(tag) the handles read Lab codes")
    for index in UInt64(0)..<6 {
        for dab in 0..<3 {
            let a = full.ink(index: index, dab: dab), b = reduced.ink(index: index, dab: dab)
            try require(a == b, "\(tag) stamp \(index) dab \(dab): \(a) against \(b)")
        }
    }
}

private func verifyColourLabCanvas() throws {
    let tag = "[colour lab canvas]"
    guard let row = colourModeRows().first(where: { $0.name == "Lab 16" }) else {
        throw ProbeFailure(description: "\(tag) no Lab row")
    }
    let lab = try openColourModeRow(row, tag: tag)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        lab.document.close()
    }
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved }
    colourBrushOptions()
    lab.controller.selectTool(.brush)
    let foreground = NSColor(srgbRed: 0.8, green: 0.2, blue: 0.1, alpha: 1)
    let background = NSColor(srgbRed: 0.1, green: 0.4, blue: 0.9, alpha: 1)
    lab.controller.canvas.paintColor = foreground
    lab.controller.backgroundColor = background
    lab.controller.canvas.brushOpacity = 1
    // The two swatches as the twin's codes, through the canvas's own ink route.
    let shared = store.sharedState
    let space = lab.document.nsColorSpace
    let fg = try PrecisionInk(color: LabPaint.color(foreground, stored: shared.foregroundLab,
                                                    in: lab.doc), space: space).rgba
    let bg = try PrecisionInk(color: LabPaint.color(background, stored: shared.backgroundLab,
                                                    in: lab.doc), space: space).rgba
    try require(abs(bg[1] - fg[1]) > 0.01 && abs(bg[2] - fg[2]) > 0.01,
                "\(tag) the swatches differ in a* and b*: \(fg) against \(bg)")
    var coloured = false
    let steps = try colourDrag(lab.controller, tag: tag, coloured: &coloured)
    guard coloured, steps.count == 1, steps[0].tool == "brush_stroke",
          let native = steps[0].arguments["native_stroke"] as? [String: Any],
          let inks = try NativeBrushStroke(canonical: native).inks, inks.count >= 3 else {
        throw ProbeFailure(description: "\(tag) the stroke takes a colour handle (\(coloured)) "
            + "and records inks: \(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
    }
    let amounts = colourOnly.color ?? BrushDynamics.ColorDynamics()
    let slack = 1e-9
    var moved = 0
    for dab in 0..<(inks.count / 3) {
        let ink = Array(inks[(dab * 3)..<(dab * 3 + 3)])
        let ta = (ink[1] - fg[1]) / (bg[1] - fg[1]), tb = (ink[2] - fg[2]) / (bg[2] - fg[2])
        try require(abs(ta - tb) <= slack && ta >= -slack && ta <= amounts.mix + slack,
                    "\(tag) dab \(dab)'s a* and b* sit at one fraction of the mix: \(ink), "
                    + "t \(ta) and \(tb)")
        let mixed = fg[0] + (bg[0] - fg[0]) * ta
        try require(ink[0] <= mixed + slack && ink[0] >= mixed * (1 - amounts.brightness) - slack,
                    "\(tag) dab \(dab)'s L* lies within the brightness of the mix's: \(ink[0]) "
                    + "against \(mixed)")
        if ta > 0.01 { moved += 1 }
    }
    try require(moved > 0, "\(tag) some dab moved toward the background")
}
