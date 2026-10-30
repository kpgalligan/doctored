import AppKit

// This file holds `ProxyScale` (over 4 MP: `k = ceil(sqrt(W·H / 262,144))`, proxy `W/k × H/k`),
// `StagedPreview` (a proxy and an exact `PreviewRenderer` under one generation counter: the proxy
// runs first, its generation's exact is queued only once it lands, any older delivery is dropped,
// `lateProxy` serves a source that arrived after the request), `ProxySourceLoader` (built once per
// sheet or gesture off main, published on main through a lock-guarded slot, so a released or
// `discard()`ed loader's product is freed when the build returns), `ByteFilterPreview` (8 bits: the
// proxy on `OperationCapture.proxy`, then the exact under a fresh single-use `RasterJobControl` the
// next request cancels — `filterOp(…, control:)`, 8-bit Gaussian Blur included (`blur`);
// a filter with no cancellable kernel only skips its
// flatten through `OperationCapture.preview(control:)`; with `reserve` the exact is made on
// main through `request(proxy:makeExact:)` under a `SelectionWorkBudget` reservation whose control
// it runs on, `onRefusal` reports a refused reservation, and `drain()` — cancel, then wait for the
// in-flight stage and the proxy build — runs before Apply admits, so the reservation is never
// charged twice) and `PrecisionProxySource` (16 bits: a resized typed facade and its
// `dr_selection_resize`d coverage, planned and reserved on main, run as its own lease;
// `PrecisionAdjustmentSession.prepareProxy`). `SliderSheetController`'s `proxy:` map opts a sheet
// in (Gaussian Blur is `SliderSheetController.gaussianBlur`); radius-like values are divided by the
// factor (Box/Surface Blur `max(1, round(r/f))`), amounts and thresholds kept. Commits never see a
// proxy. Decisions: `designs/phase-9f-selected-tone-status.md`. The 16-bit ⌘T and Crop
// straighten previews keep their own loader (`GeometryProxyPreview.swift`).

// Staged, cancellable slider-filter previews. A heavy filter sheet on a
// large canvas first shows a downscaled PROXY of its result, then the exact
// full-resolution preview; every superseded exact render is cancelled. Apply
// never sees a proxy: commits run the full-resolution kernel exactly as before.
// The decisions and the before/after latency are in
// designs/phase-9f-selected-tone-status.md.

/// The proxy's integer reduction. A canvas at or below 4 MP is previewed at
/// full resolution only; above it the factor `k = ceil(sqrt(W·H / 262,144))`
/// makes a proxy of about a quarter of a megapixel (`W/k × H/k`, integer
/// division), so a 6000 × 4000 layer previews at 600 × 400 first, with about
/// 100× less kernel work. `factor` is the exact width ratio, which radius-like
/// parameters are divided by so the proxy looks like the full-size result.
struct ProxyScale {
    static let minCanvasPixels = 4_194_304
    static let targetPixels = 262_144
    let factor: Double
    let width: Int
    let height: Int

    init?(width: Int, height: Int) {
        guard width > 0, height > 0, width <= Int.max / height,
              width * height > Self.minCanvasPixels else { return nil }
        let k = Int((Double(width * height) / Double(Self.targetPixels)).squareRoot().rounded(.up))
        guard k > 1, width / k > 0, height / k > 0 else { return nil }
        self.width = width / k
        self.height = height / k
        factor = Double(width) / Double(self.width)
    }
}

/// A lock-guarded generation, written by a render closure on the preview queue
/// and read on main when its result is delivered.
private final class GenerationBox {
    private let lock = NSLock()
    private var stored: UInt64 = 0
    var value: UInt64 { lock.lock(); defer { lock.unlock() }; return stored }
    func set(_ value: UInt64) { lock.lock(); stored = value; lock.unlock() }
}

