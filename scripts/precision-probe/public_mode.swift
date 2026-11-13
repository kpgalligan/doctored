import AppKit

private func publicState(_ source: RasterDocument) -> EditingState {
    EditingState(document: source, selection: SelectionHistoryState(), layerSelection: .single(0),
        paintTarget: .layer, jpegExportQuality: 91, embedColorProfile: true, stripMetadata: false,
        exportBitDepth: 16, exportDither: "none")
}
private func publicDocument(_ source: RasterDocument? = nil) throws -> ImageDocument {
    let source = try source ?? RasterDocument(precision: openTyped(root().appendingPathComponent("source16.png")))
    let document = ImageDocument()
    document.installEditingState(publicState(source), markSaved: true)
    return document
}
private func publicRename(_ document: ImageDocument, _ name: String) {
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename Layer", record: .notACommand) { try $0.withLayerName(0, name) }
    }
}
private func publicPump(_ message: String, until done: () -> Bool) throws {
    let deadline = Date().addingTimeInterval(20)
    while !done() && Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.005)) }
    try require(done(), message)
}
private func publicSave(_ document: ImageDocument, to destination: URL) throws {
    var complete = false
    var error: Error?
    document.save(to: destination, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) {
        error = $0; complete = true
    }
    try publicPump("Actual NSDocument Save did not finish", until: { complete })
    if let error { throw error }
}

/// Match the public MCP open lifecycle, including AppKit's file type/URL
/// initialization. A bare read(from:) only loads contents and cannot stand in
/// for NSDocumentController when a real window asks for its title/file type.
func openPublicProbeDocument(_ url: URL, show: Bool) throws -> ImageDocument {
    let controller = NSDocumentController.shared
    let type = try controller.typeForContents(of: url)
    guard let document = try controller.makeDocument(withContentsOf: url, ofType: type) as? ImageDocument else {
        throw ProbeFailure(description: "Public controller did not create an image document")
    }
    controller.addDocument(document)
    if show { document.makeWindowControllers(); document.showWindows() }
    return document
}

