import AppKit

// `MaskSheetPreview` owns a reservation for full-resolution previews.

/// A mask sheet owns its immutable inputs and one bounded preview reservation.
/// The legacy mask renderer cannot stop mid-kernel; cancellation discards its
/// result and keeps accounting alive until its closure actually finishes.
final class MaskSheetPreview {
    private weak var document: ImageDocument?
    private weak var canvas: ImageCanvasView?
    private let capture: MaskEditCapture
    private let renderer = PreviewRenderer()
    private var control: RasterJobControl?
    private var finished = false

    init(document: ImageDocument, canvas: ImageCanvasView, capture: MaskEditCapture) {
        self.document = document
        self.canvas = canvas
        self.capture = capture
        renderer.onRender = { [weak self] image in self?.accept(image) }
    }

    func request(_ operation: MaskEdit) throws {
        guard !finished else { return }
        renderer.onRender = { [weak self] image in self?.accept(image) }
        guard let document else { return }
        try capture.validate(in: document)
        control?.cancel()
        let reservation = try SelectionWorkBudget.reservePreview(
            bytes: operation.scratchEstimate(for: capture), outputBytes: 0)
        let token = reservation.control
        control = token
        renderer.cancelCompute = { token.cancel() }
        let captured = capture
        // The mask edit's own plan and the display preparation each charge
        // this lease, so the worker leaves it out of `otherRetained`.
        renderer.request { reservation.runAsOwnLease { token in
            guard !token.isCancelled else { return nil }
            let updated = (try? operation.apply(to: captured)) ?? captured.source
            guard !token.isCancelled else { return nil }
            if updated.isHighDepth {
                let result = PreparedDocumentEdit(updated).preview
                return token.isCancelled ? nil : result
            }
            let result = updated.flattened()?.makeCGImage(in: updated.colorSpace)
            return token.isCancelled ? nil : result
        } }
    }

    private func accept(_ image: CGImage?) {
        guard let document, (try? capture.validate(in: document)) != nil else {
            canvas?.previewImage = nil
            return
        }
        canvas?.previewImage = image
    }

    func cancel() {
        renderer.cancel()
        canvas?.previewImage = nil
    }

    /// Dismissal can end field editing and send one final control action.
    /// Once the sheet has finished, that action must never revive its preview.
    func finish() {
        finished = true
        cancel()
    }
}
