import AppKit
import Darwin

struct ProbeFailure: Error, CustomStringConvertible { let description: String }
func require(_ value: @autoclosure () -> Bool, _ message: String) throws {
    if !value() { throw ProbeFailure(description: message) }
}

func fixture(width: Int = 160, height: Int = 120) throws -> ImageDocument {
    var pixels = [UInt8](repeating: 255, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let i = (y * width + x) * 4
            pixels[i] = UInt8((x * 11 + y * 3) % 256)
            pixels[i + 1] = UInt8((x * 5 + y * 7) % 256)
            pixels[i + 2] = UInt8((x ^ y) % 256)
        }
    }
    guard let image = RasterImage.from(rgba: pixels, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "History fixture allocation")
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

func rename(_ document: ImageDocument, _ name: String) {
    withUndoGroup(document.undoManager) {
        document.applyEdit(name, record: .notACommand) { try $0.withLayerName(0, name) }
    }
}

func descendant<T: NSView>(_ type: T.Type, in view: NSView) -> T? {
    if let found = view as? T { return found }
    for child in view.subviews { if let found = descendant(type, in: child) { return found } }
    return nil
}

func verifyLongHistoryAndPanel() throws {
    let document = try fixture()
    defer { document.close() }
    try document.setHistoryBudget(512 * 1024)
    guard let initial = document.doc else { throw ProbeFailure(description: "Missing initial handle") }
    for index in 1...40 {
        rename(document, "Edit \(index)")
        if index == 20 { document.updateChangeCount(.changeCleared) }
    }
    try require(document.history.entries.count == 41 && document.history.cursor == 40,
                "More than 24 edits retained as 40 independently enumerable transitions")
    try require(document.history.incrementalBytes < 512 * 1024,
                "Shared-pixel property edits fit the independently selected 512KiB history budget")
    weak var discardable = document.history.entries[5].state.document
    let firstID = document.history.entries[0].id
    let latestID = document.history.entries[40].id
    document.showHistoryPanel()
    guard let window = document.historyWindowController?.window, let content = window.contentView,
          let table = descendant(NSTableView.self, in: content) else {
        throw ProbeFailure(description: "Actual History panel/table missing")
    }
    window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    try require(table.numberOfRows == 41, "History panel enumerates actual states")
    table.selectRowIndexes(IndexSet(integer: 20), byExtendingSelection: false)
    _ = table.sendAction(table.action, to: table.target)
    try require(document.history.cursor == 20 && document.doc?.layerInfo(0)?.name == "Edit 20",
                "Actual table selection navigates through Undo to the selected state")
    try require(!document.isDocumentEdited && document.windowControllers.first?.window?.isDocumentEdited == false,
                "Saved history state restores clean document and window")
    try document.navigateHistory(toID: firstID)
    try require(document.doc === initial && document.isDocumentEdited,
                "Undo past 24 edits reaches original handle and is dirty relative to later save")
    try document.navigateHistory(toID: latestID)
    try require(document.history.cursor == 40 && document.doc?.layerInfo(0)?.name == "Edit 40",
                "History navigation redoes the complete retained sequence")
    print("PASS: 40 edits in measured budget, actual History panel navigation and exact saved-state dirty markers")

    try document.setHistoryBudget(1)
    try require(descendant(NSPopUpButton.self, in: content)?.titleOfSelectedItem?.hasPrefix("Custom") == true,
                "History panel identifies a custom byte budget instead of showing an unrelated preset")
    try require(document.history.entries.count == 2 && document.history.cursor == 1
                && document.history.isOverBudget && document.history.evictedCount == 39,
                "Budget eviction retains exactly the explicit sole-Undo exception")
    try require(discardable == nil, "Evicted full-state wrapper released; Undo callbacks retain IDs only")
    document.undoManager?.undo()
    try require(document.doc?.layerInfo(0)?.name == "Edit 39" && document.undoManager?.canUndo != true,
                "Real Undo stack has no hidden evicted entries")
    document.undoManager?.redo()
    try require(document.doc?.layerInfo(0)?.name == "Edit 40", "Sole retained transition redoes correctly")
    document.undoManager?.undo()
    rename(document, "New branch")
    try require(document.undoManager?.canRedo != true && document.history.cursor == 1,
                "Editing an earlier state discards redo branch in manager and model")
    print("PASS: budget eviction releases retained owners, sole-edit exception stays reversible and redo branching is correct")
}

