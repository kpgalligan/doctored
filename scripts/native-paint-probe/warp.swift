import AppKit

private func warpSheets(_ editor: EditorViewController) -> [PersistedWarpSheetController] {
    (editor.presentedViewControllers ?? []).compactMap { $0 as? PersistedWarpSheetController }
}
private func warpSheet(_ editor: EditorViewController, after count: Int) throws -> PersistedWarpSheetController {
    try waitUntil({ warpSheets(editor).count > count }, "Warp sheet did not open")
    guard let sheet = warpSheets(editor).last else { throw DoctoredCoreError(message: "Warp sheet missing") }
    _ = sheet.view; pump(); return sheet
}
private func warpControl<T: NSView>(_ sheet: PersistedWarpSheetController,
                                    _ id: String) throws -> T {
    let wanted = NSUserInterfaceItemIdentifier(id)
    guard let found = descendants(sheet.view).compactMap({ $0 as? T })
        .first(where: { $0.identifier == wanted || $0.accessibilityIdentifier() == id }) else {
        throw DoctoredCoreError(message: "Warp sheet is missing \(id)")
    }
    return found
}
private func choose(_ popup: NSPopUpButton, index: Int) throws {
    popup.selectItem(at: index)
    guard let action = popup.action else { throw DoctoredCoreError(message: "Warp control has no action") }
    _ = NSApp.sendAction(action, to: popup.target, from: popup)
    pump()
}
private func showWarpSheet(_ editor: EditorViewController) throws -> PersistedWarpSheetController {
    let count = warpSheets(editor).count
    let item = NSMenuItem(title: "Warp…", action: #selector(EditorViewController.showPersistedWarp(_:)),
                          keyEquivalent: "")
    try require(editor.validateUserInterfaceItem(item),
                "Layer > Warp… is disabled: direct=\(editor.validatePersistedWarpItem()) sheet=\(editor.view.window?.attachedSheet != nil) document=\(editor.document != nil) source=\(editor.document?.doc != nil) layer=\(editor.document?.activeLayerIndex ?? -1) info=\(editor.document?.doc?.layerInfo(editor.document?.activeLayerIndex ?? -1) != nil)")
    editor.showPersistedWarp(nil)
    return try warpSheet(editor, after: count)
}

