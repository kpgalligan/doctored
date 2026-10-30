import CoreGraphics
import Foundation

// Mask dimensions are independent from content. Numeric feather radius stays in canvas pixels
// through geometry; described-layer rerenders preserve the already-positioned raw mask.

/// Exact masked-document previews use the existing PreviewRenderer seam in two
/// stages. Geometry finishes first; main then admits the resulting document's
/// actual projection estimate before the renderer allocates its pixels. No
/// controller, NSDocument or AppKit drawing object crosses the worker boundary.
final class TransformMaskPreview {
    let source: RasterDocument
    let revision: UInt64
    private let initialImage: CGImage?
    /// The transformed document as the image to show, in place of its
    /// composite (a plane transform previewed over that plane). Off-main.
    private let display: ((RasterDocument) -> CGImage?)?
    private let renderer = PreviewRenderer()
    private var generation: UInt64 = 0
    private var requestKey: [Double]?
    private var reservationIDs = Set<UUID>()
    private(set) var image: CGImage?
    // Keep the core projection registered while its CGImage remains displayed.
    private(set) var projection: RasterImage?
    private(set) var isPending = false
    private(set) var status = "Preview current"
    var onUpdate: (() -> Void)?

    init(source: RasterDocument, revision: UInt64, image: CGImage?,
         display: ((RasterDocument) -> CGImage?)? = nil) {
        self.source = source
        self.revision = revision
        self.initialImage = image
        self.image = image
        self.display = display
    }

    /// This preview's reservations still in `SelectionWorkBudget`.
    var outstandingReservations: Int {
        reservationIDs.filter(SelectionWorkBudget.contains).count
    }

    /// A commit's cancel: the in-flight preview's reservation would
    /// otherwise still count against the commit's own admission. Releases
    /// happen on the preview queue as its block exits, so this sleeps rather
    /// than pumping the run loop. Returns whether every reservation left; on
    /// timeout the commit proceeds to admission unchanged.
    @discardableResult
    func cancelReleasingReservations(timeout: TimeInterval = 5) -> Bool {
        cancel()
        let drained = SelectionWorkBudget.awaitRelease(reservationIDs, timeout: timeout)
        reservationIDs = Set(reservationIDs.filter(SelectionWorkBudget.contains))
        return drained
    }

    func cancel() {
        generation &+= 1
        renderer.cancel()
        requestKey = nil
        isPending = false
        onUpdate = nil
    }

    /// A transform's preview. It is never refused for work: when the
    /// interactive plan is refused, the commit's request
    /// (`TransformWork.nonInteractive()`, whose plan carries no work refusal)
    /// runs as this preview's cancellable job, superseded by
    /// `renderer.cancel()` exactly as a true one is. If that plan is refused
    /// too, ITS error is the one reported: the first error again for a
    /// validation refusal, the memory sentence when memory is what stops the
    /// preview.
    func request(_ work: TransformWork) {
        let m = work.matrix
        let key = [m.a, m.b, m.c, m.d, m.tx, m.ty].map { Double($0) }
            + (work.quad ?? []).flatMap { [Double($0.x), Double($0.y)] }
            + [Double(work.sampler.rawValue)]
        request(key: key, identity: m.isIdentity && work.quad == nil) {
            do {
                return PreviewPlan(stages: try work.stageEstimates(),
                                   compute: Self.computing(work), status: "Preview current")
            } catch {
                let commit = work.nonInteractive()
                return PreviewPlan(stages: try commit.stageEstimates(),
                                   compute: Self.computing(commit), status: "Preview current")
            }
        }
    }

    private static func computing(_ work: TransformWork)
        -> (RasterJobControl) throws -> RasterDocument {
        { token in
            guard let transformed = work.transformedDocument(control: token) else {
                throw DoctoredCoreError(message:
                    "The transform preview could not be rendered. Adjust its bounds or sampler.")
            }
            return transformed
        }
    }

    /// The generic latest-wins request: `key` dedupes, `identity` shows the
    /// initial image, `scratch` sizes the first stage's reservation and
    /// `compute` runs OFF-MAIN, so it must not touch main-only state (Move
    /// Selected Pixels passes its captured budget).
    func request(key: [Double], identity: Bool, scratch: () throws -> UInt64,
                 compute: @escaping (RasterJobControl) throws -> RasterDocument) {
        request(key: key, identity: identity) {
            PreviewPlan(stages: (core: try scratch(), artwork: 0), compute: compute,
                        status: "Preview current")
        }
    }

    /// A request's first stage: `stages` splits its scratch: the core
    /// geometry, which the worker's own plan charges, is the reservation it
    /// runs as its own lease; the described-artwork re-render keeps earlier
    /// parts' coverage live while later parts admit, so it is a payload lease
    /// every admission counts, for the worker's whole run. `status` is what
    /// a landed preview reports.
    private struct PreviewPlan {
        let stages: (core: UInt64, artwork: UInt64)
        let compute: (RasterJobControl) throws -> RasterDocument
        let status: String
    }

