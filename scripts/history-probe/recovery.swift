import AppKit
import ObjectiveC

private func recoveryPump(_ message: String, timeout: TimeInterval = 20, until done: () -> Bool) throws {
    let deadline = Date().addingTimeInterval(timeout)
    while !done() && Date() < deadline {
        RunLoop.main.run(until: Date().addingTimeInterval(0.005))
    }
    try require(done(), message)
}

private func recoverySave(_ document: ImageDocument, to url: URL) throws {
    var completed = false
    var failure: Error?
    document.save(to: url, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) {
        failure = $0; completed = true
    }
    try recoveryPump("NSDocument Save did not complete", until: { completed })
    if let failure { throw failure }
}

private func recoveryEntry(_ document: ImageDocument) throws -> URL {
    guard let id = RecoveryCoordinator.shared.status(for: document)["recovery_id"] as? String else {
        throw ProbeFailure(description: "No recovery record identifier")
    }
    return try RecoveryStore.directory(root: RecoveryCoordinator.shared.root, id: id)
}

private func recoveryButton(_ title: String, in view: NSView) -> NSButton? {
    if let button = view as? NSButton, button.title == title { return button }
    for child in view.subviews {
        if let button = recoveryButton(title, in: child) { return button }
    }
    return nil
}

private func abandonedRecovery(from document: ImageDocument, title: String) throws -> String {
    guard let state = document.captureEditingState() else { throw ProbeFailure(description: "No abandoned editing state") }
    let id = UUID().uuidString
    let directory = try RecoveryStore.directory(root: RecoveryCoordinator.shared.root, id: id)
    guard let lease = try RecoveryLease.acquire(directory: directory, create: true) else {
        throw ProbeFailure(description: "Could not claim fixture recovery")
    }
    defer { lease.release() }
    let captured = RecoveryStore.Captured(id: id, title: title, sourceURL: nil, sourceType: nil,
        generation: RecoveryStore.Generation(folder: UUID().uuidString, revision: document.revision,
            serial: 1, captured: Date()), previous: nil, state: try EditingStateArchive.capture(state),
        snapshots: try document.namedSnapshots.captureArchive())
    let gate = RecoveryPublicationGate()
    let task = try WorkerTask(scratchEstimate: captured.scratchEstimate) {
        try RecoveryStore.write(captured, directory: directory, gate: gate)
    }
    _ = try BackgroundWork.wait(task, title: "Making Recovery Fixture")
    return id
}

