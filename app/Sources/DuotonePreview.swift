import AppKit

// Duotone Options' Preview (#1 Duotone phase 2a; documented, unmeasured until phase 2b's menu
// walk): while the sheet's Preview box is on, the canvas shows the document through the sheet's
// current inks. View-only, so no MCP mirror (`set_duotone` and `render` cover it). The pattern is
// `HdrToningSheetController.requestPreview` / `showPreview`: the candidate is the captured
// document with the inks set (`settingDuotone`, every sample shared; the source itself when the
// inks change nothing), its view drawn through `DuotoneDocument.display` from the projection
// captured when the sheet opened, on `PreviewRenderer`'s queue under a preview lease of the
// candidate's projection estimate; a render that lands after the document moved on clears the
// preview.

final class DuotonePreview {
    private weak var editor: EditorViewController?
    private weak var canvas: ImageCanvasView?
    /// The handle the preview runs on, captured when the sheet opened: the renderer's closure runs
    /// on a background queue and must never reach into live document state.
    private let baseDoc: RasterDocument
    private let typed: RasterPrecisionDocument
    private let projection: RasterPrecisionImage
    private let renderer = PreviewRenderer()

    init(editor: EditorViewController, document: ImageDocument) throws {
        guard let doc = document.doc, let typed = doc.precision else {
            throw DoctoredCoreError(message: "The Duotone preview needs an 8- or 16-bit Grayscale document.")
        }
        self.editor = editor
        canvas = editor.canvas
        baseDoc = doc
        self.typed = typed
        projection = try document.currentPrecisionProjection()
        renderer.onRender = { [weak self] image in self?.show(image) }
    }

    /// The canvas through `options`; nil clears the preview.
    func request(_ options: DuotoneOptions?) {
        renderer.cancelCompute?()
        renderer.cancelCompute = nil
        guard let options else {
            renderer.request { nil }
            canvas?.previewImage = nil
            return
        }
        let projection = projection
        do {
            let candidate = try typed.settingDuotone(options) ?? typed
            let shown = try RasterDocument(precision: candidate)
            let space = shown.colorSpace
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: try shown.projectionScratchEstimate(), outputBytes: 0)
            renderer.cancelCompute = { reservation.control.cancel() }
            renderer.request { reservation.runAsOwnLease { token in
                do {
                    let context = try PrecisionWorkContext(image: projection)
                    let image = try DuotoneDocument.display(of: projection, precision: candidate,
                                                            context: context, control: token)
                    return token.isCancelled ? nil : image.makeCGImage(in: space)
                } catch { return nil }
            } }
        } catch {
            renderer.request { nil }
        }
    }

    /// Cancels the renderer and clears the preview (the sheet's Apply and Cancel).
    func end() {
        renderer.cancel()
        canvas?.previewImage = nil
    }

    /// A render that lands after the document moved on is dropped, so a stale picture never sits
    /// over the canvas.
    private func show(_ image: CGImage?) {
        guard editor?.document?.doc === baseDoc else { canvas?.previewImage = nil; return }
        canvas?.previewImage = image
    }
}