    private func request(key: [Double], identity: Bool, plan: () throws -> PreviewPlan) {
        assert(Thread.isMainThread)
        guard key != requestKey else { return }
        renderer.cancel()
        generation &+= 1
        let version = generation
        requestKey = key
        if identity {
            image = initialImage
            projection = nil
            isPending = false
            status = "Preview current"
            onUpdate?()
            return
        }
        isPending = true
        status = "Rendering preview; showing previous result"
        onUpdate?()
        do {
            let plan = try plan()
            let stages = plan.stages
            let compute = plan.compute
            let landing = plan.status
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
            let mailbox = TransformPreviewMailbox<RasterDocument>()
            renderer.cancelCompute = { reservation.control.cancel() }
            renderer.onRender = { [weak self] _ in
                guard let self, self.generation == version, let result = mailbox.take() else { return }
                // This also releases the completed stage's debounced closure
                // before admitting the projection stage on the main thread.
                self.renderer.cancel()
                switch result {
                case .success(let transformed):
                    self.render(transformed, version: version, status: landing)
                case .failure(let error): self.fail(error, version: version)
                }
            }
            renderer.request {
                reservation.runAsOwnLease { token in
                    // Released before the reservation, so a commit draining
                    // preview reservations never finds it still counted.
                    defer { artwork = nil }
                    guard !token.isCancelled else { return nil }
                    do {
                        let transformed = try compute(token)
                        guard !token.isCancelled else { return nil }
                        mailbox.put(.success(transformed))
                    } catch {
                        if !token.isCancelled { mailbox.put(.failure(error)) }
                    }
                    return nil
                }
            }
        } catch { fail(error, version: version) }
    }

    private func render(_ transformed: RasterDocument, version: UInt64, status landing: String) {
        do {
            let output = UInt64(source.width) * UInt64(source.height) * 4
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: transformed.projectionScratchEstimate() + output + 4096,
                outputBytes: output)
            track(reservation)
            let mailbox = TransformPreviewMailbox<TransformPreviewPixels>()
            let display = display
            renderer.cancelCompute = { reservation.control.cancel() }
            renderer.onRender = { [weak self] _ in
                guard let self, self.generation == version, let result = mailbox.take() else { return }
                self.renderer.cancel()
                switch result {
                case .success(let pixels):
                    self.projection = pixels.projection
                    self.image = pixels.image
                    self.isPending = false
                    self.status = landing
                    self.onUpdate?()
                case .failure(let error): self.fail(error, version: version)
                }
            }
            renderer.request {
                reservation.run { token in
                    guard !token.isCancelled else { return nil }
                    do {
                        if let display {
                            guard let image = display(transformed) else {
                                throw DoctoredCoreError(
                                    message: "Could not prepare the transform preview image.")
                            }
                            guard !token.isCancelled else { return nil }
                            mailbox.put(.success(
                                TransformPreviewPixels(projection: nil, image: image)))
                            return image
                        }
                        let projection = try transformed.render()
                        guard !token.isCancelled else { return nil }
                        guard let image = projection.makeCGImage(in: transformed.colorSpace) else {
                            throw DoctoredCoreError(message: "Could not prepare the transform preview image.")
                        }
                        guard !token.isCancelled else { return nil }
                        mailbox.put(.success(TransformPreviewPixels(projection: projection, image: image)))
                        return image
                    } catch {
                        if !token.isCancelled { mailbox.put(.failure(error)) }
                        return nil
                    }
                }
            }
        } catch { fail(error, version: version) }
    }

    private func track(_ reservation: SelectionWorkBudget.Reservation) {
        reservationIDs = Set(reservationIDs.filter(SelectionWorkBudget.contains))
        reservationIDs.insert(reservation.id)
    }

    private func fail(_ error: Error, version: UInt64) {
        guard generation == version else { return }
        isPending = false
        requestKey = nil
        status = "Preview unavailable: \(error.localizedDescription)"
        onUpdate?()
    }
}

extension SelectionWorkBudget {
    /// Polls until none of `ids` is registered or `timeout` passes; returns
    /// whether they all left. Shared by the byte and typed commit paths.
    static func awaitRelease(_ ids: Set<UUID>, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while ids.contains(where: contains) {
            guard Date() < deadline else { return false }
            Thread.sleep(forTimeInterval: 0.005)
        }
        return true
    }
}

private struct TransformPreviewPixels {
    let projection: RasterImage?
    let image: CGImage
}

/// PreviewRenderer publishes on main. A mailbox tags each request's result so
/// a completion from an older in-flight closure cannot enter a newer callback.
final class TransformPreviewMailbox<Value> {
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
