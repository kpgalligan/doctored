import AppKit

final class ProbeApplication: NSApplication {
    override func reportException(_ exception: NSException) {
        let message = "OBJECTIVE-C EXCEPTION: \(exception.name.rawValue): \(exception.reason ?? "unknown")\n"
            + exception.callStackSymbols.joined(separator: "\n")
        print(message); fflush(stdout)
        if let output = ProcessInfo.processInfo.environment["DR_CAF_UI_OUTPUT"] {
            try? JSONSerialization.data(withJSONObject: ["passed": false, "exception": message], options: .prettyPrinted)
                .write(to: URL(fileURLWithPath: output).appendingPathComponent("host-report.json"))
        }
        exit(2)
    }
}

func trace(_ message: String) { print(message); fflush(stdout) }

func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw DoctoredCoreError(message: message) }
}
func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
func find<T: NSView>(_ root: NSView, _ id: String, _: T.Type) throws -> T {
    guard let value = descendants(root).first(where: { $0.identifier?.rawValue == id }) as? T else {
        throw DoctoredCoreError(message: "Missing actual sheet control \(id)")
    }
    return value
}
func activate(_ control: NSControl) throws {
    guard let action = control.action else { throw DoctoredCoreError(message: "Control has no action") }
    control.sendAction(action, to: control.target)
}
func event(_ view: NSView, _ kind: NSEvent.EventType, _ point: CGPoint) throws {
    guard let window = view.window,
          let event = NSEvent.mouseEvent(with: kind, location: view.convert(point, to: nil), modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 0, clickCount: 1, pressure: 1) else {
        throw DoctoredCoreError(message: "Could not construct an owned mouse event")
    }
    switch kind {
    case .leftMouseDown: view.mouseDown(with: event)
    case .leftMouseDragged: view.mouseDragged(with: event)
    case .leftMouseUp: view.mouseUp(with: event)
    default: break
    }
}
func waitUntil(_ condition: () -> Bool, _ message: String) throws {
    trace("WAIT \(message)")
    let deadline = Date().addingTimeInterval(60)
    while !condition(), Date() < deadline {
        // A command-line AppKit probe must service the application event
        // queue as well as run-loop timers, including sheet appearance and
        // dismissal. This is the same seam as the selection UI qualification.
        var count = 0
        while count < 64, let event = NSApp.nextEvent(matching: .any, until: Date(),
            inMode: .default, dequeue: true) {
            NSApp.sendEvent(event); count += 1
        }
        NSApp.updateWindows()
        RunLoop.current.run(until: Date().addingTimeInterval(0.01))
    }
    try require(condition(), message)
    trace("DONE \(message)")
}
func review(_ sheet: ContentAwareFillSheetController) throws -> ContentAwareFillReview {
    // Inspection of the real private owner avoids any product-only test API.
    guard let slot = Mirror(reflecting: sheet).children.first(where: { $0.label == "review" })?.value,
          let owner = Mirror(reflecting: slot).children.first?.value as? ContentAwareFillReview else {
        throw DoctoredCoreError(message: "Sheet's captured review owner is missing")
    }
    return owner
}
func waitReady(_ owner: ContentAwareFillReview) throws {
    trace("REVIEW before wait: \(owner.state): \(owner.message), worker=\(BackgroundWork.occupied)")
    try waitUntil({ ["ready", "failed", "cancelled"].contains(owner.state) }, "Exact sheet candidate did not finish")
    try require(owner.state == "ready", "Candidate failed: \(owner.message)")
}
func values(_ doc: RasterDocument) throws -> [UInt16] {
    if let source = doc.precision {
        let budget = try PrecisionWorkContext(source: source)
        return try source.render(workingLimit: budget.limit, otherRetained: budget.otherRetained)
            .withRGBA16 { Array($0) }
    }
    return try doc.render().withPlateBytes { pointer in
        Array(UnsafeBufferPointer(start: pointer, count: doc.width * doc.height * 4)).map { UInt16($0)*257 }
    } ?? []
}
func screenshot(_ view: NSView, _ path: URL) throws {
    view.layoutSubtreeIfNeeded()
    guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
        throw DoctoredCoreError(message: "Owned sheet screenshot unavailable")
    }
    view.cacheDisplay(in: view.bounds, to: bitmap)
    guard let bytes = bitmap.representation(using: .png, properties: [:]) else {
        throw DoctoredCoreError(message: "Owned sheet PNG unavailable")
    }
    try bytes.write(to: path)
}
func openSheet(_ editor: EditorViewController) throws -> ContentAwareFillSheetController {
    try waitUntil({ editor.view.window?.attachedSheet == nil }, "Previous physical sheet did not detach")
    trace("OPEN sheet; editor window=\(String(describing: editor.view.window)), prior=\(String(describing: editor.presentedViewControllers))")
    editor.contentAwareFill(nil)
    trace("PRESENTED \(String(describing: editor.presentedViewControllers))")
    try waitUntil({ editor.presentedViewControllers?.contains(where: { $0 is ContentAwareFillSheetController }) == true },
                  "Content-aware sheet not presented")
    guard let sheet = editor.presentedViewControllers?.first(where: { $0 is ContentAwareFillSheetController })
        as? ContentAwareFillSheetController else { throw DoctoredCoreError(message: "No fill sheet") }
    _ = sheet.view
    try waitUntil({ sheet.view.window?.sheetParent != nil }, "Content-aware sheet did not attach")
    return sheet
}
func runCase(depth: Int, newLayer: Bool, fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "caf-ui-\(depth)-\(newLayer ? "new" : "layer")"
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
        throw DoctoredCoreError(message: "Actual editor missing")
    }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    guard let selection = CanvasSelection(shape: .rect(CGRect(x: 27, y: 19, width: 7, height: 7)),
        canvasWidth: 64, canvasHeight: 48) else { throw DoctoredCoreError(message: "Fixture selection missing") }
    editor.agentSetSelection(selection)
    ToolOptionsStore.shared.contentAwareFill = ContentAwareFillOptions(ring: 0, sample: .current,
        ignoreAdjustments: false, seed: 19)
    let original = try values(source)
    let sheet = try openSheet(editor), owner = try review(sheet)
    try waitReady(owner)
    let preview = try find(sheet.view, "caf.source", ContentAwareFillCanvasView.self)
    let backingScale = preview.window?.backingScaleFactor ?? 0
    try require(backingScale > 0, "The actual comparison display has no backing scale")
    let apply = try find(sheet.view, "caf.apply", NSButton.self)
    let prepare = try find(sheet.view, "caf.prepare", NSButton.self)
    let mode = try find(sheet.view, "caf.sourceMode", NSPopUpButton.self)
    let outputPopup = try find(sheet.view, "caf.output", NSPopUpButton.self)
    outputPopup.selectItem(at: newLayer ? 1 : 0); try activate(outputPopup)
    mode.selectItem(at: 4); try activate(mode)
    sheet.view.layoutSubtreeIfNeeded()
    let scale = min(preview.bounds.width/64, preview.bounds.height/48)
    let origin = CGPoint(x: preview.bounds.midX-32*scale, y: preview.bounds.midY-24*scale)
    func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
        CGPoint(x: origin.x+x*scale, y: origin.y+y*scale)
    }
    try event(preview, .leftMouseDown, point(43,4))
    try event(preview, .leftMouseDragged, point(55,42))
    try event(preview, .leftMouseUp, point(55,42))
    try require(owner.candidate == nil && !apply.isEnabled, "Source edit did not invalidate candidate")
    try require(document.doc === source, "Source editing changed document")
    try require(preview.sourceMask.operations.count == 1, "Real rectangle gesture missing")
    try screenshot(sheet.view, output.appendingPathComponent(name+"-source.png"))
    prepare.performClick(nil); try waitReady(owner)
    let first = try owner.requireCandidate()
    let compare = try find(sheet.view, "caf.compare", NSSegmentedControl.self)
    compare.selectedSegment = 1; try activate(compare)
    try require(preview.image === first.before, "Before displays a different source")
    compare.selectedSegment = 2; try activate(compare)
    try require(preview.image === first.prepared.preview, "After is not the exact prepared projection")
    let zoom = try find(sheet.view, "caf.zoom", NSSegmentedControl.self)
    zoom.selectedSegment = 1; try activate(zoom)
    try require(preview.nativeDetail, "100% view was not selected")
    mode.selectItem(at: 0); try activate(mode)
    let middle = CGPoint(x: preview.bounds.midX, y: preview.bounds.midY)
    try event(preview, .leftMouseDown, middle)
    try event(preview, .leftMouseDragged, CGPoint(x: middle.x+8,y: middle.y+8))
    try event(preview, .leftMouseUp, CGPoint(x: middle.x+8,y: middle.y+8))
    try require(owner.candidate?.prepared.document === first.prepared.document, "Comparison/pan recomputed synthesis")
    try screenshot(sheet.view, output.appendingPathComponent(name+"-detail.png"))
    zoom.selectedSegment = 0; try activate(zoom)
    let seed = try find(sheet.view, "caf.seed", NSTextField.self)
    seed.stringValue = "20"; try activate(seed)
    try require(owner.candidate == nil && !apply.isEnabled, "Parameter edit kept an old candidate")
    prepare.performClick(nil); try waitReady(owner)
    let final = try owner.requireCandidate(), expected = try values(final.prepared.document)
    try require(expected != original, "Prepared fill changes nothing")
    ActionRecorder.shared.start()
    apply.performClick(nil)
    try waitUntil({ editor.presentedViewControllers?.isEmpty != false }, "Applied sheet did not close")
    let records = ActionRecorder.shared.stop()
    try require(document.doc === final.prepared.document, "Apply did not publish the exact reviewed owner")
    try require(document.doc?.layerCount == source.layerCount+(newLayer ? 1 : 0), "Output destination mismatch")
    try require(records.count == 1 && records[0].tool == "content_aware_fill", "Apply recorded more than one command")
    try require(records[0].arguments["preview_id"] == nil && records[0].arguments["source_mask"] != nil,
                "Action did not store canonical source geometry")
    document.undoManager?.undo()
    try require(document.doc === source && !document.isDocumentEdited, "One Undo failed to restore source")
    document.undoManager?.redo()
    try require(document.doc === final.prepared.document, "Redo lost prepared owner")
    document.undoManager?.undo()
    var replay = records[0].arguments
    replay["document_id"] = AgentServer.shared.documentID(for: document)
    let record = try JSONSerialization.data(withJSONObject: replay, options: .sortedKeys)
    let reply = AgentServer.shared.execute(tool: records[0].tool, argumentsJSON: String(decoding: record, as: UTF8.self))
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, "Action worker did not finish")
    try require(try values(document.doc ?? source) == expected, "Actual UI Action replay changed native result: \(reply)")
    document.undoManager?.undo()
    let cancelledSheet = try openSheet(editor), cancelled = try review(cancelledSheet)
    try find(cancelledSheet.view, "caf.cancel", NSButton.self).performClick(nil)
    try waitUntil({ !BackgroundWork.occupied }, "Cancelled sheet left worker occupied")
    try require(cancelled.state == "cancelled" && document.doc === source, "Sheet cancellation published a result")
    try require(try values(source) == original, "Original source mutated")
    let item = NSMenuItem(title: "Content-Aware Fill", action: #selector(EditorViewController.contentAwareFill(_:)), keyEquivalent: "")
    editor.setPaintTarget(.plane(.red))
    try require(document.planeEditTarget == .plane(.red), "UI scalar target did not reach the editing state")
    try require(!editor.validateUserInterfaceItem(item), "Scalar target enabled content-aware fill")
    editor.setPaintTarget(.layer)
    let closingSheet = try openSheet(editor), closingOwner = try review(closingSheet)
    let closingPreview = try find(closingSheet.view, "caf.source", ContentAwareFillCanvasView.self)
    document.close()
    try waitUntil({ !BackgroundWork.occupied }, "Owner-close worker did not drain")
    try require(closingOwner.state == "cancelled" && closingPreview.image == nil,
                "Owner close kept the review or its displayed pixels")
    print("PASS \(name)"); fflush(stdout)
    return ["name":name,"passed":true,"native_components":expected.count,
        "same_candidate_owner":true,"owner_close_released":true,"source_operations":preview.sourceMask.operations,
        "backing_scale":backingScale,"action":records[0].arguments]
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
    guard let outputPath = env["DR_CAF_UI_OUTPUT"], let fixturesPath = env["DR_CAF_UI_FIXTURES"] else {
        throw DoctoredCoreError(message: "Private probe paths missing")
    }
    let output = URL(fileURLWithPath: outputPath), fixtures = URL(fileURLWithPath: fixturesPath)
    var results: [[String: Any]] = [], failures: [[String: String]] = []
    for depth in [8,16] { for newLayer in [false,true] {
        do { results.append(try autoreleasepool { try runCase(depth: depth, newLayer: newLayer, fixtures: fixtures, output: output) }) }
        catch { failures.append(["name":"\(depth)-\(newLayer)","error":error.localizedDescription]); print(error.localizedDescription) }
    }}
    for depth in [8,16] { for destination in [false,true] { for ellipse in [false,true] {
        do { results.append(try autoreleasepool { try runPatchCase(depth: depth, destination: destination,
            ellipse: ellipse, fixtures: fixtures, output: output) }) }
        catch { failures.append(["name":"patch-\(depth)-\(destination)-\(ellipse)","error":error.localizedDescription]); print(error.localizedDescription) }
    }}}
    let report: [String: Any] = ["passed":failures.isEmpty,"group_count":results.count,"groups":results,
        "failures":failures,"owned_documents":NSDocumentController.shared.documents.count]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted,.sortedKeys])
        .write(to: output.appendingPathComponent("host-report.json"))
    exit(failures.isEmpty ? 0 : 1)
}
Timer.scheduledTimer(withTimeInterval: 0.05, repeats: false) { _ in
    do { try runProbe() } catch { print(error.localizedDescription); exit(1) }
}
NSApp.run()
