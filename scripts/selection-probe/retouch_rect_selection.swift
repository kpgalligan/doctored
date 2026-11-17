import AppKit

// The canvas site of a selected 16-bit Clone Stamp, Healing Brush and Spot Healing stroke
// (`EditorViewController.finishPrecisionStroke`), which reads the selection over the stroke's
// `coverage_rect`. Each stroke runs through the canvas's own `beginStroke(at:)` /
// `continueStroke(to:pressure:)` / `endStroke()` on the fixture's off-screen window, with no
// mouse event, key window or activation. The constants are
// `scripts/verify_retouch_coverage_rect.py`'s, the MCP drive of the same strokes; the gates are
// fixed here before any run: one job that completes, every CUT and FAR probe as it was, at least
// one INSIDE probe changed, one undo restoring every probe.

private let rectStrokeSize = (width: 200, height: 140)
private let rectStrokeDab = CGPoint(x: 100.5, y: 70.5)
private let rectStrokeDiameter: CGFloat = 21
private let rectStrokeShift: CGFloat = 60
private let rectStrokeSelect: [String: Any] = ["x": 96, "y": 40, "width": 60, "height": 60]
/// Inside the selection and at most 8 px from the dab: the hard tip's interior.
private let rectStrokeInside = [(100, 70), (104, 70), (100, 74), (98, 66), (105, 75)]
/// Inside the tip, left of the selection's edge at x = 96.
private let rectStrokeCut = [(92, 70), (93, 72), (94, 68)]
private let rectStrokeFar = [(5, 5), (195, 5), (5, 135), (195, 135), (150, 120), (60, 20), (40, 70)]

/// The drive's `hashed`: a 32-bit mix of the pixel and a salt, its top 16 bits.
private func rectStrokeHashed(_ x: Int, _ y: Int, _ salt: Int) -> UInt32 {
    var v = UInt32(truncatingIfNeeded: x) &* 0x9E37_79B1
        &+ UInt32(truncatingIfNeeded: y) &* 0x85EB_CA77
        &+ UInt32(truncatingIfNeeded: salt) &* 0xC2B2_AE3D
    v ^= v >> 15
    v = v &* 0x2C1B_3C6D
    v ^= v >> 12
    return v >> 16
}

/// The 16-bit composite's codes at each probe.
private func rectStrokeProbes(_ document: ImageDocument, _ points: [(Int, Int)]) throws -> [[Double]] {
    try points.map { point in
        guard let reading = try PrecisionReadout.at(point, document: document) else {
            throw ProbeFailure(description: "[retouch rect selection] no sample at \(point)")
        }
        return reading.codes
    }
}

func verifyCanvasRetouchRectSelection() throws {
    let (width, height) = rectStrokeSize
    var rgba = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            for c in 0..<3 {
                rgba[(y * width + x) * 4 + c] = UInt8(truncatingIfNeeded: rectStrokeHashed(x, y, c + 1))
            }
        }
    }
    let document = try fixture(width: width, height: height, rgba: rgba)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    _ = try call(document, "convert_precision", ["mode": "rgba16", "structure": "preserve"])
    guard document.doc?.precision != nil else {
        throw ProbeFailure(description: "[retouch rect selection] the fixture did not convert to 16 bits")
    }
    let store = ToolOptionsStore.shared
    let saved = (store.clone, store.heal, store.spotHeal)
    defer { (store.clone, store.heal, store.spotHeal) = saved }
    var tip = PaintToolOptions()
    tip.smoothing = 0
    tip.pressureSize = false
    tip.aligned = false
    tip.sample = .current
    tip.outputToNewLayer = false
    tip.expandDestination = false
    store.clone = tip
    store.heal = tip
    store.spotHeal = tip

    let everything = rectStrokeInside + rectStrokeCut + rectStrokeFar
    let original = try rectStrokeProbes(document, everything)
    let inside = 0..<rectStrokeInside.count
    let cut = rectStrokeInside.count..<(rectStrokeInside.count + rectStrokeCut.count)
    let far = cut.upperBound..<everything.count
    for tool in [EditorTool.clone, .heal, .spotHeal] {
        let label = "[retouch rect selection] \(tool)"
        _ = try call(document, "select_rect", rectStrokeSelect)
        controller.selectTool(tool)
        try require(controller.currentTool == tool, "\(label) is selected")
        canvas.brushSize = rectStrokeDiameter
        canvas.brushOpacity = 1
        canvas.brushHardness = 1
        if tool != .spotHeal {
            canvas.cloneSource = CGPoint(x: rectStrokeDab.x - rectStrokeShift, y: rectStrokeDab.y)
        }
        AutomationAlerts.reset()
        let alerts = { AutomationAlerts.recorded.map { $0.messageText + " " + $0.informativeText } }
        canvas.beginStroke(at: rectStrokeDab)
        try require(canvas.strokeActive, "\(label) stroke began: \(alerts())")
        canvas.continueStroke(to: CGPoint(x: 102.5, y: 70.5), pressure: nil)
        canvas.endStroke()
        guard let job = DocumentJobs.active else {
            throw ProbeFailure(description: "\(label) started no job: \(alerts())")
        }
        try pumpUntil({ job.state != "running" }, seconds: 30, message: "\(label) job")
        try require(job.state == "completed", "\(label) job completed: \(job.state) \(job.report)")
        let after = try rectStrokeProbes(document, everything)
        try require(far.allSatisfy { after[$0] == original[$0] }, "\(label) every FAR probe is as it was")
        try require(cut.allSatisfy { after[$0] == original[$0] },
                    "\(label) every CUT probe (outside the selection) is as it was")
        try require(inside.contains { after[$0] != original[$0] }, "\(label) the stroke changes its tip")
        document.undoManager?.undo()
        let undone = try rectStrokeProbes(document, everything)
        try require(undone == original, "\(label) one undo restores")
        _ = try call(document, "deselect")
    }
    print("PASS: canvas retouch rect selection — a selected 16-bit Clone Stamp, Healing Brush and "
        + "Spot Healing canvas stroke each run as one completed job, change the tip inside the "
        + "selection and nothing outside it or far from the stroke, and undo in one step")
}
