import AppKit

// Content-Aware Move: one job, one undo step, both depths. Move fills the whole outlined
// region content-aware and then heals the moved pixels onto the filled document; Extend is the
// heal alone, today's Patch Destination path. The tool's commit
// (`EditorViewController+ContentAwareMove.contentAwareMoveCommitted`) and `content_aware_move`
// both start it through `ContentAwareMoveWork.start`.

/// One Content-Aware Move, planned and admitted on the main thread against the
/// captured source and computed on the worker.
///
/// FILL FIRST, THEN HEAL. The heal joins the moved pixels to whatever lies
/// under their new place, and when the two places overlap that is the object
/// still standing where it was. So the region is filled first and the heal
/// lands on the filled document. Its source stays the picture BEFORE the fill:
/// at 8 bits the overlay is drawn on main from the plate the gesture captured
/// (`PatchEditCapture.byteOverlay`); at 16 bits the native heal renders its
/// sampling plate from the source document while writing the filled one
/// (`PatchNativeWork.compute(control:onto:)`).
struct ContentAwareMoveWork {
    /// The heal's captured inputs, by depth.
    private enum Heal {
        /// The premultiplied overlay `healJob` takes and, in Move, the region's
        /// u8 coverage the fill takes (empty in Extend).
        case byte(overlay: [UInt8], coverage: [UInt8])
        /// Patch's native Destination work; its `region` samples are also the
        /// fill's coverage.
        case native(PatchNativeWork)
    }

    /// `PatchEditCapture.requiresLayerRGB` in this tool's name.
    static let requiresLayerRGB = "Content-Aware Move requires raster layer RGB; masks, "
        + "channels and Quick Mask are unavailable."

    /// `error` as this tool says it: the shared capture refuses a target that
    /// is not a raster layer's RGB in Patch's name, and the tool and
    /// `content_aware_move` both present it in their own.
    static func reworded(_ error: Error) -> Error {
        guard let refusal = error as? DoctoredCoreError,
              refusal.message == PatchEditCapture.requiresLayerRGB else { return error }
        return DoctoredCoreError(message: requiresLayerRGB)
    }

    let source: RasterDocument
    let layer: Int
    /// The Content-Aware Fill request for the outlined region; nil in Extend.
    let fill: PrecisionPixelCommand?
    private let heal: Heal
    /// What the worker holds outside the core's own reservations: the overlay
    /// and coverage (8 bits) or the placed region (16 bits) and, in Move, the
    /// filled document's rewritten layer, which lives until the heal returns.
    /// The job's `scratch`.
    let heldBytes: UInt64