func pumpPublicProbeEvents() {
    autoreleasepool {
        var dispatched = 0
        while dispatched < 64, let event = NSApp.nextEvent(matching: .any, until: Date(),
            inMode: .default, dequeue: true) {
            NSApp.sendEvent(event)
            dispatched += 1
        }
        NSApp.updateWindows()
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
}

private func verifyPublicWindowClose() throws {
    let oldMode = PrecisionImportRequest.mode
    PrecisionImportRequest.mode = .preserve
    defer { PrecisionImportRequest.mode = oldMode }
    for filename in ["source8.png", "source16.png"] {
        let input = try root().appendingPathComponent(filename)
        let baseline = try RasterResources.shared.report()
        weak var closed: ImageDocument?
        weak var closedWindow: NSWindow?
        weak var closedEditor: EditorViewController?
        weak var closedCanvas: ImageCanvasView?
        try autoreleasepool {
            let opened = try openPublicProbeDocument(input, show: true)
            closed = opened
            closedWindow = opened.windowControllers.first?.window
            closedEditor = opened.windowControllers.first?.contentViewController as? EditorViewController
            closedCanvas = closedEditor?.canvas
            try require(opened.fileURL == input && opened.fileType == "public.png"
                && opened.doc?.isHighDepth == (filename == "source16.png")
                && !opened.isDocumentEdited && opened.windowControllers.count == 1,
                "Actual public controller open initializes file metadata before presenting its typed window")
            closedWindow?.performClose(nil)
        }
        func released() throws -> Bool {
            let report = try RasterResources.shared.report()
            guard closed == nil else { return false }
            return try ["host_document_handles", "host_image_handles",
                "host_precision_document_handles", "host_precision_image_handles", "unique_retained_bytes"]
                .allSatisfy { try value(report, $0) == value(baseline, $0) }
        }
        let deadline = Date().addingTimeInterval(2)
        while try !released(), Date() < deadline { pumpPublicProbeEvents() }
        if try !released() {
            let report = try RasterResources.shared.report()
            let keys = ["host_document_handles", "host_image_handles", "host_precision_document_handles",
                        "host_precision_image_handles", "unique_retained_bytes", "transient_estimate_bytes"]
            let counts = Dictionary(uniqueKeysWithValues: try keys.map { ($0, try value(report, $0)) })
            throw ProbeFailure(description: "Closing public \(filename) window exceeded two-second bound: documentReleased=\(closed == nil), windowReleased=\(closedWindow == nil), editorReleased=\(closedEditor == nil), canvasReleased=\(closedCanvas == nil), live=\(counts)")
        }
    }
}

func verifyPublicPrecisionOwnership() throws {
    try verifyPublicWindowClose()
    let document = try publicDocument()
    defer { document.close() }
    guard let facade = document.doc, let source = facade.precision else { throw ProbeFailure(description: "Missing public typed owner") }
    try require(facade.ptr == nil && facade.isHighDepth && document.projection == nil,
                "Exactly one typed owner; no byte document or authoritative projection surrogate")
    try verifySource(source)
    try require(document.previewImage != nil && document.precisionProjection != nil,
                "Display and authoritative projection are retained separately")
    try refuses("Authoritative byte projection API must refuse typed state") { _ = try document.currentProjection() }
    try refuses("Byte core render API must refuse typed state") { _ = try facade.render() }
    try require(facade.layerImage(0) == nil && facade.layerPixels(0) == nil && facade.flattened() == nil,
                "Byte source reads do not sample a typed preview")
    let projection = try document.currentPrecisionProjection()
    let sample = try projection.precisionSample(x: 1, y: 0)
    try require(sample["center_rgba16"] as? [Int] == expected(1).map(Int.init),
                "Visible editor sampling reads exact native codes including alpha1")
    let display = try document.currentDisplayProjection()
    try require(display.pixelRGBA(x: 1, y: 0)?.a == 0, "Explicit display conversion is distinguishable from authoritative alpha")
    try require(facade.withLayerPixels(0, display) == nil,
                "A display image cannot be written into a typed layer through byte APIs")
    let originalProfile = facade.iccProfile
    let destination = try Data(contentsOf: root().appendingPathComponent("destination-profile.icc"))
    try require(facade.adoptingWorkingSpace(destination).document == nil && facade.iccProfile == originalProfile,
                "Typed owner never silently adopts the byte working space")
    let duplicate = try precisionRequired(try facade.duplicatingLayer(0), "Typed duplicate missing")
    let group = try precisionRequired(duplicate.groupLayers([0, 1], name: "Typed Group")?.document, "Typed group missing")
    try require(group.layerInfo(2)?.name == "Typed Group" && group.layerDepth(0) == 1
        && group.layerDepth(2) == 0 && group.layerSubtree(2)?.start == 0,
        "Cached postorder metadata drives the existing layer tree")
    try require(group.layerTree.ancestors(of: 0) == [2], "Existing UI layer tree resolves typed parent")
    print("PASS: public typed ownership, exact readout, explicit display, byte API refusal and cached layer hierarchy")
}

private func precisionRequired<T>(_ value: T?, _ message: String) throws -> T {
    guard let value else { throw ProbeFailure(description: message) }
    return value
}

func verifyPublicPrecisionHistory() throws {
    let document = try publicDocument()
    defer { document.close() }
    let original = try precisionRequired(document.doc, "Missing original")
    try require(!document.isDocumentEdited, "Installed saved typed source is clean")
    publicRename(document, "Typed Edit")
    let renamed = try precisionRequired(document.doc, "Missing rename")
    try require(renamed !== original && document.isDocumentEdited && document.undoManager?.canUndo == true,
                "Typed metadata edit uses central dirty/history state")
    document.undoManager?.undo()
    try require(document.doc === original && !document.isDocumentEdited && document.projection == nil,
                "Undo restores exact saved typed owner and clean state")
    document.undoManager?.redo()
    try require(document.doc === renamed && document.doc?.layerInfo(0)?.name == "Typed Edit", "Redo restores typed owner")
    let down = try PrecisionModeConversion(document, mode: .rgba8, dither: .none)
    let prepared = try down.compute()
    let before = try document.currentPrecisionProjection()
    let budget = try PrecisionWorkContext(image: before)
    let quantized = try before.quantized(dither: 0, workingLimit: budget.limit, otherRetained: budget.otherRetained)
    let convertedBytes = try precisionRequired(prepared.document.layerPixels(0), "Converted raster bytes missing")
    try require(quantized.matchesRGBA(convertedBytes, width: prepared.document.width, height: prepared.document.height),
                "Deliberate conversion stores independently quantized authoritative projection")
    try down.publish(prepared, to: document, record: .notACommand)
    let byteOwner = document.doc
    try require(document.doc?.precision == nil && document.projection != nil && document.precisionProjection == nil,
                "Explicit mode conversion installs one byte owner and byte projection")
    document.undoManager?.undo()
    try require(document.doc === renamed && document.projection == nil && document.precisionProjection != nil,
                "One Undo crosses precision boundary and restores typed source")
    document.undoManager?.redo()
    try require(document.doc === byteOwner, "One Redo restores converted byte owner")
    let promote = try PrecisionModeConversion(document, mode: .rgba16, dither: .none)
    let promoted = try promote.compute()
    try promote.publish(promoted, to: document, record: .notACommand)
    try require(document.doc?.precision != nil && document.projection == nil, "Explicit promotion returns typed mode")
    document.undoManager?.undo()
    try require(document.doc === byteOwner, "Promotion remains one central undo transition")
    try verifyPublicPrecisionTargetConversion()
    try verifyPublicPrecisionHistoryReentrancy()
    print("PASS: central typed edits, saved identity, Undo/Redo across explicit conversion and typed projection restoration")
}

private func verifyPublicPrecisionHistoryReentrancy() throws {
    let document = try publicDocument()
    defer { document.close() }
    let original = document.doc
    publicRename(document, "First history state")
    let first = document.doc
    publicRename(document, "Second history state")
    guard let manager = document.undoManager else { throw ProbeFailure(description: "Missing history manager") }
    var observed = 0
    var changed = false
    // This synchronous AppKit notification is emitted by real history state
    // publication after its projection is prepared, before the outer Undo has
    // completed. It makes nested Undo/Redo deterministic even on tiny sources.
    let token = NotificationCenter.default.addObserver(forName: .imageDocumentImageDidChange,
        object: document, queue: nil) { _ in
        guard manager.isUndoing && document.isPublishingEditingState else { return }
        observed += 1
        let owner = document.doc
        let revision = document.revision
        let cursor = document.history.cursor
        manager.undo()
        manager.redo()
        if document.doc !== owner || document.revision != revision || document.history.cursor != cursor {
            changed = true
        }
    }
    manager.undo()
    NotificationCenter.default.removeObserver(token)
    try require(observed == 1 && !changed && document.doc === first && !document.isPublishingEditingState,
                "Nested Undo/Redo during real typed history publication is ignored")
    manager.undo()
    try require(document.doc === original && !document.isDocumentEdited,
                "Normal Undo after publication still restores the exact saved owner")
    manager.redo()
    try require(document.doc === first, "Normal Redo survives refused nested history mutations")
}

private func verifyPublicPrecisionTargetConversion() throws {
    let typed = try openTyped(root().appendingPathComponent("source16.png"))
    let plane = try PrecisionScalarBuffer(samples: [UInt16](repeating: 65535, count: 384), width: 32, height: 12)
    let masked = try pixelEdit(typed, .setMask(layer: 0, width: 32, height: 12), plane)
    let channels = try pixelEdit(masked, .setChannel(name: "Exact saved coverage"), plane)
    let document = try publicDocument(RasterDocument(precision: channels))
    defer { document.close() }
    for target in [PaintTarget.mask, .channel(0)] {
        document.planeEditTarget = target
        let original = document.doc
        let conversion = try PrecisionModeConversion(document, mode: .rgba8, dither: .none)
        let prepared = try conversion.compute()
        try conversion.publish(prepared, to: document, record: .notACommand)
        try require(document.planeEditTarget == .layer && document.activeLayerIndex == 0
            && document.selectedLayerIndices == [0],
            "Flattened conversion installs a valid layer target")
        // The flatten discards the layer mask and keeps the saved channel at the new
        // depth (`PrecisionDocument::downconvert`, `narrow_channels`).
        try require(document.doc?.layerHasMask(0) == false,
                    "Flattened conversion discards the layer mask")
        try require(document.doc?.channelCount == 1,
                    "Flattened conversion keeps the saved channel")
        document.undoManager?.undo()
        try require(document.doc === original && document.planeEditTarget == target
            && document.doc?.channelCount == 1 && document.doc?.layerHasMask(0) == true,
            "One Undo restores exact typed owner and its mask/channel target")
    }
}

func verifyPublicPrecisionArchive() throws {
    let document = try publicDocument()
    defer { document.close() }
    let source = try precisionRequired(document.doc, "Missing public source")
    let selected = CanvasSelection(shape: .rect(CGRect(x: 2, y: 1, width: 8, height: 6)), canvasWidth: 32, canvasHeight: 12)
    document.setCanvasSelection(selected, actionName: "Select", record: .notACommand)
    document.exportBitDepth = 8
    document.exportDither = "ordered"
    let state = try precisionRequired(document.captureEditingState(), "Missing public editing state")
    let captured = try EditingStateArchive.capture(state)
    let nativeDescriptor = try precisionRequired(source.precision, "Typed archive source missing").descriptor()
    let inspection = try value(nativeDescriptor, "inspection_bytes")
    let reopenedCore = try value(nativeDescriptor, "reopen_retained_bytes")
    try require(captured.reopenedRetainedEstimate >= reopenedCore + inspection,
                "Archive reopened state includes its actual facade metadata lease")
    try require(captured.manifest.decodeEstimateBytes >= reopenedCore + inspection * 3 + 4 * 1024 * 1024,
                "Archive decode admission includes the metadata JSON expansion peak")
    let directory = try root().appendingPathComponent("public-typed-state")
    try BackgroundWork.wait(WorkerTask(scratchEstimate: captured.scratchEstimate) {
        try EditingStateArchive.write(captured, to: directory)
    }, title: "Writing Public Typed State")
    let loaded = try BackgroundWork.wait(WorkerTask { try EditingStateArchive.read(from: directory) }, title: "Reading Public Typed State")
    let restored = try loaded.editingState()
    try require(restored.document.precision != nil && restored.selection == state.selection
        && restored.exportBitDepth == 8 && restored.exportDither == "ordered", "Version2 archive preserves typed source and full editing values")
    try verifySource(precisionRequired(restored.document.precision, "Typed archive source missing"))
    publicRename(document, "Later")
    document.restoreEditingState(restored, actionName: "Restore Typed Snapshot")
    try require(document.doc === restored.document, "Snapshot restore uses central full state")
    document.undoManager?.undo()
    try require(document.doc?.layerInfo(0)?.name == "Later", "Snapshot restore is one undo transition")
    // Produce a genuine legacy-native package, then remove only host fields
    // absent from the original v1 schema (v2's storage/evaluation/export
    // fields and v3's channelView). The embedded image remains nativev10.
    let byte = try RasterDocument.open(url: root().appendingPathComponent("source8.png"))
    let oldCapture = try EditingStateArchive.capture(publicState(byte))
    let oldDirectory = try root().appendingPathComponent("public-v1-state")
    try BackgroundWork.wait(WorkerTask(scratchEstimate: oldCapture.scratchEstimate) {
        try EditingStateArchive.write(oldCapture, to: oldDirectory)
    }, title: "Writing Legacy State Fixture")
    let manifestURL = oldDirectory.appendingPathComponent("state.json")
    var manifest = try JSONSerialization.jsonObject(with: Data(contentsOf: manifestURL)) as? [String: Any] ?? [:]
    manifest["version"] = 1
    for key in ["storage", "evaluation", "exportBitDepth", "exportDither", "channelView"] {
        manifest.removeValue(forKey: key)
    }
    try JSONSerialization.data(withJSONObject: manifest).write(to: manifestURL)
    let old = try BackgroundWork.wait(WorkerTask { try EditingStateArchive.read(from: oldDirectory) }, title: "Reading Legacy State")
    try require(old.document.precision == nil, "Version1 editing-state packages retain byte semantics")
    manifest["exportBitDepth"] = 13
    manifest["exportDither"] = "future-unknown-policy"
    try JSONSerialization.data(withJSONObject: manifest).write(to: manifestURL)
    let oldWithExtraFields = try BackgroundWork.wait(WorkerTask {
        try EditingStateArchive.read(from: oldDirectory)
    }, title: "Reading Version1 Extension Fields")
    let normalized = try oldWithExtraFields.editingState()
    try require(normalized.exportBitDepth == 16 && normalized.exportDither == "none",
                "Version1 ignores later optional export fields and installs established defaults")
    let normalizedCapture = try EditingStateArchive.capture(normalized)
    let upgradedDirectory = try root().appendingPathComponent("public-v1-upgraded-state")
    try BackgroundWork.wait(WorkerTask(scratchEstimate: normalizedCapture.scratchEstimate) {
        try EditingStateArchive.write(normalizedCapture, to: upgradedDirectory)
    }, title: "Writing Migrated Version2 State")
    let upgraded = try BackgroundWork.wait(WorkerTask {
        try EditingStateArchive.read(from: upgradedDirectory)
    }, title: "Reading Migrated Version2 State")
    // A migrated package is written at the writer's current version.
    try require(upgraded.manifest.version == EditingStateArchive.version
        && upgraded.manifest.exportBitDepth == 16 && upgraded.manifest.exportDither == "none",
        "Version1 migration never republishes invalid current-version settings")
    // Exercise the same recovery writer/reader with typed state, in an owned
    // private directory; no discovery, modal prompt or existing recovery state.
    let id = UUID().uuidString
    let recoveryRoot = try root().appendingPathComponent("public-recovery", isDirectory: true)
    let recoveryDirectory = try RecoveryStore.directory(root: recoveryRoot, id: id)
    let lease = try precisionRequired(RecoveryLease.acquire(directory: recoveryDirectory, create: true), "Private recovery lease missing")
    defer { lease.release() }
    let recovery = RecoveryStore.Captured(id: id, title: "Typed recovery", sourceURL: nil, sourceType: nil,
        generation: .init(folder: UUID().uuidString, revision: document.revision, serial: 1, captured: Date()),
        previous: nil, state: captured,
        snapshots: [.init(header: .init(version: 1, name: "Retained typed snapshot", created: Date()), state: captured)])
    let reopenPeak = try RecoveryCapacity.validate(recovery)
    try require(reopenPeak >= captured.reopenedRetainedEstimate * 2 + captured.projectionScratchEstimate,
                "Recovery independently reopened snapshots count two facade metadata owners")
    try refuses("Recovery cannot subtract facade metadata from a shared source serialized twice") {
        _ = try RecoveryCapacity.validate(recovery, limit: reopenPeak - inspection * 2)
    }
    let gate = RecoveryPublicationGate()
    let published = try BackgroundWork.wait(WorkerTask(scratchEstimate: recovery.scratchEstimate) {
        try RecoveryStore.write(recovery, directory: recoveryDirectory, gate: gate)
    }, title: "Writing Typed Recovery")
    let candidate = RecoveryStore.Candidate(id: id, directory: recoveryDirectory, manifest: published, failure: nil)
    let recovered = try BackgroundWork.wait(WorkerTask { try RecoveryStore.read(candidate) }, title: "Reading Typed Recovery")
    try require(recovered.state.document.precision != nil && recovered.snapshots.count == 1
        && recovered.snapshots[0].state.document.precision != nil, "Recovery retains typed document and snapshot modes")
    try verifySource(precisionRequired(recovered.state.document.precision, "Recovered typed source missing"))
    try require(source.precision != nil, "Archive/recovery leave original owner intact")
    print("PASS: public typed state/snapshot restore, legacyv1 state compatibility and actual private recovery roundtrip")
}

func verifyPublicPrecisionStaleSave() throws {
    let initial = try RasterDocument(precision: openTyped(root().appendingPathComponent("source16.png")))
    let source = try precisionRequired(try initial.duplicatingLayer(0), "Missing stale-target fixture")
    let document = try publicDocument(source)
    defer { document.close() }
    let capture = try PrecisionEditorCapture(document)
    let typed = capture.source
    let gate = DispatchSemaphore(value: 0)
    var fired = false
    let timer = Timer.scheduledTimer(withTimeInterval: 0.001, repeats: true) { timer in
        guard BackgroundWork.occupied && BackgroundWork.isWaiting else { return }
        document.setLayerSelection(.single(1))
        fired = true; timer.invalidate(); gate.signal()
    }
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate(); gate.signal() }
    let result = try BackgroundWork.wait(WorkerTask {
        guard gate.wait(timeout: .now() + 5) == .success else { throw ProbeFailure(description: "Main-run-loop mutation did not run") }
        return try typed.settingLayerOpacity(0, opacity: 0.5)
    }, title: "Checking Captured Typed Target")
    try require(fired && document.doc === source && document.activeLayerIndex == 1,
                "Main run loop changes target while immutable typed worker is pending")
    try refuses("Captured typed result must refuse stale target") {
        _ = try capture.publish(result, to: document, name: "Stale Result")
    }
    document.setLayerSelection(.single(0))
    let staleDocument = try PrecisionEditorCapture(document)
    let replacement = try precisionRequired(source.withLayerName(0, "Changed during worker"), "Missing source-replacement fixture")
    let replacementDisplay = PreparedDocumentEdit(replacement)
    let signal = DispatchSemaphore(value: 0)
    var changed = false
    let changeTimer = Timer.scheduledTimer(withTimeInterval: 0.001, repeats: true) { timer in
        guard BackgroundWork.occupied && BackgroundWork.isWaiting else { return }
        document.preparedEdit = replacementDisplay
        withUndoGroup(document.undoManager) {
            document.applyEdit("Concurrent Rename", record: .notACommand) { current in
                current === source ? replacement : nil
            }
        }
        document.preparedEdit = nil
        changed = true; timer.invalidate(); signal.signal()
    }
    RunLoop.main.add(changeTimer, forMode: .modalPanel)
    defer { changeTimer.invalidate(); signal.signal() }
    let oldNative = staleDocument.source
    let staleResult = try BackgroundWork.wait(WorkerTask {
        guard signal.wait(timeout: .now() + 5) == .success else { throw ProbeFailure(description: "Document mutation did not run") }
        return try oldNative.settingLayerOpacity(0, opacity: 0.625)
    }, title: "Checking Captured Typed Document")
    try require(changed && document.doc === replacement, "Central document edit remains responsive during a captured worker")
    try refuses("Captured typed result must refuse a replaced document") {
        _ = try staleDocument.publish(staleResult, to: document, name: "Stale Document Result")
    }
    document.setLayerSelection(.single(1))
    let conversion = try PrecisionModeConversion(document, mode: .rgba8, dither: .none)
    let prepared = try conversion.compute()
    document.setLayerSelection(.single(0))
    try refuses("Prepared conversion must refuse stale editing state") {
        try conversion.publish(prepared, to: document, record: .notACommand)
    }
    try publicSave(document, to: root().appendingPathComponent("public-saved.rz"))
    try require(!document.isDocumentEdited && document.historySavedDocument === document.doc,
                "Actual NSDocument Save marks the captured typed owner clean")
    let save = try CapturedSave(document, type: ImageDocument.nativeTypeIdentifier)
    publicRename(document, "After capture")
    let later = document.doc
    let destination = try root().appendingPathComponent("public-captured-old.rz")
    try BackgroundWork.wait(WorkerTask(scratchEstimate: save.scratchEstimate) { try save.writeSafely(to: destination) }, title: "Writing Captured Old Typed State")
    // Same saved-marker hook as the NSDocument safe-writing completion, after
    // the actual captured save above has reached its atomic publication point.
    document.lastWrittenDocument = save.source
    document.updateChangeCount(.changeCleared)
    try require(document.doc === later && document.isDocumentEdited && document.historySavedDocument === save.source,
                "Saved old capture cannot mark a newer typed edit clean")
    let disk = try RasterDocument.open(url: destination)
    try require(disk.layerInfo(0)?.name == save.source.layerInfo(0)?.name,
                "Disk stores the captured source, not the later facade")
    document.undoManager?.undo()
    try require(document.doc === save.source && !document.isDocumentEdited, "Undo back to saved typed identity is clean")
    try verifyPublicSaveGrouping()
    try verifyCapturedPublicProjectionExport()
    print("PASS: pending worker target refusal, stale conversion refusal, actual preserving Save and captured-old saved marker")
}