private func warpSheetTrial(_ depth: String) throws -> [String: Any] {
    let opened = depth == "16" ? try openDocument(width: 40, height: 28)
                               : try openByteDocument(width: 40, height: 28)
    let (document, editor, source) = opened
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    let history = document.history.entries.count
    let initialDirty = document.isDocumentEdited
    let before = try geometryDigest(source)
    var expectedField = PersistedWarpField.identity()
    expectedField.displacements[0] = [0.35, -0.27]
    expectedField.displacements[4] = [0.48, -0.11]
    expectedField.filter = .bilinear
    let request = PersistedWarpRequest.set(layer: 0, target: .pixels, field: expectedField)
    guard let expected = try source.editingWarp(request: request.jsonRequest()) else {
        throw DoctoredCoreError(message: "Numeric Warp reference refused")
    }
    let expectedDigest = try geometryDigest(expected)
    ActionRecorder.shared.start()
    let sheet = try showWarpSheet(editor)
    let target: NSPopUpButton = try warpControl(sheet, "warp-target")
    let columns: NSPopUpButton = try warpControl(sheet, "warp-columns")
    let rows: NSPopUpButton = try warpControl(sheet, "warp-rows")
    let node: NSPopUpButton = try warpControl(sheet, "warp-node")
    let dx: NSTextField = try warpControl(sheet, "warp-dx")
    let dy: NSTextField = try warpControl(sheet, "warp-dy")
    let filter: NSPopUpButton = try warpControl(sheet, "warp-filter")
    let apply: NSButton = try warpControl(sheet, "warp-apply")
    try require(target.indexOfSelectedItem == 0 && columns.indexOfSelectedItem == 1
        && rows.indexOfSelectedItem == 1 && node.numberOfItems == 9,
        "Warp default 3×3 pixel target was not presented")
    dx.stringValue = "0.35"; dy.stringValue = "-0.27"
    try choose(node, index: 4)
    dx.stringValue = "0.48"; dy.stringValue = "-0.11"
    try choose(filter, index: 0)
    apply.performClick(nil)
    try waitUntil({ document.doc !== source }, "Warp Apply did not publish")
    sheet.dismiss(nil); pump()
    let recorded = ActionRecorder.shared.stop()
    guard let after = document.doc else { throw DoctoredCoreError(message: "Warp Apply lost document") }
    let digest = try geometryDigest(after)
    try require(digest == expectedDigest, "Warp sheet differs from numeric core output")
    try require(document.history.entries.count == history + 1, "Warp Apply did not create one history step")
    try require(document.isDocumentEdited, "Warp Apply did not dirty the document")
    try require(recorded.count == 1 && recorded[0].tool == "warp_layer",
                "Warp Apply did not record one warp_layer step")
    if depth == "16" {
        try require(try geometryRaw(after, layer: 0) == geometryRaw(expected, layer: 0),
                    "Warp sheet changed native u16 codes")
    }
    let loaded = try showWarpSheet(editor)
    let loadedNode: NSPopUpButton = try warpControl(loaded, "warp-node")
    let loadedDX: NSTextField = try warpControl(loaded, "warp-dx")
    let loadedDY: NSTextField = try warpControl(loaded, "warp-dy")
    let loadedFilter: NSPopUpButton = try warpControl(loaded, "warp-filter")
    try require(loadedDX.stringValue == "0.35" && loadedDY.stringValue == "-0.27"
        && loadedFilter.indexOfSelectedItem == 0,
        "Warp sheet did not reopen exact node/filter")
    try choose(loadedNode, index: 4)
    try require(loadedDX.stringValue == "0.48" && loadedDY.stringValue == "-0.11",
                "Warp sheet did not retain the second node")
    let cancel: NSButton = try warpControl(loaded, "warp-cancel")
    let cancelDirty = document.isDocumentEdited
    cancel.performClick(nil); loaded.dismiss(nil); pump()
    try require(try geometryDigest(document.doc ?? source) == digest
        && document.history.entries.count == history + 1
        && document.isDocumentEdited == cancelDirty,
        "Warp Cancel changed document/history/dirty state")
    _ = try geometryReplay(recorded, on: document, expectedDigest: digest)
    ActionRecorder.shared.start()
    let beforeClearHistory = document.history.entries.count
    let clearSheet = try showWarpSheet(editor)
    let clear: NSButton = try warpControl(clearSheet, "warp-clear")
    clear.performClick(nil); clearSheet.dismiss(nil); pump()
    try require(try geometryDigest(document.doc ?? source) == before,
                "Remove Warp did not restore the original native document")
    let clearSteps = ActionRecorder.shared.stop()
    try require(document.history.entries.count == beforeClearHistory + 1
        && document.isDocumentEdited
        && clearSteps.count == 1 && clearSteps[0].tool == "warp_layer",
        "Remove Warp lacked one history/Action/dirty change")
    let count = document.history.entries.count
    let identityDirty = document.isDocumentEdited
    ActionRecorder.shared.start()
    let identitySheet = try showWarpSheet(editor)
    let identityApply: NSButton = try warpControl(identitySheet, "warp-apply")
    identityApply.performClick(nil); identitySheet.dismiss(nil); pump()
    try require(document.history.entries.count == count
        && document.isDocumentEdited == identityDirty
        && (try geometryDigest(document.doc ?? source)) == before,
        "Identity Warp added history/dirty state or changed bytes")
    try require(ActionRecorder.shared.stop().isEmpty, "Identity Warp recorded a step")
    let bakeSource = document.doc ?? source
    guard let set = try bakeSource.editingWarp(request: request.jsonRequest()) else {
        throw DoctoredCoreError(message: "Bake setup refused")
    }
    document.applyEdit("Set Warp", record: .notACommand, allowWarpChange: true) { _ in set }
    let bakedSamples = try geometryDigest(document.doc ?? set)
    let beforeBakeHistory = document.history.entries.count
    ActionRecorder.shared.start()
    let bakeSheet = try showWarpSheet(editor)
    let bake: NSButton = try warpControl(bakeSheet, "warp-bake")
    bake.performClick(nil); bakeSheet.dismiss(nil); pump()
    let baked = document.doc ?? set
    try require((try baked.warpInfo(0))?["field"] is NSNull,
                "Bake Warp retained a field")
    try require(try geometryDigest(baked) != bakedSamples,
                "Bake Warp kept the WARP table")
    let bakeSteps = ActionRecorder.shared.stop()
    try require(document.history.entries.count == beforeBakeHistory + 1
        && document.isDocumentEdited
        && bakeSteps.count == 1 && bakeSteps[0].tool == "warp_layer",
        "Bake Warp lacked one history/Action/dirty change")
    return ["depth": depth, "native_digest": digest, "history_delta": 1,
            "nodes_edited": 2, "sampler": "bilinear", "reopen_exact": true,
            "cancel_clean": true, "clear_exact": true, "identity_noop": true,
            "bake_removed_field": true, "recorded_replay_exact": true,
            "initial_dirty": initialDirty, "button_history_dirty_actions": true]
}

