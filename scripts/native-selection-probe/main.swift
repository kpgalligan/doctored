import AppKit

final class ProbeApplication: NSApplication {
    var reportedErrors: [String] = []
    override func presentError(_ error: Error) -> Bool {
        reportedErrors.append(error.localizedDescription)
        trace("UNEXPECTED APP ERROR: \(error.localizedDescription)")
        return false
    }
    override func reportException(_ exception: NSException) {
        print("OBJECTIVE-C EXCEPTION: \(exception.name.rawValue): \(exception.reason ?? "unknown")")
        print(exception.callStackSymbols.joined(separator: "\n")); fflush(stdout)
        exit(2)
    }
}
func trace(_ message: String) { print(message); fflush(stdout) }
func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw DoctoredCoreError(message: message) }
}
func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
func find<T: NSView>(_ root: NSView, _ id: String, _: T.Type) throws -> T {
    guard let value = descendants(root).first(where: {
        $0.identifier?.rawValue == id || $0.accessibilityIdentifier() == id
    }) as? T else { throw DoctoredCoreError(message: "Missing actual UI control \(id)") }
    return value
}
func button(_ root: NSView, _ title: String) throws -> NSButton {
    guard let value = descendants(root).compactMap({ $0 as? NSButton }).first(where: { $0.title == title }) else {
        throw DoctoredCoreError(message: "Missing actual UI button \(title)")
    }
    return value
}
func activate(_ control: NSControl) throws {
    guard let action = control.action else { throw DoctoredCoreError(message: "Control has no action") }
    control.sendAction(action, to: control.target)
}
func pump() {
    var count = 0
    while count < 64, let event = NSApp.nextEvent(matching: .any, until: Date(), inMode: .default, dequeue: true) {
        NSApp.sendEvent(event); count += 1
    }
    NSApp.updateWindows()
    RunLoop.current.run(until: Date().addingTimeInterval(0.01))
}
func waitUntil(_ condition: () -> Bool, _ message: String) throws {
    let deadline = Date().addingTimeInterval(60)
    while !condition(), Date() < deadline { pump() }
    try require(condition(), message)
}
func member(_ owner: Any, _ key: String) -> Any? {
    guard let value = Mirror(reflecting: owner).children.first(where: { $0.label == key })?.value else { return nil }
    let mirror = Mirror(reflecting: value)
    return mirror.displayStyle == .optional ? mirror.children.first?.value : value
}
func renderer(_ sheet: NSViewController) throws -> PreviewRenderer {
    let owner = member(sheet, "precisionSession") ?? sheet
    guard let result = member(owner, "renderer") as? PreviewRenderer else {
        throw DoctoredCoreError(message: "Actual sheet preview renderer missing")
    }
    return result
}
func idle(_ renderer: PreviewRenderer) -> Bool {
    member(renderer, "isRendering") as? Bool == false && member(renderer, "pending") == nil && member(renderer, "debounce") == nil
}
func values(_ doc: RasterDocument) throws -> [UInt16] {
    if let source = doc.precision {
        let budget = try PrecisionWorkContext(source: source)
        return try source.render(workingLimit: budget.limit, otherRetained: budget.otherRetained)
            .withRGBA16 { Array($0) }
    }
    return try doc.render().withPlateBytes { pointer in
        Array(UnsafeBufferPointer(start: pointer, count: doc.width * doc.height * 4)).map { UInt16($0) * 257 }
    } ?? []
}
func displayed(_ image: CGImage) throws -> [UInt8] {
    guard let result = Bitmap.straightRGBA(from: image, fitting: image.width, height: image.height,
                                          space: Bitmap.ingestSpace(of: image).space) else {
        throw DoctoredCoreError(message: "Display projection unavailable")
    }
    return result
}
func screenshot(_ view: NSView, _ path: URL) throws {
    view.layoutSubtreeIfNeeded()
    guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
        throw DoctoredCoreError(message: "Owned UI screenshot unavailable")
    }
    view.cacheDisplay(in: view.bounds, to: bitmap)
    guard let bytes = bitmap.representation(using: .png, properties: [:]) else { throw DoctoredCoreError(message: "Screenshot encode failed") }
    try bytes.write(to: path)
}
func openDocument(depth: Int, fixtures: URL) throws -> (ImageDocument, EditorViewController, RasterDocument) {
    let path = fixtures.appendingPathComponent("fixture-\(depth)-opaque.png")
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
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Actual editor missing")
    }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    return (document, editor, source)
}
func installSelection(_ editor: EditorViewController, width: Int, height: Int) throws -> [UInt8] {
    let mask: [UInt8] = (0..<(width*height)).map { [UInt8(0), 1, 128, 255][$0 % 4] }
    guard let selected = CanvasSelection(shape: .mask(mask), canvasWidth: width, canvasHeight: height) else {
        throw DoctoredCoreError(message: "Fixture selection missing")
    }
    editor.agentSetSelection(selected)
    return mask
}
func openSheet(_ editor: EditorViewController, kind: String) throws -> NSViewController {
    try waitUntil({ editor.view.window?.attachedSheet == nil }, "Previous sheet did not detach")
    switch kind {
    case "levels": editor.showLevels(nil)
    case "threshold": editor.showThreshold(nil)
    case "blur": editor.showBlur(nil)
    default: throw DoctoredCoreError(message: "Unknown probe sheet")
    }
    guard let sheet = editor.presentedViewControllers?.last else { throw DoctoredCoreError(message: "Actual sheet was not presented") }
    _ = sheet.view
    try waitUntil({ sheet.view.window?.sheetParent != nil }, "Actual sheet did not attach")
    return sheet
}
func scalarMixReference(_ original: UInt16, _ edited: UInt16, _ coverage: UInt16) -> UInt16 {
    let weight = UInt64(coverage)
    let source = UInt64(original) * (65535 - weight)
    let destination = UInt64(edited) * weight
    return UInt16((source + destination + 32767) / 65535)
}
func scalarSamples(_ doc: RasterDocument, _ target: PaintTarget, layer: Int = 0) throws -> [UInt16] {
    if doc.isHighDepth {
        let target = try PrecisionScalarOperations.target(target, layer: layer)
        return try PrecisionScalarOperations.read(doc, target: target.0, index: target.1).samples
    }
    switch target {
    case .channel(let index): return doc.channelPlane(index)?.map { UInt16($0) * 257 } ?? []
    case .mask: return doc.layerPlane(layer, DR_PLANE_MASK)?.map { UInt16($0) * 257 } ?? []
    case .plane(let plane): return doc.layerPlane(layer, plane.dr)?.map { UInt16($0) * 257 } ?? []
    case .layer: return try values(doc)
    case .ink: throw DoctoredCoreError(message: "A byte document has no ink plate")
    }
}
func isolateChannel(_ editor: EditorViewController, index: Int) throws {
    editor.showChannels(nil); pump()
    guard let panel = editor.channelsPanel else { throw DoctoredCoreError(message:"Actual Channels panel missing") }
    panel.reload()
    guard let table = member(panel,"tableView") as? NSTableView else { throw DoctoredCoreError(message:"Channels table missing") }
    let row = 4 + (editor.document?.doc?.layerHasMask(editor.document?.activeLayerIndex ?? 0) == true ? 1 : 0) + index
    table.selectRowIndexes(IndexSet(integer:row),byExtendingSelection:false)
    if panel.visibility.rgb {
        guard let rgb = table.view(atColumn:0,row:0,makeIfNecessary:true), let eye = member(rgb,"eyeButton") as? NSButton else {
            throw DoctoredCoreError(message:"Actual RGB eye control missing")
        }
        try require(eye.isEnabled,"Native eye was disabled")
        eye.performClick(nil)
    }
    editor.refreshChannelDisplay()
    try require(editor.paintTarget == .channel(index) && editor.canvas.channelDisplay?.baseKind == .channel(index),"Actual channel isolation did not follow row/eye controls")
}
func scalarMouseDown(window: NSWindow, location: CGPoint) throws -> NSEvent {
    window.makeKeyAndOrderFront(nil)
    guard let event = NSEvent.mouseEvent(with:.leftMouseDown,location:location,modifierFlags:[],timestamp:ProcessInfo.processInfo.systemUptime,
        windowNumber:window.windowNumber,context:nil,eventNumber:1,clickCount:1,pressure:1) else { throw DoctoredCoreError(message:"Scalar fill event missing") }
    return event
}
func runScalarUI(depth: Int, fixtures: URL, output: URL) throws -> [String: Any] {
    let (document, editor, original) = try openDocument(depth: depth, fixtures: fixtures)
    let originalBrush = ToolOptionsStore.shared.brush
    defer {
        ToolOptionsStore.shared.brush = originalBrush
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    let codes: [UInt16] = (0..<(original.width * original.height)).map {
        depth == 16 ? [UInt16(0), 1, 17, 256, 257, 32769, 65534, 65535][$0 % 8]
            : [UInt16(0), 257, 32896, 65535][$0 % 4]
    }
    let selected = CanvasSelection(shape: depth == 16 ? .mask16(codes) : .mask(codes.map { UInt8($0 / 257) }),
        canvasWidth: original.width, canvasHeight: original.height)
    editor.agentSetSelection(selected)
    document.undoManager?.removeAllActions()
    editor.saveSelection(to: .newChannel("Native coverage"), mode: .replace)
    let withChannel = document.doc ?? original
    try require(try withChannel.channelCount == 1 && scalarSamples(withChannel, .channel(0)) == codes, "UI saves exact native selection")
    document.undoManager?.undo(); pump()
    try require(document.doc === original, "Save selection has one Undo")
    document.undoManager?.redo(); pump()
    editor.agentSetSelection(nil)
    editor.loadSelection(from: .channel(0), mode: .replace, invert: false)
    try require(editor.agentSelection?.maskSamples16() == codes, "UI reloads every native channel code")
    document.undoManager?.removeAllActions()
    try isolateChannel(editor,index:0)
    document.undoManager?.removeAllActions()
    editor.deleteChannel(nil); pump()
    try require(document.doc?.channelCount == 0 && editor.paintTarget == .layer,"Delete must clamp absent channel target")
    document.undoManager?.undo(); pump()
    try require(document.doc === withChannel && editor.paintTarget == .channel(0) && editor.canvas.channelDisplay?.baseKind == .channel(0),
        "Delete Undo must restore exact channel target and isolation")
    document.undoManager?.redo(); pump()
    try require(document.doc?.channelCount == 0 && editor.paintTarget == .layer,"Delete Redo target")
    document.undoManager?.undo(); pump()
    try require(editor.paintTarget == .channel(0) && editor.canvas.channelDisplay?.baseKind == .channel(0),"Repeated Undo lost scalar view")
    let display = withChannel.channelDisplayBytes(0)
    try require(display == codes.map { UInt8((UInt32($0) + 128) / 257) }, "Explicit scalar display reduction")
    let isolation = try AgentServer.planeRenderImage(withChannel, channel: "Native coverage", layer: nil)
    try require(isolation.image != nil, "MCP scalar render has an explicit derivative")
    if depth == 16 {
        document.applyEdit("Fixture precise channel display",record:.notACommand) {
            try $0.settingChannelOverlay(0,red:217,green:43,blue:79,opacity:0.543219,indicatesSelected:true)
        }
        let optionsSource=document.doc ?? original
        document.undoManager?.removeAllActions()
        editor.channelOptions(nil)
        guard let optionsSheet=editor.presentedViewControllers?.last as? ChannelOptionsSheetController else {
            throw DoctoredCoreError(message:"Actual Channel Options sheet missing")
        }
        _ = optionsSheet.view
        try screenshot(optionsSheet.view,output.appendingPathComponent("channel-options-fine-opacity.png"))
        try button(optionsSheet.view,"Apply").performClick(nil)
        try waitUntil({editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied},"Channel Options Apply stalled")
        try require(document.doc === optionsSource && document.undoManager?.canUndo != true,
            "Unchanged Channel Options rounded precise opacity or created phantom history")
        ActionRecorder.shared.start()
        editor.channelOptions(nil)
        guard let renamedSheet=editor.presentedViewControllers?.last as? ChannelOptionsSheetController,
              let nameField=member(renamedSheet,"nameField") as? NSTextField else {
            throw DoctoredCoreError(message:"Channel Options name field missing")
        }
        nameField.stringValue="Renamed coverage"
        try button(renamedSheet.view,"Apply").performClick(nil)
        try waitUntil({editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied},"Channel Options rename stalled")
        let channelSteps=ActionRecorder.shared.stop()
        try require(channelSteps.count == 1 && channelSteps[0].arguments["overlay_opacity"] as? Double == optionsSource.channelInfo(0)?.opacity,
            "Channel Options Action rounded precise overlay opacity")
        document.undoManager?.undo();pump()
        try require(document.doc === optionsSource,"Channel Options rename one Undo")
        editor.agentSetSelection(nil)
        let strokeBefore = document.doc ?? original
        document.undoManager?.removeAllActions()
        ToolOptionsStore.shared.brush.pressureSize = false
        ToolOptionsStore.shared.brush.airbrush = false
        editor.selectTool(.brush); editor.canvas.brushSize = 2; editor.canvas.brushOpacity = 0.75
        guard let window = editor.canvas.window else { throw DoctoredCoreError(message: "Scalar canvas window missing") }
        let location = editor.canvas.convert(CGPoint(x:8,y:6),to:nil)
        ActionRecorder.shared.start()
        for type in [NSEvent.EventType.leftMouseDown,.leftMouseUp] {
            guard let event = NSEvent.mouseEvent(with:type,location:location,modifierFlags:[],timestamp:ProcessInfo.processInfo.systemUptime,
                windowNumber:window.windowNumber,context:nil,eventNumber:1,clickCount:1,pressure:1) else { throw DoctoredCoreError(message:"Scalar event missing") }
            if type == .leftMouseDown { editor.canvas.mouseDown(with:event) } else { editor.canvas.mouseUp(with:event) }
        }
        try waitUntil({ !BackgroundWork.occupied },"Native scalar stroke stalled")
        let strokeSteps = ActionRecorder.shared.stop()
        let painted = try scalarSamples(document.doc ?? original,.channel(0))
        try require(painted != codes && painted[1] == 1 && painted[17] == 1,"Actual scalar brush preserves untouched tiny codes")
        try require(strokeSteps.count == 1 && strokeSteps[0].arguments["target"] as? String == "channel:Native coverage","Scalar gesture records exact target")
        try require(try values(document.doc ?? original) == values(strokeBefore),"Scalar brush changed RGB/alpha")
        document.undoManager?.undo(); pump()
        try require(document.doc === strokeBefore,"Scalar stroke one Undo")
        var replay = strokeSteps[0].arguments
        replay["document_id"] = AgentServer.shared.documentID(for:document)
        let json = try JSONSerialization.data(withJSONObject:replay,options:.sortedKeys)
        let response = AgentServer.shared.execute(tool:strokeSteps[0].tool,argumentsJSON:String(decoding:json,as:UTF8.self))
        try waitUntil({ !BackgroundWork.occupied },"Scalar stroke Action stalled")
        try require(try scalarSamples(document.doc ?? original,.channel(0)) == painted,"Scalar Action replay differs: \(response)")
        document.undoManager?.undo(); pump()
        let oldFill = ToolOptionsStore.shared.fill
        ToolOptionsStore.shared.fill.tolerance = 0
        ToolOptionsStore.shared.fill.contiguous = true
        ToolOptionsStore.shared.fill.opacity = 100
        editor.paintColor = .white; editor.selectTool(.fill)
        editor.canvas.mouseDown(with: try scalarMouseDown(window:window,location:location))
        try waitUntil({ !BackgroundWork.occupied },"Scalar bucket UI stalled")
        // The zero-valued seed is a vertical stripe; equal-code neighbors
        // connect through its entire column, with nonzero walls on both sides.
        var filled = codes
        for y in 0..<original.height { filled[y * original.width + 8] = 65535 }
        try require(try scalarSamples(document.doc ?? original,.channel(0)) == filled,"Actual scalar bucket zero tolerance changed other codes")
        try require(try values(document.doc ?? original) == values(strokeBefore),"Scalar Fill changed RGB/alpha")
        document.undoManager?.undo(); pump()
        ToolOptionsStore.shared.fill = oldFill
        let before = document.doc ?? original
        let selection = CanvasSelection(shape: .mask16((0..<codes.count).map { $0 % 3 == 0 ? 0 : 32769 }),
            canvasWidth: original.width, canvasHeight: original.height)
        editor.agentSetSelection(selection)
        document.undoManager?.removeAllActions()
        editor.modifyNativePlane(nil)
        guard let sheet = editor.presentedViewControllers?.last else { throw DoctoredCoreError(message: "Actual morphology sheet missing") }
        _ = sheet.view
        try screenshot(sheet.view, output.appendingPathComponent("scalar-morphology-16.png"))
        try button(sheet.view, "Apply").performClick(nil)
        try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied }, "Scalar morphology publication stalled")
        let after = document.doc ?? before
        let got = try scalarSamples(after, .channel(0))
        let weights = selection?.maskSamples16() ?? []
        let expected: [UInt16] = zip(codes, weights).map { pair in
            scalarMixReference(pair.0, 65535 - pair.0, pair.1)
        }
        try require(got == expected, "Actual morphology UI matches independent scalar interpolation")
        try require(try values(after) == values(before), "Scalar morphology preserves complete RGB/alpha")
        document.undoManager?.undo(); pump()
        try require(document.doc === before, "Scalar morphology one Undo")
        try require(editor.paintTarget == .channel(0) && editor.canvas.channelDisplay?.baseKind == .channel(0),"Scalar Undo lost target/view")
        document.undoManager?.redo(); pump()
        try require(document.doc === after, "Scalar morphology exact Redo")
        document.undoManager?.undo(); pump()
        editor.modifyNativePlane(nil)
        guard let cancelled = editor.presentedViewControllers?.last else { throw DoctoredCoreError(message: "Morphology cancel sheet missing") }
        try button(cancelled.view, "Cancel").performClick(nil)
        try waitUntil({ editor.view.window?.attachedSheet == nil }, "Morphology Cancel stalled")
        try require(document.doc === before, "Morphology Cancel keeps source")
        // Actual tone sheet targets the channel captured when opened.
        let tone = try openSheet(editor, kind: "threshold")
        let session = member(tone, "precisionSession")
        guard let scalar = session.flatMap({ member($0, "scalar") }),
              let preview = member(scalar, "renderer") as? PreviewRenderer else { throw DoctoredCoreError(message: "Scalar tone preview missing") }
        try waitUntil({ idle(preview) }, "Scalar preview did not settle")
        guard let inspected = editor.canvas.previewImage else { throw DoctoredCoreError(message:"Actual scalar preview is empty") }
        let previewPixels = try displayed(inspected)
        try screenshot(tone.view, output.appendingPathComponent("scalar-threshold-16.png"))
        try button(tone.view, "Apply").performClick(nil)
        try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied }, "Scalar tone Apply stalled")
        let toneSamples = try scalarSamples(document.doc ?? before, .channel(0))
        let reference: [UInt16] = zip(codes, weights).map { pair in
            scalarMixReference(pair.0, pair.0 > 32767 ? 65535 : 0, pair.1)
        }
        try require(toneSamples == reference, "Scalar Threshold preserves selection and native precision")
        let displayReference = toneSamples.flatMap { value -> [UInt8] in
            let gray = UInt8((UInt32(value)+128)/257); return [gray,gray,gray,255]
        }
        try require(previewPixels == displayReference,"Scalar preview differs from committed channel")
        try require(try values(document.doc ?? before) == values(before), "Scalar Threshold never rewrites source RGB")
    }
    trace("PASS scalar UI \(depth)")
    return ["name":"scalar-ui-\(depth)", "passed":true, "coverage_samples":codes.count]
}

