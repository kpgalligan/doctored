import AppKit

// The Blur and Smudge brushes and the History Brush in a 32-bit float
// document, through the canvas's own `beginStroke(at:)` /
// `continueStroke(to:pressure:)` / `endStroke()`, so the case needs no key
// window. Each stroke is one job and one undo; a Blur stroke spreads a 5000
// highlight, a Smudge stroke carries it along the stroke, and the History
// Brush restores a painted-over state's own numbers. The Sponge is admitted too.

private let brushPicture = (width: 160, height: 120)
private let brushHighlight: [Double] = [5000, 5000, 5000, 1]

/// One float retouch-brush stroke through the canvas: press, one drag, release.
private func brushStroke(_ controller: EditorViewController, from: CGPoint, to: CGPoint) throws {
    let canvas: ImageCanvasView = controller.canvas
    canvas.beginStroke(at: from)
    try require(canvas.strokeActive, "[float brushes] the \(canvas.tool) stroke began")
    canvas.continueStroke(to: to, pressure: nil)
    canvas.endStroke()
}

/// The job the last gesture started, pumped to its end.
private func finishedBrushJob(_ what: String) throws -> DocumentJob {
    guard let job = DocumentJobs.active else {
        throw ProbeFailure(description: "\(what) started no job")
    }
    try pumpUntil({ job.state != "running" })
    return job
}

/// The composite's straight linear RGBA at one pixel.
private func brushPixel(_ document: ImageDocument, _ x: Int, _ y: Int) throws -> [Double] {
    guard let sample = try FloatReadout.at((x: x, y: y), document: document) else {
        throw ProbeFailure(description: "No float sample at (\(x), \(y))")
    }
    return sample.values
}

private func brushNear(_ got: [Double], _ want: [Double], _ tolerance: Double = 1e-4) -> Bool {
    got.count == want.count
        && zip(got, want).allSatisfy { abs($0 - $1) <= tolerance * max(1, abs($1)) }
}

/// The four brushes' stores for one case, restored afterwards.
private func withFloatBrushOptions(_ body: () throws -> Void) throws {
    let store = ToolOptionsStore.shared
    let saved = (store.blurBrush, store.sharpenBrush, store.smudge, store.historyBrush)
    defer { (store.blurBrush, store.sharpenBrush, store.smudge, store.historyBrush) = saved }
    var tip = PaintToolOptions()
    tip.smoothing = 0
    tip.pressureSize = false
    store.blurBrush = tip
    store.sharpenBrush = tip
    store.smudge = tip
    store.historyBrush = tip
    try body()
}

