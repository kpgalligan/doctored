import AppKit

// The first Move Selected tick also starts a `SelectedPixelsFloatQuery` (this file: a
// `PreviewRenderer` under a 64 KiB preview reservation over the core's float-rect query) whose
// EMPTY answer — only a successful one; a cancel, a refused reservation or an error never answers —
// tears the preview down and refuses with `emptyRefusal` mid-drag.

/// The FLOAT's own rect: the canvas bbox of the active layer's
/// pixels with alpha > 0 under the selection coverage, asked of the core
/// (`dr_doc_selected_pixels_bounds` / its typed twin) off main. Two callers:
///
/// - ⌘T over a selection (and `transform_selected_pixels`) opens its box on
///   it through `bounds`, one short cancellable wait;
/// - the Move drag starts a `SelectedPixelsFloatQuery` on its first tick and
///   refuses an EMPTY float by name mid-drag instead of at mouse-up.
enum SelectedPixelsFloat {
    /// The float's rect, or nil for an empty float, under a Cancel/Esc wait
    /// titled `title`. The query reads in place and allocates nothing, so
    /// its admission is a fixed 64 KiB.
    static func bounds(_ source: RasterDocument, layer: Int, coverage: [UInt16],
                       title: String) throws -> CGRect? {
        try PrecisionCapturedWork.run(scratchEstimate: 65_536, title: title,
                                      revealAfter: DocumentJobs.panelDelay) { control in
            try source.selectedPixelsBounds(layer: layer, coverage: coverage, control: control)
        }
    }
}

/// One background float-rect query for the Move drag: a `PreviewRenderer`
/// under a registered 64 KiB `SelectionWorkBudget` preview reservation, its
/// answer handed back through a `TransformPreviewMailbox`.
///
/// `onResult` fires AT MOST ONCE, on main, and ONLY for a successful answer:
/// a rect, or nil for "the float is empty". A cancelled scan, a refused
/// reservation (a background operation is running, or the preview budget is
/// full) and any core refusal never call it — the drag then simply goes on,
/// and an empty float still refuses at mouse-up as the commit always did.
/// `cancel()` drops `onResult` first, so a late answer never reaches a
/// finished drag.
final class SelectedPixelsFloatQuery {
    private let source: RasterDocument
    private let layer: Int
    private let coverage: [UInt16]
    private let renderer = PreviewRenderer()
    private var started = false
    var onResult: ((CGRect?) -> Void)?

    init(source: RasterDocument, layer: Int, coverage: [UInt16]) {
        self.source = source
        self.layer = layer
        self.coverage = coverage
    }

    func start() {
        assert(Thread.isMainThread)
        let source = self.source, layer = self.layer, coverage = self.coverage
        guard !started else { return }
        started = true
        let reservation: SelectionWorkBudget.Reservation
        do {
            reservation = try SelectionWorkBudget.reservePreview(bytes: 65_536, outputBytes: 0)
        } catch { return }
        let mailbox = TransformPreviewMailbox<CGRect?>()
        renderer.cancelCompute = { reservation.control.cancel() }
        renderer.onRender = { [weak self] _ in
            guard let self, let result = mailbox.take() else { return }
            self.renderer.cancel()
            guard case .success(let rect) = result, let onResult = self.onResult else { return }
            self.onResult = nil
            onResult(rect)
        }
        renderer.request {
            reservation.run { control in
                guard !control.isCancelled else { return nil }
                do {
                    let rect = try source.selectedPixelsBounds(
                        layer: layer, coverage: coverage, control: control)
                    guard !control.isCancelled else { return nil }
                    mailbox.put(.success(rect))
                } catch {}
                return nil
            }
        }
    }

    func cancel() {
        onResult = nil
        renderer.cancel()
    }
}