private func verifyCapturedPublicProjectionExport() throws {
    let document = try publicDocument()
    defer { document.close() }
    let capture = try PrecisionExportSource(document)
    try require(capture.source === document.doc && capture.projection === document.precisionProjection,
                "Public export captures the authoritative projection already paired with its document")
    let oldExport = try PrecisionExportCapture(source: capture, format: .png)
    withUndoGroup(document.undoManager) {
        document.applyEdit("Later opacity", record: .notACommand) { try $0.withLayerOpacity(0, 0.5) }
    }
    let later = document.doc
    try require(later !== capture.source && document.precisionProjection !== capture.projection,
                "Later edit has an independent authoritative projection")
    let oldURL = try root().appendingPathComponent("public-captured-projection16.png")
    let report = try oldExport.run(to: oldURL)
    try require(report.carries(DR_CARRIES_PROFILE) && report.carries(DR_CARRIES_XMP),
                "Captured projection export retains its original profile and metadata")
    let reopened = try openTyped(oldURL)
    let image = try precisionRequired(reopened.layerSource(0), "Captured export source missing")
    try capture.projection.withRGBA16 { expected in
        try image.withRGBA16 { actual in
            try require(actual.elementsEqual(expected), "Export stores the exact captured u16 projection without rendering again")
        }
    }
    let latest = try PrecisionExportSource(document)
    _ = try PrecisionExportCapture(source: latest, format: .tiff).run(
        to: root().appendingPathComponent("public-current-projection16.tiff"))
    try require(document.doc === later && document.isDocumentEdited,
                "Exporting either captured revision never changes the current or saved owner")
}

