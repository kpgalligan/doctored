import AppKit

// The Develop RAW window's range rows, generated masks and output space,
// through its own controls. The camera files are the three synthetic DNGs
// `raw_fixtures.py` writes into DR_JOBS_PROBE_RAW_DIR before the probe runs; a
// case whose file is absent FAILS, naming the file and the builder's log.
//
// The step driver and the three helpers below are this probe's own copies:
// `scripts/raw-local-probe` is a separate probe set and is not compiled here.

/// One camera fixture, or a failure naming it and the log that says why.
private func developFixture(_ name: String) throws -> URL {
    guard let folder = ProcessInfo.processInfo.environment["DR_JOBS_PROBE_RAW_DIR"],
          !folder.isEmpty else {
        throw ProbeFailure(description: "DR_JOBS_PROBE_RAW_DIR is not set, so \(name) was never "
            + "built (scripts/jobs-probe/verify.py sets it)")
    }
    let url = URL(fileURLWithPath: folder).appendingPathComponent(name)
    guard FileManager.default.fileExists(atPath: url.path) else {
        throw ProbeFailure(description: "\(name) is missing from \(folder); "
            + "raw-fixtures.log beside that folder says why it was skipped")
    }
    return url
}

/// A camera file opened as its retained 16-bit development.
/// Master copy: `localOpen` in `scripts/raw-local-probe/main.swift`.
private func developOpen(_ url: URL) throws -> RasterDocument {
    let snapshot = try RawSourceSnapshot.capture(url, maximumBytes: 256 * 1024 * 1024)
    let captured = try RetainedRawDecoder.capture(
        url: snapshot.url, filename: url.lastPathComponent, sha256: snapshot.sha256)
    var descriptor = captured
    descriptor.recipe = try captured.recipe.updating(RawDevelopSettings())
    let desired = descriptor
    return try BackgroundWork.wait(try WorkerTask {
        let decoded = try RetainedRawDecoder.develop(
            url: snapshot.url, descriptor: desired,
            whiteBalanceChanged: desired.recipe.values.customWhiteBalance)
        return try RawSourceWork.initialDocument(
            asset: decoded.0, descriptor: decoded.1, bytes: snapshot.bytes, highDepth: true)
    }, title: "Opening the Develop fixture")
}

/// The development as an open document with its editor.
/// Master copy: `LocalUI.document` in `scripts/raw-local-probe/ui.swift`; this
/// one keeps the window off screen and the undo groups explicit, as `fixture`
/// does.
private func developDocument(_ source: RasterDocument) throws -> ImageDocument {
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90,
        embedColorProfile: true, stripMetadata: false, exportBitDepth: 16, exportDither: "none"),
        markSaved: true)
    document.undoManager?.groupsByEvent = false
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    return document
}

/// One click on the Develop canvas's gesture view, at the middle of the
/// displayed picture. Master copy: `LocalUI.gesture` / `LocalUI.drag` in
/// `scripts/raw-local-probe/ui.swift`.
private func developClick(_ controller: RawDevelopWindowController) throws {
    guard let gesture = controller.localGesture, let window = gesture.window,
          let rect = gesture.mapping()?.rect else {
        throw ProbeFailure(description: "the Develop canvas shows no picture to click")
    }
    try require(gesture.enabled, "the source gesture is enabled for the displayed frame")
    let point = gesture.convert(CGPoint(x: rect.midX, y: rect.midY), to: nil)
    func event(_ type: NSEvent.EventType) throws -> NSEvent {
        guard let event = NSEvent.mouseEvent(
            with: type, location: point, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
            throw ProbeFailure(description: "could not create the click")
        }
        return event
    }
    gesture.mouseDown(with: try event(.leftMouseDown))
    gesture.mouseUp(with: try event(.leftMouseUp))
}

/// The Develop window's own controller, from its Apply button's target.
private func developController(_ window: NSWindow) -> RawDevelopWindowController? {
    guard let card = window.contentView,
          let apply = try? uiControl(card, id: "raw-develop-apply", as: NSButton.self) else {
        return nil
    }
    return apply.target as? RawDevelopWindowController
}

/// One step of the Develop driver: it waits for the window's preview to be
/// ready (`previewIsReady`), then runs. Master copy: `LocalUIDriver` in
/// `scripts/raw-local-probe/ui.swift`.
private func developStep(_ name: String,
                         _ action: @escaping (RawDevelopWindowController, NSView) throws -> Void)
    -> ModalSteps.Step {
    (name, { window in
        guard let controller = developController(window), controller.previewIsReady,
              let card = window.contentView else { return false }
        try action(controller, card)
        return true
    })
}

