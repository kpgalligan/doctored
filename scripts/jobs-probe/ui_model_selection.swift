import AppKit

// The model selections through their own UI: Settings ▸ Models' six entries, Select ▸
// Sky, Select ▸ Person Parts…'s sheet and the Object Selection tool's Object Finder clicks. The
// models come from the shared fixture store ($DR_JOBS_PROBE_FIXTURES/models, filled once by
// `scripts/fill_model_fixtures.py`; read only). A case whose model is not ready there prints its
// SKIP line; the photos are the phase-6 fixtures (`scripts/phase6_quality_fixtures.py`).

/// The fixture store, swapped in as the app's for `body` when `capability`'s model is ready there;
/// false (after the SKIP line) when it is not.
private func withFixtureModel(_ capability: String, _ name: String, _ body: () throws -> Void) throws -> Bool {
    let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_JOBS_PROBE_FIXTURES"]
        ?? (NSHomeDirectory() + "/devel/doctored-fixtures")).appendingPathComponent("models")
    let store = ModelStore(directory: folder, catalog: ModelCatalog.entries)
    guard ModelCatalog.entries(capability: capability).contains(where: { store.state($0.id) == .ready }) else {
        print("SKIP: \(name) — no ready \(capability) model in the fixture store at \(folder.path)")
        return false
    }
    let previous = ModelStore.shared
    ModelStore.shared = store
    defer { ModelStore.shared = previous }
    try body()
    return true
}

