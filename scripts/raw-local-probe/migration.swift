import AppKit

/// Genuine four-local migration uses the same admitted parent through every
/// camera and native stage. This drives cancellation during the native stage.
func runRawLocalMigrationProbe(source: RasterDocument, directory: URL) throws -> [String: Any] {
    let saved = try RawLocalWork.state(source, layer: 0)
    try localCheck(saved.nodes.count == 4 && saved.width == 3008 && saved.height == 2000,
        "Migration pressure fixture must remain genuine full-size/four-local")
    let forged = RawSourceDescriptor(schema_version: saved.descriptor.schema_version,
        original_filename: saved.descriptor.original_filename, content_sha256: saved.descriptor.content_sha256,
        decoder: .init(identifier: saved.descriptor.decoder.identifier, version: saved.descriptor.decoder.version,
            implementation: "unavailable-four-local-migration-probe"), recipe: saved.descriptor.recipe)
    let unavailable = try RawSourceWork.replacing(source: source, layer: 0,
        asset: source.rawLocalBaseSource(layer: 0), descriptor: forged)
    let owner = localDocument(unavailable)
    defer { owner.close() }
    let revision = owner.revision
    let info = try unavailable.retainedSourceInfo(0) ?? [:]
    let encoded = try PrecisionDocumentDescriptor.integer(info["raw"] as? [String: Any] ?? [:], "encoded_bytes")
    let budget = try RawLocalWork.scratch(width: saved.width, height: saved.height, encoded: encoded)
    let migrationBudget = try PrecisionDocumentDescriptor.sum(budget,
        UInt64(saved.width) * UInt64(saved.height) * 8)
    let ordinary = try RawSourceWork.scratch(width: saved.width, height: saved.height, encoded: encoded)
    try localCheck(budget == ordinary + UInt64(saved.width * saved.height) * 56 + 4 * 1024 * 1024,
        "Migration and ordinary local work share the full peak plan")
    let available = try RasterResources.shared.admit(scratch: 0)
    try localCheck(available > migrationBudget + 16 * 1024 * 1024, "Migration admission fixture needs spare metadata capacity")
    var pressure: PrecisionEditPayload? = try PrecisionEditPayload.admitted(bytes: available - migrationBudget + 1)
    try withExtendedLifetime(pressure) {
        try localRefuses("Four-local migration denied at its whole-pipeline admission") {
            _ = try RawDecoderMigration.prepare(in: owner, layer: 0)
        }
    }
    pressure = nil
    try localCheck(owner.doc === unavailable && owner.revision == revision && !owner.isDocumentEdited,
        "Denied migration preserves source/revision/dirty state")

    let control = try RasterJobControl()
    var cancelledNative = false, observedScratch: UInt64 = 0
    let timer = Timer(timeInterval: 0.002, repeats: true) { _ in
        if control.scratchBytes > 0 {
            observedScratch = max(observedScratch, control.scratchBytes)
            cancelledNative = true
            control.cancel()
        }
    }
    RunLoop.main.add(timer, forMode: .common); RunLoop.main.add(timer, forMode: .modalPanel)
    let started = Date()
    defer { timer.invalidate() }
    var unexpectedCompletion = false
    var cancellationReason = ""
    do {
        _ = try RawDecoderMigration.prepare(in: owner, layer: 0, control: control)
        unexpectedCompletion = true
    } catch { cancellationReason = error.localizedDescription }
    timer.invalidate()
    try localCheck(!unexpectedCompletion, "Four-local migration unexpectedly completed during cancellation")
    try localCheck(cancelledNative && observedScratch > 0 && observedScratch <= migrationBudget && control.isCancelled,
        "The same user cancellation owner reaches the bounded native composition stage; observed \(observedScratch), declared \(migrationBudget), refusal \(cancellationReason)")
    try localCheck(owner.doc === unavailable && owner.revision == revision && !owner.isDocumentEdited,
        "Cancelled migration preserves source/revision/dirty state")
    try localCheck(control.scratchBytes == 0, "Cancelled migration native scratch released")

    let candidate = try RawDecoderMigration.prepare(in: owner, layer: 0)
    let reviewed = try localPixels(candidate.asset, retained: false)
    try candidate.apply(in: owner)
    guard let result = owner.doc else { throw LocalProbeFailure(description: "Four-local migration lost its document") }
    try localCheck(try localPixels(result) == reviewed, "Four-local migration publishes exact reviewed pixels")
    let migrated = try RawLocalWork.state(result, layer: 0)
    for (before, after) in zip(saved.masks, migrated.masks) {
        try localCheck(try before.sha256() == after.sha256(), "Four-local migration preserves exact native planes")
    }
    owner.undoManager?.undo()
    try localCheck(owner.doc === unavailable, "Four-local migration is one complete Undo")
    let revisionBeforeClose = owner.revision
    let closeControl = try RasterJobControl()
    let closeTimer = Timer(timeInterval: 0.25, repeats: false) { _ in owner.close() }
    RunLoop.main.add(closeTimer, forMode: .common); RunLoop.main.add(closeTimer, forMode: .modalPanel)
    defer { closeTimer.invalidate() }
    try localRefuses("Owner close cancels in-flight four-local migration") {
        _ = try RawDecoderMigration.prepare(in: owner, layer: 0, control: closeControl)
    }
    try localCheck(closeControl.isCancelled && closeControl.scratchBytes == 0,
        "Owner close reaches the same cancellation control and releases native scratch")
    try localRefuses("Closed migration owner cannot create a new capture") { _ = try ImageSourceCapture(owner) }
    try localCheck(owner.doc === unavailable && owner.revision == revisionBeforeClose && !owner.isDocumentEdited,
        "Closing migration owner cannot publish a candidate")
    return ["passed": true, "group_count": 4, "budget_bytes": migrationBudget,
        "cancelled_native_scratch_bytes": observedScratch, "elapsed_seconds": Date().timeIntervalSince(started),
        "groups": ["genuine_four_local_migration_full_plan_refusal", "same_parent_native_stage_cancel_cleanup",
                   "genuine_four_local_review_exact_acceptance_masks_and_undo", "owner_close_cancels_remaining_camera_stages"]]
}
