import AppKit

// RGB and alpha remain separate on a new layer/mask; old roots hide without losing original pixels.
// Assembly checks cancellation during foreground/matte and shared record/string copying. The
// cutout's strictly qualified one-visible-root projection drives the existing byte
// compositor/quantizer in bounded chunks; the shared general compositor defaults are unchanged.
//
// CoreGraphics and individual admitted/capped metadata-validation calls remain internally
// noncancellable. A claimed compound parent forwards Cancel and document-close across the complete
// preparation. Both cutout stages use `BackgroundWork.wait`'s task factory so the actual Cancel
// panel exists before worker dispatch; nested busy failures restore any outer waiting state. Its
// callers are Select and Mask's New Layer outputs (`SelectionRefinement+NewLayer.swift`: the sheet,
// the `refine_selection` MCP tool and Actions).

/// The New Layer output's plumbing. The foreground and the refined alpha are
/// separate inputs; the original layers remain recoverable and the result is
/// one document edit.
struct RefinementCutoutCapture {
    let selection: SelectionCapture
    let trimap: RefinementTrimapSession.Snapshot

    init(document: ImageDocument, session: RefinementTrimapSession) throws {
        // Foreground is supplied separately. Capture identity/selection without
        // making a display-derived source, including for native RGBA16 output.
        selection = try SelectionCapture(document: document, needsSource: false)
        trimap = try session.snapshot()
        guard trimap.trimap.width == selection.doc.width,
              trimap.trimap.height == selection.doc.height else {
            throw DoctoredCoreError(message: "The trimap session does not match the document canvas.")
        }
    }

    func validate(in document: ImageDocument, session: RefinementTrimapSession) throws {
        guard !document.hasClosed else {
            throw DoctoredCoreError(message: "The cutout document closed before publication.")
        }
        try selection.requireCurrent(in: document)
        guard session.isCurrent(trimap) else {
            throw DoctoredCoreError(message: "The refinement strokes changed. Recompute the cutout before applying it.")
        }
    }

    /// Caller supplies RGB in the captured document's profile; the
    /// foreground's own alpha is kept on the new layer. Every
    /// result is tied to both captured document/selection and local stroke
    /// revision. Assembly copies pixels and document records in bounded chunks;
    /// the internal one-root projection drives the shared byte compositor and
    /// quantizer in bounded chunks. CoreGraphics construction retains its
    /// noncancellable interior, with Cancel/owner-close boundary checks.
    func prepare(foreground: RasterImage, alpha: [UInt8], in document: ImageDocument,
                 session: RefinementTrimapSession,
                 control: RasterJobControl? = nil) throws -> PreparedRefinementCutout? {
        try validate(in: document, session: session)
        let captured = selection.doc
        guard foreground.width == captured.width, foreground.height == captured.height,
              alpha.count == captured.width * captured.height else {
            throw DoctoredCoreError(message: "Refinement foreground and alpha must match the captured canvas.")
        }
        let preparation = try RefinementCutoutPreparation(document: document, control: control)
        defer { preparation.finish() }
        let token = preparation.control
        let coreScratch = captured.refinementCutoutScratchBytes
        let scratch = coreScratch + UInt64(alpha.count)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let result = try BackgroundWork.wait(title: "Creating Cutout", control: token) {
            try WorkerTask(scratchEstimate: scratch) {
                try token.withChildOperation(from: 0, to: 0.25) { child in
                    guard child.setScratchLimit(coreScratch) else {
                        throw DoctoredCoreError(message: "Could not configure cutout assembly memory.")
                    }
                    return try captured.addingRefinementCutout(foreground: foreground, alpha: alpha, control: child)
                }
            }
        }
        try validate(in: document, session: session)
        guard !token.isCancelled else { throw JobCancelled() }
        guard let result else { return nil }
        let renderScratch = try result.refinementCutoutRenderScratchBytes()
        let projectionScratch = renderScratch + UInt64(alpha.count) * 5
        _ = try RasterResources.shared.admit(scratch: projectionScratch)
        let prepared = try BackgroundWork.wait(title: "Preparing Cutout Display", control: token) {
            try WorkerTask(scratchEstimate: projectionScratch) {
                try token.withChildOperation(from: 0.25, to: 1) { child in
                    try child.beginCompoundOperation()
                    let projection = try child.withChildOperation(from: 0, to: 0.98) { renderControl in
                        guard renderControl.setScratchLimit(renderScratch) else {
                            throw DoctoredCoreError(message: "Could not configure byte cutout display memory.")
                        }
                        return try result.renderRefinementCutout(control: renderControl)
                    }
                    let prepared = PreparedDocumentEdit(result, renderedByteProjection: projection,
                        borrowedInputBytes: UInt64(alpha.count), control: child)
                    if let error = prepared.error { throw DoctoredCoreError(message: error) }
                    return prepared
                }
            }
        }
        try validate(in: document, session: session)
        guard !token.isCancelled else { throw JobCancelled() }
        guard prepared.projection != nil, prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "The cutout display could not be prepared.")
        }
        return PreparedRefinementCutout(capture: self, edit: prepared, control: token)
    }
}

struct PreparedRefinementCutout {
    let capture: RefinementCutoutCapture
    let edit: PreparedDocumentEdit
    let control: RasterJobControl
    var document: RasterDocument { edit.document }

    /// No permission is implied by preparation: publication rechecks both
    /// revisions, document identity and cancellation immediately before edit.
    /// `name` is the history step's; `record` its Action step.
    func commit(in document: ImageDocument, session: RefinementTrimapSession,
                name: String = "Refine to New Layer",
                record: [ActionStep] = .notACommand) throws {
        try capture.validate(in: document, session: session)
        guard !control.isCancelled else { throw JobCancelled() }
        document.preparedEdit = edit
        try document.preflightPublication(edit.document, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit(name, record: record) { _ in edit.document }
            document.activeLayerIndex = edit.document.layerCount - 1
            document.planeEditTarget = .layer
            // applyEdit has already posted its document-change notification.
            // Retarget the actual canvas/panels as well as the history model.
            for controller in document.windowControllers {
                if let editor = controller.contentViewController as? EditorViewController {
                    editor.setPaintTarget(.layer)
                    editor.activeLayerDidChange()
                }
            }
        }
    }
}
