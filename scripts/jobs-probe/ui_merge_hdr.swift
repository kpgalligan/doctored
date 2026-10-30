import AppKit

// File ▸ Merge to HDR… past its open panel and Layer ▸ Merge Layers to HDR…,
// each through its exposure card. The frames are three exposures of
// one value-noise scene: its linear light at gains 1/4, 1 and 4, clipped at
// white (`uiStackFrame`'s `gain`).

private let hdrSize = (width: 400, height: 300)

/// The three exposures, darkest first by name.
private func hdrFrames(_ folder: URL) throws -> [URL] {
    try [("bracket 1", 0.25), ("bracket 2", 1.0), ("bracket 3", 4.0)].map { name, gain in
        try uiStackFrame(folder, name, width: hdrSize.width, height: hdrSize.height, gain: gain)
    }
}

/// Puts `text` in an EV field the way typing does: the field's own
/// `textDidChange(_:)`, so its delegate hears the change.
private func hdrType(_ field: NSTextField, _ text: String, in window: NSWindow) {
    field.stringValue = text
    let editor: Any = window.fieldEditor(true, for: field) ?? field
    field.textDidChange(Notification(name: NSText.didChangeNotification, object: editor))
}

private func hdrFields(_ card: NSView, _ tag: String) throws -> [NSTextField] {
    try (0..<3).map { try uiControl(card, id: "merge-hdr-ev-\($0)", as: NSTextField.self) }
}

/// The alerts a merge may show, against what was recorded since `count`:
/// nothing, or exactly `notice` under `MergeHDR.title`.
private func hdrRequireReport(_ notice: String?, since count: Int, _ tag: String) throws {
    let raised = Array(AutomationAlerts.recorded.dropFirst(count))
    if let notice {
        try require(raised.count == 1 && raised[0].messageText == MergeHDR.title
                    && raised[0].informativeText == notice,
                    "\(tag) exactly the merge's notice is shown: \(uiAlerts())")
    } else {
        try require(raised.isEmpty, "\(tag) no alert: \(uiAlerts())")
    }
}

/// "merge to hdr sheet": the action past its open panel, through the card.
func verifyMergeToHDRSheet() throws {
    let tag = "[merge to hdr sheet]"
    let folder = try uiCaseFolder("ui-merge-hdr")
    let urls = try hdrFrames(folder)
    let bytes = try urls.map { try Data(contentsOf: $0) }
    // What building this stack discloses, which the merge reports afterwards.
    let notice = try StackLoad.build(urls).notice
    let delegate = AppDelegate()
    var opened: [ImageDocument] = []
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        for document in opened { document.close() }
    }

    var before = NSDocumentController.shared.documents
    var alerts = AutomationAlerts.recorded.count
    var shown: [String] = []
    let merge = ModalSteps(title: MergeHDR.title, steps: [
        ("the card's fields", { window in
            guard let card = window.contentView else { return false }
            let fields = try hdrFields(card, tag)
            let button = try uiControl(card, id: "merge-hdr-merge", as: NSButton.self)
            shown = fields.map(\.stringValue)
            if shown.contains("") {
                // The estimate paired nothing: the card asks for every value.
                try require(!button.isEnabled, "\(tag) Merge waits for the unknown EVs")
                for (field, ev) in zip(fields, ["-2", "0", "2"]) {
                    hdrType(field, ev, in: window)
                }
            }
            try require(button.isEnabled, "\(tag) Merge is enabled with three EVs: \(shown)")
            let kept = fields[1].stringValue
            hdrType(fields[1], "", in: window)
            try require(!button.isEnabled, "\(tag) emptying an EV disables Merge")
            hdrType(fields[1], "x", in: window)
            try require(!button.isEnabled, "\(tag) a non-number keeps Merge disabled")
            hdrType(fields[1], kept, in: window)
            try require(button.isEnabled, "\(tag) the EV put back enables Merge")
            // An edited value: the darkest frame a tenth of a stop lower.
            let edited = (Double(fields[0].stringValue) ?? -2) - 0.1
            hdrType(fields[0], String(format: "%.2f", edited), in: window)
            try require(button.isEnabled, "\(tag) an edited EV keeps Merge enabled")
            return true
        }),
        ("options, then Merge", { window in
            guard let card = window.contentView else { return false }
            let align = try uiControl(card, id: "merge-hdr-align", as: NSButton.self)
            let deghost = try uiControl(card, id: "merge-hdr-deghost", as: NSButton.self)
            try require(align.state == .on && deghost.state == .off,
                        "\(tag) the card opens with Align on and Remove Ghosts off")
            align.performClick(nil)
            deghost.performClick(nil)
            try require(align.state == .off && deghost.state == .on,
                        "\(tag) Align unticked and Remove Ghosts ticked")
            let button = try uiControl(card, id: "merge-hdr-merge", as: NSButton.self)
            try uiSend(button, "\(tag) Merge")
            return true
        }),
    ])
    let steps = try uiRecorded {
        try merge.run { delegate.mergeToHDR(urls: [urls[2], urls[0], urls[1]]) }
    }
    var made = uiNewDocuments(since: before)
    opened += made
    made.forEach(uiSettle)
    try require(made.count == 1 && made.first?.doc?.isFloat == true,
                "\(tag) one new 32-bit float document: \(made.count); \(uiAlerts())")
    guard let merged = made.first?.doc else { throw ProbeFailure(description: "\(tag) no merge") }
    try require(merged.width == hdrSize.width && merged.height == hdrSize.height
                && merged.layerCount == 1,
                "\(tag) the merge is one \(hdrSize.width) × \(hdrSize.height) layer: "
                + "\(merged.width) × \(merged.height), \(merged.layerCount) layers")
    // A merge whose EVs were unknown reports no notice (`MergeHDR.merge`).
    let expected = shown.contains("") ? nil : notice
    try hdrRequireReport(expected, since: alerts, tag)
    try require(steps.isEmpty, "\(tag) the files form records no Action step: \(steps.map(\.tool))")
    let after = try urls.map { try Data(contentsOf: $0) }
    try require(after == bytes, "\(tag) the files' bytes are unchanged")

    // Cancel on the card: nothing opens.
    before = NSDocumentController.shared.documents
    alerts = AutomationAlerts.recorded.count
    let cancel = ModalSteps(title: MergeHDR.title, steps: [
        ("Cancel", { window in
            guard let card = window.contentView,
                  let button = buttons(card).first(where: { $0.keyEquivalent == "\u{1b}" })
            else { return false }
            try uiSend(button, "\(tag) Cancel")
            return true
        }),
    ])
    try cancel.run { delegate.mergeToHDR(urls: urls) }
    made = uiNewDocuments(since: before)
    opened += made
    try require(made.isEmpty, "\(tag) Cancel opens nothing")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) Cancel shows no alert: \(uiAlerts())")
    try require(!BackgroundWork.occupied, "\(tag) the worker slot is free")
    print("PASS: merge to hdr sheet — the action past its open panel shows three EV fields "
        + "(\(shown)); an empty or non-numeric field disables Merge; with an edited EV, Align off "
        + "and Remove Ghosts on, Merge opens one new 32-bit float document, shows "
        + (expected == nil ? "no alert" : "exactly the stack's notice")
        + " and leaves the files' bytes unchanged; Cancel on the card opens nothing")
}

