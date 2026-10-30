import AppKit

func check(_ value: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !value() { throw DoctoredCoreError(message: message) }
}
func idle() throws {
    let deadline = Date().addingTimeInterval(60)
    while (BackgroundWork.occupied || DocumentJobs.active != nil) && Date() < deadline {
        RunLoop.current.run(until: Date().addingTimeInterval(0.01))
    }
    try check(!BackgroundWork.occupied && DocumentJobs.active == nil, "Owned workers finish")
}
func samples(_ doc: RasterDocument) throws -> [UInt16] {
    if let source = doc.precision {
        let budget = try PrecisionWorkContext(source: source)
        return try source.render(workingLimit: budget.limit, otherRetained: budget.otherRetained)
            .withRGBA16 { Array($0) }
    }
    return try doc.render().withPlateBytes { pointer in
        Array(UnsafeBufferPointer(start: pointer, count: doc.width * doc.height * 4)).map { UInt16($0) * 257 }
    } ?? []
}
func event(_ view: NSView, _ kind: NSEvent.EventType, _ point: CGPoint,
           flags: NSEvent.ModifierFlags = []) throws {
    guard let window = view.window,
          let event = NSEvent.mouseEvent(with: kind, location: view.convert(point, to: nil), modifierFlags: flags,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
            eventNumber: 0, clickCount: 1, pressure: 1) else { throw DoctoredCoreError(message: "Owned event unavailable") }
    switch kind {
    case .leftMouseDown: view.mouseDown(with: event)
    case .leftMouseDragged: view.mouseDragged(with: event)
    case .leftMouseUp: view.mouseUp(with: event)
    case .mouseMoved: view.mouseMoved(with: event)
    default: break
    }
}
func screenshot(_ view: NSView, _ url: URL) throws {
    view.layoutSubtreeIfNeeded()
    guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw DoctoredCoreError(message: "No owned screenshot") }
    view.cacheDisplay(in: view.bounds, to: bitmap)
    guard let data = bitmap.representation(using: .png, properties: [:]) else { throw DoctoredCoreError(message: "No PNG") }
    try data.write(to: url)
}
func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
func verifyControls(_ editor: EditorViewController, output: URL) throws {
    editor.view.layoutSubtreeIfNeeded()
    editor.optionsBar.layoutSubtreeIfNeeded()
    guard let more = descendants(editor.view).compactMap({ $0 as? MoreButton }).first else {
        throw DoctoredCoreError(message: "Missing actual More button")
    }
    try event(more, .leftMouseDown, CGPoint(x: more.bounds.midX, y: more.bounds.midY))
    RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    defer { editor.optionsBar.closeOverflow() }
    // Inspect the bar's actual owned overflow panel rather than guessing
    // at NSApp.windows.
    let slot = Mirror(reflecting: editor.optionsBar).children.first { $0.label == "overflowPanel" }?.value
    let panel = slot.flatMap { ($0 as? OverflowPanel) ?? (Mirror(reflecting: $0).children.first?.value as? OverflowPanel) }
    guard let panel, panel.isVisible, let content = panel.contentView else {
        throw DoctoredCoreError(message: "Source controls are not reachable in More; owned overflow panel visible: \(panel?.isVisible ?? false)")
    }
    let labels = descendants(content).compactMap { $0 as? NSTextField }.map(\.stringValue)
    for title in ["Source scale", "Source rotation", "Source overlay", "Expand destination",
                  "Flip source horizontally", "Flip source vertically"] {
        try check(labels.contains(title), "Visible control: " + title)
    }
    try screenshot(content, output)
}
func runCase(depth: Int, tool: EditorTool, fixtures: URL, output: URL, artwork: Bool = false) throws -> [String: Any] {
    let name = "ui-\(depth)-\(tool.agentToolName)" + (artwork ? "-artwork-new-layer" : "")
    print("START \(name)"); fflush(stdout)
    let filename = tool == .redEye ? "red-eye-\(depth).png" : "photo-\(depth).png"
    var source: RasterDocument
    if depth == 16 {
        let budget = try PrecisionWorkContext()
        source = try RasterDocument(precision: RasterPrecisionDocument.openPreserving(
            url: fixtures.appendingPathComponent(filename), workingLimit: budget.limit, otherRetained: budget.otherRetained))
    } else { source = try RasterDocument.open(url: fixtures.appendingPathComponent(filename)) }
    var original = try samples(source)
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: depth, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers(); document.showWindows()
    defer { document.close() }
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Actual editor missing")
    }
    _ = editor.view
    editor.view.window?.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    var targetLayer = 0
    if artwork {
        let args: [String: Any] = ["kind": "rect", "x": 26, "y": 18, "w": 8, "h": 8,
            "fill_rgba": [0.9, 0.15, 0.08, 1.0], "document_id": AgentServer.shared.documentID(for: document)]
        let reply = AgentServer.shared.execute(tool: "add_shape_layer", argumentsJSON:
            String(decoding: try JSONSerialization.data(withJSONObject: args), as: UTF8.self))
        guard let described = document.doc, described.layerCount == 2 else {
            throw DoctoredCoreError(message: "Artwork setup failed: \(reply)")
        }
        source = described; original = try samples(source); targetLayer = 1
        document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
            layerSelection: .single(targetLayer), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
            stripMetadata: false, exportBitDepth: depth, exportDither: "none"), markSaved: true)
    }
    let description = source.layerMeta(targetLayer)
    var options = PaintToolOptions()
    options.size = 12; options.pressureSize = false; options.smoothing = 0
    options.sample = .currentAndBelow; options.outputToNewLayer = tool == .clone || artwork
    options.sourceScale = 125; options.sourceRotation = 10; options.sourceFlipY = true
    ToolOptionsStore.shared.setPaintOptions(options, for: tool)
    editor.selectTool(tool); editor.syncCanvasPaintState()
    try idle()
    let canvas = editor.canvas
    try check(canvas.tool == tool && DocumentPrecision.allows(tool, document: document), "Actual tool available")
    let origin = CGPoint(x: 14.25, y: 22.75), from = CGPoint(x: 29.25, y: 22.25), to = CGPoint(x: 31.25, y: 22.25)
    if tool == .clone || tool == .heal {
        try verifyControls(editor, output: output.appendingPathComponent(name + "-controls.png"))
        try event(canvas, .leftMouseDown, origin, flags: .option)
        try event(canvas, .leftMouseUp, origin, flags: .option)
        try event(canvas, .mouseMoved, from)
        try screenshot(editor.view, output.appendingPathComponent(name + "-hover.png"))
        try check(editor.retouchSourceOverlay?.source === source && editor.retouchSourceOverlay?.image != nil,
            "Hover source is a current native capture")
        try check(document.doc === source && !document.isDocumentEdited, "Hover changes no editing state")
    }
    ActionRecorder.shared.start()
    if tool == .redEye {
        try event(canvas, .leftMouseDown, CGPoint(x: 20, y: 12))
        try event(canvas, .leftMouseDragged, CGPoint(x: 40, y: 32))
        try event(canvas, .leftMouseUp, CGPoint(x: 40, y: 32))
    } else {
        try event(canvas, .leftMouseDown, from)
        if depth == 16 { try check(document.doc === source, "Typed mouse-down remains unpublished") }
        try event(canvas, .leftMouseDragged, to)
        try screenshot(editor.view, output.appendingPathComponent(name + "-drag.png"))
        try event(canvas, .leftMouseUp, to)
    }
    let launchedJob = DocumentJobs.active
    try idle()
    if let launchedJob { print("JOB \(launchedJob.report)"); fflush(stdout) }
    let records = ActionRecorder.shared.stop()
    guard let result = document.doc else { throw DoctoredCoreError(message: "Missing published result") }
    try check(result !== source && document.isDocumentEdited, "Actual gesture publishes")
    let expected = try samples(result)
    try check(expected != original, "Actual gesture changes native samples")
    try check(records.count == 1 && records[0].tool == tool.agentToolName, "One canonical recorded gesture")
    if tool == .clone || tool == .heal {
        try check((records[0].arguments["source_transform"] as? [Double])?.count == 6, "Continuous matrix recorded")
        try check(records[0].arguments["sample"] as? String == "current_and_below", "Captured sampling recorded")
    }
    if tool == .clone || artwork { try check(result.layerCount == source.layerCount + 1, "New retouch layer published") }
    if artwork {
        try check(description != nil && result.layerMeta(targetLayer) == description,
            "New-layer spot repair preserves original artwork description exactly")
        try check(result.hasArtworkDescription(targetLayer), "Original artwork remains editable")
    }
    document.undoManager?.undo()
    try idle()
    try check(document.doc === source && !document.isDocumentEdited, "One Undo restores saved source owner")
    try check(try samples(source) == original, "Source samples immutable")
    document.undoManager?.redo()
    try check(document.doc === result, "Redo restores exact owner")
    document.undoManager?.undo()
    var replay = records[0].arguments
    replay["document_id"] = AgentServer.shared.documentID(for: document)
    let args = try JSONSerialization.data(withJSONObject: replay, options: .sortedKeys)
    let reply = AgentServer.shared.execute(tool: records[0].tool, argumentsJSON: String(decoding: args, as: UTF8.self))
    try idle()
    try check(try samples(document.doc ?? source) == expected, "Recorded UI stroke replay matches every native component; \(reply)")
    if tool == .heal, let first = canvas.strokeCloneGeometry {
        let base = document.doc
        try event(canvas, .leftMouseDown, CGPoint(x: 39.25, y: 24.25))
        guard let next = canvas.strokeCloneGeometry else { throw DoctoredCoreError(message: "Aligned source missing") }
        try check(zip(first.values, next.values).allSatisfy { abs($0 - $1) < 1e-12 },
            "Second aligned gesture keeps the complete affine mapping")
        guard let key = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: canvas.window!.windowNumber,
            context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53)
        else { throw DoctoredCoreError(message: "No owned Escape event") }
        canvas.keyDown(with: key)
        try check(document.doc === base, "Escape cancels aligned gesture without a mutation")
    }
    print("PASS \(name)"); fflush(stdout)
    return ["name": name, "passed": true, "replay_components": expected.count,
        "backing_scale": editor.view.window?.backingScaleFactor ?? 0, "action": records[0].arguments]
}
extension EditorTool {
    var agentToolName: String {
        switch self { case .clone: return "clone_stamp"; case .heal: return "heal_stroke"
        case .spotHeal: return "spot_heal_stroke"; default: return "red_eye" }
    }
}
/// AppKit opens an untitled document at launch unless the app delegate refuses.
final class ProbeAppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }
}
let probeAppDelegate = ProbeAppDelegate()
_ = NSApplication.shared
NSApp.delegate = probeAppDelegate
AutomationAlerts.installIfEnabled()
_ = NSApp.setActivationPolicy(.accessory)
func runProbe() throws {
let environment = ProcessInfo.processInfo.environment
var results: [[String: Any]] = [], failures: [[String: String]] = []
let output = URL(fileURLWithPath: environment["DR_RETOUCH_UI_OUTPUT"]!)
let fixtures = URL(fileURLWithPath: environment["DR_RETOUCH_UI_FIXTURES"]!)
for depth in [8, 16] {
    for tool: EditorTool in [.clone, .heal, .spotHeal, .redEye] {
        do { results.append(try autoreleasepool { try runCase(depth: depth, tool: tool, fixtures: fixtures, output: output) }) }
        catch {
            let message = error.localizedDescription
            print("FAIL \(depth) \(tool.agentToolName): \(message)"); fflush(stdout)
            failures.append(["name": "\(depth)-\(tool.agentToolName)", "error": message])
        }
    }
}
for depth in [8, 16] {
    do { results.append(try autoreleasepool { try runCase(depth: depth, tool: .spotHeal,
        fixtures: fixtures, output: output, artwork: true) }) }
    catch { failures.append(["name": "\(depth)-artwork-new-layer", "error": error.localizedDescription]) }
}
let report: [String: Any] = ["passed": failures.isEmpty, "group_count": results.count, "groups": results,
    "failures": failures, "owned_documents": NSDocumentController.shared.documents.count]
try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
    .write(to: output.appendingPathComponent("host-report.json"))
exit(failures.isEmpty ? 0 : 1)

}
Timer.scheduledTimer(withTimeInterval: 0.05, repeats: false) { _ in
    do { try runProbe() } catch { print(error.localizedDescription); exit(1) }
}
NSApp.run()