func verifyRecoveryLifecycle() throws {
    guard let path = ProcessInfo.processInfo.environment["DR_HISTORY_PROBE_DIR"] else {
        throw ProbeFailure(description: "No private recovery probe directory")
    }
    UserDefaults.standard.set(true, forKey: "RecoveryDiscoverySuppressed")
    let directory = URL(fileURLWithPath: path)
    let coordinator = RecoveryCoordinator.shared
    coordinator.start()

    let cleanSave = try fixture()
    rename(cleanSave, "Protected before Save")
    try coordinator.checkpoint(cleanSave, wait: true)
    let savedEntry = try recoveryEntry(cleanSave)
    try require(FileManager.default.fileExists(atPath: savedEntry.appendingPathComponent("manifest.json").path),
                "Initial recovery checkpoint missing")
    try recoverySave(cleanSave, to: directory.appendingPathComponent("confirmed-native.rz"))
    try require(!cleanSave.isDocumentEdited && !FileManager.default.fileExists(atPath: savedEntry.path),
                "Confirmed current native Save must clear obsolete recovery")
    cleanSave.close()
    print("PASS: confirmed preserving Save clears only the now-clean recovery checkpoint")

    let racedSave = try fixture(width: 2400, height: 1600)
    defer { racedSave.close() }
    rename(racedSave, "Captured Save")
    try coordinator.checkpoint(racedSave, wait: true)
    let racedEntry = try recoveryEntry(racedSave)
    let captured = racedSave.doc
    var editedDuringSave = false
    let editTimer = Timer(timeInterval: 0.001, repeats: true) { timer in
        guard BackgroundWork.occupied, BackgroundWork.isWaiting else { return }
        timer.invalidate()
        rename(racedSave, "Newer than Save")
        editedDuringSave = true
    }
    RunLoop.main.add(editTimer, forMode: .common)
    defer { editTimer.invalidate() }
    let racedURL = directory.appendingPathComponent("captured-native.rz")
    try recoverySave(racedSave, to: racedURL)
    editTimer.invalidate()
    try require(editedDuringSave && racedSave.doc !== captured && racedSave.isDocumentEdited,
                "Save race: editedDuringSave=\(editedDuringSave), handleChanged=\(racedSave.doc !== captured), dirty=\(racedSave.isDocumentEdited), name=\(racedSave.doc?.layerInfo(0)?.name ?? "missing")")
    let disk = try RasterDocument.open(url: racedURL)
    try require(disk.layerInfo(0)?.name == "Captured Save", "Save did not write exactly its captured revision")
    try require(FileManager.default.fileExists(atPath: racedEntry.appendingPathComponent("manifest.json").path),
                "An older captured Save removed recovery while newer edits remained")
    try coordinator.checkpoint(racedSave, wait: true)
    let racedManifest = try RecoveryStore.readManifest(at: racedEntry)
    let recoveredPixels = try RasterDocument.open(url: racedEntry.appendingPathComponent(racedManifest.current.folder).appendingPathComponent("image.rz"))
    try require(recoveredPixels.layerInfo(0)?.name == "Newer than Save", "Next checkpoint did not preserve the newer revision")
    racedSave.close()
    print("PASS: editing during captured Save remains dirty, retains recovery and checkpoints the newer revision")

    let withSnapshots = try fixture()
    _ = try withSnapshots.namedSnapshots.create(name: "Session point")
    try coordinator.checkpoint(withSnapshots, wait: true)
    let snapshotEntry = try recoveryEntry(withSnapshots)
    try recoverySave(withSnapshots, to: directory.appendingPathComponent("saved-with-session-list.rz"))
    try require(!withSnapshots.isDocumentEdited && coordinator.status(for: withSnapshots)["needed"] as? Bool == true,
                "Clean images with session snapshots still need crash recovery")
    try require(FileManager.default.fileExists(atPath: snapshotEntry.path), "Save discarded the session snapshot recovery list")
    withSnapshots.close()
    try recoveryPump("Snapshot recovery cleanup did not finish", until: { !BackgroundWork.occupied })
    try require(!FileManager.default.fileExists(atPath: snapshotEntry.path), "Normal close retained session-only recovery")
    print("PASS: session snapshots survive confirmed image Save through recovery and expire on normal close")

    let closing = try fixture(width: 800, height: 600)
    rename(closing, "Close during encode")
    try coordinator.checkpoint(closing, wait: false)
    let closedEntry = try recoveryEntry(closing)
    try require(BackgroundWork.occupied, "Recovery worker did not start")
    closing.close()
    try recoveryPump("Closed recovery worker did not release resources", until: { !BackgroundWork.occupied })
    try require(!FileManager.default.fileExists(atPath: closedEntry.path), "Late recovery publication resurrected an explicitly closed document")
    print("PASS: closing during native recovery encoding prevents late checkpoint resurrection")

    let panelDocument = try fixture()
    rename(panelDocument, "Panel checkpoint")
    coordinator.showRecoveryWindow(nil)
    guard let window = NSApp.windows.first(where: { $0.title == "Recover Unsaved Work" }),
          let content = window.contentView,
          let checkpointButton = recoveryButton("Save Recovery Now", in: content),
          let table = descendant(NSTableView.self, in: content),
          let recoverButton = recoveryButton("Recover Copy", in: content),
          let discardButton = recoveryButton("Discard…", in: content) else {
        throw ProbeFailure(description: "Recovery panel controls missing")
    }
    window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    checkpointButton.performClick(nil)
    let panelEntry = try recoveryEntry(panelDocument)
    try require(FileManager.default.fileExists(atPath: panelEntry.appendingPathComponent("manifest.json").path),
                "Save Recovery Now must target the photo while the recovery panel is key")
    let recoverID = try abandonedRecovery(from: panelDocument, title: "UI recovery fixture")
    let discardID = try abandonedRecovery(from: panelDocument, title: "UI discard fixture")
    coordinator.showRecoveryWindow(nil)
    var available = try coordinator.available()
    guard let recoverRow = available.firstIndex(where: { $0.id == recoverID }) else {
        throw ProbeFailure(description: "Recovery panel omitted fixture")
    }
    table.selectRowIndexes(IndexSet(integer: recoverRow), byExtendingSelection: false)
    coordinator.showRecoveryWindow(nil)
    try require(table.selectedRow == recoverRow, "Automatic reload lost the selected recovery row")
    let priorDocuments = Set(NSDocumentController.shared.documents.map(ObjectIdentifier.init))
    recoverButton.performClick(nil)
    guard let recovered = NSDocumentController.shared.documents.compactMap({ $0 as? ImageDocument })
        .first(where: { !priorDocuments.contains(ObjectIdentifier($0)) }) else {
        throw ProbeFailure(description: "Actual Recover Copy button created no document")
    }
    try require(recovered.fileURL == nil && recovered.isDocumentEdited
                && recovered.doc?.layerInfo(0)?.name == "Panel checkpoint",
                "Recovery UI must open an independent dirty native copy")
    recovered.close()
    coordinator.showRecoveryWindow(nil)
    available = try coordinator.available()
    guard let discardRow = available.firstIndex(where: { $0.id == discardID }),
          let modalMethod = class_getInstanceMethod(NSAlert.self, #selector(NSAlert.runModal)) else {
        throw ProbeFailure(description: "Discard fixture/confirmation unavailable")
    }
    table.selectRowIndexes(IndexSet(integer: discardRow), byExtendingSelection: false)
    var sawConfirmation = false
    let confirmation: @convention(block) (AnyObject) -> Int = { object in
        guard let alert = object as? NSAlert else { return NSApplication.ModalResponse.abort.rawValue }
        sawConfirmation = alert.buttons.contains { $0.title == "Discard Recovery" }
            && alert.informativeText.contains("Original files are unchanged")
        return NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
    }
    let replacement = imp_implementationWithBlock(confirmation)
    let original = method_setImplementation(modalMethod, replacement)
    discardButton.performClick(nil)
    method_setImplementation(modalMethod, original)
    imp_removeBlock(replacement)
    let afterDiscard = try coordinator.available()
    try require(sawConfirmation && !afterDiscard.contains(where: { $0.id == discardID }),
                "Actual Discard control must confirm and remove only the chosen recovery")
    panelDocument.close()
    window.orderOut(nil)
    print("PASS: actual recovery panel checkpoint, selected-row retention, Recover Copy and confirmed Discard controls")
}

