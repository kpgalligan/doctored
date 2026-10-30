import AppKit

private struct OutsideMaskRaw: Equatable {
    let mask: [UInt16]
    let pixels: [UInt16]
}

private func outsideMaskRaw(_ owner: RasterDocument) throws -> OutsideMaskRaw {
    if let source = owner.precision {
        guard let image = source.layerSource(0) else { throw ProbeFailure(description: "Missing native mask source") }
        return try OutsideMaskRaw(mask: PrecisionScalarOperations.read(owner, target: "mask", index: 0).samples,
                                  pixels: image.withRGBA16 { Array($0) })
    }
    guard let storage = owner.layerMaskStorage(0),
          let bytes = RasterDocument.from(image: storage.image)?.layerPixels(0),
          let pixels = owner.layerPixels(0) else { throw ProbeFailure(description: "Missing byte mask source") }
    return OutsideMaskRaw(mask: stride(from: 0, to: bytes.count, by: 4).map { UInt16(bytes[$0]) },
                          pixels: pixels.map(UInt16.init))
}

private func outsideMaskOwner(_ url: URL) throws -> ImageDocument {
    let owner = try RasterDocument.open(url: url)
    let document = try fixture(width: 4, height: 3)
    withUndoGroup(document.undoManager) {
        document.applyEdit("Outside mask fixture", record: .notACommand,
                           intentionalDepthConversion: true) { _ in owner }
    }
    document.setLayerSelection(.single(0))
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    return document
}

private func checkOutsideMask(_ owner: RasterDocument, raw: OutsideMaskRaw) throws {
    guard let p = owner.layerMaskProperties(0) else { throw ProbeFailure(description: "Missing outside mask properties") }
    let actual = try outsideMaskRaw(owner)
    try require(p.outsideCoverage == 255 && p.density == 0.5 && p.feather == 1 && actual == raw,
                "Mask metadata, exact raw samples and hidden source codes survive archive")
}

func verifyMaskOutsideCoverage() throws {
    guard let input = ProcessInfo.processInfo.environment["DR_MASK_FOUNDATION_FIXTURES"],
          let workspace = ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] else {
        throw ProbeFailure(description: "Outside mask probe requires independently authored fixture directory")
    }
    let directory = URL(fileURLWithPath: workspace)
    for depth in [8, 16] {
        let url = URL(fileURLWithPath: input).appendingPathComponent("\(depth)-outside-white.rz")
        let document = try outsideMaskOwner(url), replay = try outsideMaskOwner(url)
        defer { _ = ActionRecorder.shared.stop(); document.close(); replay.close() }
        let controller = try editor(document)
        guard let original = document.doc, let properties = original.layerMaskProperties(0) else {
            throw ProbeFailure(description: "Outside mask facade missing")
        }
        let raw = try outsideMaskRaw(original)
        try require(properties.outsideCoverage == 255 && properties.offsetX == 1 && properties.offsetY == 1,
                    "Independent native fixture preserves mask background and rectangle")
        try require(raw.mask == (depth == 8 ? [0,63,127,255] : [0,123,32769,65535]),
                    "Independently authored exact mask samples enter host")
        _ = try call(document, "start_recording", ["discard": true])
        let sheet = try MaskPropertiesSheetController(document: document, canvas: controller.canvas)
        try present(sheet, in: controller)
        func set(_ id: String, _ value: String) throws {
            guard let field = allViews(sheet.view).compactMap({ $0 as? NSTextField })
                .first(where: { $0.identifier?.rawValue == id }) else { throw ProbeFailure(description: "Missing \(id)") }
            field.stringValue = value
            _ = field.sendAction(field.action, to: field.target)
        }
        try set("maskProperties.density", "50")
        try set("maskProperties.feather", "1")
        try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 })
        try require(document.doc === original && !document.isDocumentEdited, "Mask preview leaves immutable owner unchanged")
        guard let apply = allViews(sheet.view).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Apply" }) else {
            throw ProbeFailure(description: "Mask Apply control missing")
        }
        let errors = try capturedPresentationError { apply.performClick(nil) }
        try require(errors.isEmpty, "Actual mask Apply error: \(errors)")
        guard let modified = document.doc else { throw ProbeFailure(description: "Missing edited outside mask") }
        try checkOutsideMask(modified, raw: raw)
        try require(document.isDocumentEdited, "Actual mask Apply marks document dirty")
        _ = try call(document, "stop_recording")
        let steps = ActionRecorder.shared.steps
        try require(steps.count == 1 && steps[0].tool == "set_mask_properties", "Existing mask UI records one canonical Action")
        let played = ActionPlayer.run(Action(name: "Outside mask properties", steps: steps), on: replay)
        try require(played.ok, "Outside mask Action replay succeeds")
        guard let replayed = replay.doc else { throw ProbeFailure(description: "Missing replay owner") }
        try checkOutsideMask(replayed, raw: raw)
        document.undoManager?.undo()
        try require(document.doc === original && !document.isDocumentEdited && document.undoManager?.canUndo != true,
                    "One Undo restores exact original outside-mask owner and clean identity")
        document.undoManager?.redo()
        try require(document.doc === modified, "Redo restores exact edited outside-mask owner")
        let path = directory.appendingPathComponent("outside-mask-\(depth).rz")
        try modified.saveNative(to: path)
        let bytes = try Data(contentsOf: path)
        // Since 12E-1 the identity word (45 byte / 46 typed) is ALWAYS the
        // outermost version (`identity_native.rs`), so the conditional
        // outside-mask word 28/29 sits further down the leading chain of
        // wrapper version words, and its `MOUT` table follows the tails.
        let words = leadingVersionWords(bytes)
        let identity: UInt32 = depth == 8 ? 45 : 46
        let outside: UInt32 = depth == 8 ? 28 : 29
        let table = bytes.range(of: Data("MOUT".utf8)) != nil
        try require(words.first == identity && words.contains(outside) && table,
                    "Outside mask uses conditional native wrapper: \(words)")
        try checkOutsideMask(RasterDocument.open(url: path), raw: raw)
        let snapshot = try document.namedSnapshots.create(name: "Outside mask")
        let snapshotURL = directory.appendingPathComponent("outside-mask-\(depth).drsnapshot")
        try document.namedSnapshots.save(id: snapshot.id, to: snapshotURL)
        let restored = try document.namedSnapshots.load(from: snapshotURL)
        try checkOutsideMask(restored.state.document, raw: raw)
        try verifyOutsideMaskRecovery(document, directory: directory, raw: raw)
    }
    print("PASS: byte28/native29 independent masks, actual density/feather UI, Actions, exact-owner Undo/Redo, exact raw codes, reopen, snapshot and private recovery")
}

