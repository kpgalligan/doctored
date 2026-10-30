import AppKit

// The Develop RAW window's lens line (58f5cc9c, b7114688) through its own
// controls, on `camera/camera.NEF` from the shared fixture folder: a file
// with no decoder lens profile that Lensfun matches. The line under Enable
// Profile Corrections names the Lensfun profile and the database; the
// checkbox changes the development both ways; and a radial local's Apply
// leaves the pixels outside its mask as they were with the profile on.
//
// The helpers below are this file's own copies: `ui_develop.swift` keeps its
// copies private, and `scripts/raw-local-probe` is a separate probe set.

/// A camera file opened as its retained 16-bit development.
/// Master copy: `localOpen` in `scripts/raw-local-probe/main.swift` (via
/// `developOpen` in `ui_develop.swift`).
private func lensDevelopOpen(_ url: URL) throws -> RasterDocument {
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
/// Master copy: `LocalUI.document` in `scripts/raw-local-probe/ui.swift` (via
/// `developDocument` in `ui_develop.swift`).
private func lensDevelopDocument(_ source: RasterDocument) throws -> ImageDocument {
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

/// The Develop window's own controller, from its Apply button's target.
/// Master copy: `developController` in `ui_develop.swift`.
private func lensDevelopController(_ window: NSWindow) -> RawDevelopWindowController? {
    guard let card = window.contentView,
          let apply = try? uiControl(card, id: "raw-develop-apply", as: NSButton.self) else {
        return nil
    }
    return apply.target as? RawDevelopWindowController
}

/// One step that waits for the window's preview to be ready, then runs.
/// Master copy: `LocalUIDriver` in `scripts/raw-local-probe/ui.swift` (via
/// `developStep` in `ui_develop.swift`).
private func lensDevelopStep(_ name: String,
                             _ action: @escaping (RawDevelopWindowController, NSView) throws -> Void)
    -> ModalSteps.Step {
    (name, { window in
        guard let controller = lensDevelopController(window), controller.previewIsReady,
              let card = window.contentView else { return false }
        try action(controller, card)
        return true
    })
}

/// One press, one drag and one release on the Develop canvas's gesture view,
/// between two points given as fractions of the displayed frame.
/// Master copy: `LocalUI.gesture` / `LocalUI.drag` in
/// `scripts/raw-local-probe/ui.swift`.
private func lensDevelopDrag(_ controller: RawDevelopWindowController,
                             from: CGPoint, to: CGPoint) throws {
    guard let gesture = controller.localGesture, let window = gesture.window,
          let rect = gesture.mapping()?.rect else {
        throw ProbeFailure(description: "the Develop canvas shows no picture to drag on")
    }
    try require(gesture.enabled, "the source gesture is enabled for the displayed frame")
    func event(_ type: NSEvent.EventType, _ fraction: CGPoint) throws -> NSEvent {
        let point = CGPoint(x: rect.minX + fraction.x * rect.width,
                            y: rect.minY + fraction.y * rect.height)
        guard let event = NSEvent.mouseEvent(
            with: type, location: gesture.convert(point, to: nil), modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 1, clickCount: 1, pressure: 1) else {
            throw ProbeFailure(description: "could not create the drag")
        }
        return event
    }
    gesture.mouseDown(with: try event(.leftMouseDown, from))
    gesture.mouseDragged(with: try event(.leftMouseDragged, to))
    gesture.mouseUp(with: try event(.leftMouseUp, to))
}

private func lensDevelopBytes(_ controller: RawDevelopWindowController) -> Data? {
    controller.workflowPanel?.canvas.preview?.afterDisplay.dataProvider?.data as Data?
}

private func lensSamples16(_ doc: RasterDocument?) throws -> [UInt16] {
    guard let image = doc?.precision?.layerSource(0) else {
        throw ProbeFailure(description: "the development has no 16-bit layer")
    }
    return try image.withRGBA16 { Array($0) }
}

/// "develop lens line": the lens line, the checkbox both ways, and a radial
/// local that leaves the rest of the frame as it was.
func verifyDevelopLensLine() throws {
    let tag = "[develop lens line]"
    let source = try lensDevelopOpen(try uiFixture("camera/camera.NEF"))
    let document = try lensDevelopDocument(source)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let alerts = AutomationAlerts.recorded.count
    var line = ""
    var lens = ""
    var on: Data?
    var off: Data?
    let steps = try uiRecorded {
        try ModalSteps(title: "Develop RAW", timeout: 120, steps: [
            lensDevelopStep("the lens line; untick") { develop, card in
                guard let profile = develop.lensfunProfile else {
                    throw ProbeFailure(description: "\(tag) the recipe holds no Lensfun profile")
                }
                lens = profile.lens
                line = try uiControl(card, id: "raw-develop-lens-profile", as: NSTextField.self)
                    .stringValue
                try require(line.contains("corrected by the Lensfun profile \(lens) "
                                          + "(Lensfun database v0.3.4)"),
                            "\(tag) the line names the Lensfun profile and the database: \(line)")
                guard let row = develop.lensRow, let box = row.checkbox else {
                    throw ProbeFailure(description: "\(tag) the window has no lens row")
                }
                try require(row.isEnabled && box.isEnabled && box.state == .on,
                            "\(tag) Enable Profile Corrections is enabled and on")
                on = lensDevelopBytes(develop)
                try require(on != nil, "\(tag) the window shows a preview")
                box.state = .off
                try uiSend(box, "\(tag) untick Enable Profile Corrections")
                try require(!develop.previewIsReady, "\(tag) the untick starts a development")
            },
            lensDevelopStep("the uncorrected preview; tick") { develop, _ in
                off = lensDevelopBytes(develop)
                try require(off != nil && off != on, "\(tag) the untick changes the preview")
                guard let box = develop.lensRow?.checkbox else {
                    throw ProbeFailure(description: "\(tag) the lens checkbox went away")
                }
                box.state = .on
                try uiSend(box, "\(tag) tick Enable Profile Corrections")
                try require(!develop.previewIsReady, "\(tag) the tick starts a development")
            },
            lensDevelopStep("the corrected preview; the Local Masks tab") { develop, card in
                let again = lensDevelopBytes(develop)
                try require(again != nil && again != off, "\(tag) the tick changes the preview back")
                let tabs = try uiControl(card, id: "raw-develop-section", as: NSSegmentedControl.self)
                tabs.selectedSegment = 1
                try uiSend(tabs, "\(tag) raw-develop-section")
            },
            lensDevelopStep("New Radial") { develop, card in
                try require(develop.localSession?.state.nodes.isEmpty == true,
                            "\(tag) the development starts with no local")
                try uiSend(try uiControl(card, id: "raw-local-new-radial", as: NSButton.self),
                           "\(tag) raw-local-new-radial")
            },
            lensDevelopStep("the radial over the centre") { develop, _ in
                try lensDevelopDrag(develop, from: CGPoint(x: 0.38, y: 0.38),
                                    to: CGPoint(x: 0.62, y: 0.62))
            },
            lensDevelopStep("Exposure +1") { develop, card in
                try require(develop.localSession?.state.nodes.count == 1,
                            "\(tag) the drag made one radial local")
                guard let slider = try uiControl(card, id: "raw-local-exposure",
                                                 as: CardControlRow.self).slider else {
                    throw ProbeFailure(description: "\(tag) raw-local-exposure has no slider")
                }
                slider.doubleValue = 1
                try uiSend(slider, "\(tag) raw-local-exposure")
            },
            lensDevelopStep("Apply") { _, card in
                try uiSend(try uiControl(card, id: "raw-develop-apply", as: NSButton.self),
                           "\(tag) Apply")
            },
        ]).run { controller.redevelopRaw(nil) }
        try pumpUntil({ !BackgroundWork.occupied && DocumentJobs.active == nil }, seconds: 60)
    }
    guard let applied = document.doc, applied !== source else {
        throw ProbeFailure(description: "\(tag) Apply published nothing: \(uiAlerts())")
    }
    try require(document.undoManager?.canUndo == true && steps.count == 1
                && steps[0].tool == "edit_raw_development",
                "\(tag) one recorded edit_raw_development step: \(steps.map(\.tool))")
    let before = try lensSamples16(source), after = try lensSamples16(applied)
    let (width, height) = (source.width, source.height)
    try require(before.count == after.count && before.count == width * height * 4,
                "\(tag) the published layer keeps the frame's size")
    let strip = max(1, Int(Double(height) * 0.08))
    var worst = 0
    for y in Array(0..<strip) + Array((height - strip)..<height) {
        for i in (y * width * 4)..<((y + 1) * width * 4) {
            worst = max(worst, abs(Int(after[i]) - Int(before[i])))
        }
    }
    try require(worst <= 1, "\(tag) the top and bottom 8% strips stay within 1 code: \(worst)")
    var lifted = 0.0, was = 0.0
    for y in (height * 45 / 100)..<(height * 55 / 100) {
        for x in (width * 45 / 100)..<(width * 55 / 100) {
            let i = (y * width + x) * 4
            for c in 0..<3 { lifted += Double(after[i + c]); was += Double(before[i + c]) }
        }
    }
    try require(lifted > was, "\(tag) the centre is brighter under the radial: \(lifted) vs \(was)")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(uiAlerts())")
    document.undoManager?.undo()
    try require(document.doc === source && document.undoManager?.canUndo != true,
                "\(tag) one undo restores the development")
    print("PASS: develop lens line — on camera.NEF Redevelop RAW's line reads \"\(line)\" with "
        + "Enable Profile Corrections enabled and on; unticking changes the preview and ticking "
        + "changes it back; a radial over the centre with Exposure +1 is one recorded "
        + "edit_raw_development step whose top and bottom 8% strips stay within \(worst) code of the "
        + "development and whose centre is brighter, one undo (58f5cc9c, b7114688)")
}
