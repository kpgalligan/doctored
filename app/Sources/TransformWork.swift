import AppKit

// The preview's `TransformWork` carries `interactive: true`, so its core request keeps the
// interactive work limit while the commit's (default `false`) takes its caller's cap, which is none
// under a job control; `TransformWork.wait` asserts it never runs a preview. A preview whose
// interactive plan is refused runs `nonInteractive()`, the commit's request, as its cancellable
// preview job (`TransformMaskPreview.request`).

/// The raster-only final transform seam. AppKit rendering of described sources
/// stays in the existing main-thread compose path. This value captures only an
/// immutable core document, geometry, sampler and description indices to drop.
struct TransformWork {
    let source: RasterDocument
    let layers: [Int]
    let quad: [CGPoint]?
    let matrix: CGAffineTransform
    let sampler: DrResizeFilter
    let droppingDescriptions: [Int]
    let sourceRect: CGRect?
    let artwork: ArtworkGeometryWork?
    /// A ⌘T PREVIEW: its core request carries `interactive` and keeps
    /// the interactive work limit; every commit leaves it false.
    let interactive: Bool

    init(source: RasterDocument, layers: [Int], quad: [CGPoint]?, matrix: CGAffineTransform,
         sampler: DrResizeFilter, droppingDescriptions: [Int], sourceRect: CGRect? = nil,
         rasterizeArtwork: Bool = false, interactive: Bool = false) throws {
        self.source = source
        self.interactive = interactive
        self.layers = layers
        self.quad = quad
        self.matrix = matrix
        self.sampler = sampler
        self.droppingDescriptions = droppingDescriptions
        self.sourceRect = sourceRect
        if source.movingSet(layers).contains(where: source.hasArtworkDescription) {
            guard Thread.isMainThread else {
                throw DoctoredCoreError(message: "Capture artwork before transform work starts.")
            }
            artwork = try ArtworkGeometryWork(source: source, request: Self.request(
                source: source, layers: layers, quad: quad, matrix: matrix,
                sampler: sampler, sourceRect: sourceRect),
                                              rasterizeArtwork: rasterizeArtwork)
        } else {
            artwork = nil
        }
    }

    private init(copying work: TransformWork, interactive: Bool) {
        source = work.source
        layers = work.layers
        quad = work.quad
        matrix = work.matrix
        sampler = work.sampler
        droppingDescriptions = work.droppingDescriptions
        sourceRect = work.sourceRect
        artwork = work.artwork
        self.interactive = interactive
    }

    /// This work as the commit would send it: the same captured
    /// source, geometry and artwork with `interactive` false, nothing
    /// re-captured. A preview whose interactive plan is refused runs it as a
    /// cancellable preview job, because the commit's plan carries no work
    /// refusal.
    func nonInteractive() -> TransformWork {
        TransformWork(copying: self, interactive: false)
    }

    private func setRequest() throws -> GeometrySetRequest {
        try Self.request(source: source, layers: layers, quad: quad, matrix: matrix,
                         sampler: sampler, sourceRect: sourceRect)
    }

    private static func request(source: RasterDocument, layers: [Int], quad: [CGPoint]?,
                                matrix: CGAffineTransform, sampler: DrResizeFilter,
                                sourceRect: CGRect?) throws -> GeometrySetRequest {
        if let quad {
            let rect: CGRect
            if let sourceRect {
                rect = sourceRect
            } else if layers.count == 1, let info = source.layerInfo(layers[0]) {
                rect = CGRect(x: info.offsetX, y: info.offsetY,
                              width: info.width, height: info.height)
            } else {
                throw DoctoredCoreError(message: "A transform set needs a captured source rectangle.")
            }
            return .perspective(layers: layers, sourceRect: rect, quad: quad, sampler: sampler)
        }
        return .affine(layers: layers, matrix: matrix, sampler: sampler)
    }

    /// The two sequential stages' scratch: the core geometry plan
    /// (destination raster/mask buffers) and the described-artwork re-render,
    /// 0 without artwork members. The display preparation admits its own.
    /// Platform allocator and GPU overhead remains outside these estimates.
    func stageEstimates() throws -> (core: UInt64, artwork: UInt64) {
        let plan = try source.geometryEditPlan(
            request: setRequest().command(interactive: interactive).request)
        return (try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"),
                artwork?.scratchBytes ?? 0)
    }

