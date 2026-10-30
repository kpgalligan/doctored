import AppKit

func verifyNativeCutoutPersistence(_ document: ImageDocument, directory: URL,
                                   exact: [UInt16], matte: [UInt16],
                                   selection: SelectionHistoryState) throws {
    let limit = RasterResources.shared.workingLimit
    func check(_ owner: RasterDocument) throws {
        guard let native = owner.precision, let image = native.layerSource(1) else {
            throw ProbeFailure(description: "Native cutout archive lost its source")
        }
        let pixels = try image.withRGBA16 { Array($0) }
        let plane = try native.readingPlane(request: #"{"op":"read_plane","target":"mask","index":1}"#,
            workingLimit: limit, otherRetained: 0)
        try require(pixels == exact && plane == matte && owner.layerInfo(0)?.visible == false,
                    "Archive preserves independent foreground/source alpha/matte and hidden originals")
    }
    let snapshot = try document.namedSnapshots.create(name: "Native cutout")
    let snapshotURL = directory.appendingPathComponent("native-cutout.drsnapshot")
    try document.namedSnapshots.save(id: snapshot.id, to: snapshotURL)
    let loaded = try document.namedSnapshots.load(from: snapshotURL)
    try check(loaded.state.document)
    try require(loaded.state.selection == selection, "Named snapshot retains exact native selection coverage")
    guard let state = document.captureEditingState() else { throw ProbeFailure(description: "Missing cutout editing state") }
    let captured = try EditingStateArchive.capture(state)
    let id = UUID().uuidString
    let recoveryDirectory = try RecoveryStore.directory(root: directory.appendingPathComponent("native-recovery"), id: id)
    guard let lease = try RecoveryLease.acquire(directory: recoveryDirectory, create: true) else {
        throw ProbeFailure(description: "Missing private recovery lease")
    }
    defer { lease.release() }
    let recovery = RecoveryStore.Captured(id: id, title: "Native cutout recovery", sourceURL: nil, sourceType: nil,
        generation: .init(folder: UUID().uuidString, revision: document.revision, serial: 1, captured: Date()),
        previous: nil, state: captured,
        snapshots: [.init(header: .init(version: 1, name: "Native cutout snapshot", created: Date()), state: captured)])
    _ = try RecoveryCapacity.validate(recovery)
    let gate = RecoveryPublicationGate()
    let published = try BackgroundWork.wait(WorkerTask(scratchEstimate: recovery.scratchEstimate) {
        try RecoveryStore.write(recovery, directory: recoveryDirectory, gate: gate)
    }, title: "Writing Native Cutout Recovery")
    let candidate = RecoveryStore.Candidate(id: id, directory: recoveryDirectory, manifest: published, failure: nil)
    let recovered = try BackgroundWork.wait(WorkerTask { try RecoveryStore.read(candidate) }, title: "Reading Native Cutout Recovery")
    try check(recovered.state.document)
    let recoveredState = try recovered.state.editingState()
    try require(recoveredState.selection == selection, "Recovery retains exact native selection coverage")
    try require(recovered.snapshots.count == 1, "Recovery retains the named native cutout snapshot")
    try check(recovered.snapshots[0].state.document)
    let recoveredSnapshot = try recovered.snapshots[0].state.editingState()
    try require(recoveredSnapshot.selection == selection,
                "Recovered snapshot retains exact native selection coverage")
    print("PASS: named native snapshot save/load and private recovery writer/reader preserve exact source alpha plus separate refinement coverage")
}
