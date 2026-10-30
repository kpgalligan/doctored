import AppKit

// The warp worker creates progress/Cancel before dispatch and prepares the final projection before
// one history/Action publication.

/// Captured native final edit and prepared display. The caller publishes once.
struct PersistedWarpWork {
    let source: RasterDocument
    let request: PersistedWarpRequest
    /// On an Indexed document, the colour an uncovered pixel's entry is nearest with no
    /// transparent index (#547 Indexed phase 3b, the core's `indexed_background` on a `set`);
    /// nil is white.
    var indexedBackground: NSColor? = nil

    /// The core request: `request`'s JSON, a `set` on an Indexed document with the background key
    /// appended (the field's own spelling kept exactly, `IndexedDocument.splicedRequest`).
    private func requestText() throws -> String {
        let text = try request.jsonRequest()
        guard case .set = request else { return text }
        return try IndexedDocument.splicedRequest(
            text, IndexedDocument.backgroundFields(indexedBackground, in: source))
    }

    func wait(in document: ImageDocument, title: String) throws -> PreparedDocumentEdit? {
        assert(Thread.isMainThread)
        guard document.doc === source, !document.isLiveEditing,
              let targetID = source.layerInfo(request.layer)?.id else {
            throw DoctoredCoreError(message: "The warp target changed before work started.")
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let text = try requestText()
        let plan = try source.warpEditPlan(request: text)
        let coreScratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        if coreScratch == 0 {
            guard document.doc === source, document.revision == revision,
                  !document.isLiveEditing, document.isCurrentEditingState(state),
                  source.layerInfo(request.layer)?.id == targetID else {
                throw DoctoredCoreError(message: "The warp target or editing state changed while work ran.")
            }
            return nil
        }
        let projection = UInt64(source.width) * UInt64(source.height) * 24
        let scratch = try PrecisionDocumentDescriptor.sum(coreScratch, projection)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        let prepared = try BackgroundWork.wait(title: title, control: control) {
            try WorkerTask(scratchEstimate: scratch) {
                try control.beginCompoundOperation()
                guard let next = try control.withChildOperation(from: 0, to: 0.75, { child in
                    try source.editingWarp(request: text, control: child)
                }) else { return nil as PreparedDocumentEdit? }
                let prepared = try control.withChildOperation(from: 0.75, to: 1) { child in
                    PreparedDocumentEdit(next, scratch: BackgroundWork.scratch, control: child)
                }
                if let error = prepared.error { throw DoctoredCoreError(message: error) }
                return prepared
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.doc === source, document.revision == revision,
              !document.isLiveEditing, document.isCurrentEditingState(state),
              source.layerInfo(request.layer)?.id == targetID else {
            throw DoctoredCoreError(message: "The warp target or editing state changed while work ran.")
        }
        return prepared
    }
}
