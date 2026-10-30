import AppKit

/// The controller half of dragging layers between documents
/// (LayerDrag.swift): this window as a drag's DESTINATION (its canvas through
/// `FileDropView`, its Layers panel through the table's drop delegate), and
/// the Move tool's drag turning into a layer drag when it leaves the window.
/// MCP `copy_layers_to_document` is the twin of the drop.
extension EditorViewController {
    /// Called once from `loadView`, before the panel and the root view exist:
    /// only the canvas is wired here. `installLayerDropTargets` does the rest.
    func installLayerDrag() {
        canvas.onLayerDragOut = { [weak self] in self?.beginLayerDragOut($0) ?? false }
    }

    /// Called once from `loadView`, after the Layers panel is made: the panel
    /// and the window's root view forward a foreign layer drag here.
    func installLayerDropTargets(root: FileDropView) {
        root.onLayerDropOperation = { [weak self] in self?.layerDropOperation($0) ?? [] }
        root.onLayerDrop = { [weak self] in self?.acceptLayerDrop($0) ?? false }
        layersPanel.onLayerDropOperation = { [weak self] in self?.layerDropOperation($0) ?? [] }
        layersPanel.onLayerDrop = { [weak self] in self?.acceptLayerDrop($0) ?? false }
    }

    /// What a layer drag over this window would do: copy, when it comes from
    /// ANOTHER document; nothing for this document's own layers (the panel's
    /// own reorder handles those) or for a drag that is not a layer drag.
    func layerDropOperation(_ info: NSDraggingInfo) -> NSDragOperation {
        guard let session = LayerDrag.session(for: info), let source = session.source,
              let document, document.doc != nil, source !== document,
              !document.isLiveEditing, view.window?.attachedSheet == nil
        else { return [] }
        return .copy
    }

    /// The drop: the dragged layers pasted above the active layer as one undo
    /// step — in place with ⇧, by Edit ▸ Paste's rule otherwise — and this
    /// window brought forward. The step is not replayable (an Action runs in
    /// one document), so the recording shows a named placeholder.
    func acceptLayerDrop(_ info: NSDraggingInfo) -> Bool {
        guard layerDropOperation(info) == .copy, let session = LayerDrag.session(for: info),
              let source = session.source, let document
        else { return false }
        // Never paste under an open text, shape or transform session.
        guard commitPendingSessions() else { return false }
        do {
            let entry = try LayerDrag.entry(from: source, indices: session.indices)
            let before = document.doc
            pasteLayers(
                entry, placement: NSEvent.modifierFlags.contains(.shift) ? .inPlace : .auto,
                record: .unrecorded("Drag Layers"))
            view.window?.makeKeyAndOrderFront(nil)
            // A profile-mismatch alert finishes the paste later; the drop was taken.
            return document.doc !== before || view.window?.attachedSheet != nil
        } catch {
            document.presentError(error)
            return false
        }
    }

    /// A Move drag left the window: when the drag is moving LAYERS (no pixel
    /// selection, no ⌥ duplicate) and there is another document to drop on,
    /// the live move is cancelled — the layer goes back where it was, with no
    /// undo step — and the canvas starts a drag of the selected layers.
    private func beginLayerDragOut(_ event: NSEvent) -> Bool {
        guard let document, document.doc != nil, document.canvasSelection == nil,
              moveSelectedSession == nil, moveDuplicate == nil, moveAppliedDelta != nil,
              NSDocumentController.shared.documents.contains(where: { $0 !== document })
        else { return false }
        let indices = document.selectedLayerIndices
        guard !indices.isEmpty else { return false }
        if precisionMoveGesture != nil {
            moveDidCancel()
        } else {
            moveAppliedDelta = nil
            moveDidEnd()
            document.cancelLiveEdit()
        }
        LayerDrag.current = LayerDrag.Session(source: document, indices: indices, sourceView: canvas)
        canvas.beginLayerDragSession(with: event)
        return true
    }
}
