import AppKit

// Edit ▸ Sky Replacement… through its own sheet at 8 and 16 bits, with the sky model and
// the sky library from the shared fixture store ($DR_JOBS_PROBE_FIXTURES/models, filled once by
// `scripts/fill_model_fixtures.py --release-dir ~/devel/doctored-fixtures/models-release`; read
// only). Without both ready there the case prints its SKIP line. The photo is the phase-6
// fixture `sky.jpg`.

/// The fixture store as the app's for `body` when the sky model and the sky library are both
/// ready there; false (after the SKIP line) when either is not.
private func withSkyFixtures(_ body: () throws -> Void) throws -> Bool {
    let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_JOBS_PROBE_FIXTURES"]
        ?? (NSHomeDirectory() + "/devel/doctored-fixtures")).appendingPathComponent("models")
    let store = ModelStore(directory: folder, catalog: ModelCatalog.entries)
    for capability in ["sky", SkyLibrary.capability] {
        guard ModelCatalog.entries(capability: capability).contains(where: { store.state($0.id) == .ready }) else {
            print("SKIP: sky replacement sheet — no ready \(capability) entry in the fixture store at \(folder.path)")
            return false
        }
    }
    let previous = ModelStore.shared
    ModelStore.shared = store
    defer { ModelStore.shared = previous }
    try body()
    return true
}

/// sky.jpg as an off-screen document, converted to 16 bits for `depth` 16.
private func skyPhoto(depth: Int) throws -> ImageDocument {
    let url = try uiFixture("phase6/photos/sky.jpg")
    guard let document = ImageDocument.makeUntitled(with: try RasterImage.open(url: url)) else {
        throw ProbeFailure(description: "sky.jpg did not open as a document")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    if depth == 16 {
        _ = try agentOK("convert_precision", document, ["mode": "rgba16", "structure": "preserve"])
    }
    document.undoManager?.removeAllActions()
    return document
}

/// At 8 bits OK keeps the Output To default, New Layers (the group: five new entries with
/// Decontaminate Edge on, the group active at the top); at 16 bits the popup picks Duplicate
/// Layer (one new layer, active).
private func verifySkyReplacementSheet(depth: Int) throws {
    let tag = "[sky replacement sheet \(depth)-bit]"
    let output: SkyReplacementOutput = depth == 8 ? .newLayers : .duplicateLayer
    let document = try skyPhoto(depth: depth)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.isHighDepth == (depth == 16) else {
        throw ProbeFailure(description: "\(tag) the fixture's depth")
    }
    let presets = SkyLibrary.presets()
    guard presets.count >= 2, let entry = ModelGate.ready("sky")?.entry else {
        throw ProbeFailure(description: "\(tag) the library lists \(presets.count) skies")
    }
    try require(uiValidates(controller, "showSkyReplacement:"), "\(tag) Sky Replacement… validates")
    let entries = document.history.entries.count
    let layers = base.layerCount
    let alerts = AutomationAlerts.recorded.count
    let recorded = try uiRecorded {
        let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
        controller.showSkyReplacement(nil)
        let opened = {
            (controller.presentedViewControllers ?? []).compactMap { $0 as? SkyReplacementSheetController }
                .last { !known.contains(ObjectIdentifier($0)) }
        }
        try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 240)
        guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Sky Replacement sheet") }
        try require(sheet.choice == .library(presets[0]), "\(tag) the first library sky is picked")
        try require(sheet.proxy.width <= SkyReplacementSheetController.proxyLongSide
                    && sheet.proxy.height <= SkyReplacementSheetController.proxyLongSide,
                    "\(tag) the proxy is at most 1024 px: \(sheet.proxy.width)×\(sheet.proxy.height)")
        let full = try SkyLibrary.image(.library(presets[0]))
        let f = min(1, 4 * max(Double(sheet.proxy.width) / Double(full.width),
                               Double(sheet.proxy.height) / Double(full.height)))
        guard let reduced = sheet.previewSky else { throw ProbeFailure(description: "\(tag) no preview sky") }
        try require(Double(reduced.width) <= (Double(full.width) * f).rounded() + 1
                    && Double(reduced.height) <= (Double(full.height) * f).rounded() + 1,
                    "\(tag) the preview sky is at most f = \(f) of \(full.width)×\(full.height): "
                    + "\(reduced.width)×\(reduced.height)")
        try require(sheet.preview.image != nil, "\(tag) the preview renders")
        let before = sheet.preview.image
        let lighting = try uiControl(sheet.view, id: "sky-replacement-lighting", as: NSSlider.self)
        lighting.doubleValue = 80
        try uiSend(lighting, "\(tag) Lighting Adjustment")
        try require(sheet.settings.lighting == 80, "\(tag) the slider sets Lighting Adjustment")
        try require(sheet.preview.image != nil && sheet.preview.image !== before, "\(tag) the slider re-renders")
        let popup = try uiControl(sheet.view, id: "sky-replacement-sky", as: NSPopUpButton.self)
        guard let index = popup.itemArray.firstIndex(where: { $0.title.trimmingCharacters(in: .whitespaces)
                                                                == presets[1].name }) else {
            throw ProbeFailure(description: "\(tag) the popup lists \(presets[1].name)")
        }
        popup.selectItem(at: index)
        try uiSend(popup, "\(tag) the sky popup")
        try require(sheet.choice == .library(presets[1]), "\(tag) picking a sky selects it")
        let decontaminate = try uiControl(sheet.view, id: "sky-replacement-decontaminate", as: NSButton.self)
        try require(decontaminate.title == "Decontaminate Edge" && decontaminate.state == .on
                    && sheet.settings.decontaminate,
                    "\(tag) Decontaminate Edge is on by default")
        let outputs = try uiControl(sheet.view, id: "sky-replacement-output", as: NSPopUpButton.self)
        try require(outputs.itemTitles == ["New Layers", "Duplicate Layer"] && outputs.indexOfSelectedItem == 0
                    && sheet.settings.output == .newLayers,
                    "\(tag) Output To lists New Layers (selected) then Duplicate Layer: \(outputs.itemTitles)")
        if output == .duplicateLayer {
            outputs.selectItem(at: 1)
            try uiSend(outputs, "\(tag) the Output To popup")
            try require(sheet.settings.output == .duplicateLayer, "\(tag) Output To picks Duplicate Layer")
        }
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) the preview leaves the document untouched")
        guard let ok = buttons(sheet.view).first(where: { $0.keyEquivalent == "\r" }) else {
            throw ProbeFailure(description: "\(tag) no OK button")
        }
        try uiSend(ok, "\(tag) OK")
        try pumpUntil({ document.doc !== base && sheet.view.window?.isVisible != true
                        && !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 240)
    }
    try require(document.history.entries.count == entries + 1
                && document.undoManager?.undoActionName == "Sky Replacement",
                "\(tag) one \"Sky Replacement\" step: \(document.history.entries.map(\.name))")
    let added = output == .newLayers ? 5 : 1
    try require(document.doc?.layerCount == layers + added && document.activeLayerIndex == added,
                "\(tag) \(output.title): \(added) new entries above the photo, the output active: "
                + "\(document.doc?.layerCount ?? 0) layers, active \(document.activeLayerIndex)")
    if output == .newLayers, let doc = document.doc {
        try require(doc.layerInfo(5)?.isGroup == true && doc.layerInfo(5)?.name == "Sky Replacement Group"
                    && (1...4).map({ doc.layerInfo($0)?.name ?? "" })
                        == ["Foreground Edge", "Foreground Color", "Foreground Lighting", "Sky"]
                    && !doc.layerIsAdjustment(1) && doc.layerIsAdjustment(2) && doc.layerIsAdjustment(3)
                    && !doc.layerIsAdjustment(4),
                    "\(tag) the Sky Replacement Group holds the Foreground Edge, its two adjustments and "
                    + "the Sky")
    }
    let number = { (key: String) in (recorded.first?.arguments[key] as? NSNumber)?.doubleValue }
    try require(recorded.count == 1 && recorded[0].tool == "sky_replacement"
                && recorded[0].arguments["sky"] as? String == presets[1].id && number("lighting") == 80
                && recorded[0].arguments["output"] as? String == output.rawValue
                && (recorded[0].arguments["decontaminate"] as? NSNumber)?.boolValue == true
                && recorded[0].note?.contains("\(entry.id) @ \(entry.version)") == true,
                "\(tag) one recorded sky_replacement step with the sky, the settings and a note naming the "
                + "model: \(recorded.map { "\($0.tool) \($0.arguments) \($0.note ?? "")" })")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the document")
}

