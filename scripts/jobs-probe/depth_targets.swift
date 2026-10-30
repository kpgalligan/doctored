import AppKit

// Batch 70: Quick Mask Fill in a 32-bit float document, and a missing
// `target` is the layer at the depth and retained-source gates. Private
// helpers only: an MCP call through `AgentServer.execute`, a float opener
// that never activates the app, and a PNG writer.

private struct DepthReply {
    let isError: Bool
    let text: String
    let result: [String: Any]
}

private func depthCall(_ tool: String, _ document: ImageDocument,
                       _ arguments: [String: Any] = [:]) throws -> DepthReply {
    var arguments = arguments
    arguments["document_id"] = AgentServer.shared.documentID(for: document)
    let encoded = try JSONSerialization.data(withJSONObject: arguments, options: .sortedKeys)
    let raw = AgentServer.shared.execute(tool: tool,
                                         argumentsJSON: String(decoding: encoded, as: UTF8.self))
    guard let reply = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
          let content = reply["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String
    else {
        throw ProbeFailure(description: "Malformed MCP response to \(tool): \(raw)")
    }
    let isError = reply["isError"] as? Bool == true
    let result = isError ? [:]
        : (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
    return DepthReply(isError: isError, text: text, result: result)
}

private func depthOK(_ tool: String, _ document: ImageDocument,
                     _ arguments: [String: Any] = [:]) throws -> [String: Any] {
    let reply = try depthCall(tool, document, arguments)
    try require(!reply.isError, "\(tool) refused: \(reply.text)")
    return reply.result
}

/// The committed 11F fixture, opened through the app's preserving path and
/// installed the way `precisionFixture` installs a 16-bit document: the
/// window is ordered in off-screen and the app is never activated.
private func openFloatFixture() throws -> ImageDocument {
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    let url = root.appendingPathComponent("core/tests/fixtures/float_11f/scene.tiff")
    try require(FileManager.default.fileExists(atPath: url.path),
                "Float fixture missing at \(url.path)")
    let context = try PrecisionWorkContext()
    let precision = try RasterPrecisionDocument.openPreserving(
        url: url, workingLimit: context.limit, otherRetained: context.otherRetained)
    try require(precision.isFloat, "The float fixture did not open as a float document")
    let source = try RasterDocument(precision: precision)
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
    document.undoManager?.groupsByEvent = false
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    try require(document.doc?.isFloat == true, "Owned fixture is not a 32-bit float document")
    return document
}

private func validates(_ controller: EditorViewController, _ action: Selector) -> Bool {
    controller.validateUserInterfaceItem(NSMenuItem(title: "", action: action, keyEquivalent: ""))
}

/// The Quick Mask coverage, all zero when there is no selection.
private func quickMaskCoverage(_ document: ImageDocument, count: Int) -> [UInt16] {
    document.canvasSelection?.maskSamples16() ?? [UInt16](repeating: 0, count: count)
}

/// In a 32-bit float document Fill… and its five chords validate in
/// Quick Mask and fill the native u16 coverage as one file-clean "Fill"
/// step, recorded as `fill_selection` with `target: "quick_mask"`; MCP
/// `fill_selection` fills it too; outside Quick Mask both stay refused.
func verifyFloatQuickMaskFill() throws {
    let document = try openFloatFixture()
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let source = document.doc else { throw ProbeFailure(description: "Float source") }
    let count = source.width * source.height
    let original = [UInt16](repeating: 129, count: count)
    controller.agentSetSelection(CanvasSelection(shape: .mask16(original),
                                                 canvasWidth: source.width,
                                                 canvasHeight: source.height))
    controller.toggleQuickMask(nil)
    try require(document.quickMaskActive, "[float] Quick Mask is on")
    try require(quickMaskCoverage(document, count: count) == original,
                "[float] the Quick Mask carries the 129 coverage")

    let selectors: [Selector] = [
        #selector(EditorViewController.fillSelection(_:)),
        #selector(EditorViewController.fillSelectionFromDeleteKey(_:)),
        #selector(EditorViewController.fillWithForeground(_:)),
        #selector(EditorViewController.fillWithBackground(_:)),
        #selector(EditorViewController.fillWithForegroundPreserving(_:)),
        #selector(EditorViewController.fillWithBackgroundPreserving(_:)),
    ]
    for selector in selectors {
        try require(validates(controller, selector),
                    "[float] \(NSStringFromSelector(selector)) validates in Quick Mask")
    }

    document.undoManager?.removeAllActions()
    let entries = document.history.entries.count
    controller.paintColor = NSColor(colorSpace: document.nsColorSpace,
                                    components: [1, 1, 1, 1], count: 4)
    ActionRecorder.shared.start()
    controller.fillWithForeground(nil)
    let steps = ActionRecorder.shared.stop()
    let full = [UInt16](repeating: 65535, count: count)
    try require(quickMaskCoverage(document, count: count) == full,
                "[float] Fill with Foreground white fills the Quick Mask to 65535")
    try require(document.doc === source, "[float] the document pixels are untouched")
    try require(!document.isDocumentEdited, "[float] the Quick Mask fill leaves the file clean")
    try require(document.history.entries.count == entries + 1
                && document.history.entries.last?.name == "Fill",
                "[float] one history entry named Fill: "
                + "\(document.history.entries.map(\.name))")
    let fills = steps.filter { $0.tool == "fill_selection" }
    try require(steps.count == 1 && fills.count == 1
                && fills.first?.arguments["target"] as? String == "quick_mask",
                "[float] one recorded fill_selection step pinned to quick_mask: "
                + "\(steps.map { "\($0.tool) \($0.arguments)" })")

    document.undoManager?.undo()
    try require(quickMaskCoverage(document, count: count) == original,
                "[float] Undo restores the 129 coverage")
    document.undoManager?.redo()
    try require(quickMaskCoverage(document, count: count) == full,
                "[float] Redo fills to 65535 again")
    document.undoManager?.undo()
    try require(quickMaskCoverage(document, count: count) == original,
                "[float] Undo again restores the 129 coverage")

    let black = try depthOK("fill_selection", document, ["color": "#000000"])
    try require(black["target"] as? String == "quick_mask" && black["changed"] as? Bool == true,
                "[float] MCP fill_selection fills the Quick Mask: \(black)")
    try require(quickMaskCoverage(document, count: count).allSatisfy { $0 == 0 },
                "[float] MCP black empties the Quick Mask")
    try require(document.doc === source, "[float] MCP left the pixels untouched")
    document.undoManager?.undo()
    try require(quickMaskCoverage(document, count: count) == original,
                "[float] Undo restores the coverage after the MCP fill")

    let stroke = try depthCall("stroke_selection", document, ["width": 2, "color": "#000000"])
    try require(stroke.isError, "[float] stroke_selection still refuses in Quick Mask")

    controller.toggleQuickMask(nil)
    try require(!document.quickMaskActive, "[float] Quick Mask is off")
    try require(validates(controller, #selector(EditorViewController.fillSelection(_:)))
                && validates(controller, #selector(EditorViewController.fillWithForeground(_:))),
                "[float] Fill… and Fill with Foreground validate outside Quick Mask")
    let layerFill = try depthCall("fill_selection", document, ["color": "#000000"])
    try require(!layerFill.isError && layerFill.result["changed"] as? Bool == true
                && layerFill.result["storage"] as? String == "rgba32_float",
                "[float] fill_selection outside Quick Mask fills the float layer: \(layerFill.text)")
    try require(document.doc !== source, "[float] the layer fill changed the float pixels")
    let painted = document.doc
    let pinned = try depthCall("fill_selection", document,
                               ["color": "#000000", "target": "quick_mask"])
    try require(pinned.isError && pinned.text.contains("is not in Quick Mask"),
                "[float] a pinned Quick Mask fill refuses outside it: \(pinned.text)")
    try require(document.doc === painted, "[float] the refused pinned fill painted nothing")
    print("PASS: float-quick-mask-fill — in a 32-bit float document the six fill items "
        + "validate in Quick Mask, Fill with Foreground fills the native coverage 129 → 65535 as "
        + "one file-clean \"Fill\" step recorded as fill_selection target quick_mask, Undo/Redo "
        + "walk it, MCP fill_selection fills it, stroke_selection still refuses, and outside "
        + "Quick Mask Fill validates and fill_selection fills the float layer")
}

/// With the panel on a mask, a call with no `target` is a LAYER edit at the
/// 16-bit depth gate, as the handlers read it. The tool is `eraser_stroke`: a
/// Multiply `gradient` is admitted on a layer, and the eraser still has no
/// blend mode.
func verifyTargetDefaultLayer() throws {
    let document = try precisionFixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    _ = try depthOK("add_layer_mask", document, ["layer": 0])
    controller.setPaintTarget(.mask)
    try require(document.planeEditTarget == .mask, "[16] the panel targets layer 0's mask")

    let normalOnly = "16-bit retouch supports Normal blending only."
    let untargeted = DocumentPrecision.refusal(
        tool: "eraser_stroke", arguments: ["blend_mode": "Multiply"], document: document)
    try require(untargeted == normalOnly,
                "[16] an untargeted Multiply eraser stroke is a layer edit: \(untargeted ?? "nil")")
    let masked = DocumentPrecision.refusal(
        tool: "eraser_stroke", arguments: ["blend_mode": "Multiply", "target": "mask"],
        document: document)
    try require(masked == nil, "[16] target mask is admitted: \(masked ?? "nil")")

    print("PASS: target-default-layer — with the panel on a mask, an untargeted 16-bit Multiply "
        + "eraser stroke answers the layer's Normal-only sentence and target mask is admitted")
}