func verifyGroupedAndSelectionHistory() throws {
    let document = try fixture(width: 80, height: 60)
    defer { document.close() }
    guard let original = document.doc else { throw ProbeFailure(description: "Missing grouped source") }
    withUndoGroup(document.undoManager) {
        document.applyEdit("First in group", record: .notACommand) { try $0.withLayerName(0, "First") }
        document.applyEdit("Second in group", record: .notACommand) { try $0.withLayerName(0, "Second") }
    }
    try require(document.history.entries.count == 2, "Nested central edits form one real outer-group history row")
    document.undoManager?.undo()
    try require(document.doc === original && document.undoManager?.canUndo != true, "Grouped undo restores baseline once")
    document.undoManager?.removeAllActions()
    document.withReplayUndo("Replay") {
        document.applyEdit("Action pixels", record: .notACommand) { try $0.withLayerName(0, "Action") }
        document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 5, y: 6, width: 20, height: 18)),
            canvasWidth: 80, canvasHeight: 60), actionName: "Action selection")
    }
    try require(document.history.entries.count == 2 && document.history.entries.last?.name == "Replay",
                "Actions replay records one complete editing-state row")
    let selected = document.selectionHistory
    document.undoManager?.undo()
    try require(document.doc === original && document.selectionHistory.selection == nil
                && !document.isDocumentEdited && document.undoManager?.canUndo != true,
                "Action undo restores pixels and selection as one clean state")
    document.undoManager?.redo()
    try require(document.selectionHistory == selected && document.doc?.layerInfo(0)?.name == "Action",
                "Action redo restores final coverage and document")
    print("PASS: nested groups and Actions preserve one complete history transition")

    let masks = try fixture(width: 80, height: 60)
    defer { masks.close() }
    for index in 0..<32 {
        var bytes = [UInt8](repeating: 0, count: 80 * 60)
        for y in 10..<30 { for x in (index + 1)..<(index + 12) { bytes[y * 80 + x] = 255 } }
        masks.setCanvasSelection(CanvasSelection(shape: .mask(bytes), canvasWidth: 80, canvasHeight: 60),
            actionName: "Selection \(index)")
    }
    try require(masks.history.entries.count == 33 && !masks.isDocumentEdited,
                "Selection-only history passes 24 edits without dirtying the file")
    guard let state = masks.captureEditingState() else { throw ProbeFailure(description: "No mask editing state") }
    let once = try EditingState.retainedBytes(in: [state])
    let shared = try EditingState.retainedBytes(in: [state, state])
    try require(once == shared, "Selection arrays/images/contours deduplicate across identical owners")
    try require(masks.history.incrementalBytes > 80 * 60 * 20,
                "Older selection buffers participate in retention budget")
    try masks.setHistoryBudget(1)
    try require(masks.history.isOverBudget && masks.history.entries.count == 2,
                "Sole oversized selection transition uses the same explicit exception")
    masks.undoManager?.undo()
    try require(!masks.isDocumentEdited && masks.undoManager?.canUndo != true,
                "Selection eviction leaves one usable Undo and preserves clean state")
    print("PASS: 32 selection edits, Swift allocation deduplication, selection budget eviction and clean dirty state")
}

func verifyCompleteEditingState() throws {
    let document = try fixture()
    defer { document.close() }
    guard let controller = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw ProbeFailure(description: "Editing-state controller missing")
    }
    document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 4, y: 5, width: 20, height: 15)),
        canvasWidth: 160, canvasHeight: 120))
    document.setQuickMaskActive(true)
    controller.setPaintTarget(.plane(.red))
    document.jpegExportQuality = 72
    document.embedColorProfile = false
    document.stripMetadata = true
    guard let saved = document.captureEditingState(),
          let changed = try? saved.document.withLayerName(0, "Snapshot change") else {
        throw ProbeFailure(description: "Full-state fixture failed")
    }
    document.undoManager?.removeAllActions()
    let other = EditingState(document: changed, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .plane(.blue), jpegExportQuality: 95,
        embedColorProfile: true, stripMetadata: false)
    document.restoreEditingState(other, actionName: "Restore Snapshot")
    try require(document.isDocumentEdited && document.planeEditTarget == .plane(.blue)
                && document.jpegExportQuality == 95, "Snapshot edit installs complete state")
    document.undoManager?.undo()
    try require(document.captureEditingState()?.equivalent(to: saved) == true
                && controller.paintTarget == .plane(.red) && document.undoManager?.canUndo != true,
                "Snapshot Undo restores coverage, Quick Mask, editor target, settings and document together")
    try require(!document.isDocumentEdited && document.windowControllers.first?.window?.isDocumentEdited == false,
                "Snapshot Undo restores saved dirty indicators")
    document.undoManager?.redo()
    document.restoreEditingState(saved, actionName: "Restore Saved Snapshot")
    try require(!document.isDocumentEdited && document.windowControllers.first?.window?.isDocumentEdited == false,
                "A new snapshot edit targeting the saved handle becomes clean")
    document.undoManager?.undo()
    try require(document.isDocumentEdited && document.doc === changed, "Undo restored saved snapshot returns to dirty edit")
    document.undoManager?.redo()
    try require(!document.isDocumentEdited && document.doc === saved.document, "Redo restores exact saved handle cleanly")
    print("PASS: full editing-state restoration includes Quick Mask, targets and export settings; saved-handle dirty identity remains exact")
}