private func verifyPublicSaveGrouping() throws {
    for highDepth in [false, true] {
        try autoreleasepool {
            let byte = highDepth ? nil : try RasterDocument.open(url: root().appendingPathComponent("source8.png"))
            let document = try publicDocument(byte)
            defer { document.close() }
            guard let manager = document.undoManager else { throw ProbeFailure(description: "Save test needs the real history manager") }
            let original = document.doc
            try require(manager.groupsByEvent && manager.groupingLevel == 0, "Actual editor event grouping starts enabled")
            withUndoGroup(manager) {
                publicRename(document, "First grouped rename")
                publicRename(document, "Saved grouped rename")
            }
            let saved = document.doc
            try require(manager.groupsByEvent && manager.groupingLevel == 0
                && document.history.entries.count == 2, "Nested central edits preserve event policy and one outer transition")
            let suffix = highDepth ? "16" : "8"
            try publicSave(document, to: root().appendingPathComponent("event-group-save\(suffix).rz"))
            try require(manager.groupsByEvent && manager.groupingLevel == 0 && !document.isDocumentEdited,
                        "Actual byte/typed Save balances default event groups and marks its captured state clean")
            manager.undo()
            try require(document.doc === original && document.isDocumentEdited && manager.canUndo == false,
                        "After Save one Undo reverses the whole nested edit group")
            manager.redo()
            try require(document.doc === saved && !document.isDocumentEdited, "After Save Redo reaches exact saved state")
            publicRename(document, "State before failed Save")
            let failedSource = document.doc
            let fileParent = try root().appendingPathComponent("save-error-parent\(suffix)")
            try Data([1]).write(to: fileParent)
            try refuses("Saving beneath a regular file must report an actual filesystem failure") {
                try publicSave(document, to: fileParent.appendingPathComponent("unwritable.rz"))
            }
            try require(document.doc === failedSource && document.historySavedDocument === saved
                && document.isDocumentEdited && manager.groupsByEvent && manager.groupingLevel == 0,
                "Failed actual Save preserves current/saved owners and balanced event grouping")
            manager.undo()
            try require(document.doc === saved && !document.isDocumentEdited, "Undo remains exact after failed Save")
            manager.redo()
            try require(document.doc === failedSource && document.isDocumentEdited, "Redo remains exact after failed Save")
        }
    }
}

