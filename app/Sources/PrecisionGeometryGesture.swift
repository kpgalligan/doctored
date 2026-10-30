import AppKit

// Native projection is a display derivative; the byte plate never becomes the source. A stale
// document/target/selection refuses on release, and cancellation retains each preview reservation
// until its worker exits. `finish` is ONE compound job and ONE wait — geometry, the
// transformed selection, then the 16-bit display preparation (`PreparedDocumentEdit` on a child) —
// published through `PrecisionEditorCapture.publish(prepared:)` → `ImageDocument.applyPrepared`
// (the publication half of `applyPreparedPrecision`, no second wait). `SelectionWorkBudget`
// registers reservation IDs; the gesture snapshots the current registered ID immediately before
// terminal cancellation and the lifecycle probe waits for that ID and all older owner IDs to drain.
// Float admits integer Move only.
//
// Both preview stages (`request`, `preparePreview`) run as
// `SelectionWorkBudget.Reservation.runAsOwnLease`, so the preview worker's `otherRetained` does not
// count its own lease beside its core plan, and `request` reserves only the core stage
// (`stageEstimates().core`) and holds an artwork stage's scratch as a `PrecisionEditPayload` lease
// for the worker's run.
//
// `finishOutcome` and `TransformWork.wait` admit their largest sequential stage (`stageEstimates()`,
// `CanvasGeometryJob`'s model; a true perspective set keeps core + artwork summed), with
// `admittingArtwork: true` re-admitting the artwork stage on the worker and no display term;
// previews keep the sum (`scratchEstimate()`) and never admit on their worker.
//
// A ⌘T or Crop straighten whose true preview plan is refused previews at reduced
// resolution (`GeometryProxyPreview.swift`): the request is mapped onto a proxy document built
// once per gesture off main, sized from the commit plan's `work_units`, and lands with
// `GeometryProxyPreview.status` and `imageIsReduced`. `finishOutcome` discards the proxy loader
// (waiting for its build) before the commit admits; the commit never sees a proxy.
//
// A preview is never refused for work: a request with no proxy (a float document, a failed proxy
// build or plan) runs the COMMIT request, whose plan carries no work refusal, as its cancellable
// preview job, at full size, and reports that plan's own error when it is refused in its turn
// (memory, or the first error again).

/// A typed gesture owns one editor capture from press/session open to release.
/// Preview and final use the same value-only work builder and native source.
final class PrecisionGeometryGesture {
    let capture: PrecisionEditorCapture
    private weak var document: ImageDocument?
    private let renderer = PreviewRenderer()
    private let initialImage: CGImage?
    /// The transformed document as the image to show, in place of its
    /// display projection (a plane transform previewed over that plane).
    /// Off-main.
    private let display: ((RasterDocument) -> CGImage?)?
    /// What the work runs on in place of the captured document — a
    /// continued Move float's source. `capture` still checks currency against
    /// the live document. nil = the captured document.
    private let workSource: RasterDocument?
    /// #1 Indexed phase 3b: the background swatch every work this gesture makes carries
    /// (`PrecisionGeometryWork.indexedBackground`); nil is white.
    private let indexedBackground: NSColor?
    private var generation: UInt64 = 0
    private var active = true
    private var previewReservationIDs = Set<UUID>()
    private var currentPreviewReservationID: UUID?
    private var terminalCancelledReservationIDs = Set<UUID>()
    private(set) var previewReservationsIssued = 0
    var outstandingPreviewReservations: Int {
        previewReservationIDs.filter(SelectionWorkBudget.contains).count
    }
    var activeReservationsAtTerminalCancellation: Int {
        terminalCancelledReservationIDs.count
    }
    var outstandingTerminalCancelledReservations: Int {
        terminalCancelledReservationIDs.filter(SelectionWorkBudget.contains).count
    }
    /// Every assignment ends a reduced preview; a proxy landing sets the flag
    /// again right after it assigns.
    private(set) var image: CGImage? { didSet { imageIsReduced = false } }
    /// True exactly while `image` is a landed proxy preview, drawn at
    /// the proxy's size (`GeometryProxyPreview.swift`).
    private(set) var imageIsReduced = false
    private(set) var isPending = false
    private(set) var status = "Preview current"
    var onUpdate: (() -> Void)?
    /// The reduced-resolution preview's source and the scale it was
    /// built at. Made on the first request that needs one, and again only
    /// for a request that needs a larger factor (a proxy only gets smaller).
    private var proxyScale: GeometryProxyScale?
    private var proxyLoader: ProxySourceLoader<RasterDocument?>?
    /// The request waiting for the proxy source to be published.
    private var heldRequest: PrecisionGeometryRequest?