func verifyRecoveryDiscoveryErrors() throws {
    let coordinator = RecoveryCoordinator.shared
    let document = try fixture()
    defer { document.close() }
    rename(document, "Readable beside failed recovery")
    let validID = try abandonedRecovery(from: document, title: "Complete recovery remains discoverable")
    let validDirectory = try RecoveryStore.directory(root: coordinator.root, id: validID)
    defer { try? RecoveryStore.discard(directory: validDirectory) }

    let unreadableID = UUID().uuidString
    let unreadable = try RecoveryStore.directory(root: coordinator.root, id: unreadableID)
    let incompleteID = UUID().uuidString
    let incomplete = try RecoveryStore.directory(root: coordinator.root, id: incompleteID)
    try FileManager.default.createDirectory(at: unreadable, withIntermediateDirectories: false)
    try FileManager.default.createDirectory(at: incomplete, withIntermediateDirectories: false)
    defer {
        try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: unreadable.path)
        try? RecoveryStore.discard(directory: unreadable)
        try? RecoveryStore.discard(directory: incomplete)
    }
    // A failed first write can create its directory and fail before a lease is
    // created. Deny that later lease creation without filling the real volume.
    try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: unreadable.path)
    let found = try coordinator.available()
    try require(found.contains(where: { $0.id == validID && $0.manifest != nil && $0.failure == nil }),
                "An inaccessible incomplete entry hid a valid recovery checkpoint")
    try require(found.contains(where: { $0.id == unreadableID && $0.manifest == nil && $0.failure != nil }),
                "Lease-creation failure must be reported per recovery entry")
    try require(!FileManager.default.fileExists(atPath: unreadable.appendingPathComponent("lease").path),
                "Unreadable fixture unexpectedly allowed lease creation")
    try require(found.contains(where: { $0.id == incompleteID && $0.manifest == nil && $0.failure != nil }),
                "An ordinary unfinished first checkpoint must remain visible and discardable")
    try coordinator.discard(id: incompleteID)
    try require(!FileManager.default.fileExists(atPath: incomplete.path),
                "Discard did not remove the ordinary unfinished checkpoint")
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: unreadable.path)
    try coordinator.discard(id: unreadableID)
    let stillAvailable = try coordinator.available()
    try require(stillAvailable.contains(where: { $0.id == validID && $0.manifest != nil }),
                "Cleaning incomplete entries damaged the valid checkpoint")
    print("PASS: failed initial lease/write discovery is isolated per entry; valid recovery remains available and incomplete entries stay discardable")
}