func verifyPublicPrecisionResources() throws {
    let baseline = try RasterResources.shared.report()
    weak var released: RasterDocument?
    try autoreleasepool {
        let typed = try RasterDocument(precision: openTyped(root().appendingPathComponent("source16.png")))
        released = typed
        let clone = try precisionRequired(typed.clone(), "Missing typed facade clone")
        let byte = try RasterDocument.open(url: root().appendingPathComponent("source8.png"))
        let one = try RasterDocument.resourceReport(documents: [typed], images: [], scratch: 0, limit: 0)
        let aliases = try RasterDocument.resourceReport(documents: [typed, clone, typed], images: [], scratch: 0, limit: 0)
        try require(try value(one, "unique_retained_bytes") == value(aliases, "unique_retained_bytes"),
                    "Facade aliases deduplicate typed storage and shared metadata lease")
        let mixed = try RasterDocument.resourceReport(documents: [typed, clone, byte], images: [], scratch: 0, limit: 0)
        let byteOnly = try RasterDocument.resourceReport(documents: [byte], images: [], scratch: 0, limit: 0)
        try require(try value(mixed, "unique_retained_bytes") == value(one, "unique_retained_bytes") + value(byteOnly, "unique_retained_bytes"),
                    "Mixed history accounting combines both owner families exactly once")
        let global = try RasterResources.shared.report()
        try require(try value(global, "unique_retained_bytes") >= value(mixed, "unique_retained_bytes"), "Global admission includes typed facade and metadata")
        try require(typed.ptr == nil && clone.precision === typed.precision, "Facade clone does not invent a byte allocation")
    }
    try require(released == nil, "Discarded typed facade is not retained by registry")
    let after = try RasterResources.shared.report()
    for key in ["unique_retained_bytes", "host_precision_document_handles", "host_precision_image_handles", "host_precision_capture_payloads"] {
        try require(try value(after, key) == value(baseline, key), "Mixed owner scope releases every retained payload")
    }
    print("PASS: mixed byte/typed history inventory, metadata lease deduplication and complete owner release")
}
