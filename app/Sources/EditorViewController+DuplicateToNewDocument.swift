import AppKit

/// Layer ▸ Duplicate to Document… (#1 phase 4c). The sheet picks the
/// Destination. "New Document" (the default) copies the selection's
/// independent roots into a new one through ONE core call
/// (`ImageDocument.openDuplicate`); the source gets no undo step and no dirty
/// change, so the step is recorded against it here, on the success path
/// only. An open document takes the same layers as a layers paste in place
/// in that document's window (`pasteLayers`, `LayerDrag.entry`, the drop's
/// own path) — one undo step there, converted into its colour mode and
/// profile — which is not a replayable step (an Action runs in one
/// document). MCP twins: `duplicate_to_new_document` and
/// `copy_layers_to_document` with placement `in_place`.
extension EditorViewController {
    @objc func duplicateToNewDocument(_ sender: Any?) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let indices = document.selectedLayerIndices
        let roots = doc.layerTree.independentRoots(indices)
        guard let top = roots.last else {
            NSSound.beep()
            return
        }
        let defaultName = doc.layerInfo(top)?.name ?? ""
        let others = NSDocumentController.shared.documents
            .compactMap { $0 as? ImageDocument }
            .filter { $0 !== document && $0.doc != nil && !$0.editors.isEmpty }
        let sheet = DuplicateToNewDocumentSheetController(
            defaultName: defaultName, destinations: others.map(\.displayName)) {
            [weak self] name, destination in
            if destination == 0 {
                self?.commitDuplicateToNewDocument(source: doc, layers: indices, name: name)
            } else if destination <= others.count {
                self?.commitDuplicate(source: doc, layers: indices, into: others[destination - 1])
            }
        }
        presentAsSheet(sheet)
    }

    /// The sheet's Apply: the layers captured when it opened, refused when the
    /// document changed behind the sheet (an agent edit), since the captured
    /// indices would then name other layers.
    private func commitDuplicateToNewDocument(source: RasterDocument, layers: [Int], name: String) {
        guard let document else { return }
        guard document.doc === source, !document.hasClosed else {
            document.presentError(DoctoredCoreError(
                message: "The document changed. Choose Duplicate to Document again."))
            return
        }
        do {
            try ImageDocument.openDuplicate(of: document, layers: layers, name: name)
            ActionRecorder.shared.record(.duplicateToNewDocument(name: name), on: document)
        } catch {
            document.presentError(error)
        }
    }

    /// An open destination: the captured layers pasted in place into
    /// `target`'s window as its own layers paste, that window brought
    /// forward.
    private func commitDuplicate(source: RasterDocument, layers: [Int], into target: ImageDocument) {
        guard let document else { return }
        guard document.doc === source, !document.hasClosed, !target.hasClosed,
              let editor = target.editors.first else {
            document.presentError(DoctoredCoreError(
                message: "The document changed. Choose Duplicate to Document again."))
            return
        }
        do {
            let entry = try LayerDrag.entry(from: document, indices: layers)
            editor.pasteLayers(entry, placement: .inPlace,
                               record: .unrecorded("Duplicate to Document"))
            editor.view.window?.makeKeyAndOrderFront(nil)
        } catch {
            document.presentError(error)
        }
    }
}
