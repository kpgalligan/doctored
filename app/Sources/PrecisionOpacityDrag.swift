import AppKit

// `PrecisionOpacityDrag` uses ONE `PrecisionEditorCapture` for the whole drag and ONE builder
// (`PrecisionOpacityDrag.document(_:opacity:)`) shared by every preview tick and by the commit,
// typed all the way down with no `try?` on the publish path. `finish` and `cancel` share a
// cancel-FIRST prefix — bump the generation, `control?.cancel()`, `renderer.cancel()` (which nils
// `onRender`, so a late result can never land) — and only THEN does `finish` re-check the capture
// and build the commit, so the commit's display preparation is never admitted on top of a live
// preview reservation; `previewImage = nil` and the controller's session-nil sit in `defer`s
// covering every exit including the throw. `cancel()`'s one caller is
// `releaseDisplayForWindowClose()`, so a gesture interrupted without an `ended` tick — a window
// closed mid-drag — drops its capture and its preview there.

/// The Layers panel's continuous OPACITY drag at TYPED depth (Phase 12E-2,
/// P7-27): one `PrecisionEditorCapture` for the whole gesture, one builder
/// shared by every preview and by the commit, ONE undo step at release, and a
/// stale gesture refused by the capture's own sentence.
///
/// The byte path is unchanged and keeps using
/// `beginLiveEdit`/`updateLiveEdit`/`endLiveEdit`: a byte gesture holds
/// `liveEditBase`, so `refuseLiveGestureMutation` refuses a foreign edit and
/// the document revision cannot move under it. A typed edit has no live-edit
/// base — it publishes whole handles — so staleness is this session's own
/// contract, checked against the captured revision.
final class PrecisionOpacityDrag {
    private weak var document: ImageDocument?
    private weak var canvas: ImageCanvasView?
    private let capture: PrecisionEditorCapture
    private let renderer = PreviewRenderer()
    private var control: RasterJobControl?
    private var generation: UInt64 = 0
    private let delivered = DeliveredGeneration()

    init(_ document: ImageDocument, canvas: ImageCanvasView) throws {
        capture = try PrecisionEditorCapture(document)
        guard capture.facade.precision != nil else {
            throw DoctoredCoreError(message: "A high-depth document is required.")
        }
        self.document = document
        self.canvas = canvas
        renderer.onRender = { [weak self] image in
            guard let self, let document = self.document else { return }
            // A result from an earlier tick is discarded rather than shown:
            // the drag has already moved on (`PrecisionAdjustmentSession`).
            guard self.delivered.value == self.generation else { return }
            do {
                try self.capture.requireCurrent(document)
                self.canvas?.previewImage = image
            } catch {
                self.canvas?.previewImage = nil
            }
        }
    }

    /// THE builder, shared by every preview and by the commit, so what the
    /// drag shows is what the release writes. `nil` means the gesture would
    /// change nothing — the core's own no-op rule, layer by layer.
    ///
    /// Typed all the way down: the opacity is set through the typed edit, not
    /// through a byte surrogate, and nothing here uses `try?`.
    static func document(_ capture: PrecisionEditorCapture, opacity: Double) throws
        -> RasterDocument? {
        var current = capture.facade
        var changed = false
        for index in capture.selectedLayers {
            guard let precision = current.precision else {
                throw DoctoredCoreError(message: "A high-depth document is required.")
            }
            if let next = try precision.settingLayerOpacity(index, opacity: Float(opacity)) {
                current = try RasterDocument(precision: next)
                changed = true
            }
        }
        return changed ? current : nil
    }

    /// One drag tick: the preview is rendered off the SAME capture and the
    /// SAME builder the commit uses, under its own bounded reservation.
    func preview(opacity: Double) {
        guard let document else { return }
        generation &+= 1
        let generation = generation
        control?.cancel()
        do {
            try capture.requireCurrent(document)
            guard let next = try Self.document(capture, opacity: opacity) else {
                canvas?.previewImage = nil
                return
            }
            let display = try capture.facade.projectionScratchEstimate()
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: display + Self.bookkeepingBytes, outputBytes: 0)
            control = reservation.control
            let previewControl = reservation.control
            renderer.cancelCompute = { previewControl.cancel() }
            let delivered = self.delivered
            // The display preparation charges its own projection, so the
            // worker leaves this lease out of `otherRetained`.
            renderer.request {
                reservation.runAsOwnLease { token in
                    let prepared = PreparedDocumentEdit(next)
                    delivered.set(generation)
                    return token.isCancelled ? nil : prepared.preview
                }
            }
        } catch {
            canvas?.previewImage = nil
        }
    }

    /// The release: ONE publication, or nothing at all.
    ///
    /// The in-flight preview is cancelled FIRST — generation, job token and
    /// renderer, which nils `onRender` so a late result can never land — and
    /// only then is the capture re-checked and the commit built. That order is
    /// what keeps the commit's own display preparation from being refused on
    /// top of a preview reservation that is still live.
    ///
    /// `true` when one undo step was published. Throws the capture's sentence
    /// when the document, target or selection moved under the gesture, having
    /// published nothing. The preview image is cleared on EVERY exit.
    @discardableResult
    func finish(opacity: Double) throws -> Bool {
        cancelInFlight()
        defer { canvas?.previewImage = nil }
        guard let document else { return false }
        try capture.requireCurrent(document)
        guard let next = try Self.document(capture, opacity: opacity) else { return false }
        return try capture.publish(
            next.precision, to: document, name: "Layer Opacity",
            record: .selectionProperty(
                "opacity", ActionArgs.number(opacity),
                selectionCount: capture.selectedLayers.count, note: "Layer Opacity"))
    }

    /// The same cancel-first prefix as `finish`, publishing nothing.
    func cancel() {
        cancelInFlight()
        canvas?.previewImage = nil
    }

    private func cancelInFlight() {
        generation &+= 1
        control?.cancel()
        renderer.cancel()
    }

    /// The reservation's own bookkeeping slack: `reservePreview` requires a
    /// total strictly greater than the declared output, and a projection
    /// estimate alone can be its exact size.
    private static let bookkeepingBytes: UInt64 = 65_536
}

/// Which generation the worker actually delivered, across the preview
/// boundary — the same immutable hand-off `PrecisionAdjustmentSession` uses.
private final class DeliveredGeneration {
    private let lock = NSLock()
    private var generation: UInt64 = 0
    var value: UInt64 {
        lock.lock()
        defer { lock.unlock() }
        return generation
    }
    func set(_ value: UInt64) {
        lock.lock()
        generation = value
        lock.unlock()
    }
}
