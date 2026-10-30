import AppKit

/// The Layer Style sheet's live preview on a 16-bit document. A 32-bit
/// float document keeps the sheet's no-preview hint.
///
/// Each request applies the sheet's style and light to the BASE facade as two
/// bookkeeping edits on main (no pixels: `previewingNativeLayerStyle`,
/// `withGlobalLight`), reserves that styled document's own projection charge
/// (`projectionScratchEstimate()`, the figure `PreparedDocumentEdit` admits,
/// plus `PrecisionOpacityDrag`'s bookkeeping slack) and renders it on a
/// `PreviewRenderer` queue as the reservation's own lease. Nothing here is a
/// new memory plan: the reservation is the projection's.
///
/// The base is the captured facade on a canvas of at most 4 MP. Above that
/// (`ProxyScale`) it is a reduced facade built once off main, the style is
/// sent with `scale: 1 / factor` so the effects shrink with the picture, and
/// the status line says the preview is reduced; a CMYK document (#547: its
/// effects in ink, sent by the sheet with each slot's ink) takes the same
/// proxy since phase 4b's `resize` on ink. The light edit is sent only when
/// the sheet's light differs from the base's: an unchanged one is the core's
/// `None` anyway. Apply always commits the authored style on the full
/// document; this class publishes nothing.
///
/// A refused bookkeeping edit or reservation reports its sentence through
/// `onStatus` and leaves the last preview up.
final class PrecisionStylePreview {
    static let reducedStatus = "Preview at reduced resolution; Apply renders at full size."

    /// Main: a rendered preview, current request only.
    var onRender: ((CGImage?) -> Void)?
    /// Main: a refusal's sentence, the reduced-resolution note, or "" when a
    /// full-size request was admitted.
    var onStatus: ((String) -> Void)?

    private let capture: PrecisionEditorCapture
    private weak var document: ImageDocument?
    private let layer: Int
    private let renderer = PreviewRenderer()
    private let scale: ProxyScale?
    /// The reduced base, or why it could not be built.
    private var loader: ProxySourceLoader<Result<RasterDocument, Error>>?
    private var control: RasterJobControl?
    private var generation: UInt64 = 0
    private let delivered = DeliveredStyleGeneration()
    /// The newest request, re-run when the reduced base arrives.
    private var latest: (style: LayerStyle?, light: GlobalLight)?
    private var finished = false

    /// `document` is what `capture.requireCurrent` is asked about before each
    /// request and each delivery.
    init(capture: PrecisionEditorCapture, layer: Int, document: ImageDocument) {
        self.capture = capture
        self.layer = layer
        self.document = document
        scale = ProxyScale(width: capture.facade.width, height: capture.facade.height)
        renderer.onRender = { [weak self] image in self?.landed(image) }
        guard let scale else { return }
        let facade = capture.facade
        // Off main: the typed resize plans and reserves its own work
        // (`PrecisionProxySource`). A failed build is published as a failure,
        // so its sentence reaches main through the loader's own delivery
        // (`PrecisionGeometryGesture` publishes a failed build the same way).
        loader = ProxySourceLoader(
            build: { () -> Result<RasterDocument, Error> in
                do {
                    guard let reduced = try facade.resized(
                        w: scale.width, h: scale.height, filter: DR_FILTER_BILINEAR)
                    else {
                        throw DoctoredCoreError(
                            message: "Could not reduce the document for a style preview.")
                    }
                    return .success(reduced)
                } catch {
                    return .failure(error)
                }
            },
            onReady: { [weak self] in self?.baseReady() })
    }

