import AppKit
import Darwin

struct ProbeFailure: Error, CustomStringConvertible { let description: String }
func require(_ value: @autoclosure () -> Bool, _ message: String) throws {
    if !value() { throw ProbeFailure(description: message) }
}
func fixture(width: Int = 90, height: Int = 60) throws -> ImageDocument {
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    for y in 0..<height { for x in 0..<width {
        let i = (y * width + x) * 4
        pixels[i + min(2, x * 3 / width)] = 255
        pixels[i + 3] = y < height - 5 ? 255 : 0
    } }
    guard let image = RasterImage.from(rgba: pixels, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Selection fixture allocation")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    document.windowControllers.first?.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    document.windowControllers.first?.window?.orderFront(nil)
    document.undoManager?.removeAllActions()
    return document
}
func editor(_ document: ImageDocument) throws -> EditorViewController {
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw ProbeFailure(description: "Missing editor")
    }
    return editor
}
func allViews(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(allViews) }
func pumpUntil(_ condition: () -> Bool, seconds: TimeInterval = 5, message: String = "condition",
               file: StaticString = #fileID, line: UInt = #line) throws {
    let deadline = Date().addingTimeInterval(seconds)
    while !condition(), Date() < deadline {
        // NSApplication normally drains window/animation events around the run
        // loop. A command-line AppKit probe must service that event queue too.
        var dispatched = 0
        while dispatched < 64, let event = NSApp.nextEvent(matching: .any, until: Date(),
            inMode: .default, dequeue: true) {
            NSApp.sendEvent(event)
            dispatched += 1
        }
        NSApp.updateWindows()
        RunLoop.current.run(until: Date().addingTimeInterval(0.01))
    }
    try require(condition(), "Timed out at \(file):\(line) waiting for \(message); scratch=\(SelectionWorkBudget.scratchBytes), occupied=\(BackgroundWork.occupied)")
}
func call(_ document: ImageDocument, _ name: String, _ values: [String: Any] = [:]) throws -> [String: Any] {
    var arguments = values
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let data = try JSONSerialization.data(withJSONObject: arguments)
    let raw = AgentServer.shared.execute(tool: name, argumentsJSON: String(decoding: data, as: UTF8.self))
    guard let result = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Invalid tool result: \(raw)")
    }
    guard result["isError"] as? Bool != true,
          let content = result["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String,
          let payload = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Tool refused or returned no JSON payload: \(raw)")
    }
    return payload
}
func present(_ sheet: NSViewController, in controller: EditorViewController) throws {
    if let previous = controller.presentedViewControllers, !previous.isEmpty {
        let windows = previous.map { item in
            "\(type(of: item)): visible=\(item.view.window?.isVisible ?? false), attached=\(item.view.window?.sheetParent != nil)"
        }.joined(separator: "; ")
        print("SHEET state before \(type(of: sheet)): \(windows)")
        fflush(stdout)
    }
    try pumpUntil({ controller.view.window?.attachedSheet == nil },
                  message: "physical sheet to detach before \(type(of: sheet))")
    controller.presentAsSheet(sheet)
    try pumpUntil({ sheet.view.window != nil }, message: "\(type(of: sheet)) presentation")
    sheet.view.window?.animationBehavior = .none
}
func event(_ kind: NSEvent.EventType, _ point: CGPoint, canvas: ImageCanvasView,
           flags: NSEvent.ModifierFlags = []) throws -> NSEvent {
    guard let window = canvas.window,
          let event = NSEvent.mouseEvent(with: kind, location: canvas.convert(point, to: nil),
              modifierFlags: flags, timestamp: 0, windowNumber: window.windowNumber,
              context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "Could not build canvas event")
    }
    return event
}
func verifyFreehand() throws {
    let document = try fixture()
    defer { document.close() }
    let controller = try editor(document), canvas = try editor(document).canvas
    let native = document.doc
    controller.selectFreehandLassoTool(nil)
    canvas.mouseDown(with: try event(.leftMouseDown, CGPoint(x: 5, y: 5), canvas: canvas))
    canvas.mouseDragged(with: try event(.leftMouseDragged, CGPoint(x: 24, y: 5), canvas: canvas))
    canvas.mouseDragged(with: try event(.leftMouseDragged, CGPoint(x: 24, y: 25), canvas: canvas))
    try require(document.canvasSelection == nil && document.undoManager?.canUndo != true,
                "Freehand outline remains transient until release")
    canvas.mouseUp(with: try event(.leftMouseUp, CGPoint(x: 5, y: 25), canvas: canvas))
    try require(document.canvasSelection?.maskBytes()[15 * 90 + 15] == 255,
                "Actual freehand canvas mouse gesture closes and selects interior")
    try require(document.doc === native && !document.isDocumentEdited && document.history.entries.count == 2,
                "Freehand selects in one clean history step without changing native pixels")
    let selected = document.selectionHistory
    document.undoManager?.undo()
    try require(document.canvasSelection == nil && document.undoManager?.canUndo != true, "Single Undo reverses lasso")
    document.undoManager?.redo()
    try require(document.selectionHistory == selected, "Freehand Redo restores full selection state")
    canvas.mouseDown(with: try event(.leftMouseDown, CGPoint(x: 40, y: 10), canvas: canvas))
    canvas.mouseDragged(with: try event(.leftMouseDragged, CGPoint(x: 50, y: 25), canvas: canvas))
    _ = canvas.cancelSelectionStroke()
    try require(document.selectionHistory == selected, "Cancelling a new outline preserves existing selection")
    print("PASS: actual Freehand Lasso mouse wiring, release-only commit, one Undo/Redo and clean native state")
}
func verifyColorRangeSheet() throws {
    let document = try fixture()
    defer { document.close() }
    let controller = try editor(document)
    let sheet = try ColorRangeSheetController(editor: controller, document: document)
    try present(sheet, in: controller)
    sheet.sample(CGPoint(x: 10, y: 10))
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try require(document.canvasSelection == nil, "Color Range preview leaves selection/history untouched")
    let buttons = allViews(sheet.view).compactMap { $0 as? NSButton }
    guard let apply = buttons.first(where: { $0.title == "Apply" }) else {
        throw ProbeFailure(description: "Color Range Apply missing")
    }
    apply.performClick(nil)
    let selected = document.canvasSelection?.maskBytes()
    try require(selected?[10 * 90 + 10] == 255 && selected?[10 * 90 + 45] == 0
                && selected?[57 * 90 + 10] == 0, "Sampled red chooses its island and excludes other colors/transparency")
    try require(!document.isDocumentEdited && document.history.entries.count == 2, "Color Range Apply is one clean selection Undo")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "Color Range Undo restores prior empty selection")
    let tonal = try call(document, "select_color_range", ["range": "highlights", "softness": 0])
    try require(tonal["ok"] as? Bool == true && document.canvasSelection?.maskBytes()[10 * 90 + 45] == 255
                && document.canvasSelection?.maskBytes()[10 * 90 + 75] == 0, "Tonal highlight selection uses perceptual lightness")
    print("PASS: Color Range captured-image sampling, actual Apply, tonal MCP parity and transparent exclusion")
}
func verifySelectionTransformSheet() throws {
    let document = try fixture()
    defer { document.close() }
    let controller = try editor(document), native = document.doc
    document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 5, y: 6, width: 12, height: 10)),
        canvasWidth: 90, canvasHeight: 60))
    let baseline = document.selectionHistory
    document.undoManager?.removeAllActions()
    let sheet = try SelectionTransformSheetController(editor: controller, document: document)
    try present(sheet, in: controller)
    guard let field = allViews(sheet.view).compactMap({ $0 as? NSTextField }).first(where: {
        $0.identifier?.rawValue == "selectionTransform.translate_x"
    }) else { throw ProbeFailure(description: "Selection transform translation field") }
    field.stringValue = "20"
    _ = field.sendAction(field.action, to: field.target)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
    try require(document.selectionHistory == baseline, "Transform preview does not mutate coverage")
    guard let apply = allViews(sheet.view).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Apply" }) else {
        throw ProbeFailure(description: "Transform Apply")
    }
    apply.performClick(nil)
    try require(document.canvasSelection?.bounds == CGRect(x: 25, y: 6, width: 12, height: 10),
                "Actual selection transform Apply moves coverage by numeric translation")
    try require(document.doc === native && !document.isDocumentEdited, "Selection transform leaves layer pixels and dirty state intact")
    document.undoManager?.undo()
    try require(document.selectionHistory == baseline && document.undoManager?.canUndo != true,
                "Selection transform has one exact Undo")
    let before = document.history.entries.count
    _ = try call(document, "transform_selection")
    try require(document.history.entries.count == before && document.selectionHistory == baseline,
                "Identity transform preserves geometry and does not add a history step")
    print("PASS: actual selection transform sheet preview/Apply, independent pixels, exact Undo and identity no-op")
}
func verifyQuickAndCapture() throws {
    let document = try fixture()
    defer { document.close() }
    let controller = try editor(document), canvas = try editor(document).canvas
    // The tool's own settings, stated: a 30 px brush, no Enhance Edge. The tool always passes
    // smooth 1, and the release 2.8 px from the press lies inside the first dab (closer than half
    // the brush radius), so the stroke is the one seed.
    QuickSelectionSettings.shared.size = 30
    QuickSelectionSettings.shared.enhanceEdge = false
    controller.selectQuickSelectTool(nil)
    canvas.mouseDown(with: try event(.leftMouseDown, CGPoint(x: 10, y: 10), canvas: canvas))
    canvas.mouseUp(with: try event(.leftMouseUp, CGPoint(x: 12, y: 12), canvas: canvas))
    try require(document.canvasSelection?.maskBytes()[10 * 90 + 10] == 255
                && document.canvasSelection?.maskBytes()[10 * 90 + 45] == 0,
                "Actual Quick Selection seeds grow only the connected red region")
    let red = document.canvasSelection?.maskBytes()
    document.undoManager?.undo()
    let result = try call(document, "quick_select",
        ["seeds": [[10, 10]], "tolerance": 25, "edge": 12, "brush_size": 30, "smooth": 1])
    try require(result["ok"] as? Bool == true && document.canvasSelection?.maskBytes() == red,
                "Quick Selection MCP reproduces canvas seed coverage exactly")
    // A long drag with a large brush thins to half the brush radius: 4,000 points 3 px apart at
    // Size 300 (radius 149.5) keep one point every 75 px or so, far below the 4,096-seed limit.
    QuickSelectionSettings.shared.size = 300
    var long = SelectionStrokeSession(capture: try SelectionCapture(document: document), tool: .quickSelect,
                                      mode: .add, feather: 0, point: .zero)
    for index in 1..<4000 { long.append(CGPoint(x: Double(index) * 3, y: 0), minimumDistance: 0.5) }
    QuickSelectionSettings.shared.size = 30
    try require(long.points.count <= 170 && !long.overflowed && long.radius == 149.5,
                "A long large-brush Quick Selection drag thins its seeds: kept \(long.points.count)")
    let capture = try SelectionCapture(document: document)
    document.setCanvasSelection(nil)
    try require(!capture.isCurrent(in: document), "Selection change makes capture stale without a native pixel revision")
    do {
        try capture.commit(.mask([UInt8](repeating: 255, count: 90 * 60)), in: document, name: "Stale")
        throw ProbeFailure(description: "Stale selection capture committed")
    } catch is DoctoredCoreError {}
    document.beginLiveEdit()
    try require((try? SelectionCapture(document: document)) == nil, "Capture refuses an active live edit before its first revision")
    document.endLiveEdit("No-op", record: .notACommand)
    let workerCapture = try SelectionCapture(document: document)
    let timer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: false) { _ in
        document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 1, y: 1, width: 4, height: 4)),
            canvasWidth: 90, canvasHeight: 60))
    }
    defer { timer.invalidate() }
    RunLoop.main.add(timer, forMode: .modalPanel)
    do {
        _ = try workerCapture.run(in: document, title: "Captured selection test", scratchBytes: 100_000) { _ in
            Thread.sleep(forTimeInterval: 0.1)
            return [UInt8](repeating: 255, count: 90 * 60)
        }
        throw ProbeFailure(description: "Worker result applied across a changed selection")
    } catch is DoctoredCoreError {}
    try require(document.canvasSelection?.bounds.width == 4 && !BackgroundWork.occupied,
                "Worker pumps the run loop and stale completion preserves newer selection")
    print("PASS: Quick Selection UI/MCP equivalence, seed thinning, capture/live-edit refusal and stale worker result rejection")
}
func verifyPreviewLifetime() throws {
    var renderer: PreviewRenderer? = PreviewRenderer()
    let reservation = try SelectionWorkBudget.reservePreview(bytes: 100_000, outputBytes: 1000)
    renderer?.request { reservation.run { _ in nil } }
    renderer?.cancel()
    renderer = nil
    try require(SelectionWorkBudget.scratchBytes >= 100_000, "Live reservation remains accounted while directly owned")
    // The scoped helper below proves canceled queued closures also release without ever computing.
    func queued() throws {
        let renderer = PreviewRenderer()
        let lease = try SelectionWorkBudget.reservePreview(bytes: 120_000, outputBytes: 1000)
        renderer.request { lease.run { _ in nil } }
        renderer.cancel()
    }
    try queued()
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 100_000 })
    withExtendedLifetime(reservation) {}
    print("PASS: canceled queued preview reservations release through closure lifetime without blocking")
}