    /// Core + artwork, a preview's whole first-stage figure: its worker never
    /// admits, so it keeps the sum (reserved as core, with artwork held as a
    /// payload lease).
    func scratchEstimate() throws -> UInt64 {
        let stages = try stageEstimates()
        return try PrecisionDocumentDescriptor.sum(stages.core, stages.artwork)
    }

    /// Shared by final jobs and the exact masked-document preview. The source
    /// and geometry are immutable; no AppKit description rendering occurs here.
    func transformedDocument(control: RasterJobControl? = nil) -> RasterDocument? {
        try? transformedDocumentChecked(control: control)
    }

    /// `admittingArtwork` re-admits the artwork stage on the worker, just
    /// before it runs (`geometrySetPrepared`); only `wait` passes true.
    func transformedDocumentChecked(control: RasterJobControl? = nil,
                                    admittingArtwork: Bool = false) throws -> RasterDocument? {
        let result = try source.geometrySetPrepared(setRequest(), artwork: artwork, control: control,
                                                    admittingArtwork: admittingArtwork,
                                                    interactive: interactive)
        let losses = droppingDescriptions.filter {
            !source.hasArtworkDescription($0) || artwork?.perspectiveLoss == true
        }
        return result?.droppingDescriptions(in: losses)
    }

    /// Runs under a `RasterJobControl`: Cancel/Esc stops the core kernel
    /// (child 0…0.8) or the display preparation (child 0.8…1), and a cancel
    /// surfaces as `JobCancelled` with the document untouched. A changed
    /// document never receives the old result. The panel appears only after
    /// `DocumentJobs.panelDelay`, so a short transform never flashes it.
    ///
    /// Per-stage admission (`CanvasGeometryJob`'s model): the stages
    /// run one after the other, so the parent is admitted at the largest of
    /// them, the artwork stage re-admits itself on the worker against what is
    /// then live, and `PreparedDocumentEdit` admits its own display. A true
    /// perspective set (`perspectiveLoss`) keeps the core + artwork sum: its
    /// re-rendered artwork is published before the core stage and stays live
    /// through it.
    ///
    /// `base` is the handle the document must hold at entry and after the
    /// job; nil means `source`. Free Transform a Copy transforms a
    /// duplicate of the document, never the document's own handle.
    func wait(in document: ImageDocument, title: String,
              base: RasterDocument? = nil) throws -> PreparedDocumentEdit? {
        assert(Thread.isMainThread)
        assert(!interactive)
        let base = base ?? source
        guard document.doc === base, !document.isLiveEditing else {
            throw DoctoredCoreError(message: "The document changed before the transform started.")
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let stages = try stageEstimates()
        let estimate = try artwork?.perspectiveLoss == true
            ? PrecisionDocumentDescriptor.sum(stages.core, stages.artwork)
            : max(stages.core, stages.artwork)
        _ = try RasterResources.shared.admit(scratch: estimate)
        let control = try RasterJobControl()
        guard control.setScratchLimit(estimate) else {
            throw DoctoredCoreError(message: "Could not bound transform work.")
        }
        let result = try BackgroundWork.wait(
            title: title, control: control, revealAfter: DocumentJobs.panelDelay
        ) {
            try WorkerTask(scratchEstimate: estimate) { [self] in
                try control.beginCompoundOperation()
                let transformed = try control.withChildOperation(from: 0, to: 0.8) { child in
                    guard child.setScratchLimit(estimate) else {
                        throw DoctoredCoreError(message: "Could not bound transform work.")
                    }
                    return try transformedDocumentChecked(control: child, admittingArtwork: true)
                }
                guard let transformed else { return nil as PreparedDocumentEdit? }
                return try control.withChildOperation(from: 0.8, to: 1) { child in
                    PreparedDocumentEdit(transformed, scratch: BackgroundWork.scratch, control: child)
                }
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.revision == revision, document.doc === base, !document.isLiveEditing,
              (!source.isHighDepth && source.retainedSourceIndices.isEmpty)
                || document.isCurrentEditingState(state) else {
            throw DoctoredCoreError(
                message: "The document changed while the transform ran. Run the transform again.")
        }
        return result
    }
}