/// Two `PreviewRenderer`s — a proxy queue and an exact queue — under one
/// generation counter. Each request bumps the generation and any delivery from
/// an older one is dropped, so a stale render never replaces a newer one. The
/// proxy runs first; that generation's exact is queued only once its proxy has
/// landed, so while a slider drags only proxies run. A nil proxy is not shown
/// and its exact still runs; a proxy that lands after its generation's exact is
/// dropped. A closure whose generation is already superseded when the queue
/// reaches it returns nil without running.
final class StagedPreview {
    private let proxyRenderer = PreviewRenderer()
    private let exactRenderer = PreviewRenderer()
    private let proxyDelivery = GenerationBox()
    private let exactDelivery = GenerationBox()
    private let latest = GenerationBox()
    private var generation: UInt64 = 0
    private var exactLanded = false
    private var heldExact: (() -> CGImage?)?
    /// The current generation's exact, not yet planned, and whether
    /// that generation's proxy is still to be delivered.
    private var heldMakeExact: (() -> (() -> CGImage?)?)?
    private var proxyPending = false

    /// The exact stage, as `PreviewRenderer.onRender`, current generation only.
    var onRender: ((CGImage?) -> Void)?
    /// The proxy stage, current generation only.
    var onProxy: ((CGImage) -> Void)?
    /// Forwarded as the exact `PreviewRenderer.cancelCompute`.
    var cancelCompute: (() -> Void)? {
        get { exactRenderer.cancelCompute }
        set { exactRenderer.cancelCompute = newValue }
    }

    init() {
        proxyRenderer.onRender = { [weak self] image in self?.proxyLanded(image) }
        exactRenderer.onRender = { [weak self] image in self?.exactArrived(image) }
        exactRenderer.onIdle = { [weak self] in self?.runHeldMakeExact() }
    }

    func request(_ exact: @escaping () -> CGImage?) {
        request(proxy: nil, exact: exact)
    }

    func request(proxy: (() -> CGImage?)?, exact: @escaping () -> CGImage?) {
        generation &+= 1
        latest.set(generation)
        exactLanded = false
        heldMakeExact = nil
        proxyPending = false
        guard let proxy else {
            heldExact = nil
            queueExact(exact)
            return
        }
        heldExact = exact
        queueProxy(proxy)
    }

    /// Queues `proxy` now (as `request(proxy:exact:)` does). `makeExact`
    /// runs on main once this generation's proxy has been delivered (at once
    /// when proxy is nil) AND the exact renderer is idle, only if the
    /// generation is still current; its non-nil result is queued as this
    /// generation's exact, nil queues nothing. A newer request, `supersede()`
    /// or `cancel()` drops a not-yet-run makeExact, so a superseded exact has
    /// returned before the next one is planned and reserved.
    func request(proxy: (() -> CGImage?)?, makeExact: @escaping () -> (() -> CGImage?)?) {
        generation &+= 1
        latest.set(generation)
        exactLanded = false
        heldExact = nil
        heldMakeExact = makeExact
        proxyPending = proxy != nil
        if let proxy { queueProxy(proxy) }
        runHeldMakeExact()
    }

    /// A proxy for the CURRENT generation whose source became available after
    /// the request; its exact is already queued. No-op once that exact landed.
    func lateProxy(_ proxy: @escaping () -> CGImage?) {
        guard !exactLanded else { return }
        queueProxy(proxy)
    }

    /// Ends the current generation without starting another: anything still
    /// queued or in flight for it is dropped on delivery, and its held exact
    /// never runs (releasing what it retains). Called before a request is
    /// planned, so one refused before it reaches `request` shows nothing stale.
    func supersede() {
        generation &+= 1
        latest.set(generation)
        heldExact = nil
        heldMakeExact = nil
        proxyRenderer.dropPending()
        exactRenderer.dropPending()
    }

    /// Also drops a held, not-yet-queued exact closure (and what it retains).
    func cancel() {
        heldExact = nil
        heldMakeExact = nil
        proxyRenderer.cancel()
        exactRenderer.cancel()
    }