/// The first sixteen little-endian u32 words after a `.rz` magic: the chain of
/// conditional wrapper version words, outermost first. Since 12E-1 the
/// identity word (45 byte / 46 typed) always leads it (`identity_native.rs`),
/// so a wrapper's own word is looked for further down the chain.
func leadingVersionWords(_ bytes: Data) -> [UInt32] {
    var words: [UInt32] = []
    for offset in stride(from: 4, to: min(bytes.count - 3, 4 + 4 * 16), by: 4) {
        let low = UInt32(bytes[offset]) | (UInt32(bytes[offset + 1]) << 8)
        let high = (UInt32(bytes[offset + 2]) << 16) | (UInt32(bytes[offset + 3]) << 24)
        words.append(low | high)
    }
    return words
}

/// Whether `run` appears as consecutive words of `leadingVersionWords`.
func versionChain(_ words: [UInt32], contains run: [UInt32]) -> Bool {
    words.indices.contains { start in
        start + run.count <= words.count && Array(words[start..<(start + run.count)]) == run
    }
}

private func verifyOutsideMaskRecovery(_ document: ImageDocument, directory: URL, raw: OutsideMaskRaw) throws {
    guard let state = document.captureEditingState() else { throw ProbeFailure(description: "Missing outside mask editing state") }
    let captured = try EditingStateArchive.capture(state)
    let id = UUID().uuidString
    let location = try RecoveryStore.directory(root: directory.appendingPathComponent("outside-mask-recovery"), id: id)
    guard let lease = try RecoveryLease.acquire(directory: location, create: true) else {
        throw ProbeFailure(description: "Private outside mask recovery lease")
    }
    defer { lease.release() }
    let recovery = RecoveryStore.Captured(id: id, title: "Outside mask recovery", sourceURL: nil, sourceType: nil,
        generation: .init(folder: UUID().uuidString, revision: document.revision, serial: 1, captured: Date()),
        previous: nil, state: captured, snapshots: [])
    _ = try RecoveryCapacity.validate(recovery)
    let gate = RecoveryPublicationGate()
    let published = try BackgroundWork.wait(WorkerTask(scratchEstimate: recovery.scratchEstimate) {
        try RecoveryStore.write(recovery, directory: location, gate: gate)
    }, title: "Writing Outside Mask Recovery")
    let candidate = RecoveryStore.Candidate(id: id, directory: location, manifest: published, failure: nil)
    let read = try BackgroundWork.wait(WorkerTask { try RecoveryStore.read(candidate) }, title: "Reading Outside Mask Recovery")
    try checkOutsideMask(read.state.document, raw: raw)
}