_ = NSApplication.shared
AutomationAlerts.installIfEnabled()
var failures = 0
let checks: [(String, () throws -> Void)] = [
    ("native layer blending", verifyNativeLayerBlending),
    ("mask outside coverage", verifyMaskOutsideCoverage),
    ("cutout worker factory", verifyCutoutWorkerFactory),
    ("byte cutout projection cancellation", verifyByteCutoutProjectionCancellation),
    ("cutout preparation cancellation", verifyCutoutPreparationCancellation),
    ("native refinement cutout output", verifyNativeRefinementCutout),
    ("refinement cutout output", verifyRefinementCutout),
    ("trimap model", verifyTrimapModel), ("advanced refinement gate", verifyAdvancedRefinementGate),
    ("freehand", verifyFreehand), ("color range", verifyColorRangeSheet),
    ("selection transform", verifySelectionTransformSheet), ("quick/capture", verifyQuickAndCapture),
    ("preview lifetime", verifyPreviewLifetime),
    ("subject targeting", verifySubjectTargets),
    ("float subject targeting", verifyFloatSubjectTargets),
    ("face parts", verifyFaceParts),
    ("people instances", verifyPeopleInstances),
    ("selection Actions", verifySelectionActions),
    ("construction bounds", verifySelectionConstructionBounds),
    ("independent masks", verifyIndependentMasks),
    ("mask property precision", verifyMaskPropertiesPrecision),
    ("canvas mask transform preview", verifyMaskTransformPreview),
    ("refinement workspace", verifyRefinementWorkspace),
    ("refinement mask output", verifyRefinementMaskOutput),
    ("liquify workspace", verifyLiquifyWorkspace),
]
let requested = Set((ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_CHECKS"] ?? "")
    .split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) })
let selectedChecks = checks.filter { requested.isEmpty || requested.contains($0.0) }
if !requested.isEmpty, selectedChecks.count != requested.count {
    fputs("Unknown probe check name\n", stderr)
    exit(1)
}
for (name, check) in selectedChecks {
    print("RUN: \(name)")
    fflush(stdout)
    do { try check() }
    catch { fputs("FAIL [\(name)]: \(error)\n", stderr); failures += 1 }
    fflush(stdout)
}
exit(failures == 0 ? 0 : 1)