private func warpStaleSheetTrial() throws -> [String: Any] {
    let (document, editor, _) = try openDocument(width: 40, height: 28)
    defer { document.close(); pump() }
    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Warp stale-sheet presenter unavailable")
    }
    for cause in ["document", "target", "selection"] {
        if cause == "target" {
            _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "reveal_all"], document)
        }
        let sheet = try showWarpSheet(editor)
        let dx: NSTextField = try warpControl(sheet, "warp-dx")
        let apply: NSButton = try warpControl(sheet, "warp-apply")
        dx.stringValue = "0.35"
        app.errors.removeAll()
        let before = document.doc
        switch cause {
        case "document":
            _ = try geometryCall("set_layer_label", ["layer": 0, "label": "red"], document)
        case "target":
            editor.setPaintTarget(.mask)
            try require(document.doc === before, "Paint target replaced the Warp source")
        default:
            _ = try geometryCall("select_rect", ["x": 2, "y": 3, "width": 6, "height": 7], document)
            try require(document.doc === before, "Selection replaced the Warp source")
        }
        let expectedHistory = document.history.entries.count
        let changed = document.doc
        apply.performClick(nil); pump()
        try require(document.doc === changed && document.history.entries.count == expectedHistory,
                    "Stale \(cause) sheet published a Warp edit")
        try require(app.errors.contains { $0.contains("warp sheet's document, target or selection changed") },
                    "Stale \(cause) sheet did not report its named refusal: \(app.errors)")
        sheet.dismiss(nil); pump()
        if cause == "target" { editor.setPaintTarget(.layer) }
    }
    return ["document": true, "target": true, "selection": true,
            "no_stale_publication": true]
}

private func warpCancellationTrial() throws -> [String: Any] {
    let (document, editor, source) = try openDocument(width: 896, height: 672)
    defer { document.close(); pump() }
    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Warp cancellation presenter unavailable")
    }
    let sheet = try showWarpSheet(editor)
    let dx: NSTextField = try warpControl(sheet, "warp-dx")
    let apply: NSButton = try warpControl(sheet, "warp-apply")
    dx.stringValue = "0.35"
    let history = document.history.entries.count
    let dirty = document.isDocumentEdited
    app.errors.removeAll()
    var cancelled = false, progress = 0.0, scratch: UInt64 = 0
    let timer = Timer(timeInterval: 0.001, repeats: true) { timer in
        guard let panel = NSApp.windows.first(where: { $0.title == "Warp Layer" && $0.isVisible }),
              let content = panel.contentView,
              let indicator = descendants(content).compactMap({ $0 as? NSProgressIndicator }).first,
              indicator.doubleValue > 0,
              let button = descendants(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Cancel" }) else { return }
        progress = indicator.doubleValue
        scratch = BackgroundWork.scratchEstimate
        cancelled = true
        timer.invalidate()
        button.performClick(nil)
    }
    RunLoop.main.add(timer, forMode: .common)
    RunLoop.main.add(timer, forMode: .modalPanel)
    apply.performClick(nil)
    timer.invalidate(); pump()
    try require(cancelled && progress > 0 && scratch > 0,
                "Warp work did not expose started owned progress before Cancel")
    try require(document.doc === source && document.history.entries.count == history
        && document.isDocumentEdited == dirty && !BackgroundWork.occupied,
        "Cancelled Warp published or retained active work")
    try require(app.errors.isEmpty, "Cancelled Warp presented an error: \(app.errors)")
    return ["started_progress": progress, "owned_scratch_during": scratch,
            "cancelled_via_actual_button": true, "no_publication": true]
}