func verifyFloatRetouchBrushes() throws {
    let document = try floatFixture()
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    let (width, height) = brushPicture

    // The picture: 160 × 120, red a ramp from 0 to 8 across the width, green
    // 0.5 and blue 0.25, with one 5000 blob at (100…104, 80).
    _ = try agentOK("canvas_size", document,
                    ["width": width, "height": height, "anchor": "top-left"])
    _ = try agentOK("gradient", document, [
        "layer": 0, "start_rgba32": [0, 0.5, 0.25, 1], "end_rgba32": [8, 0.5, 0.25, 1],
        "x0": 0.5, "y0": 60, "x1": Double(width) - 0.5, "y1": 60,
    ])
    let blobCentre = (x: 102, y: 80)
    _ = try agentOK("brush_stroke", document, [
        "layer": 0, "points": [[100.5, 80.5], [104.5, 80.5]], "size": 10, "hardness": 100,
        "rgba32": brushHighlight,
    ])
    guard let base = document.doc, base.isFloat, base.width == width, base.height == height else {
        throw ProbeFailure(description: "[float brushes] the 160 × 120 float picture was not built")
    }
    let scene = { (x: Int) -> [Double] in [8 * Double(x) / Double(width - 1), 0.5, 0.25, 1] }
    let lit = try brushPixel(document, blobCentre.x, blobCentre.y)
    try require(brushNear(lit, brushHighlight), "[float brushes] the blob is painted: \(lit)")
    guard let baseState = document.history.currentID else {
        throw ProbeFailure(description: "[float brushes] the picture has no history state")
    }

    // The tool rail: the five brushes are admitted, the Sponge included.
    for tool in [EditorTool.blurBrush, .sharpenBrush, .smudge, .historyBrush, .sponge] {
        try require(DocumentPrecision.allows(tool, document: document),
                    "[float brushes] \(tool) is admitted")
    }

    /// One undo returns to exactly the state and document before the stroke.
    func oneUndo(_ what: String, to before: RasterDocument, state: String) throws {
        try require(document.history.currentID != state, "[float brushes] \(what) added a state")
        document.undoManager?.undo()
        try require(document.doc === before && document.history.currentID == state,
                    "[float brushes] \(what) is exactly one undo")
    }

    try withFloatBrushOptions {
        // (a) A Blur stroke across the blob: one job; a pixel off the blob is
        // lifted far above white and the blob's centre is lowered.
        let beside = (x: 102, y: 88)
        let before = try brushPixel(document, beside.x, beside.y)
        try require(brushNear(before, scene(beside.x)),
                    "[float brushes] the pixel beside the blob holds the scene: \(before)")
        controller.selectTool(.blurBrush)
        try require(controller.currentTool == .blurBrush, "[float brushes] the Blur brush is selected")
        canvas.brushSize = 24
        canvas.brushOpacity = 1
        AutomationAlerts.reset()
        try brushStroke(controller, from: CGPoint(x: 96.5, y: 80.5), to: CGPoint(x: 108.5, y: 80.5))
        let blur = try finishedBrushJob("[float brushes] the Blur stroke")
        try require(blur.state == "completed" && document.doc !== base,
                    "[float brushes] the Blur stroke completed: \(blur.report)")
        try require(AutomationAlerts.recorded.isEmpty,
                    "[float brushes] the Blur stroke raised no alert: \(AutomationAlerts.recorded)")
        let spread = try brushPixel(document, beside.x, beside.y)
        let centre = try brushPixel(document, blobCentre.x, blobCentre.y)
        try require(spread[1] > 1 && spread[3] == 1,
                    "[float brushes] the highlight spread above white beside the blob: \(spread)")
        try require(centre[1] > 1 && centre[1] < brushHighlight[1],
                    "[float brushes] the blob's centre is lowered and stays above white: \(centre)")
        try oneUndo("the Blur stroke", to: base, state: baseState)

        // (b) A Smudge stroke from the blob 30 px left: one job; the end of
        // the stroke, scene before, reads the carried highlight.
        let end = (x: 72, y: 80)
        let dark = try brushPixel(document, end.x, end.y)
        try require(brushNear(dark, scene(end.x)),
                    "[float brushes] the stroke's end holds the scene: \(dark)")
        controller.selectTool(.smudge)
        try require(controller.currentTool == .smudge, "[float brushes] Smudge is selected")
        canvas.brushSize = 16
        canvas.brushOpacity = 1
        try brushStroke(controller, from: CGPoint(x: 102.5, y: 80.5), to: CGPoint(x: 72.5, y: 80.5))
        let smudge = try finishedBrushJob("[float brushes] the Smudge stroke")
        try require(smudge.state == "completed" && document.doc !== base,
                    "[float brushes] the Smudge stroke completed: \(smudge.report)")
        let carried = try brushPixel(document, end.x, end.y)
        try require(carried[1] > 1000 && carried[3] == 1,
                    "[float brushes] the highlight was carried to the stroke's end: \(carried)")
        try oneUndo("the Smudge stroke", to: base, state: baseState)

        // (c) The History Brush: the picture is the source state; a bright
        // stroke is painted over the ramp, and the History Brush at full
        // opacity restores the ramp's own numbers there.
        document.history.brushSourceID = baseState
        let from = CGPoint(x: 30.5, y: 40.5), to = CGPoint(x: 50.5, y: 40.5)
        let middle = (x: 40, y: 40)
        let original = try brushPixel(document, middle.x, middle.y)
        try require(brushNear(original, scene(middle.x)) && original[0] > 1,
                    "[float brushes] the ramp is above white where it is painted over: \(original)")
        _ = try agentOK("brush_stroke", document, [
            "layer": 0, "points": [[from.x, from.y], [to.x, to.y]], "size": 10, "hardness": 100,
            "rgba32": [20, 10, 5, 1],
        ])
        guard let painted = document.doc, painted !== base,
              let paintedState = document.history.currentID else {
            throw ProbeFailure(description: "[float brushes] the bright stroke was not painted")
        }
        let bright = try brushPixel(document, middle.x, middle.y)
        try require(brushNear(bright, [20, 10, 5, 1]),
                    "[float brushes] the bright stroke is painted: \(bright)")
        controller.selectTool(.historyBrush)
        try require(controller.currentTool == .historyBrush,
                    "[float brushes] the History Brush is selected")
        canvas.brushSize = 16
        canvas.brushOpacity = 1
        try brushStroke(controller, from: from, to: to)
        let history = try finishedBrushJob("[float brushes] the History Brush stroke")
        try require(history.state == "completed" && document.doc !== painted,
                    "[float brushes] the History Brush stroke completed: \(history.report)")
        let restored = try brushPixel(document, middle.x, middle.y)
        try require(restored == original,
                    "[float brushes] the History Brush restores the state's own numbers: "
                    + "\(restored) vs \(original)")
        try oneUndo("the History Brush stroke", to: painted, state: paintedState)
    }
    print("PASS: float-retouch-brushes — in a 32-bit float document the Blur, Sharpen, Smudge and "
        + "History brushes are admitted, and the Sponge too; a Blur stroke across a 5000 blob is "
        + "one job that lifts a pixel beside it above white and lowers its centre, one undo; a "
        + "Smudge stroke from the blob is one job that carries the highlight 30 px to the stroke's "
        + "end, one undo; a History Brush stroke at full opacity over a painted line is one job "
        + "that restores the source state's own numbers, one undo")
}
