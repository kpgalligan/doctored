import Foundation

/// The ONE canvas geometry job: the Crop straighten and the filtered
/// Rotate Canvas, at both depths, as ONE cancellable compound job on the
/// bounded worker — the core geometry, then the described-artwork re-render,
/// then the display preparation — instead of a second "Rendering … Artwork"
/// wait and a display prepared on main after publication. The prepared edit
/// is published through `ImageDocument.preparedEdit`, so `applyEdit` /
/// `performGroupedEdit` skip their own preparation.
extension RasterDocument {
    /// Runs `command` (a `straighten` or `rotate_canvas` request) under one
    /// job control with progress and Cancel/Esc. The core child is 0…0.6
    /// when there are artwork members (0.6…0.8 re-renders them), else
    /// 0…0.8; the display preparation is 0.8…1. `dropping` loses its
    /// descriptions after the core step. `deletingCroppedPixels` at 8
    /// bits trims each plain raster to the new canvas between the two
    /// (`dr_doc_delete_cropped_pixels`); a typed `straighten` carries the flag
    /// itself. nil is the core's "unchanged"; a refusal is thrown with the
    /// core's sentence, a cancel as `JobCancelled`.
    ///
    /// **Main thread only**: the plan, the 8-bit working-memory budget
    /// (`legacyPrecisionWorkContext()` is main-only) and the parent admission
    /// are read here; the worker captures values only. The panel reveals after
    /// `DocumentJobs.panelDelay`, like the other geometry commits.
    func performingCanvasGeometry(_ command: PrecisionPixelCommand, title: String,
                                  artwork: ArtworkGeometryWork,
                                  dropping: [Int],
                                  deletingCroppedPixels: Bool = false) throws -> PreparedDocumentEdit? {
        assert(Thread.isMainThread)
        let source = self
        let trims = deletingCroppedPixels && !isHighDepth
        let request = command.request
        let coreScratch = try PrecisionDocumentDescriptor.integer(
            geometryEditPlan(request: request), "scratch_bytes")
        let budget: (limit: UInt64, other: UInt64)? =
            isHighDepth ? nil : try legacyPrecisionWorkContext()
        let artworkScratch = artwork.scratchBytes
        let hasArtwork = !artwork.members.isEmpty
        let geometryEnd = hasArtwork ? 0.6 : 0.8
        // Per-stage admission: the stages run one after the
        // other, so the parent is admitted at the largest of them; the
        // artwork stage re-admits itself on the worker against what is then
        // live (the geometry result), and `PreparedDocumentEdit` admits its
        // own display estimate.
        let estimate = max(coreScratch, artworkScratch)
        _ = try RasterResources.shared.admit(scratch: estimate)
        let control = try RasterJobControl()
        guard control.setScratchLimit(estimate) else {
            throw DoctoredCoreError(message: "Could not bound canvas geometry work.")
        }
        let task = try WorkerTask(scratchEstimate: estimate) {
            try control.beginCompoundOperation()
            let geometry = try control.withChildOperation(from: 0, to: geometryEnd) { child in
                guard child.setScratchLimit(coreScratch) else {
                    throw DoctoredCoreError(message: "Could not bound canvas geometry work.")
                }
                return try source.editingGeometry(request: request, budget: budget,
                                                  control: child)
            }
            guard var current = geometry else { return nil as PreparedDocumentEdit? }
            // Before `dropping`: a layer whose description the straighten
            // drops is still described here, so it keeps its whole raster, as
            // the typed straighten (which reads the source layer) leaves it.
            if trims { current = current.deletingCroppedPixels() ?? current }
            for index in dropping {
                current = current.withLayerMeta(index, nil) ?? current
            }
            if hasArtwork {
                let rendered = current
                _ = try RasterResources.shared.admit(scratch: artworkScratch)
                current = try control.withChildOperation(from: geometryEnd, to: 0.8) { child in
                    guard child.setScratchLimit(artworkScratch) else {
                        throw DoctoredCoreError(message: "Could not bound artwork rendering.")
                    }
                    return try artwork.prepareAfter(rendered, control: child)
                }
                current = try current.followingHostPlacement(
                    of: artwork.members.map(\.index), placedBy: rendered)
            }
            let finished = current
            return try control.withChildOperation(from: 0.8, to: 1) { child in
                PreparedDocumentEdit(finished, scratch: BackgroundWork.scratch, control: child)
            }
        }
        let prepared = try BackgroundWork.wait(task, title: title, control: control,
                                               revealAfter: DocumentJobs.panelDelay)
        guard !control.isCancelled else { throw JobCancelled() }
        return prepared
    }
}
