import AppKit

// Filter ▸ Neural Filters ▸ Depth Blur… through its own sheet at 8 and 16 bits, with the
// depth model from the shared fixture store ($DR_JOBS_PROBE_FIXTURES/models, filled once by
// `scripts/fill_model_fixtures.py`; read only, never deleted from). Without a ready model there
// the case prints its SKIP line.

private func depthBlurSheet(_ controller: EditorViewController, _ tag: String) throws -> DepthBlurSheetController {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.showDepthBlur(nil)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? DepthBlurSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 120)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Depth Blur sheet") }
    return sheet
}

private func verifyDepthBlurSheet(depth: Int) throws {
    let tag = "[depth blur sheet \(depth)-bit]"
    let document = depth == 8
        ? try fixture(width: 320, height: 240) : try precisionFixture(width: 320, height: 240)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc, base.isHighDepth == (depth == 16) else {
        throw ProbeFailure(description: "\(tag) the fixture's depth")
    }
    try require(uiValidates(controller, "showDepthBlur:"), "\(tag) Depth Blur… validates")
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count
    let recorded = try uiRecorded {
        let sheet = try depthBlurSheet(controller, tag)
        let strength = try uiControl(sheet.view, id: "depth-blur-blur_strength", as: NSSlider.self)
        strength.doubleValue = 90
        try uiSend(strength, "\(tag) Blur Strength")
        try require(sheet.settings.blurStrength == 90, "\(tag) the slider sets Blur Strength")
        try pumpUntil({ canvas.previewImage != nil }, seconds: 120)
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) the preview leaves the document untouched")
        guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
            throw ProbeFailure(description: "\(tag) no Apply button")
        }
        try uiSend(apply, "\(tag) Apply")
        try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 120)
    }
    try require(canvas.previewImage == nil, "\(tag) Apply leaves no preview")
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Depth Blur",
                "\(tag) one \"Depth Blur\" step: \(document.history.entries.map(\.name))")
    let number = { (key: String) in (recorded.first?.arguments[key] as? NSNumber)?.doubleValue }
    try require(recorded.count == 1 && recorded[0].tool == "depth_blur" && number("blur_strength") == 90
                && recorded[0].arguments["depth"] == nil && recorded[0].arguments["output"] as? String == "layer",
                "\(tag) one recorded depth_blur step with the settings and no depth map: "
                + "\(recorded.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the document")
}

/// "depth blur sheet": Filter ▸ Neural Filters ▸ Depth Blur… at 8 and 16 bits with the fixture
/// store's model: the sheet runs the model once, Blur Strength previews with the document
/// untouched, and Apply is one "Depth Blur" step recording the settings and no depth map.
func verifyDepthBlurSheet() throws {
    // The sheet captures Vision's subject whenever it finds one; the request carries it only
    // with Focus Subject on, as the recorded step's replay does (`needsSubject: focus_subject`).
    let input = DepthBlurInput(frame: CGRect(x: 0, y: 0, width: 2, height: 2),
                               depth: DepthMap(width: 2, height: 2, samples: [0, 1, 2, 3]),
                               subject: [255, 0, 0, 0])
    var settings = DepthBlurSettings()
    for focus in [true, false] {
        settings.focusSubject = focus
        let depth = DepthBlurRun.params(settings, input, layerOrigin: .zero, scale: 1)["depth"] as? [String: Any]
        try require((depth?["subject"] != nil) == focus,
                    "[depth blur sheet] the subject mask is sent only with Focus Subject on (focus \(focus))")
    }
    let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_JOBS_PROBE_FIXTURES"]
        ?? (NSHomeDirectory() + "/devel/doctored-fixtures")).appendingPathComponent("models")
    let store = ModelStore(directory: folder, catalog: ModelCatalog.entries)
    guard let entry = ModelCatalog.entry(capability: "depth"), store.state(entry.id) == .ready else {
        print("SKIP: depth blur sheet — no fixture model store at \(folder.path)")
        return
    }
    let previous = ModelStore.shared
    ModelStore.shared = store
    defer { ModelStore.shared = previous }
    try verifyDepthBlurSheet(depth: 8)
    try verifyDepthBlurSheet(depth: 16)
    print("PASS: depth blur sheet — at 8 and 16 bits Filter ▸ Neural Filters ▸ Depth Blur… runs the fixture "
        + "store's model once and opens the sheet; Blur Strength 90 previews with the document untouched; "
        + "Apply is one \"Depth Blur\" step, one recorded depth_blur step carrying the settings and no depth "
        + "map, and one undo; the request carries the subject mask only with Focus Subject on")
}