private func developRun(_ steps: [ModalSteps.Step], _ body: () throws -> Void) throws {
    try ModalSteps(title: "Develop RAW", timeout: 120, steps: steps).run(body)
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 30)
}

private func developRow(_ card: NSView, _ id: String) throws -> CardControlRow {
    try uiControl(card, id: id, as: CardControlRow.self)
}

private func developSlide(_ card: NSView, _ id: String, _ value: Double) throws {
    guard let slider = try developRow(card, id).slider else {
        throw ProbeFailure(description: "\(id) has no slider")
    }
    slider.doubleValue = value
    try uiSend(slider, id)
}

private func developChoose(_ card: NSView, _ id: String, _ index: Int) throws {
    guard let popup = try developRow(card, id).popUp else {
        throw ProbeFailure(description: "\(id) has no popup")
    }
    popup.selectItem(at: index)
    try uiSend(popup, id)
}

private func developPress(_ card: NSView, _ id: String) throws {
    try uiSend(try uiControl(card, id: id, as: NSButton.self), id)
}

private func developCancel(_ card: NSView) throws {
    guard let cancel = buttons(card).first(where: {
        $0.action.map(NSStringFromSelector) == "cancelClicked:"
    }) else { throw ProbeFailure(description: "the Develop window has no Cancel") }
    try uiSend(cancel, "Cancel")
}

private func developLocalTab(_ card: NSView) throws {
    let tabs = try uiControl(card, id: "raw-develop-section", as: NSSegmentedControl.self)
    tabs.selectedSegment = 1
    try uiSend(tabs, "raw-develop-section")
}

private func developStatus(_ card: NSView) throws -> String {
    try uiControl(card, id: "raw-develop-status", as: NSTextField.self).stringValue
}

private func developBytes(_ image: CGImage?) -> Data? { image?.dataProvider?.data as Data? }

