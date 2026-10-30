import AppKit

/// Override only numeric capacity descriptors on a tiny, otherwise valid input.
/// This exercises production refusal paths without allocating a large fixture.
private func capacityState(
    _ source: EditingStateArchive.Captured, retained: UInt64, decode: UInt64, projection: UInt64
) throws -> EditingStateArchive.Captured {
    let encoded = try JSONEncoder().encode(source.manifest)
    guard var json = try JSONSerialization.jsonObject(with: encoded) as? [String: Any] else {
        throw ProbeFailure(description: "Capacity manifest fixture")
    }
    json["decodeEstimateBytes"] = decode
    let manifest = try JSONDecoder().decode(EditingStateArchive.Manifest.self,
        from: JSONSerialization.data(withJSONObject: json))
    return EditingStateArchive.Captured(document: source.document, manifest: manifest,
        masks: source.masks, scratchEstimate: source.scratchEstimate,
        reopenedRetainedEstimate: retained, projectionScratchEstimate: projection)
}

private func capacityCapture(_ state: EditingStateArchive.Captured,
                             snapshots: [EditingStateArchive.Captured] = [],
                             id: String = UUID().uuidString,
                             previous: RecoveryStore.Manifest? = nil) -> RecoveryStore.Captured {
    RecoveryStore.Captured(id: id, title: "Capacity Probe", sourceURL: nil, sourceType: nil,
        generation: RecoveryStore.Generation(folder: UUID().uuidString, revision: 1,
            serial: previous == nil ? 1 : 2, captured: Date()), previous: previous, state: state,
        snapshots: snapshots.enumerated().map { index, state in
            NamedSnapshots.Archived(header: NamedSnapshots.Header(version: 1,
                name: "Same Source \(index)", created: Date()), state: state)
        })
}

private func capacityRefuses(_ captured: RecoveryStore.Captured, limit: UInt64) -> Bool {
    do { _ = try RecoveryCapacity.validate(captured, limit: limit); return false }
    catch { return true }
}

func verifyRecoveryCapacity() throws {
    let document = try fixture(width: 24, height: 16)
    defer { document.close() }
    guard let state = document.captureEditingState(),
          let rootPath = ProcessInfo.processInfo.environment["DR_HISTORY_PROBE_DIR"] else {
        throw ProbeFailure(description: "Capacity probe has no document/private directory")
    }
    let base = try EditingStateArchive.capture(state)
    let first = try capacityState(base, retained: 100, decode: 140, projection: 60)
    let second = try capacityState(base, retained: 200, decode: 350, projection: 99_999)
    let one = capacityCapture(first)
    let repeated = capacityCapture(first, snapshots: [first])
    try require(one.state.document === repeated.snapshots[0].state.document,
                "Capacity fixture genuinely shares the same immutable source handle")
    let onePeak = try RecoveryCapacity.validate(one, limit: 161)
    try require(onePeak == 160, "One source needs 100 retained + max(40 temporary, 60 projection)")
    try require(capacityRefuses(repeated, limit: 201),
                "Serializing the same source twice must count twice despite in-memory sharing")
    let repeatedPeak = try RecoveryCapacity.validate(repeated, limit: 261)
    try require(repeatedPeak == 260, "Two independently reopened 100-byte sources plus one temporary/projected peak")

    // 100 + 200 + 100 permanently retained, then the sequential stages: a
    // maximum 150-byte temporary decode allocation, or only the CURRENT
    // image's 60-byte projection (max(), not a sum).
    let varied = capacityCapture(first, snapshots: [second, first])
    let variedPeak = try RecoveryCapacity.validate(varied, limit: 551)
    try require(variedPeak == 550, "Sequential decode and projection use their maximum; snapshot projections are not rendered")
    try require(capacityRefuses(varied, limit: 550) && capacityRefuses(varied, limit: 549)
                && capacityRefuses(varied, limit: 0),
                "The admission boundary is strictly below the working limit, including zero")
    do {
        _ = try RecoveryCapacity.validate(varied, limit: 550)
        throw ProbeFailure(description: "Varied recovery set admitted at its own peak")
    } catch let refusal as RecoveryCapacity.Refusal {
        try require(refusal.stage == .reopen && refusal.neededBytes == variedPeak && refusal.limitBytes == 550
                    && refusal.message.hasPrefix(RecoveryCapacity.offPrefix),
                    "A reopen refusal is a .reopen Refusal reporting the peak it needs")
    }
    print("PASS: recovery capacity independently counts duplicated sources, retained sum, sequential peak and strict admission boundary")

    let directory = URL(fileURLWithPath: rootPath).appendingPathComponent("capacity-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let id = UUID().uuidString
    let entry = try RecoveryStore.directory(root: directory, id: id)
    guard let lease = try RecoveryLease.acquire(directory: entry, create: true) else {
        throw ProbeFailure(description: "Capacity probe could not obtain its own recovery lease")
    }
    defer { lease.release() }
    let good = capacityCapture(base, id: id)
    guard let manifest = try RecoveryStore.write(good, directory: entry, gate: RecoveryPublicationGate()) else {
        throw ProbeFailure(description: "Initial capacity checkpoint did not publish")
    }
    let manifestURL = entry.appendingPathComponent("manifest.json")
    let before = try Data(contentsOf: manifestURL)
    let children = try FileManager.default.contentsOfDirectory(atPath: entry.path).sorted()
    let fraction = RasterResources.shared.workingLimit / 16
    let inflated = try capacityState(base, retained: fraction, decode: fraction + 1, projection: 0)
    let tooLarge = capacityCapture(inflated, snapshots: Array(repeating: inflated, count: 32),
                                   id: id, previous: manifest)
    var refused = false
    do { _ = try RecoveryStore.write(tooLarge, directory: entry, gate: RecoveryPublicationGate()) }
    catch { refused = error.localizedDescription.contains("when reopened") }
    let after = try Data(contentsOf: manifestURL)
    let remaining = try FileManager.default.contentsOfDirectory(atPath: entry.path).sorted()
    try require(refused && before == after && remaining == children,
                "A recovery set over the real working limit refuses before creating files or replacing the previous manifest")
    let loaded = try RecoveryStore.read(RecoveryStore.Candidate(id: id, directory: entry,
        manifest: manifest, failure: nil))
    try require(loaded.documentMatches(state.document), "Previous published pixels remain readable after capacity refusal")
    print("PASS: production recovery publication refuses an oversized shared-source set before touching the prior checkpoint")
}

private extension RecoveryStore.Loaded {
    func documentMatches(_ expected: RasterDocument) -> Bool {
        state.document.width == expected.width && state.document.height == expected.height
            && state.document.layerPixels(0) == expected.layerPixels(0)
    }
}