    init(_ document: ImageDocument, display: ((RasterDocument) -> CGImage?)? = nil,
         initialImage: CGImage? = nil, workSource: RasterDocument? = nil,
         indexedBackground: NSColor? = nil) throws {
        capture = try PrecisionEditorCapture(document)
        self.document = document
        self.initialImage = initialImage ?? document.previewImage
        self.display = display
        self.workSource = workSource
        self.indexedBackground = indexedBackground
        image = self.initialImage
    }

    func request(_ operation: PrecisionGeometryRequest) {
        assert(Thread.isMainThread)
        guard active else { return }
        // Every request supersedes one held for the proxy source.
        heldRequest = nil
        renderer.cancel()
        currentPreviewReservationID = nil
        generation &+= 1
        let version = generation
        guard let document else { return }
        do {
            try capture.requireCurrent(document)
            let truth = try PrecisionGeometryWork(source: workSource ?? capture.facade,
                                                  request: operation, interactive: true,
                                                  indexedBackground: indexedBackground)
            // The core stage's plan charges the reservation, which the worker
            // runs as its own lease; the described-artwork re-render
            // keeps earlier parts' coverage live while later parts admit, so
            // its scratch is a payload lease every admission counts.
            // A refused true plan previews a reduced-resolution proxy in its
            // place when the commit's own plan sizes one; otherwise
            // the commit request runs as this preview's job.
            let work: PrecisionGeometryWork
            let stages: (core: UInt64, artwork: UInt64)
            let landing: String
            let reduced: Bool
            do {
                (work, stages, landing) = (truth, try truth.stageEstimates(), "Preview current")
                reduced = false
            } catch {
                switch proxyWork(for: truth, operation: operation) {
                case .unavailable:
                    // The commit's plan carries no work refusal, and
                    // its job is cancelled by `renderer.cancel()` as any
                    // preview's is. An error building or planning it is the
                    // one reported, never the first: the first error again
                    // for a validation refusal, the memory sentence when
                    // memory is what stops the preview.
                    let commit = try PrecisionGeometryWork(
                        source: workSource ?? capture.facade, request: operation,
                        interactive: false, indexedBackground: indexedBackground)
                    (work, stages, landing) = (commit, try commit.stageEstimates(),
                                               "Preview current")
                    reduced = false
                case .building:
                    // Re-issued by `proxySourceReady` if still held then.
                    heldRequest = operation
                    isPending = true
                    status = "Rendering preview"
                    onUpdate?()
                    return
                case let .ready(proxy, planned):
                    (work, stages, landing) = (proxy, planned, GeometryProxyPreview.status)
                    reduced = true
                }
            }
            var artwork = stages.artwork > 0
                ? try PrecisionEditPayload.admitted(bytes: stages.artwork) : nil
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: stages.core, outputBytes: 0)
            track(reservation)
            // The kernels' own cap stays the two stages' sum, as before.
            if artwork != nil {
                guard reservation.control.setScratchLimit(
                    try PrecisionDocumentDescriptor.sum(stages.core, stages.artwork)) else {
                    throw DoctoredCoreError(message: "Could not bound transform preview memory.")
                }
            }
            let mailbox = PrecisionGeometryMailbox<RasterDocument?>()
            isPending = true
            status = "Rendering preview"
            onUpdate?()
            renderer.cancelCompute = { reservation.control.cancel() }
            renderer.onRender = { [weak self] _ in
                guard let self, self.active, self.generation == version,
                      let result = mailbox.take() else { return }
                self.renderer.cancel()
                self.currentPreviewReservationID = nil
                guard self.requireCurrentForPreview() else { return }
                switch result {
                case .success(let next):
                    if let next {
                        self.preparePreview(next, version: version, status: landing, reduced: reduced)
                    }
                    // Identity on a work source is that source, not
                    // the captured document's image.
                    else if let source = self.workSource {
                        self.preparePreview(source, version: version, status: landing,
                                            reduced: false)
                    }
                    else {
                        self.image = self.initialImage
                        self.isPending = false
                        self.status = "Preview current"
                        self.onUpdate?()
                    }
                case .failure(let error): self.fail(error, version: version)
                }
            }
            // The work's own core plan charges this lease.
            renderer.request {
                reservation.runAsOwnLease { control in
                    // Released before the reservation, so a commit draining
                    // preview reservations never finds it still counted.
                    defer { artwork = nil }
                    guard !control.isCancelled else { return nil }
                    do {
                        let next = try work.compute(control: control)
                        guard !control.isCancelled else { return nil }
                        mailbox.put(.success(next))
                    } catch {
                        if !control.isCancelled { mailbox.put(.failure(error)) }
                    }
                    return nil
                }
            }
        } catch { fail(error, version: version) }
    }

    /// Callers pass `resultingSelection` (computed before) or
    /// `transformedSelection` (computed in the same job, after the
    /// geometry, and only when the geometry changed something), never both.
    ///
    /// ONE compound job, ONE wait: the geometry (0…0.8, or 0…0.45
    /// when there is selection work), the transformed selection (0.45…0.8)
    /// and then the 16-bit display preparation (0.8…1, skipped when the
    /// geometry changed nothing), published through `capture.publish(prepared:)`
    /// with no second "Preparing 16-bit Display" wait.
    ///
    /// Per-stage admission (`CanvasGeometryJob`'s model): the parent is
    /// admitted at the largest of the geometry (core, artwork) and selection
    /// stages; the artwork and selection stages re-admit themselves on the
    /// worker against what is then live, and `PreparedDocumentEdit` admits its
    /// own display. A true perspective set keeps core + artwork summed, since
    /// its re-rendered artwork stays live through the core stage.
    ///
    /// True when the geometry was published. `finishOutcome` tells a refused
    /// publication from a geometry that changed nothing.
    @discardableResult
    func finish(_ operation: PrecisionGeometryRequest, name: String,
                record: [ActionStep], changesCanvasGeometry: Bool = false,
                resultingSelection: SelectionHistoryState? = nil,
                transformedSelection: TransformedSelection? = nil) throws -> Bool {
        try finishOutcome(operation, name: name, record: record,
                          changesCanvasGeometry: changesCanvasGeometry,
                          resultingSelection: resultingSelection,
                          transformedSelection: transformedSelection) == .published
    }

    /// `finish`, answering the publication's `PublishOutcome`: `.unchanged`
    /// when the geometry changed nothing, `.refused` when `applyEdit`
    /// refused the result. The wait reveals its panel only after
    /// `DocumentJobs.panelDelay`, so a short typed Move, ⌘T, ⌥⌘T or
    /// Crop commit never flashes it.
    func finishOutcome(_ operation: PrecisionGeometryRequest, name: String,
                       record: [ActionStep], changesCanvasGeometry: Bool = false,
                       resultingSelection: SelectionHistoryState? = nil,
                       transformedSelection: TransformedSelection? = nil) throws -> PublishOutcome {
        // The proxy source's build and its document must be gone
        // before admission too. `clearPending` only releases the loader;
        // `discard` waits for a build in flight and frees what it built. A
        // gesture re-armed by a cancel builds a fresh one when it needs it.
        let loader = proxyLoader
        clearPending()
        loader?.discard()
        // The cancelled preview's reservation must leave before admission, or
        // it counts against this commit.
        _ = SelectionWorkBudget.awaitRelease(previewReservationIDs, timeout: 5)
        // A cancel re-arms the gesture instead: the session stays open on the
        // original preview and later handle changes request previews again.
        var cancelled = false
        defer {
            if !cancelled { image = nil }
            onUpdate?()
        }
        guard let document else { return .unchanged }
        try capture.requireCurrent(document)
        let work = try PrecisionGeometryWork(source: workSource ?? capture.facade,
                                             request: operation,
                                             indexedBackground: indexedBackground)
        let selectionWork = transformedSelection?.work == nil ? nil : transformedSelection
        let geometryEnd = selectionWork == nil ? 0.8 : 0.45
        let stages = try work.stageEstimates()
        let geometry = try work.artwork?.perspectiveLoss == true
            ? PrecisionDocumentDescriptor.sum(stages.core, stages.artwork)
            : max(stages.core, stages.artwork)
        let estimate = max(geometry, transformedSelection?.scratchEstimate ?? 0)
        _ = try RasterResources.shared.admit(scratch: estimate)
        let control = try RasterJobControl()
        guard control.setScratchLimit(estimate) else {
            throw DoctoredCoreError(message: "Could not bound captured geometry work.")
        }
        let task = try WorkerTask(scratchEstimate: estimate) {
            try control.beginCompoundOperation()
            guard let next = try control.withChildOperation(from: 0, to: geometryEnd, { child in
                try work.compute(control: child, admittingArtwork: true)
            }) else { return (nil as PreparedDocumentEdit?, nil as [UInt16]?) }
            var samples: [UInt16]?
            if let selectionWork {
                _ = try RasterResources.shared.admit(scratch: selectionWork.scratchEstimate)
                samples = try control.withChildOperation(from: 0.45, to: 0.8) { child in
                    try selectionWork.compute(control: child)
                }
            }
            let prepared = try control.withChildOperation(from: 0.8, to: 1) { child in
                PreparedDocumentEdit(next, scratch: BackgroundWork.scratch, control: child)
            }
            return (prepared, samples)
        }
        let prepared: PreparedDocumentEdit?
        let samples: [UInt16]?
        do {
            (prepared, samples) = try BackgroundWork.wait(
                task, title: name, control: control, revealAfter: DocumentJobs.panelDelay)
            guard !control.isCancelled else { throw JobCancelled() }
        } catch let error as JobCancelled {
            cancelled = true
            active = true
            image = initialImage
            throw error
        }
        try capture.requireCurrent(document)
        var selection = resultingSelection
        if let transformedSelection, prepared != nil {
            selection = try transformedSelection.state(samples)
        }
        return try capture.publish(prepared: prepared, to: document, name: name,
                                   record: record, changesCanvasGeometry: changesCanvasGeometry,
                                   resultingSelection: selection)
    }

    func cancel() {
        clearPending()
        image = nil
        onUpdate?()
        onUpdate = nil
    }

    /// A foreign mutation must invalidate the displayed result immediately,
    /// while retaining the original capture for its named release refusal.
    func invalidateIfStale() {
        guard active, !requireCurrentForPreview() else { return }
        captureTerminalReservations()
        renderer.cancel()
        currentPreviewReservationID = nil
        generation &+= 1
        heldRequest = nil
        image = nil
        isPending = false
        onUpdate?()
    }

    private func clearPending() {
        captureTerminalReservations()
        active = false
        generation &+= 1
        renderer.cancel()
        currentPreviewReservationID = nil
        isPending = false
        // Released without waiting: a build in flight drops its product.
        heldRequest = nil
        proxyLoader = nil
        proxyScale = nil
    }

    private func requireCurrentForPreview() -> Bool {
        guard let document else { return false }
        do { try capture.requireCurrent(document); return true }
        catch {
            image = nil
            isPending = false
            status = error.localizedDescription
            onUpdate?()
            return false
        }
    }

    private enum ProxyAttempt {
        /// The mapped request over the proxy source, planned.
        case ready(PrecisionGeometryWork, (core: UInt64, artwork: UInt64))
        /// The proxy source is still being built.
        case building
        /// No proxy for this request: the commit request runs as the
        /// preview.
        case unavailable
    }

    /// The reduced-resolution work for a refused true preview. A
    /// proxy exists for an integer document whose commit plan reports work
    /// units a `GeometryProxyScale` can be made from. The gesture keeps one
    /// source: it builds one when it has none, or when this request needs a
    /// larger factor than the kept one (the old loader is released), and
    /// maps the request with the kept scale. A source that failed to build, a
    /// request with no map, or a proxy plan that is refused in its turn is
    /// `.unavailable`, which `request` answers with the commit request.
    private func proxyWork(for truth: PrecisionGeometryWork,
                           operation: PrecisionGeometryRequest) -> ProxyAttempt {
        let facade = workSource ?? capture.facade
        guard !facade.isFloat, let units = truth.commitWorkUnits(),
              let wanted = GeometryProxyScale(width: facade.width, height: facade.height,
                                              workUnits: units) else { return .unavailable }
        if proxyLoader == nil || (proxyScale?.factor ?? 0) < wanted.factor {
            proxyScale = wanted
            proxyLoader = ProxySourceLoader(
                build: { GeometryProxyPreview.source(of: facade, scale: wanted) },
                onReady: { [weak self] in self?.proxySourceReady() })
        }
        guard let scale = proxyScale, let loader = proxyLoader else { return .unavailable }
        guard let published = loader.source else { return .building }
        guard let source = published, let mapped = scale.map(operation),
              let work = try? PrecisionGeometryWork(source: source, request: mapped,
                                                    interactive: true,
                                                    indexedBackground: indexedBackground),
              let planned = try? work.stageEstimates() else { return .unavailable }
        return .ready(work, planned)
    }

    /// The proxy source was published (nil for a failed build): the request
    /// that waited for it runs now, if one is still held.
    private func proxySourceReady() {
        guard active, let held = heldRequest else { return }
        request(held)
    }

    private func preparePreview(_ next: RasterDocument, version: UInt64, status landing: String,
                                reduced: Bool) {
        do {
            let output = UInt64(next.width) * UInt64(next.height) * 4
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: try PrecisionDocumentDescriptor.sum(
                    next.projectionScratchEstimate(), output + 65_536),
                outputBytes: output)
            track(reservation)
            let mailbox = PrecisionGeometryMailbox<CGImage>()
            let display = display
            renderer.cancelCompute = { reservation.control.cancel() }
            renderer.onRender = { [weak self] _ in
                guard let self, self.active, self.generation == version,
                      let result = mailbox.take() else { return }
                self.renderer.cancel()
                self.currentPreviewReservationID = nil
                guard self.requireCurrentForPreview() else { return }
                switch result {
                case .success(let image):
                    self.image = image
                    self.imageIsReduced = reduced
                    self.isPending = false
                    self.status = landing
                    self.onUpdate?()
                case .failure(let error): self.fail(error, version: version)
                }
            }
            // The display preparation's own plan charges this lease.
            renderer.request {
                reservation.runAsOwnLease { control in
                    guard !control.isCancelled else { return nil }
                    if let display {
                        if let image = display(next), !control.isCancelled {
                            mailbox.put(.success(image))
                            return image
                        }
                        if !control.isCancelled {
                            mailbox.put(.failure(DoctoredCoreError(
                                message: "Could not prepare the geometry preview.")))
                        }
                        return nil
                    }
                    let prepared = PreparedDocumentEdit(next, control: control)
                    if let image = prepared.preview, !control.isCancelled {
                        mailbox.put(.success(image))
                        return image
                    }
                    if !control.isCancelled {
                        mailbox.put(.failure(DoctoredCoreError(message:
                            prepared.error ?? "Could not prepare the geometry preview.")))
                    }
                    return nil
                }
            }
        } catch { fail(error, version: version) }
    }

    private func fail(_ error: Error, version: UInt64) {
        guard active, generation == version else { return }
        image = nil
        isPending = false
        status = "Preview unavailable: \(error.localizedDescription)"
        onUpdate?()
    }

    private func track(_ reservation: SelectionWorkBudget.Reservation) {
        previewReservationIDs = Set(previewReservationIDs.filter(SelectionWorkBudget.contains))
        previewReservationIDs.insert(reservation.id)
        currentPreviewReservationID = reservation.id
        previewReservationsIssued += 1
    }

    /// Snapshot only IDs still registered at the instant a terminal path
    /// cancels them. Keeping IDs cannot prolong the reservation's lifetime.
    private func captureTerminalReservations() {
        guard renderer.cancelCompute != nil, let currentPreviewReservationID,
              SelectionWorkBudget.contains(currentPreviewReservationID) else { return }
        terminalCancelledReservationIDs.insert(currentPreviewReservationID)
    }
}

private final class PrecisionGeometryMailbox<Value> {
    private let lock = NSLock()
    private var result: Result<Value, Error>?
    func put(_ value: Result<Value, Error>) {
        lock.lock()
        result = value
        lock.unlock()
    }
    func take() -> Result<Value, Error>? {
        lock.lock()
        defer { lock.unlock() }
        let value = result
        result = nil
        return value
    }
}