/// "develop range rows": a New Range local, its luminance sliders, the Sample
/// Colour path and the effective-mask overlay, then Cancel.
func verifyDevelopRangeRows() throws {
    let tag = "[develop range rows]"
    let source = try developOpen(try developFixture("develop.dng"))
    let document = try developDocument(source)
    defer { document.close() }
    let controller = try editor(document)
    let alerts = AutomationAlerts.recorded.count
    var fullOverlay: Data?
    var narrowOverlay: Data?
    var lab: [Double] = []
    try developRun([
        developStep("the Local Masks tab") { _, card in try developLocalTab(card) },
        developStep("New Range") { develop, card in
            try require(develop.localSession?.state.nodes.isEmpty == true,
                        "\(tag) the development starts with no local")
            try developPress(card, "raw-local-new-range")
        },
        developStep("the luminance sliders") { develop, card in
            guard let node = develop.localSession?.state.nodes.first,
                  develop.localSession?.state.nodes.count == 1 else {
                throw ProbeFailure(description: "\(tag) New Range made no local")
            }
            try require(node.range?.luminance == RawLuminanceRange(low: 0, high: 1)
                        && node.range?.color == nil,
                        "\(tag) a new range is the whole luminance band: "
                        + "\(String(describing: node.range))")
            let overlay = try uiControl(card, id: "raw-local-overlay", as: NSButton.self)
            try require(overlay.state == .on, "\(tag) the overlay box is ticked")
            fullOverlay = developBytes(develop.localGesture?.overlay)
            try require(fullOverlay != nil, "\(tag) the full range shows an overlay")
            try developSlide(card, "raw-local-range-low", 0.25)
            try developSlide(card, "raw-local-range-high", 0.75)
            try developSlide(card, "raw-local-range-low-falloff", 0.1)
        },
        developStep("the range lands; Colour with no colour; one click") { develop, card in
            guard let session = develop.localSession,
                  let sent = (session.operations.last?["range"] as? [String: Any])?["luminance"]
                    as? [String: Any] else {
                throw ProbeFailure(description: "\(tag) the last operation carries no "
                    + "range.luminance: \(develop.localSession?.operations.last ?? [:])")
            }
            let number = { (key: String) in (sent[key] as? NSNumber)?.doubleValue }
            try require(number("low") == 0.25 && number("high") == 0.75
                        && number("low_falloff") == 0.1 && number("high_falloff") == 0,
                        "\(tag) the operation carries the sliders' band: \(sent)")
            try require(session.state.nodes.first?.range?.luminance
                        == RawLuminanceRange(low: 0.25, high: 0.75, lowFalloff: 0.1),
                        "\(tag) the local holds the band: "
                        + "\(String(describing: session.state.nodes.first?.range))")
            narrowOverlay = developBytes(develop.localGesture?.overlay)
            try require(narrowOverlay != nil && narrowOverlay != fullOverlay,
                        "\(tag) the narrowed range draws a different overlay")
            // Colour ticked with no colour: the tool becomes Sample Colour.
            guard let box = try developRow(card, "raw-local-range-color").checkbox,
                  let tool = try developRow(card, "raw-local-tool").popUp else {
                throw ProbeFailure(description: "\(tag) the Colour tick and the tool popup")
            }
            let readout = try uiControl(card, id: "raw-local-range-lab", as: NSTextField.self)
            box.state = .on
            try uiSend(box, "\(tag) the Colour tick")
            try require(tool.titleOfSelectedItem == "Sample Colour",
                        "\(tag) the mask tool shows Sample Colour: "
                        + "\(tool.titleOfSelectedItem ?? "-")")
            try require(readout.stringValue == "Click the image with Sample Colour",
                        "\(tag) the readout asks for a sample: \(readout.stringValue)")
            try require(develop.previewIsReady && session.state.nodes.first?.range?.color == nil,
                        "\(tag) a tick with no colour sends nothing")
            try developClick(develop)
            try require(!develop.previewIsReady, "\(tag) the click starts a development")
        },
        developStep("the sampled colour; Cancel") { develop, card in
            let readout = try uiControl(card, id: "raw-local-range-lab", as: NSTextField.self)
            guard let color = develop.localSession?.state.nodes.first?.range?.color,
                  let sampled = color.lab, sampled.count == 3 else {
                throw ProbeFailure(description: "\(tag) the local has no range.color.lab: "
                    + "\(String(describing: develop.localSession?.state.nodes.first?.range))")
            }
            lab = sampled
            try require(color.sample == nil, "\(tag) the resolved colour keeps no sample point")
            try require(readout.stringValue
                        == String(format: "L %.1f  a %.1f  b %.1f", lab[0], lab[1], lab[2]),
                        "\(tag) the readout shows the sampled Lab: \(readout.stringValue)")
            try developCancel(card)
        },
    ]) { controller.redevelopRaw(nil) }
    try require(document.doc === source && !document.isDocumentEdited
                && document.undoManager?.canUndo != true,
                "\(tag) Cancel publishes nothing")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: develop range rows — in Redevelop RAW a New Range local starts as the whole "
        + "luminance band; the Low, High and Low falloff sliders reach the last operation and the "
        + "local, and the effective-mask overlay changes with them; Colour ticked with no colour "
        + "selects Sample Colour and asks for a click, and one click resolves "
        + String(format: "L %.1f a %.1f b %.1f", lab.first ?? 0, lab.dropFirst().first ?? 0,
                 lab.last ?? 0)
        + " into the local; Cancel publishes nothing")
}