private func warpTightNoopTrial(_ depth: String) throws -> [String: Any] {
    let (document, _, source) = depth == "16"
        ? try openDocument(width: 40, height: 28)
        : try openByteDocument(width: 40, height: 28)
    defer { document.close(); pump() }
    let projection = UInt64(source.width) * UInt64(source.height) * 24
    func underPressure(_ check: () throws -> Void) throws {
        _ = try RasterResources.shared.report(evict: true)
        let available = try RasterResources.shared.admit(scratch: 0)
        try require(available > projection + 4096,
                    "Owned Warp no-op pressure fixture lacks headroom")
        let lease = try SelectionWorkBudget.reservePreview(bytes: available - 4096,
                                                            outputBytes: 1)
        try withExtendedLifetime(lease) {
            let remaining = try RasterResources.shared.admit(scratch: 0)
            try require(remaining < projection && SelectionWorkBudget.contains(lease.id),
                        "Owned Warp no-op pressure reservation was not active")
            try check()
            try require(SelectionWorkBudget.contains(lease.id),
                        "Warp no-op consumed its unrelated pressure reservation")
        }
    }
    let emptyRequests: [PersistedWarpRequest] = [
        .set(layer: 0, target: .pixels, field: .identity()),
        .clear(layer: 0), .bake(layer: 0)
    ]
    let initialHistory = document.history.entries.count
    let initialDirty = document.isDocumentEdited
    try underPressure {
        for request in emptyRequests {
            let plan = try source.warpEditPlan(request: request.jsonRequest())
            try require(try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes") == 0,
                        "Canonical Warp no-op has a nonzero core plan")
            let result = try PersistedWarpWork(source: source, request: request)
                .wait(in: document, title: "Warp No-op")
            try require(result == nil && !BackgroundWork.occupied,
                        "Canonical Warp no-op entered background/projection work")
        }
    }
    try require(document.doc === source && document.history.entries.count == initialHistory
        && document.isDocumentEdited == initialDirty,
        "Canonical Warp no-op changed the saved document")

    var field = PersistedWarpField.identity()
    field.displacements[0] = [0.35, -0.27]
    let set = PersistedWarpRequest.set(layer: 0, target: .pixels, field: field)
    guard let attached = try source.editingWarp(request: set.jsonRequest()) else {
        throw DoctoredCoreError(message: "Warp no-op re-edit setup refused")
    }
    document.applyEdit("Warp Fixture", record: .notACommand, allowWarpChange: true) { _ in attached }
    try require(document.doc === attached, "Warp no-op re-edit setup did not publish")
    let history = document.history.entries.count
    let dirty = document.isDocumentEdited
    try underPressure {
        let plan = try attached.warpEditPlan(request: set.jsonRequest())
        try require(try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes") == 0,
                    "Repeated Warp field has a nonzero core plan")
        let result = try PersistedWarpWork(source: attached, request: set)
            .wait(in: document, title: "Warp No-op")
        try require(result == nil && !BackgroundWork.occupied,
                    "Repeated Warp field entered background/projection work")
    }
    try require(document.doc === attached && document.history.entries.count == history
        && document.isDocumentEdited == dirty,
        "Repeated Warp field changed history or dirty state")
    return ["depth": depth, "empty_ops": 3, "repeated_set": true,
            "projection_admission_bypassed": true, "history_dirty_unchanged": true]
}