/// A phase-6 photo as an off-screen document (`fixture`'s window handling), converted to
/// `mode` ("rgba8" leaves it as opened).
private func modelPhoto(_ name: String, mode: String = "rgba8") throws -> ImageDocument {
    let url = try uiFixture("phase6/photos/" + name)
    guard let document = ImageDocument.makeUntitled(with: try RasterImage.open(url: url)) else {
        throw ProbeFailure(description: "\(name) did not open as a document")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    if mode != "rgba8" {
        _ = try agentOK("convert_precision", document, ["mode": mode, "structure": "preserve"])
    }
    document.undoManager?.removeAllActions()
    return document
}

private func noteNames(_ step: ActionStep?, _ capability: String) -> Bool {
    guard let note = step?.note, let entry = ModelGate.ready(capability)?.entry else { return false }
    return note.contains("\(entry.id) @ \(entry.version)")
}

/// "model selection entries": Settings ▸ Models lists the ten catalogue entries (the nine models
/// and the sky library), each with its state, licence link, tier and training-data note.
func verifyModelSelectionEntries() throws {
    let tag = "[model selection entries]"
    let entries = ModelCatalog.entries
    try require(entries.map(\.capability)
                == ["depth", "sky", "person_parts", "object_prompt", "object_prompt", "sky_library",
                    "inpaint", "super_resolution", "jpeg_artifacts", "colorize"],
                "\(tag) the catalogue's ten capabilities in order: \(entries.map(\.capability))")
    try require(ModelCatalog.entries(capability: "object_prompt").map(\.id)
                == ["apple/coreml-sam2.1-large", "apple/coreml-sam2.1-tiny"],
                "\(tag) SAM 2.1 Large is preferred to Tiny")
    let folder = directory.appendingPathComponent("model-stores").appendingPathComponent("selection-entries")
    let previous = ModelStore.shared
    ModelStore.shared = ModelStore(directory: folder, catalog: entries)
    defer { ModelStore.shared = previous }
    let pane = ModelsSettingsPane()
    let texts = uiViews(pane.view, as: NSTextField.self).map(\.stringValue)
    for entry in entries {
        let state = try uiControl(pane.view, id: "model-state-\(entry.id)", as: NSTextField.self)
        try require(state.stringValue == "Not downloaded", "\(tag) \(entry.id) is not downloaded: \(state.stringValue)")
        let link = try uiControl(pane.view, id: "model-license-\(entry.id)", as: NSButton.self)
        try require(link.title == "Licence: \(entry.license.name)", "\(tag) \(entry.id) licence: \(link.title)")
        try require(texts.contains("Tier: \(entry.license.tier.title)")
                    && texts.contains("Training data: \(entry.trainingData)"),
                    "\(tag) \(entry.id) shows its tier and training data")
        try require(!entry.trainingData.isEmpty && entry.packages.allSatisfy { package in
            entry.files.contains { $0.path.hasPrefix(package + "/") } },
                    "\(tag) \(entry.id) names training data and every package is among its files")
    }
    print("PASS: model selection entries — Settings ▸ Models lists the depth, sky, person parts, SAM 2.1 "
        + "Large and Tiny, MI-GAN, Real-ESRGAN, FBCNN and DDColor models and the sky library, each Not "
        + "downloaded with its licence link, tier and training-data note; every package is among the "
        + "entry's files; Large comes before Tiny")
}

private func verifySelectSkyMenu(mode: String) throws {
    let tag = "[select sky menu \(mode)]"
    let document = try modelPhoto("sky.jpg", mode: mode)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    try require(uiValidates(controller, "selectSky:"), "\(tag) Select ▸ Sky validates")
    let alerts = AutomationAlerts.recorded.count
    let recorded = try uiRecorded { controller.selectSky(nil) }
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    guard let selection = document.canvasSelection else { throw ProbeFailure(description: "\(tag) no selection") }
    try require(document.undoManager?.undoActionName == "Select Sky", "\(tag) one \"Select Sky\" step")
    let bounds = selection.bounds
    try require(bounds.minY < 50 && bounds.width > 1500, "\(tag) the sky spans the top: \(bounds)")
    try require(recorded.count == 1 && recorded[0].tool == "select_sky" && noteNames(recorded.first, "sky"),
                "\(tag) one recorded select_sky step whose note names the model: "
                + "\(recorded.map { "\($0.tool) \($0.note ?? "")" })")
    document.undoManager?.undo()
    try require(document.canvasSelection == nil, "\(tag) one undo removes the selection")
}

/// "select sky menu": Select ▸ Sky on sky.jpg at 8 and 16 bits.
func verifySelectSkyMenu() throws {
    let ran = try withFixtureModel("sky", "select sky menu") {
        try verifySelectSkyMenu(mode: "rgba8")
        try verifySelectSkyMenu(mode: "rgba16")
    }
    guard ran else { return }
    print("PASS: select sky menu — at 8 and 16 bits Select ▸ Sky selects the sky of sky.jpg as one "
        + "\"Select Sky\" step with no alert, records one select_sky step whose note names the model's id "
        + "and version, and one undo removes it")
}

/// "person parts sheet": Select ▸ Person Parts… on the portrait, Hair, Apply.
func verifyPersonPartsSheet() throws {
    let ran = try withFixtureModel("person_parts", "person parts sheet") {
        let tag = "[person parts sheet]"
        let document = try modelPhoto("portrait.png")
        defer {
            if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
            document.close()
        }
        let controller = try editor(document)
        try require(uiValidates(controller, "showPersonParts:"), "\(tag) Person Parts… validates")
        let alerts = AutomationAlerts.recorded.count
        let recorded = try uiRecorded {
            let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
            controller.showPersonParts(nil)
            let opened = {
                (controller.presentedViewControllers ?? []).compactMap { $0 as? PersonPartsSheetController }
                    .last { !known.contains(ObjectIdentifier($0)) }
            }
            try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 120)
            guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Person Parts sheet") }
            let hair = try uiControl(sheet.view, id: "person-parts-hair", as: NSButton.self)
            let people = try uiControl(sheet.view, id: "person-parts-people", as: NSPopUpButton.self)
            try require(hair.state == .on && sheet.selectedParts == [.hair], "\(tag) Hair is chosen")
            try require(people.titleOfSelectedItem == "All People", "\(tag) All People by default")
            guard let apply = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
                throw ProbeFailure(description: "\(tag) no Apply button")
            }
            try uiSend(apply, "\(tag) Apply")
            try pumpUntil({ sheet.view.window?.isVisible != true && !BackgroundWork.occupied }, seconds: 120)
        }
        try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
        try require(document.canvasSelection != nil && document.undoManager?.undoActionName == "Person Parts",
                    "\(tag) one \"Person Parts\" step")
        try require(recorded.count == 1 && recorded[0].tool == "select_person_parts"
                    && (recorded[0].arguments["parts"] as? [String]) == ["hair"]
                    && noteNames(recorded.first, "person_parts"),
                    "\(tag) one recorded select_person_parts step for hair naming the model: "
                    + "\(recorded.map { "\($0.tool) \($0.arguments) \($0.note ?? "")" })")
        document.undoManager?.undo()
        try require(document.canvasSelection == nil, "\(tag) one undo removes the selection")
    }
    guard ran else { return }
    print("PASS: person parts sheet — Select ▸ Person Parts… on the portrait opens with Hair and All People; "
        + "Apply is one \"Person Parts\" step and one recorded select_person_parts step for hair whose note "
        + "names the model, and one undo removes it")
}

