import AppKit

func verifySelectionConstructionBounds() throws {
    let width = 512, height = 512
    var checker = [UInt8](repeating: 0, count: width * height)
    for y in 0..<height { for x in 0..<width where (x + y) % 2 == 0 { checker[y * width + x] = 255 } }
    guard let selection = CanvasSelection(shape: .mask(checker), canvasWidth: width, canvasHeight: height) else {
        throw ProbeFailure(description: "Fragmented coverage fixture")
    }
    try require(selection.contourIsSimplified && selection.maskContour == nil,
                "Fragmented coverage stops contour allocation at its declared vertex cap")
    try require(selection.maskBytes() == checker && selection.maskImage != nil && selection.inverseMaskImage != nil,
                "Contour fallback changes only display geometry, preserving exact coverage and outside dimming")
    try require(selection.outlineNotice?.contains("coverage remains exact") == true,
                "Simplified contour exposes an explicit display-only notice")
    guard let simple = CanvasSelection(shape: .mask(Array(repeating: 255, count: 100 * 100)),
                                       canvasWidth: 100, canvasHeight: 100) else {
        throw ProbeFailure(description: "Simple contour fixture")
    }
    try require(!simple.contourIsSimplified && simple.maskContour != nil,
                "Simple masks retain exact contour geometry")
    var parameters = ColorRangeParameters()
    parameters.range = "highlights"
    parameters.lower = .nan
    parameters.tolerance = .nan
    let valid = try parameters.validated()
    try require(JSONSerialization.isValidJSONObject(valid.arguments)
                && valid.arguments["lower"] == nil && valid.arguments["tolerance"] == nil,
                "Changing to a tonal preset ignores and never records invalid disabled fields")
    print("PASS: fragmented-mask contour allocation is capped; exact coverage survives and range presets ignore unused fields")

    let document = try fixture(width: 600, height: 400)
    defer { document.close() }
    let capture = try SelectionCapture(document: document, needsSource: false)
    let newSelection = CanvasSelection(shape: .rect(CGRect(x: 1, y: 1, width: 4, height: 4)),
                                      canvasWidth: 600, canvasHeight: 400)
    var serviced = false
    let timer = Timer.scheduledTimer(withTimeInterval: 0.001, repeats: true) { timer in
        guard BackgroundWork.isWaiting else { return }
        serviced = true
        document.setCanvasSelection(newSelection)
        timer.invalidate()
    }
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate() }
    do {
        try capture.commit(.rect(CGRect(x: 30, y: 30, width: 200, height: 180)),
                           in: document, name: "Feather stale test", feather: 25)
        throw ProbeFailure(description: "Feather committed across changed selection")
    } catch is DoctoredCoreError {}
    try require(serviced && document.canvasSelection?.bounds.width == 4 && !BackgroundWork.occupied,
                "Nonzero selection feather runs off-main and refuses its stale result after servicing newer input")
    print("PASS: feathered selection construction uses responsive bounded work and preserves a newer selection")
}
