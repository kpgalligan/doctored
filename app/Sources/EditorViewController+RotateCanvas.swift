import AppKit

// Image ▸ Rotate Arbitrary… (Phase 12F-1b3). Its MCP mirror is the `rotate`
// tool's filtered branch (AgentServer+RotateCanvas.swift).
extension EditorViewController {
    @objc func rotateArbitrary(_ sender: Any?) {
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(
            RotateCanvasSheetController(document: document) { [weak self] degrees, sampler in
                self?.commitCanvasRotation(degrees: degrees, sampler: sampler)
            })
    }

    /// The commit, re-planned against the document's CURRENT handle: a
    /// document-modal sheet does not stop the main run loop, so an agent's
    /// edit can land behind it and the canvas the sheet measured may not be
    /// the canvas being rotated.
    func commitCanvasRotation(degrees: Double, sampler: DrResizeFilter) {
        guard let document = document, let doc = document.doc,
            let plan = RasterDocument.canvasRotationPlan(
                width: doc.width, height: doc.height, degrees: degrees)
        else {
            NSSound.beep()
            return
        }
        switch plan.kind {
        case .identity:
            // A whole number of full turns moves nothing. No step, no alert,
            // no dirtying — the Canvas Size sheet's own answer to a request
            // for the size it already is.
            return
        case .quarter(let op):
            // Delegated to the Image menu's own path, so a text or shape
            // description COMPOSES with the turn and the recorded
            // step keeps its integer form.
            document.applyEdit(
                op.actionName, record: Self.geometryRecord(op), changesCanvasGeometry: true,
                resultingSelection: document.selectionCarried(through: op)
            ) {
                try $0.applyingDocumentGeometry(op)
            }
        case .filtered:
            guard plan.width > 0, plan.height > 0 else {
                NSSound.beep()
                return
            }
            // The resample rewrites the pixels a described layer was rendered
            // from. Text and shape layers, the only described kinds, are
            // regenerated from their descriptions inside the same edit
            // (`rotatingCanvas`), so nothing is asked.
            let described = (0..<doc.layerCount).filter { document.layerDescribesSource($0) }
            let background = backgroundColor
            // A core refusal arrives as a thrown sentence and propagates to
            // `applyEdit`, which PRESENTS it (no beep): the core's sentence
            // is the only thing that can tell the user what to do about it.
            // The rotation runs as ONE background job with its display
            // preparation, so Cancel/Esc throws `JobCancelled`,
            // which `applyEdit` presents as nothing.
            defer { document.preparedEdit = nil }
            document.applyEdit(
                "Rotate Canvas",
                record: .rotateCanvas(
                    degrees: plan.degrees, sampler: Self.samplerAgentName(sampler)),
                changesCanvasGeometry: true,
                // The selection turns with the canvas.
                resultingSelection: document.selectionCarried(
                    by: plan.map(canvasWidth: doc.width, canvasHeight: doc.height),
                    width: plan.width, height: plan.height)
            ) { current in
                guard let prepared = try current.rotatingCanvas(
                    degrees: plan.degrees, sampler: sampler, described: described,
                    indexedBackground: background)
                else { return nil }
                document.preparedEdit = prepared
                return prepared.document
            }
        }
    }
}