/// AppKit opens an untitled document at launch unless the app delegate refuses.
final class ProbeAppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }
}
let probeAppDelegate = ProbeAppDelegate()
_ = ProbeApplication.shared
NSApp.delegate = probeAppDelegate
AutomationAlerts.installIfEnabled()
_ = NSApp.setActivationPolicy(.accessory)
func runProbe() throws {
    let env = ProcessInfo.processInfo.environment
    guard let outputPath = env["DR_TONE_UI_OUTPUT"], let fixturePath = env["DR_TONE_UI_FIXTURES"] else { throw DoctoredCoreError(message: "Private probe paths missing") }
    let output = URL(fileURLWithPath: outputPath), fixtures = URL(fileURLWithPath: fixturePath)
    var groups: [[String: Any]] = [], failures: [[String: String]] = []
    for depth in [8, 16] {
        do { groups.append(try autoreleasepool { try runScalarUI(depth: depth, fixtures: fixtures, output: output) }) }
        catch { failures.append(["name":"scalar-ui-\(depth)","error":error.localizedDescription]); trace(error.localizedDescription) }
    }
    do { groups.append(contentsOf: try verifyAuxiliaryMattes(output: output)) }
    catch { failures.append(["name":"auxiliary","error":error.localizedDescription]); trace(error.localizedDescription) }
    do { groups.append(contentsOf: try verifyScalarAutoAndMask(fixtures: fixtures, output: output)) }
    catch { failures.append(["name":"scalar-auto-mask","error":error.localizedDescription]); trace(error.localizedDescription) }
    do { groups.append(contentsOf: try verifyScalarHistogram(fixtures: fixtures, output: output)) }
    catch { failures.append(["name":"scalar-histogram","error":error.localizedDescription]); trace(error.localizedDescription) }
    do { groups.append(contentsOf: try verifyNativeSelections(fixtures: fixtures, output: output)) }
    catch { failures.append(["name":"selections","error":error.localizedDescription]); trace(error.localizedDescription) }
    // Every document in this isolated process was created by this probe,
    // including outputs made by UI commands. Close and drain them explicitly.
    for document in NSDocumentController.shared.documents { document.close() }
    do { try waitUntil({ NSDocumentController.shared.documents.isEmpty },"Owned probe documents did not close") }
    catch { failures.append(["name":"owned-document-cleanup","error":error.localizedDescription]) }
    for error in (NSApp as? ProbeApplication)?.reportedErrors ?? [] {
        failures.append(["name":"unexpected-app-error","error":error])
    }
    let report: [String: Any] = ["passed":failures.isEmpty,"group_count":groups.count,"groups":groups,"failures":failures,
                               "owned_documents":NSDocumentController.shared.documents.count]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted,.sortedKeys])
        .write(to: output.appendingPathComponent("host-report.json"))
    exit(failures.isEmpty ? 0 : 1)
}
Timer.scheduledTimer(withTimeInterval: 0.05, repeats: false) { _ in
    do { try runProbe() } catch { trace(error.localizedDescription); exit(1) }
}
NSApp.run()