/// "merge layers to hdr sheet": the layers form, and its menu rule.
func verifyMergeLayersToHDRSheet() throws {
    let tag = "[merge layers to hdr sheet]"
    let folder = try uiCaseFolder("ui-merge-layers-hdr")
    let urls = try hdrFrames(folder)
    let document = try ImageDocument.openStack(urls, name: nil).document
    uiSettle(document)
    var opened: [ImageDocument] = []
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        for made in opened { made.close() }
        document.close()
    }
    let controller = try editor(document)
    guard let base = document.doc, base.layerCount == 3, !base.isHighDepth else {
        throw ProbeFailure(description: "\(tag) an 8-bit document holding three exposures")
    }
    controller.setSelectedLayers(.single(1))
    try require(!uiValidates(controller, "mergeLayersToHDR:"),
                "\(tag) Merge Layers to HDR… does not validate with one layer selected")
    controller.setSelectedLayers(LayerSelection(primary: 2, others: [0, 1]))
    try require(uiValidates(controller, "mergeLayersToHDR:"),
                "\(tag) Merge Layers to HDR… validates with three raster layers selected")

    // The float rule, where only it can decide: two selected raster layers of
    // a float document pass Auto-Align's test, and the item is still off.
    do {
        let float = try floatFixture()
        defer { float.close() }
        let floatController = try editor(float)
        floatController.newLayer(nil)
        try require(float.doc?.layerCount == 2 && float.doc?.isFloat == true,
                    "\(tag) the float document has two layers: \(float.doc?.layerCount ?? 0)")
        floatController.setSelectedLayers(LayerSelection(primary: 1, others: [0]))
        try require(floatController.canAutoAlignLayers,
                    "\(tag) two selected raster layers of a float document pass Auto-Align's test")
        try require(!uiValidates(floatController, "mergeLayersToHDR:"),
                    "\(tag) Merge Layers to HDR… does not validate in a 32-bit float document")
    }

    let entries = document.history.entries.count
    let before = NSDocumentController.shared.documents
    let alerts = AutomationAlerts.recorded.count
    var shown: [String] = []
    let merge = ModalSteps(title: "Merge Layers to HDR", steps: [
        ("Merge", { window in
            guard let card = window.contentView else { return false }
            let fields = try hdrFields(card, tag)
            shown = fields.map(\.stringValue)
            if shown.contains("") {
                for (field, ev) in zip(fields, ["-2", "0", "2"]) { hdrType(field, ev, in: window) }
            }
            let button = try uiControl(card, id: "merge-hdr-merge", as: NSButton.self)
            try uiSend(button, "\(tag) Merge")
            return true
        }),
    ])
    let steps = try uiRecorded {
        try merge.run { controller.mergeLayersToHDR(nil) }
    }
    let made = uiNewDocuments(since: before)
    opened += made
    made.forEach(uiSettle)
    try require(made.count == 1 && made.first?.doc?.isFloat == true,
                "\(tag) one new 32-bit float document: \(made.count); \(uiAlerts())")
    try require(made.first?.doc?.width == hdrSize.width
                && made.first?.doc?.height == hdrSize.height,
                "\(tag) the merge has the source's canvas")
    try hdrRequireReport(nil, since: alerts, tag)
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true && !document.isDocumentEdited,
                "\(tag) the source document is not edited: same handle, no step, not dirty")
    try require(steps.isEmpty, "\(tag) nothing is recorded: \(steps.map(\.tool))")
    try require(!BackgroundWork.occupied, "\(tag) the worker slot is free")
    print("PASS: merge layers to hdr sheet — Merge Layers to HDR… validates with three selected "
        + "raster layers of an 8-bit document and not with one, and not in a float document whose "
        + "two selected layers pass Auto-Align's test; its card (EVs \(shown)) and Merge open one "
        + "new 32-bit float document with no alert, and the source keeps its handle, has no undo "
        + "step, is not dirty and records nothing")
}