private func recoveryLabels(_ view: NSView) -> [NSTextField] {
    (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(recoveryLabels)
}

/// Hold a preview reservation that leaves 16 MiB of the working limit, below
/// any native recovery capture's scratch (its 32 MiB save overhead alone).
private func recoveryCrowdOut() throws -> SelectionWorkBudget.Reservation {
    let limit = RasterResources.shared.workingLimit
    guard let retained = (try RasterResources.shared.report(evict: true)["unique_retained_bytes"] as? NSNumber)?.uint64Value,
          retained + 16 * 1024 * 1024 < limit else {
        throw ProbeFailure(description: "Recovery off probe could not measure retained bytes")
    }
    return try SelectionWorkBudget.reservePreview(bytes: limit - retained - 16 * 1024 * 1024, outputBytes: 0)
}

private func recoveryRefusal(_ document: ImageDocument) throws -> RecoveryCapacity.Refusal {
    do {
        try RecoveryCoordinator.shared.checkpoint(document, wait: true)
    } catch let refusal as RecoveryCapacity.Refusal {
        return refusal
    }
    throw ProbeFailure(description: "Recovery checkpoint was admitted beside a crowding preview reservation")
}

/// A refused capture reports `failed` with memory figures, the editor's
/// status notice says "Recovery off", the Recover Unsaved Work panel names the
/// document, and the notice clears once a checkpoint is written.
func verifyRecoveryOffNotice() throws {
    UserDefaults.standard.set(true, forKey: "RecoveryDiscoverySuppressed")
    let coordinator = RecoveryCoordinator.shared
    coordinator.start()
    let document = try fixture()
    defer { document.close() }
    rename(document, "Recovery off probe")
    let title = document.displayName ?? "Untitled"

    var reservation: SelectionWorkBudget.Reservation? = try recoveryCrowdOut()
    let refusal = try recoveryRefusal(document)
    try require(refusal.stage == .capture && refusal.message.hasPrefix(RecoveryCapacity.offPrefix),
                "A crowded-out capture refuses as a .capture Refusal starting with the off prefix: \(refusal.message)")
    let status = coordinator.status(for: document)
    try require(status["state"] as? String == "failed"
                && (status["memory"] as? [String: Any])?["stage"] as? String == "capture",
                "get_recovery_status reports failed with capture memory figures: \(status)")
    guard let notice = coordinator.offNotice(for: document) else {
        throw ProbeFailure(description: "A refused capture shows no Recovery off notice")
    }
    try require(notice.text == "Recovery off" && notice.detail.contains("It has no recovery copy."),
                "The first refusal says Recovery off and that no copy exists: \(notice.detail)")
    coordinator.showRecoveryWindow(nil)
    guard let window = NSApp.windows.first(where: { $0.title == "Recover Unsaved Work" }),
          let content = window.contentView else {
        throw ProbeFailure(description: "Recovery panel missing")
    }
    window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    try require(recoveryLabels(content).contains { $0.stringValue.contains("\(title): \(RecoveryCapacity.offPrefix)") },
                "The panel names the unprotected document")

    reservation = nil
    try coordinator.checkpoint(document, wait: true)
    try require(coordinator.offNotice(for: document) == nil
                && coordinator.status(for: document)["state"] as? String == "protected",
                "A written checkpoint turns the notice off")

    rename(document, "Recovery off probe again")
    reservation = try recoveryCrowdOut()
    let again = try recoveryRefusal(document)
    try require(again.stage == .capture, "The second capture refuses as a .capture Refusal")
    try require(coordinator.offNotice(for: document)?.detail.contains("is kept") == true,
                "A refusal after a written copy says that copy is kept")
    reservation = nil
    _ = reservation
    window.orderOut(nil)
    print("PASS: a refused recovery capture reports memory figures, shows Recovery off in the editor and panel, and clears after a written checkpoint")
}