    /// Main only: returns once neither stage has a render in flight.
    func waitUntilIdle() {
        proxyRenderer.waitUntilIdle()
        exactRenderer.waitUntilIdle()
    }

    private func runHeldMakeExact() {
        guard !proxyPending, !exactRenderer.isRendering, let make = heldMakeExact else { return }
        heldMakeExact = nil
        if let exact = make() { queueExact(exact) }
    }

    private func queueProxy(_ proxy: @escaping () -> CGImage?) {
        let generation = generation, delivery = proxyDelivery, latest = latest
        proxyRenderer.request {
            delivery.set(generation)
            return latest.value == generation ? proxy() : nil
        }
    }

    private func queueExact(_ exact: @escaping () -> CGImage?) {
        let generation = generation, delivery = exactDelivery, latest = latest
        exactRenderer.request {
            delivery.set(generation)
            return latest.value == generation ? exact() : nil
        }
    }

    private func proxyLanded(_ image: CGImage?) {
        guard proxyDelivery.value == generation else { return }
        if let image, !exactLanded { onProxy?(image) }
        if let exact = heldExact {
            heldExact = nil
            queueExact(exact)
        }
        proxyPending = false
        runHeldMakeExact()
    }

    private func exactArrived(_ image: CGImage?) {
        guard exactDelivery.value == generation else { return }
        exactLanded = true
        onRender?(image)
    }
}

/// The serial utility queue every proxy source is built on.
private let proxySourceQueue = DispatchQueue(label: "com.kgalligan.doctored.preview-proxy", qos: .utility)

/// A built proxy source on its way from the build queue to main, and whether
/// its loader was discarded. The value waits here, not in the main hop's
/// closure, so nothing but the loader keeps it alive once the build returns.
private final class ProxySourceSlot<Source> {
    private let lock = NSLock()
    private var built: Source?
    private var discarded = false

    var isDiscarded: Bool { lock.lock(); defer { lock.unlock() }; return discarded }

    /// Keeps `value` unless the loader was discarded (then it is dropped).
    func set(_ value: Source) {
        lock.lock()
        if !discarded { built = value }
        lock.unlock()
    }

    func take() -> Source? {
        lock.lock(); defer { lock.unlock() }
        let value = built
        built = nil
        return value
    }

    func discard() {
        lock.lock()
        discarded = true
        built = nil
        lock.unlock()
    }
}

/// Builds a sheet's proxy source once, off main, and publishes it on main.
/// `onReady` runs on main, on success only; a failed build leaves `source`
/// nil, and every request then runs its exact stage alone. The built value
/// crosses to main in a lock-guarded slot the loader owns, which the main hop
/// empties: a loader released or discarded while it builds frees its product
/// when the build returns.
final class ProxySourceLoader<Source> {
    /// Main only.
    private(set) var source: Source?
    private let slot = ProxySourceSlot<Source>()

    init(build: @escaping () throws -> Source, onReady: @escaping () -> Void) {
        let slot = slot
        let publish: () -> Void = { [weak self] in
            guard let self, let built = self.slot.take() else { return }
            self.source = built
            onReady()
        }
        let alive: () -> Bool = { [weak self] in self != nil }
        proxySourceQueue.async {
            guard alive(), !slot.isDiscarded, let built = try? build() else { return }
            guard alive() else { return }
            slot.set(built)
            DispatchQueue.main.async(execute: publish)
        }
    }

    /// Main only: returns once the build has returned (its publication
    /// still hops to main).
    func waitUntilBuilt() { proxySourceQueue.sync {} }

    /// Main only. Returns once the build has returned, drops what it built and
    /// `source`; `onReady` never runs afterwards.
    func discard() {
        slot.discard()
        proxySourceQueue.sync {}
        source = nil
    }
}