private func warpPublicationRefusalTrial(_ depth: String) throws -> [String: Any] {
    let (document, _, source) = depth == "16"
        ? try openDocument(width: 40, height: 28)
        : try openByteDocument(width: 40, height: 28)
    defer { document.close(); pump() }
    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Warp publication presenter unavailable")
    }
    var field = PersistedWarpField.identity()
    field.displacements[0] = [0.35, -0.27]
    let request = PersistedWarpRequest.set(layer: 0, target: .pixels, field: field)
    guard let warped = try source.editingWarp(request: request.jsonRequest()) else {
        throw DoctoredCoreError(message: "Warp publication fixture refused")
    }
    document.applyEdit("Warp Fixture", record: .notACommand, allowWarpChange: true) { _ in warped }
    try require(document.doc === warped, "Warp publication fixture did not publish")
    let history = document.history.entries.count
    let dirty = document.isDocumentEdited
    let digest = try geometryDigest(warped)
    app.errors.removeAll()
    document.applyEdit("Incompatible Apply", record: .notACommand) { _ in source }
    try require(app.errors.contains { $0.localizedCaseInsensitiveContains("warp") },
                "applyEdit did not name Warp loss: \(app.errors)")
    try require(document.doc === warped && document.history.entries.count == history
        && document.isDocumentEdited == dirty, "applyEdit replaced the live Warp")
    app.errors.removeAll()
    document.beginLiveEdit()
    document.updateLiveEdit(source)
    document.endLiveEdit("Incompatible Live", record: .notACommand)
    try require(app.errors.contains { $0.localizedCaseInsensitiveContains("warp") },
                "updateLiveEdit did not name Warp loss: \(app.errors)")
    try require(document.doc === warped && document.history.entries.count == history
        && document.isDocumentEdited == dirty && !document.isLiveEditing,
        "updateLiveEdit replaced the live Warp")
    do {
        try AgentServer.shared.performGroupedEdit(document, "Incompatible Grouped") { _ in source }
        throw DoctoredCoreError(message: "performGroupedEdit accepted Warp loss")
    } catch let error as AgentServer.ToolError {
        try require(error.message.localizedCaseInsensitiveContains("warp"),
                    "performGroupedEdit lacked named Warp refusal: \(error)")
    }
    try require(document.doc === warped && document.history.entries.count == history
        && document.isDocumentEdited == dirty && (try geometryDigest(warped)) == digest,
        "performGroupedEdit replaced the live Warp")
    return ["depth": depth, "apply_edit": true, "live_edit": true,
            "grouped_edit": true, "history_dirty_native_unchanged": true]
}

func persistedWarpSheetUI() throws -> [String: Any] {
    let byte = try warpSheetTrial("8")
    let typed = try warpSheetTrial("16")
    let tightNoops = try [warpTightNoopTrial("8"), warpTightNoopTrial("16")]
    let publication = try [warpPublicationRefusalTrial("8"), warpPublicationRefusalTrial("16")]
    let stale = try warpStaleSheetTrial()
    let cancellation = try warpCancellationTrial()
    let (floatDocument, floatEditor, _) = try openFloatDocument()
    defer { floatDocument.close(); pump() }
    let item = NSMenuItem(title: "Warp…", action: #selector(EditorViewController.showPersistedWarp(_:)),
                          keyEquivalent: "")
    try require(!floatEditor.validateUserInterfaceItem(item), "Float Warp menu should be disabled")
    guard let app = NSApp as? ProbeApplication else {
        throw DoctoredCoreError(message: "Float Warp selector probe unavailable")
    }
    let floatSource = floatDocument.doc
    let floatHistory = floatDocument.history.entries.count
    let floatDirty = floatDocument.isDocumentEdited
    app.errors.removeAll()
    floatEditor.showPersistedWarp(nil); pump()
    try require(app.errors.contains { $0.contains("Warp") && $0.contains("32-bit float") },
                "Float Warp selector lacked named refusal: \(app.errors)")
    try require(warpSheets(floatEditor).isEmpty && floatDocument.doc === floatSource
        && floatDocument.history.entries.count == floatHistory
        && floatDocument.isDocumentEdited == floatDirty,
        "Float Warp selector opened a sheet or changed document state")
    return ["case": "persisted-warp-sheet-ui", "trials": [byte, typed],
            "tight_budget_noops": tightNoops,
            "publication_refusals": publication,
            "stale_sheets": stale, "cancellation": cancellation,
            "float_menu_disabled": true, "float_selector_refused": true]
}