/// "sky replacement sheet": Edit ▸ Sky Replacement… on sky.jpg at 8 and 16 bits with the fixture
/// store's sky model and library: the sheet runs the model once and previews from the reduced
/// sky; Lighting Adjustment and a sky pick re-render with the document untouched; Output To
/// lists New Layers (the default) and Duplicate Layer; Decontaminate Edge is on; OK is one "Sky
/// Replacement" step adding the active group with its Foreground Edge (8 bits, New Layers) or one
/// active layer (16 bits, Duplicate Layer), one recorded step (decontaminate true) whose note
/// names the model, and one undo.
func verifySkyReplacementSheet() throws {
    let ran = try withSkyFixtures {
        try verifySkyReplacementSheet(depth: 8)
        try verifySkyReplacementSheet(depth: 16)
    }
    guard ran else { return }
    print("PASS: sky replacement sheet — at 8 and 16 bits Edit ▸ Sky Replacement… runs the fixture store's "
        + "sky model once and previews a ≤1024 px proxy with the sky reduced by f = min(1, 4·max(pw/sw, "
        + "ph/sh)); Lighting Adjustment 80 and a second library sky re-render with the document untouched; "
        + "Output To lists New Layers (default) then Duplicate Layer; Decontaminate Edge is on by default; "
        + "OK is one \"Sky Replacement\" step adding the active Sky Replacement Group (five entries, the "
        + "Foreground Edge first) at 8 bits and one active layer (Duplicate Layer picked) at 16 bits, one "
        + "recorded sky_replacement step naming the sky, the output and decontaminate true and a note "
        + "with the model's id and version, and one undo restores it")
}