/// The 8-bit slider-sheet preview: a proxy (when the sheet is proxied and the
/// canvas is large enough), then the exact preview under a fresh single-use
/// `RasterJobControl`. A new request cancels the previous exact's control, so a
/// superseded `dr_image_filter_op` stops in the core and skips its flatten.
/// With `reserve` the exact is planned and reserved on main first, and a
/// refused reservation is reported through `onRefusal` instead of running.
final class ByteFilterPreview {
    /// Both stages deliver here.
    var onRender: ((CGImage?) -> Void)?
    /// Main: a reserved exact's refusal sentence, or nil once one is admitted.
    var onRefusal: ((String?) -> Void)?
    private let capture: OperationCapture?
    private let staged = StagedPreview()
    private var scale: ProxyScale?
    private var loader: ProxySourceLoader<OperationCapture>?
    private var control: RasterJobControl?
    private var latestProxyOp: ((RasterImage, Double) -> RasterImage?)?
    /// The current request's exact was refused; a late proxy is not shown over it.
    private var refused = false

    init(capture: OperationCapture?, proxied: Bool) {
        self.capture = capture
        staged.onRender = { [weak self] image in self?.onRender?(image) }
        staged.onProxy = { [weak self] image in self?.onRender?(image) }
        guard proxied, let capture,
              let scale = ProxyScale(width: capture.doc.width, height: capture.doc.height) else { return }
        self.scale = scale
        loader = ProxySourceLoader(build: { try capture.proxy(scale) },
                                   onReady: { [weak self] in self?.proxyReady() })
    }

    /// `op` is the exact filter, handed the request's control (nil when one
    /// could not be made); `proxyOp` the same filter on the proxy, handed the
    /// scale factor. A filter whose kernel cannot be cancelled ignores the
    /// control and still skips the flatten once cancelled.
    ///
    /// `reserve` nil is the unreserved request. Otherwise the exact is
    /// made on main through `staged.request(proxy:makeExact:)`, after a
    /// superseded exact has returned: `reserve(capture)` succeeding makes the
    /// reservation's control the exact's (the next request cancels it), runs
    /// the exact inside `reservation.run` and reports `onRefusal?(nil)`; a
    /// throw clears the preview (`onRender?(nil)`), reports the sentence and
    /// runs no exact.
    func request(op: @escaping (RasterImage, RasterJobControl?) -> RasterImage?,
                 proxyOp: ((RasterImage, Double) -> RasterImage?)?,
                 reserve: ((OperationCapture) throws -> SelectionWorkBudget.Reservation)? = nil) {
        guard let capture else { return }
        control?.cancel()
        latestProxyOp = proxyOp
        refused = false
        guard let reserve else {
            let control = try? RasterJobControl()
            self.control = control
            staged.request(proxy: proxyWork(proxyOp),
                           exact: { capture.preview(control: control) { op($0, control) } })
            return
        }
        control = nil
        staged.request(proxy: proxyWork(proxyOp)) { [weak self] in
            self?.reservedExact(op, capture: capture, reserve: reserve)
        }
    }

    func cancel() {
        control?.cancel()
        control = nil
        latestProxyOp = nil
        onRender = nil
        onRefusal = nil
        staged.cancel()
    }

    /// Main thread: `cancel()`, then returns once no stage is in flight
    /// and the proxy source build has returned, so the preview's reservation
    /// is released before Apply admits its own plan.
    func drain() {
        cancel()
        staged.waitUntilIdle()
        loader?.waitUntilBuilt()
    }

    private func reservedExact(
        _ op: @escaping (RasterImage, RasterJobControl?) -> RasterImage?, capture: OperationCapture,
        reserve: (OperationCapture) throws -> SelectionWorkBudget.Reservation
    ) -> (() -> CGImage?)? {
        do {
            let reservation = try reserve(capture)
            control = reservation.control
            onRefusal?(nil)
            return { reservation.run { control in capture.preview(control: control) { op($0, control) } } }
        } catch {
            refused = true
            onRender?(nil)
            onRefusal?(error.localizedDescription)
            return nil
        }
    }