/// "develop generated masks": New Subject, Recompute and New People on a
/// portrait, then both on a picture with nothing to find.
func verifyDevelopGeneratedMasks() throws {
    let tag = "[develop generated masks]"
    let portraitURL = try developFixture("portrait.dng")
    let flatURL = try developFixture("flat.dng")
    let alerts = AutomationAlerts.recorded.count

    let source = try developOpen(portraitURL)
    let document = try developDocument(source)
    defer { document.close() }
    let controller = try editor(document)
    try developRun([
        developStep("the Local Masks tab") { _, card in try developLocalTab(card) },
        developStep("New Subject") { _, card in
            let recompute = try uiControl(card, id: "raw-local-recompute", as: NSButton.self)
            try require(!recompute.isEnabled, "\(tag) Recompute is off with no generated local")
            try developPress(card, "raw-local-new-subject")
        },
        developStep("the subject mask; Recompute") { develop, card in
            guard let state = develop.localSession?.state, state.nodes.count == 1,
                  state.masks.count == 1 else {
                throw ProbeFailure(description: "\(tag) New Subject made no local: "
                    + ((try? developStatus(card)) ?? ""))
            }
            try require(state.nodes[0].generated == .subject,
                        "\(tag) the local records it was generated as a subject")
            let subject = try state.masks[0].encodedBytes()
            try require(subject.dropFirst(16).contains { $0 != 0 },
                        "\(tag) the subject mask holds coverage")
            let recompute = try uiControl(card, id: "raw-local-recompute", as: NSButton.self)
            try require(recompute.isEnabled, "\(tag) Recompute is on for the generated local")
            try uiSend(recompute, "\(tag) Recompute")
            try require(!develop.previewIsReady, "\(tag) Recompute starts a development")
        },
        developStep("one local still; New People") { develop, card in
            try require(develop.localSession?.state.nodes.count == 1
                        && develop.localSession?.state.nodes.first?.generated == .subject,
                        "\(tag) Recompute keeps the one subject local")
            try developPress(card, "raw-local-new-people")
        },
        developStep("the people mask; Apply") { develop, card in
            guard let state = develop.localSession?.state, state.nodes.count == 2 else {
                throw ProbeFailure(description: "\(tag) New People made no local: "
                    + ((try? developStatus(card)) ?? ""))
            }
            try require(state.nodes[1].generated == .people,
                        "\(tag) the second local records it was generated as people")
            let people = try state.masks[1].encodedBytes()
            try require(people.dropFirst(16).contains { $0 != 0 },
                        "\(tag) the people mask holds coverage")
            try developPress(card, "raw-develop-apply")
        },
    ]) { controller.redevelopRaw(nil) }
    guard let applied = document.doc, applied !== source else {
        throw ProbeFailure(description: "\(tag) Apply published nothing: \(uiAlerts())")
    }
    let kinds = try RawLocalWork.state(applied, layer: 0).nodes.map(\.generated)
    try require(kinds == [.subject, .people],
                "\(tag) the applied development holds both generated locals: \(kinds)")
    try require(document.isDocumentEdited, "\(tag) Apply dirties the document")
    document.undoManager?.undo()
    try require(document.doc === source && document.undoManager?.canUndo != true,
                "\(tag) Apply is one undo step")

    // Nothing to find: each button leaves no local and says so in the status line.
    let flat = try developOpen(flatURL)
    let flatDocument = try developDocument(flat)
    defer { flatDocument.close() }
    let flatController = try editor(flatDocument)
    let noSubject = RawLocalGeneratedKind.subject.nothingFound
    let noPeople = RawLocalGeneratedKind.people.nothingFound
    try require(noSubject == "No subject was found in the image."
                && noPeople == SubjectTargeting.noPeopleMessage,
                "\(tag) the two refusal sentences: \(noSubject) / \(noPeople)")
    try developRun([
        developStep("the Local Masks tab") { _, card in try developLocalTab(card) },
        developStep("New Subject") { _, card in try developPress(card, "raw-local-new-subject") },
        developStep("no subject; New People") { develop, card in
            let status = try developStatus(card)
            try require(develop.localSession?.state.nodes.isEmpty == true
                        && develop.localSession?.operations.isEmpty == true,
                        "\(tag) a subject request that finds nothing leaves no local")
            try require(status.hasPrefix(noSubject), "\(tag) the status line says so: \(status)")
            try developPress(card, "raw-local-new-people")
        },
        developStep("no people; Cancel") { develop, card in
            let status = try developStatus(card)
            try require(develop.localSession?.state.nodes.isEmpty == true
                        && develop.localSession?.operations.isEmpty == true,
                        "\(tag) a people request that finds nothing leaves no local")
            try require(status.hasPrefix(noPeople), "\(tag) the status line says so: \(status)")
            try developCancel(card)
        },
    ]) { flatController.redevelopRaw(nil) }
    try require(flatDocument.doc === flat && !flatDocument.isDocumentEdited,
                "\(tag) the flat picture's Cancel publishes nothing")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: develop generated masks — on a portrait New Subject makes a local generated as "
        + "subject with coverage, Recompute is enabled only for it and keeps the one local, New "
        + "People adds a people local, and Apply is one undo step; on a flat picture each button "
        + "leaves no local and the status line reads \"\(noSubject)\" / \"\(noPeople)\", with no "
        + "alert")
}

