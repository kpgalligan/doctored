import AppKit

/// The controller half of the Move tool's Show Transform Controls
/// (`ImageCanvasView+MoveControls.swift`): which box the canvas shows, and
/// what a press on one of its handles does.
///
/// The box is the one a Free Transform over the same layers opens around a
/// set — the union of the moving set's content bounds
/// (`transformSourceRect`) — and a handle press opens that Free Transform
/// (⌘T's own `freeTransform`, with every refusal it has) and gives it the
/// same handle, so the drag that started on the Move tool's box carries on
/// as a Free Transform scale. Nothing is committed until the session is.
extension EditorViewController {
    /// Called once from `loadView`.
    func installMoveControls() {
        canvas.onMoveControlsHandle = { [weak self] in self?.moveControlsHandlePressed($0, $1) }
    }

    /// Recomputes the box. Called when the tool, the option, the document or
    /// the layer selection changes. During a live drag the box stands down
    /// (the content it frames is moving under it).
    func refreshMoveControls() {
        var rect: CGRect?
        if currentTool == .move, ToolOptionsStore.shared.move.transformControlsShown,
           transformSession == nil, let document, !document.isLiveEditing, let doc = document.doc {
            rect = transformSourceRect(doc, layers: doc.movingSet(document.selectedLayerIndices))
        }
        if canvas.moveControlsRect != rect { canvas.moveControlsRect = rect }
    }

    /// A press on handle `index` of the Move tool's box: open Free Transform
    /// and start its drag on the same handle, as `canvas.onTransformMouseDown`
    /// would for a press on the session's own box.
    private func moveControlsHandlePressed(_ index: Int, _ modifiers: NSEvent.ModifierFlags) {
        freeTransform(nil)
        guard let session = transformSession else { return }
        let handles = ImageCanvasView.transformHandlePoints(
            session.transform.warpedQuad(of: session.sourceRect))
        guard handles.indices.contains(index) else { return }
        transformMouseDown(handles[index], modifiers)
    }
}