    private func proxyWork(_ proxyOp: ((RasterImage, Double) -> RasterImage?)?) -> (() -> CGImage?)? {
        guard let proxyOp, let source = loader?.source, let factor = scale?.factor else { return nil }
        return { source.preview { proxyOp($0, factor) } }
    }

    /// The opening request (and any made while the source was building) gets
    /// its proxy now, on the proxy queue; its queued exact is left alone.
    private func proxyReady() {
        guard !refused else { return }
        if let work = proxyWork(latestProxyOp) { staged.lateProxy(work) }
    }
}

extension OperationCapture {
    /// The same capture on a `scale`-reduced copy of the document: the layer,
    /// target and preview base are kept, the document is reduced with the
    /// document resize (bilinear) and the selection coverage with the same
    /// kernel (`dr_selection_resize`), so it still fits the proxy canvas.
    func proxy(_ scale: ProxyScale) throws -> OperationCapture {
        guard let reduced = try doc.resized(w: scale.width, h: scale.height, filter: DR_FILTER_BILINEAR)
        else { throw DoctoredCoreError(message: "Could not reduce the document for a preview proxy.") }
        let coverage = try selection.map {
            try RasterImage.resizingSelection($0, width: doc.width, height: doc.height,
                                              toWidth: scale.width, toHeight: scale.height)
        }
        guard let proxy = OperationCapture(doc: reduced, layerIndex: layerIndex, target: target,
                                           selection: coverage, previewBase: previewBase)
        else { throw DoctoredCoreError(message: "The preview proxy could not capture its target.") }
        return proxy
    }
}

/// The 16-bit proxy source: a typed `scale`-reduced facade, the captured layer
/// and the selection's coverage reduced through `dr_selection_resize` and
/// promoted ×257, so a proxy edit is the same selected-filter request the
/// exact preview runs, on a smaller document.
struct PrecisionProxySource {
    let facade: RasterDocument
    let layer: Int
    let coverage: PrecisionScalarBuffer?
    let scale: ProxyScale

    /// Off main: the typed resize plans and reserves its own work.
    init(_ capture: PrecisionEditorCapture, scale: ProxyScale) throws {
        guard let reduced = try capture.facade.resized(w: scale.width, h: scale.height,
                                                       filter: DR_FILTER_BILINEAR)
        else { throw DoctoredCoreError(message: "Could not reduce the document for a preview proxy.") }
        facade = reduced
        layer = capture.layer
        let width = capture.facade.width, height = capture.facade.height
        coverage = try capture.selection.map { selection in
            let bytes = try RasterImage.resizingSelection(selection.maskBytes(), width: width, height: height,
                                                          toWidth: scale.width, toHeight: scale.height)
            return try PrecisionScalarBuffer(samples: bytes.map { UInt16($0) * 257 },
                                             width: scale.width, height: scale.height)
        }
        self.scale = scale
    }

    /// Main: plans the edit and reserves its preview scratch (both main-only);
    /// the returned work runs on the proxy queue as its reservation's own lease.
    func previewWork(filter: String, params: [String: Any]) throws -> () -> CGImage? {
        guard let source = facade.precision else {
            throw DoctoredCoreError(message: "The preview proxy is not a high-depth document.")
        }
        // A proxy is only built for a layer target (`prepareProxy`).
        let params = try FilterFrame.kernelParameters(filter, params, target: .layer)
        let request = try SelectedAdjustment.request(layer: layer, filter: filter, params: params)
        let edit = try PrecisionCapturedEdit(source, operation: .selected(request: request, coverage: coverage))
        let display = try facade.projectionScratchEstimate()
        let reservation = try SelectionWorkBudget.reservePreview(
            bytes: edit.previewReservationBytes(display: display), outputBytes: 0)
        return {
            reservation.runAsOwnLease { token in
                do {
                    let result = try edit.compute(control: token) ?? edit.source
                    guard !token.isCancelled else { return nil }
                    return PreparedDocumentEdit(try RasterDocument(precision: result)).preview
                } catch { return nil }
            }
        }
    }
}