    /// Main thread. Plans and admits both steps against the captured source:
    /// the heal as Patch's Destination commit does, then the fill as
    /// Content-Aware Fill does (ring 0, the gesture's sampling, in place, its
    /// box the region's bounds). Throws `PatchEditCapture.missesCanvas` when
    /// the displaced region leaves the picture, and the fill's own sentences
    /// for a region it cannot take.
    init(capture: PatchEditCapture, result: PatchSession.Result, extend: Bool,
         seed: UInt64) throws {
        let source = capture.capture.source
        let region = result.region
        guard capture.destination else {
            throw DoctoredCoreError(message: "Content-Aware Move needs a Destination capture.")
        }
        guard region.canvasWidth == source.width, region.canvasHeight == source.height else {
            throw DoctoredCoreError(
                message: "The Content-Aware Move region belongs to a different canvas.")
        }
        self.source = source
        layer = capture.layer
        var command: PrecisionPixelCommand?
        if !extend {
            var settings = ContentAwareFillSettings()
            settings.ring = 0
            settings.seed = seed
            settings.choice = capture.choice
            settings.output = .layer
            var fields = settings.arguments(layer: capture.layer)
            fields["op"] = "content_aware_fill"
            // `bounds` is integral and already clipped to the canvas.
            fields["x"] = Int(region.bounds.minX)
            fields["y"] = Int(region.bounds.minY)
            fields["width"] = Int(region.bounds.width)
            fields["height"] = Int(region.bounds.height)
            command = try PrecisionPixelCommand(fields)
        }
        fill = command
        // The fill's result is a second copy of the target layer for as long
        // as the heal runs on it.
        let info = source.layerInfo(capture.layer)
        let layerPixels = UInt64(max(info?.width ?? 0, 0)) * UInt64(max(info?.height ?? 0, 0))
        if let precision = source.precision {
            let native = try capture.nativeWork(result)
            var held = native.heldBytes
            if let command {
                let context = try PrecisionWorkContext(source: precision)
                let plan = try precision.pixelEditPlan(
                    request: command.request, workingLimit: context.limit,
                    otherRetained: context.otherRetained)
                guard try PrecisionDocumentDescriptor.integer(plan, "input_samples")
                        == UInt64(native.region.count), plan["output_samples"] == nil else {
                    throw DoctoredCoreError(
                        message: "High-depth input samples do not match the planned edit.")
                }
                _ = try RasterResources.shared.admit(
                    scratch: PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"))
                // The held layer copy: 8 bytes a pixel at 16 bits, 16 at 32-bit float.
                held = try PrecisionDocumentDescriptor.sum(held, layerPixels * (source.isFloat ? 16 : 8))
            }
            heal = .native(native)
            heldBytes = held
        } else {
            let overlay = try capture.byteOverlay(result)
            var coverage: [UInt8] = []
            var held = UInt64(overlay.count)
            if let command {
                _ = try RasterResources.shared.admit(
                    scratch: source.contentAwareFillScratch(command.request))
                coverage = region.maskBytes()
                held += UInt64(coverage.count) + layerPixels * 4
            }
            heal = .byte(overlay: overlay, coverage: coverage)
            heldBytes = held
        }
    }

    /// Worker. Move: the fill under one child of the job's control (0–0.7 of
    /// its progress), then the heal under another (0.7–1) on the filled
    /// document, or on the source when the fill changed nothing. Extend: the
    /// heal alone, on the job's own control as Patch runs it. The result is
    /// the healed document, else the filled one, else nil (nothing changed).
    func compute(control: RasterJobControl) throws -> RasterDocument? {
        switch heal {
        case .byte(let overlay, let coverage):
            guard let fill else {
                return try source.healJob(layer, overlay: overlay, strength: 1, control: control)
            }
            let filled = try control.withChildOperation(from: 0, to: 0.7) {
                try source.contentAwareFillEdit(fill.request, coverage: coverage, control: $0)
            }
            if control.isCancelled { throw JobCancelled() }
            let healed = try control.withChildOperation(from: 0.7, to: 1) {
                try (filled ?? source).healJob(layer, overlay: overlay, strength: 1, control: $0)
            }
            return healed ?? filled
        case .native(let native):
            guard let fill else {
                return try native.compute(control: control).map { try RasterDocument(precision: $0) }
            }
            guard let precision = source.precision else {
                throw DoctoredCoreError(message: "Content-Aware Move has no native source owner.")
            }
            let filled = try control.withChildOperation(from: 0, to: 0.7) {
                child -> RasterPrecisionDocument? in
                let context = try PrecisionWorkContext(source: precision)
                return try precision.editingPixels(
                    request: fill.request, samples: native.region, workingLimit: context.limit,
                    otherRetained: context.otherRetained, control: child)
            }
            if control.isCancelled { throw JobCancelled() }
            let healed = try control.withChildOperation(from: 0.7, to: 1) {
                child -> RasterPrecisionDocument? in
                if let filled { return try native.compute(control: child, onto: filled) }
                return try native.compute(control: child)
            }
            return try (healed ?? filled).map { try RasterDocument(precision: $0) }
        }
    }

    /// Starts the gesture's one job, titled "Content-Aware Move". The captured
    /// source must still be the document's (`requireCurrent`), as Patch's
    /// starts require.
    static func start(capture: PatchEditCapture, result: PatchSession.Result, extend: Bool,
                      seed: UInt64, document: ImageDocument, record: [ActionStep],
                      showProgress: Bool) throws -> DocumentJob {
        let work = try ContentAwareMoveWork(
            capture: capture, result: result, extend: extend, seed: seed)
        try capture.capture.requireCurrent(document)
        return try DocumentJobs.start(
            document: document, layer: work.layer, title: "Content-Aware Move",
            scratch: work.heldBytes, record: record, showProgress: showProgress,
            compute: { _, control in try work.compute(control: control) })
    }
}