/// "develop output space": the import dialog's output row and its depth note.
func verifyDevelopOutputSpace() throws {
    let tag = "[develop output space]"
    let url = try developFixture("develop.dng")
    let snapshot = try RawSourceSnapshot.capture(url, maximumBytes: 256 * 1024 * 1024)
    let descriptor = try RetainedRawDecoder.capture(
        url: snapshot.url, filename: url.lastPathComponent, sha256: snapshot.sha256)
    guard let probe = RawImage.inspect(url: snapshot.url) else {
        throw ProbeFailure(description: "\(tag) develop.dng is not a camera file the app reads")
    }
    let alerts = AutomationAlerts.recorded.count
    let profile = { (develop: RawDevelopWindowController) in
        develop.workflowPanel?.canvas.preview?.after.image.profileData
    }
    /// The note under the output row: its text when shown, nil when hidden.
    func note(_ card: NSView) throws -> String? {
        let label = try uiControl(card, id: "raw-develop-output-note", as: NSTextField.self)
        return label.isHiddenOrHasHiddenAncestor ? nil : label.stringValue
    }
    var imported: (settings: RawDevelopSettings, highDepth: Bool)?
    try developRun([
        developStep("the four spaces; Adobe RGB") { develop, card in
            guard let popup = try developRow(card, "raw-develop-output-space").popUp else {
                throw ProbeFailure(description: "\(tag) the output row has no popup")
            }
            try require(popup.itemTitles == RawOutputSpace.allCases.map(\.displayName)
                        && popup.itemTitles.count == 4,
                        "\(tag) the popup lists the four spaces: \(popup.itemTitles)")
            try require(profile(develop) == RawOutputSpace.sRGB.profileData,
                        "\(tag) the first preview is sRGB")
            let first = try note(card)
            try require(first == nil, "\(tag) sRGB at 16 bits shows no note: \(first ?? "")")
            try developChoose(card, "raw-develop-output-space",
                              RawOutputSpace.allCases.firstIndex(of: .adobeRGB) ?? 2)
            try require(!develop.previewIsReady, "\(tag) a new space starts a development")
        },
        developStep("the Adobe RGB preview; ProPhoto RGB") { develop, card in
            try require(profile(develop) == RawOutputSpace.adobeRGB.profileData,
                        "\(tag) the preview carries the Adobe RGB (1998) profile")
            let adobe = try note(card)
            try require(adobe == RawOutputSpace.adobeRGB.depthNote(eightBit: false),
                        "\(tag) Adobe RGB at 16 bits shows no note: \(adobe ?? "")")
            try developChoose(card, "raw-develop-output-space",
                              RawOutputSpace.allCases.firstIndex(of: .proPhotoRGB) ?? 3)
        },
        developStep("the ProPhoto RGB preview; the note at both depths; Open") { develop, card in
            try require(profile(develop) == RawOutputSpace.proPhotoRGB.profileData,
                        "\(tag) the preview carries the ProPhoto RGB profile")
            for (row, eightBit) in [(0, false), (1, true), (0, false), (1, true)] {
                try developChoose(card, "raw-develop-document-depth", row)
                let want = RawOutputSpace.proPhotoRGB.depthNote(eightBit: eightBit)
                let shown = try note(card)
                try require(shown == want,
                            "\(tag) depth row \(row): the note is \(want ?? "hidden"): "
                            + "\(shown ?? "hidden")")
            }
            try require(RawOutputSpace.proPhotoRGB.depthNote(eightBit: true) != nil
                        && RawOutputSpace.proPhotoRGB.depthNote(eightBit: false) == nil,
                        "\(tag) the note exists only for ProPhoto RGB into 8 bits")
            try require(develop.previewIsReady, "\(tag) the depth row starts no development")
            try developPress(card, "raw-develop-apply")
        },
    ]) {
        imported = RawDevelopWindowController.runImport(probe, descriptor: descriptor,
                                                        snapshot: snapshot)
    }
    try require(imported?.settings.outputSpace == .proPhotoRGB,
                "\(tag) Open returns ProPhoto RGB: \(String(describing: imported?.settings.outputSpace))")
    try require(imported?.highDepth == false, "\(tag) Open returns the 8-bit depth row")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    print("PASS: develop output space — the import dialog's output popup lists sRGB, Display P3, "
        + "Adobe RGB (1998) and ProPhoto RGB; choosing Adobe RGB, then ProPhoto RGB, lands a "
        + "preview carrying that space's profile; the note under the row is "
        + "RawOutputSpace.depthNote at both Document depth rows (shown only for ProPhoto RGB "
        + "into 8 bits); Open returns ProPhoto RGB")
}
