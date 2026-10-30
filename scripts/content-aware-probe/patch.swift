import AppKit

/// Actual Patch gestures and replay, with an independent affine-background
/// expectation. The fixture carries genuine sub-byte distinctions at depth16.
func runPatchCase(depth: Int, destination: Bool, ellipse: Bool,
                  fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "patch-ui-\(depth)-\(destination ? "destination" : "source")-\(ellipse ? "selection" : "polygon")"
    print("START \(name)"); fflush(stdout)
    let path = fixtures.appendingPathComponent("ramp-\(depth).png")
    let source: RasterDocument
    if depth == 16 {
        let budget = try PrecisionWorkContext()
        source = try RasterDocument(precision: RasterPrecisionDocument.openPreserving(url: path,
            workingLimit: budget.limit, otherRetained: budget.otherRetained))
    } else { source = try RasterDocument.open(url: path) }
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: depth, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers(); document.showWindows()
    defer { document.close() }
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Patch editor missing")
    }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    let rect = CGRect(x: destination ? 8 : 24, y: 16, width: 16, height: 16)
    let shape: CanvasSelection.Shape = ellipse ? .ellipse(rect) : .polygon([
        CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: rect.maxX, y: rect.minY),
        CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY)])
    guard let region = CanvasSelection(shape: shape, canvasWidth: 64, canvasHeight: 48) else {
        throw DoctoredCoreError(message: "Patch selection fixture missing")
    }
    editor.agentSetSelection(region)
    var options = PatchToolOptions()
    options.directionIndex = destination ? 1 : 0
    options.sample = .current
    ToolOptionsStore.shared.patch = options
    editor.selectTool(.patch)
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, "Patch setup worker")
    RunLoop.current.run(until: Date().addingTimeInterval(0.02))
    let original = try values(source)
    let canvas = editor.canvas
    try require(canvas.tool == .patch && DocumentPrecision.allows(.patch, document: document), "Patch tool available")
    let from = CGPoint(x: rect.midX, y: rect.midY)
    let to = CGPoint(x: from.x + (destination ? 16 : -16), y: from.y)
    ActionRecorder.shared.start()
    try event(canvas, .leftMouseDown, from)
    try event(canvas, .leftMouseDragged, to)
    try require(document.doc === source, "Patch drag remains unpublished")
    try screenshot(editor.view, output.appendingPathComponent(name + "-drag.png"))
    // Options changed after capture must not alter either end of this gesture.
    ToolOptionsStore.shared.patch.directionIndex = destination ? 0 : 1
    ToolOptionsStore.shared.patch.sample = .all
    try event(canvas, .leftMouseUp, to)
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, "Native Patch worker")
    RunLoop.current.run(until: Date().addingTimeInterval(0.02))
    let records = ActionRecorder.shared.stop()
    guard let edited = document.doc else { throw DoctoredCoreError(message: "Patch result missing") }
    let expected = try values(edited)
    try require(edited !== source && expected != original, "Patch changes native pixels")
    try require(records.count == 1 && records[0].tool == "patch_region", "One recorded Patch action")
    try require(records[0].arguments["direction"] as? String == (destination ? "destination" : "source"), "Captured direction recorded")
    try require(records[0].arguments["sample"] as? String == "current", "Captured sampling recorded")
    try require((records[0].arguments["points"] == nil) == ellipse, "Selection/polygon recording contract")
    let covered = region.maskBytes()
    for y in 0..<48 { for x in 0..<64 {
        let index = (y * 64 + x) * 4
        try require(expected[index + 3] == original[index + 3], "Patch preserves alpha")
        let sx = x - (destination ? 16 : 0)
        if sx < 0 || sx >= 64 || covered[y * 64 + sx] < 128 {
            for c in 0..<4 { try require(expected[index + c] == original[index + c], "Patch preserves outside coverage") }
        }
    }}
    let centre = (22 * 64 + 30) * 4
    let ramp = 12001 + 97 * 30 + 71 * 22
    for c in 0..<3 {
        let reference = ramp + c * 3001
        let limit = depth == 16 ? 4 : 2 * 257
        try require(abs(Int(expected[centre + c]) - reference) <= limit, "Independent affine Patch reference")
    }
    document.undoManager?.undo()
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, "Patch Undo")
    try require(document.doc === source && !document.isDocumentEdited, "One Undo restores Patch source")
    document.undoManager?.redo()
    try require(document.doc === edited, "Patch Redo exact owner")
    document.undoManager?.undo()
    var replayArguments = records[0].arguments
    replayArguments["document_id"] = AgentServer.shared.documentID(for: document)
    let arguments = try JSONSerialization.data(withJSONObject: replayArguments, options: .sortedKeys)
    let reply = AgentServer.shared.execute(tool: "patch_region",
        argumentsJSON: String(decoding: arguments, as: UTF8.self))
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, "Patch Action replay")
    try require(try values(document.doc ?? source) == expected, "Patch native replay exact: \(reply)")

    // A captured document/selection change refuses even when its layer index
    // remains valid; cancellation also drops the sole source capture owner.
    let captured = try PatchEditCapture(document: document, layer: 0,
        choice: RetouchSamplingChoice(mode: .current, ignoreAdjustments: false),
        destination: destination, preview: true)
    editor.agentSetSelection(nil)
    do {
        try captured.capture.requireCurrent(document)
        throw DoctoredCoreError(message: "Stale Patch capture unexpectedly accepted")
    } catch let error as DoctoredCoreError {
        try require(error.message.contains("changed"), "Stale source refusal")
    }
    var session = PatchSession()
    _ = session.mouseDown(from, clickCount: 1, selection: region, in: captured.previewImage,
        magnification: 1, sampling: { captured })
    _ = session.mouseDragged(to)
    session.cancel()
    try require(session.mouseUp(to) == nil, "Cancelled Patch has no commit")
    print("PASS \(name)"); fflush(stdout)
    return ["name":name,"passed":true,"native_components":expected.count,
        "backing_scale":canvas.window?.backingScaleFactor ?? 0,"action":records[0].arguments]
}