private func finderMouse(_ canvas: ImageCanvasView, _ type: NSEvent.EventType, _ point: CGPoint,
                         _ flags: NSEvent.ModifierFlags) throws -> NSEvent {
    guard let window = canvas.window, let event = NSEvent.mouseEvent(
        with: type, location: canvas.convert(point, to: nil), modifierFlags: flags,
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 0, clickCount: 1, pressure: 1) else {
        throw ProbeFailure(description: "[object finder click] could not make a \(type.rawValue) event")
    }
    return event
}

/// "object finder click": the Object Selection tool with Object Finder on, on product.jpg: a click,
/// a Shift-click and an ⌥-click are three steps, the last two continuing the first's prompt.
func verifyObjectFinderClick() throws {
    let ran = try withFixtureModel("object_prompt", "object finder click") {
        let tag = "[object finder click]"
        let settings = ObjectSelectionSettings.shared
        let (mode, finder) = (settings.mode, settings.objectFinder)
        settings.mode = .rectangle
        settings.objectFinder = true
        let document = try modelPhoto("product.jpg")
        defer {
            settings.mode = mode
            settings.objectFinder = finder
            if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
            document.close()
        }
        let controller = try editor(document)
        let canvas: ImageCanvasView = controller.canvas
        controller.selectObjectSelectTool(nil)
        let alerts = AutomationAlerts.recorded.count
        // The camera body's centre, a point on the body above the cap, and the cap.
        let clicks: [(CGPoint, NSEvent.ModifierFlags)] = [
            (CGPoint(x: 1303, y: 924), []), (CGPoint(x: 600, y: 900), [.shift]), (CGPoint(x: 1544, y: 1030), [.option]),
        ]
        let recorded = try uiRecorded {
            for (point, flags) in clicks {
                canvas.mouseDown(with: try finderMouse(canvas, .leftMouseDown, point, flags))
                canvas.mouseUp(with: try finderMouse(canvas, .leftMouseUp, point, flags))
                try pumpUntil({ !BackgroundWork.occupied }, seconds: 120)
            }
        }
        try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
        try require(document.canvasSelection != nil && document.undoManager?.undoActionName == "Object Selection",
                    "\(tag) the last step is \"Object Selection\"")
        let continued = recorded.map { $0.arguments["continue"] as? Bool == true }
        let counts = recorded.map { ($0.arguments["points"] as? [Any])?.count ?? 0 }
        try require(recorded.count == 3 && recorded.allSatisfy { $0.tool == "object_finder_select" }
                    && continued == [false, true, true] && counts == [1, 2, 3]
                    && recorded.allSatisfy({ noteNames($0, "object_prompt") }),
                    "\(tag) three recorded object_finder_select steps, the last two continuing with the "
                    + "whole prompt: \(recorded.map { "\($0.tool) \($0.arguments)" })")
        for _ in 0..<3 { document.undoManager?.undo() }
        try require(document.canvasSelection == nil, "\(tag) three undos remove the three steps")
    }
    guard ran else { return }
    print("PASS: object finder click — with Object Finder on, a click, a Shift-click and an ⌥-click on "
        + "product.jpg are three \"Object Selection\" steps with no alert, recorded as object_finder_select "
        + "with 1, 2 and 3 points, the last two continuing the prompt, each note naming the model; three "
        + "undos remove them")
}