func verifyBookkeepingCapAndSnapshotsPanel() throws {
    let document = try fixture(width: 24, height: 16)
    defer { document.close() }
    document.history = DocumentHistory(document: document, maximumTransitions: 4)
    document.undoManager?.removeAllActions()
    for index in 1...6 { rename(document, "Capped \(index)") }
    try require(document.history.entries.count == 5 && document.history.cursor == 4
                && document.history.evictedCount == 2 && !document.history.isOverBudget,
                "Bookkeeping cap evicts oldest transitions even when shared pixels fit the byte budget")
    for _ in 0..<4 { document.undoManager?.undo() }
    try require(document.doc?.layerInfo(0)?.name == "Capped 2" && document.undoManager?.canUndo != true,
                "Bookkeeping eviction rebuilds exactly the retained manager transitions")
    print("PASS: independently lowered bookkeeping cap bounds cheap edit sequences without phantom Undo entries")

    let snapshots = try fixture(width: 24, height: 16)
    defer { snapshots.close() }
    guard let controller = snapshots.windowControllers.first?.contentViewController as? EditorViewController,
          let original = snapshots.doc else { throw ProbeFailure(description: "Snapshot panel fixture") }
    _ = try snapshots.namedSnapshots.create(name: "Panel Checkpoint")
    rename(snapshots, "After Checkpoint")
    controller.showSnapshots(nil)
    guard let panel = snapshots.snapshotsWindowController?.window, let content = panel.contentView,
          let table = descendant(NSTableView.self, in: content) else {
        throw ProbeFailure(description: "Actual Snapshots panel missing")
    }
    panel.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    func restoreButton(_ view: NSView) -> NSButton? {
        if let button = view as? NSButton, button.title == "Restore" { return button }
        for child in view.subviews { if let button = restoreButton(child) { return button } }
        return nil
    }
    guard let restore = restoreButton(content) else { throw ProbeFailure(description: "Snapshot Restore button") }
    try require(table.numberOfRows == 1, "Snapshots panel displays captured full state")
    table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
    restore.performClick(nil)
    try require(snapshots.doc === original && !snapshots.isDocumentEdited,
                "Actual Snapshot Restore button returns to captured saved state")
    snapshots.undoManager?.undo()
    try require(snapshots.doc?.layerInfo(0)?.name == "After Checkpoint" && snapshots.isDocumentEdited,
                "Snapshot panel restoration is one reversible edit")
    print("PASS: actual Snapshots panel table/Restore action and saved-state Undo")
}

func verifyNavigationBudgetCredit() throws {
    let document = try fixture(width: 16, height: 16)
    defer { document.close() }
    try document.setHistoryBudget(4096)
    rename(document, "Before Enlargement")
    guard let image = RasterImage.from(rgba: [UInt8](repeating: 128, count: 256 * 256 * 4),
                                      width: 256, height: 256),
          let enlarged = RasterDocument.from(image: image) else {
        throw ProbeFailure(description: "Navigation budget fixture allocation")
    }
    document.applyEdit("Enlarge Canvas", record: .notACommand, changesCanvasGeometry: true) { _ in enlarged }
    let before = document.history.report()
    try require(document.history.entries.count == 3 && !document.history.isOverBudget,
                "Large active state receives baseline credit before navigation")
    document.undoManager?.undo()
    let navigated = document.history.report()
    try require(document.doc?.width == 16 && document.history.entries.count == 3
                && document.history.isOverBudget && document.undoManager?.canRedo == true,
                "Undo preserves the large redo state even though smaller active baseline increases incremental charge")
    try require(before["retained_bytes"] as? UInt64 == navigated["retained_bytes"] as? UInt64,
                "Navigation changed accounting credit, not the retained state allocation union")
    try require((navigated["retention_policy"] as? String)?.contains("Navigation keeps the same retained state union") == true,
                "History report explains navigation budget enforcement honestly")
    document.showHistoryPanel()
    guard let content = document.historyWindowController?.window?.contentView else {
        throw ProbeFailure(description: "Navigation budget panel")
    }
    document.historyWindowController?.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    func labels(_ view: NSView) -> [NSTextField] {
        (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(labels)
    }
    try require(labels(content).contains { $0.stringValue.contains("after navigation") },
                "Panel distinguishes retained navigation states from the single-transition exception")
    document.undoManager?.redo()
    try require(document.doc === enlarged && !document.history.isOverBudget,
                "Redo retains the large state and restores its baseline credit")
    print("PASS: smaller-canvas navigation preserves retained union and Redo; report/panel explain changing budget credit")
}

do {
    _ = NSApplication.shared
    AutomationAlerts.installIfEnabled()
    var failures = 0
    let checks: [(String, () throws -> Void)] = [
        ("long history/panel/budget", verifyLongHistoryAndPanel),
        ("groups/selections", verifyGroupedAndSelectionHistory),
        ("full editing state", verifyCompleteEditingState),
        ("bookkeeping/snapshot panel", verifyBookkeepingCapAndSnapshotsPanel),
        ("navigation budget credit", verifyNavigationBudgetCredit),
        ("recovery capacity", verifyRecoveryCapacity),
        ("recovery lifecycle", verifyRecoveryLifecycle),
        ("recovery off notice", verifyRecoveryOffNotice),
        ("recovery discovery failures", verifyRecoveryDiscoveryErrors),
    ]
    for (name, check) in checks {
        do { try check() }
        catch { fputs("FAIL [\(name)]: \(error)\n", stderr); failures += 1 }
    }
    exit(failures == 0 ? 0 : 1)
}
