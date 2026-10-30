import AppKit

final class ProbeApplication: NSApplication {
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
func runTone(depth: Int, kind: String, fixtures: URL, output: URL) throws -> [String: Any] {
    let name = "tone-ui-\(depth)-\(kind)"
    trace("START \(name)")
    let (document, editor, source) = try openDocument(depth: depth, fixtures: fixtures)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let mask = try installSelection(editor, width: source.width, height: source.height)
    let original = try values(source)
    let sheet = try openSheet(editor, kind: kind)
    let previewRenderer = try renderer(sheet)
    try waitUntil({ idle(previewRenderer) && editor.canvas.previewImage != nil }, "Initial native preview failed")
    let backingScale = sheet.view.window?.backingScaleFactor ?? 0
    try require(backingScale > 0, "No actual sheet display")
    let maximum = depth == 16 ? 65535.0 : 255.0
    let black = (depth == 16 ? 1001.0 : 3.25) / maximum
    let white = (depth == 16 ? 61003.0 : 241.5) / maximum
    let threshold = (depth == 16 ? 34123.5 : 133.25) / maximum
    if kind == "blur" {
        guard let slider = descendants(sheet.view).compactMap({ $0 as? NSSlider }).first else { throw DoctoredCoreError(message: "Blur slider missing") }
        slider.doubleValue = 1.3; try activate(slider)
    } else {
        let units = try find(sheet.view, "tone.\(kind).units", NSPopUpButton.self)
        units.selectItem(at: 1); try activate(units)
        let entries = kind == "levels" ? [("black",black*maximum),("white",white*maximum),("gamma",1.13)] : [("level",threshold*maximum)]
        for (key, value) in entries {
            let field = try find(sheet.view, "tone.\(kind).\(key)", NSTextField.self)
            field.stringValue = String(value); try activate(field)
        }
        units.selectItem(at: 0); try activate(units)
        let fine = try find(sheet.view, "tone.\(kind).\(kind == "levels" ? "black" : "level")", NSTextField.self)
        try require(abs(fine.doubleValue - (kind == "levels" ? black : threshold)) < 1e-14, "Units changed stored fine value")
        sheet.view.layoutSubtreeIfNeeded()
        try require(fine.bounds.width >= 185, "Fine field clips ordinary normalized precision")
        if kind == "levels" {
            for (key, value) in [("blackLabel",black),("whiteLabel",white),("gammaLabel",1.13)] {
                guard let label = member(sheet,key) as? NSTextField else { throw DoctoredCoreError(message:"Legacy Levels readout missing") }
                try require(label.stringValue == String(format:"%.2f",value), "Fine entry left stale \(key): \(label.stringValue)")
            }
        } else {
            guard let labels = member(sheet,"valueLabels") as? [NSTextField], let label = labels.first else { throw DoctoredCoreError(message:"Threshold readout missing") }
            try require(label.stringValue == String(format:"%.2f",threshold), "Fine entry left stale Threshold readout")
        }
    }
    try waitUntil({ idle(previewRenderer) && editor.canvas.previewImage != nil }, "Fine native preview failed")
    guard let preview = editor.canvas.previewImage else { throw DoctoredCoreError(message: "Final preview missing") }
    let previewBytes = try displayed(preview)
    try require(document.doc === source, "Preview published a document edit")
    try screenshot(sheet.view, output.appendingPathComponent(name+"-sheet.png"))
    ActionRecorder.shared.start()
    try button(sheet.view, "Apply").performClick(nil)
    try waitUntil({ editor.view.window?.attachedSheet == nil && !BackgroundWork.occupied && DocumentJobs.active == nil }, "Apply did not finish and detach")
    pump()
    let records = ActionRecorder.shared.stop()
    guard let edited = document.doc, let projection = document.previewImage else { throw DoctoredCoreError(message: "Applied result missing") }
    let expected = try values(edited)
    try require(edited !== source && expected != original, "Selected edit changed nothing")
    try require(try displayed(projection) == previewBytes, "Preview differs from committed display")
    try require(records.count == 1 && records[0].tool == "apply_filter", "One canonical filter Action")
    for i in mask.indices {
        let p = i*4
        if mask[i] == 0 { try require(Array(expected[p..<p+4]) == Array(original[p..<p+4]), "Outside selection changed") }
        if kind != "blur" {
            try require(expected[p+3] == original[p+3], "Tone changed alpha")
            let rgb = (0..<3).map { Double(original[p+$0])/65535 }
            let transformed: [Double]
            if kind == "levels" { transformed = rgb.map { pow(min(1,max(0,($0-black)/(white-black))),1/1.13) } }
            else { transformed = Array(repeating: zip(rgb,[0.2126,0.7152,0.0722]).reduce(0) { $0+$1.0*$1.1 } >= threshold ? 1 : 0,count:3) }
            for c in 0..<3 {
                let full = depth == 8 ? (transformed[c]*255).rounded()/255 : transformed[c]
                let t = Double(mask[i])/255
                let reference = ((rgb[c]*(1-t)+full*t)*maximum).rounded() * (depth == 8 ? 257 : 1)
                try require(abs(Double(expected[p+c])-reference) <= (depth == 8 ? 257 : 1), "Independent fine tone reference")
            }
        }
    }
    document.undoManager?.undo(); pump()
    try require(document.doc === source && !document.isDocumentEdited, "One Undo restores saved source")
    document.undoManager?.redo(); pump()
    try require(document.doc === edited, "Redo restores exact owner")
    document.undoManager?.undo(); pump()
    var arguments = records[0].arguments
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let data = try JSONSerialization.data(withJSONObject: arguments, options: .sortedKeys)
    let response = AgentServer.shared.execute(tool: "apply_filter", argumentsJSON: String(decoding: data, as: UTF8.self))
    try waitUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, "Action worker failed to finish")
    try require(try values(document.doc ?? source) == expected, "UI Action replay differs: \(response)")
    document.undoManager?.undo(); pump()
    let cancelled = try openSheet(editor, kind: kind)
    try button(cancelled.view, "Cancel").performClick(nil)
    try waitUntil({ editor.view.window?.attachedSheet == nil }, "Cancel did not detach")
    try require(document.doc === source && editor.canvas.previewImage == nil, "Cancel retained or published preview")
    trace("PASS \(name)")
    return ["name":name,"passed":true,"native_components":expected.count,"backing_scale":backingScale,"action":records[0].arguments]
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
    var groups: [[String:Any]] = [], failures: [[String:String]] = []
    for depth in [8,16] { for kind in ["levels","threshold","blur"] {
        do { groups.append(try autoreleasepool { try runTone(depth:depth,kind:kind,fixtures:fixtures,output:output) }) }
        catch { failures.append(["name":"\(depth)-\(kind)","error":error.localizedDescription]); trace(error.localizedDescription) }
    }}
    for depth in [8,16] {
        do { groups.append(try autoreleasepool { try runInfo(depth:depth,fixtures:fixtures,output:output) }) }
        catch { failures.append(["name":"info-\(depth)","error":error.localizedDescription]); trace(error.localizedDescription) }
        do { groups.append(try autoreleasepool { try runCurves(depth:depth,fixtures:fixtures,output:output) }) }
        catch { failures.append(["name":"curves-\(depth)","error":error.localizedDescription]); trace(error.localizedDescription) }
        do { groups.append(try autoreleasepool { try runAuto(depth:depth,fixtures:fixtures,output:output) }) }
        catch { failures.append(["name":"auto-\(depth)","error":error.localizedDescription]); trace(error.localizedDescription) }
        do { groups.append(try autoreleasepool { try runCurvesDestructive(depth:depth,fixtures:fixtures,output:output) }) }
        catch { failures.append(["name":"curves-destructive-\(depth)","error":error.localizedDescription]); trace(error.localizedDescription) }
        do { groups.append(try autoreleasepool { try runPhotoshopPreset(depth:depth,fixtures:fixtures,output:output) }) }
        catch { failures.append(["name":"photoshop-preset-\(depth)","error":error.localizedDescription]); trace(error.localizedDescription) }
    }
    let report: [String:Any] = ["passed":failures.isEmpty,"group_count":groups.count,"groups":groups,"failures":failures,
                               "owned_documents":NSDocumentController.shared.documents.count]
    try JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys]).write(to:output.appendingPathComponent("host-report.json"))
    exit(failures.isEmpty ? 0 : 1)
}
Timer.scheduledTimer(withTimeInterval:0.05,repeats:false) { _ in
    do { try runProbe() } catch { trace(error.localizedDescription); exit(1) }
}
NSApp.run()