    /// One edit of the sheet: `style` (nil or an identity clears) under `light`.
    func request(style: LayerStyle?, light: GlobalLight) {
        guard !finished, let document else { return }
        latest = (style, light)
        generation &+= 1
        let generation = generation
        control?.cancel()
        let base: RasterDocument
        var styleScale: Double?
        if let scale {
            // The reduced base is still building (`baseReady` runs this
            // again), or its build failed: the base is built once, so every
            // request says why again.
            guard case .success(let reduced)? = loader?.source else {
                if case .failure(let error)? = loader?.source {
                    onStatus?(error.localizedDescription)
                }
                return
            }
            base = reduced
            styleScale = 1 / scale.factor
        } else {
            base = capture.facade
        }
        do {
            try capture.requireCurrent(document)
            let target = style.flatMap { $0.isIdentity ? nil : $0 }
            var json: String?
            if let target {
                guard let encoded = target.json() else {
                    throw DoctoredCoreError(message: "Invalid layer style.")
                }
                json = encoded
            }
            let styled = try base.previewingNativeLayerStyle(layer, json, scale: styleScale) ?? base
            var lit = styled
            if light != base.globalLight {
                lit = try styled.withGlobalLight(light) ?? styled
            }
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: try lit.projectionScratchEstimate() + Self.bookkeepingBytes, outputBytes: 0)
            control = reservation.control
            let previewControl = reservation.control
            renderer.cancelCompute = { previewControl.cancel() }
            let delivered = self.delivered
            // The display preparation charges its own projection, so the
            // worker leaves this lease out of `otherRetained`.
            renderer.request {
                reservation.runAsOwnLease { token in
                    let prepared = PreparedDocumentEdit(lit, control: token)
                    delivered.set(generation, failure: token.isCancelled ? nil : prepared.error)
                    return token.isCancelled ? nil : prepared.preview
                }
            }
            onStatus?(scale == nil ? "" : Self.reducedStatus)
        } catch {
            onStatus?(error.localizedDescription)
        }
    }

    /// Ends the preview for good: the in-flight render is cancelled, nothing
    /// is delivered afterwards and no later request runs. The sheet's Cancel.
    func cancel() {
        finished = true
        generation &+= 1
        control?.cancel()
        control = nil
        latest = nil
        renderer.cancel()
    }

    /// Main only. Cancels the in-flight request and returns once no render is
    /// in flight and the reduced base's build has returned, so the preview's
    /// reservation is released before Apply admits its publish. A result
    /// already on its way is dropped on delivery, and a base that arrives
    /// later re-runs nothing. The preview stays usable: a refused Apply keeps
    /// the sheet up, and its next edit requests again.
    func drain() {
        generation &+= 1
        control?.cancel()
        control = nil
        latest = nil
        renderer.dropPending()
        renderer.waitUntilIdle()
        loader?.waitUntilBuilt()
    }

    private func landed(_ image: CGImage?) {
        let result = delivered.value
        guard !finished, let document, result.generation == generation else { return }
        guard let image else {
            // A render the display preparation refused: its sentence, with the
            // last preview left up.
            if let failure = result.failure { onStatus?(failure) }
            return
        }
        // A document that moved under the sheet shows no stale preview; Apply
        // will refuse with the capture's own sentence.
        guard (try? capture.requireCurrent(document)) != nil else { return }
        onRender?(image)
    }

    /// Main, from the loader: the reduced base was published. A failed build
    /// reports its sentence; a built one runs the newest request.
    private func baseReady() {
        guard !finished else { return }
        if case .failure(let error)? = loader?.source {
            onStatus?(error.localizedDescription)
            return
        }
        guard let latest else { return }
        request(style: latest.style, light: latest.light)
    }

    /// `PrecisionOpacityDrag.bookkeepingBytes`: `reservePreview` requires a
    /// total strictly greater than the declared output, and a projection
    /// estimate alone can be its exact size.
    private static let bookkeepingBytes: UInt64 = 65_536
}

/// Which generation the worker delivered, and why it produced no image when
/// it did not, across the preview boundary (`PrecisionOpacityDrag`'s hand-off).
private final class DeliveredStyleGeneration {
    private let lock = NSLock()
    private var stored: (generation: UInt64, failure: String?) = (0, nil)
    var value: (generation: UInt64, failure: String?) {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }
    func set(_ generation: UInt64, failure: String?) {
        lock.lock()
        stored = (generation, failure)
        lock.unlock()
    }
}
