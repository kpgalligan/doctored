import AppKit

/// The controller half of a path shape layer's outline as the current path
/// (ShapeOutlineTarget.swift): the selection-tool press that makes it current, and the commit
/// the Pen, Path Selection, Direct Selection and Combine Selected Subpaths share, which names a
/// lock before it re-renders. The Paths panel's shape-path row makes it current too
/// (PathsPanelViewController.swift); `edit_shape_layer` with `subpaths` is the MCP twin, and
/// `get_shape_layer` reads the outline back.
extension EditorViewController {
    /// Called once from `loadView`.
    func installShapeOutlineTarget() {
        canvas.onPathSelectNeedsTarget = { [weak self] in self?.adoptActiveShapeOutline() ?? false }
    }

    /// A Path Selection or Direct Selection press with no current path to edit: when the active
    /// layer is a path shape, its outline becomes the current path and the press goes on to it
    /// (true). Anything else is the press's beep. A current path that resolves is never
    /// replaced by a press — the row in the Paths panel is how to leave it.
    func adoptActiveShapeOutline() -> Bool {
        guard let document, let doc = document.doc,
              document.resolvedPath(document.currentPath) == nil,
              let layer = doc.layerInfo(document.activeLayerIndex),
              doc.shapeOutlinePath(document.activeLayerIndex) != nil
        else { return false }
        setCurrentPath(.shapeOutline(layerID: layer.id))
        return canvas.penSession.shape != nil
    }

    /// One gesture on the outline of layer `layerID`, as one undo step re-rendering the layer
    /// (`ImageDocument.commitShapeOutline`). An outline whose held subpath operations the layer
    /// cannot take (a stroke, an unbounded fold: `heldOpsRefusal`) is named in an alert first; a
    /// pixel lock next, as the shape re-edit names it; a transparency lock only refuses a
    /// re-render that moves or resizes the raster, so it is named after the fact. True when it
    /// was committed.
    func commitShapeOutlineGesture(
        _ shape: PathShape, layerID: UInt64, name: String, note: String
    ) -> Bool {
        guard let document, let index = document.doc?.layerIndex(id: layerID) else {
            NSSound.beep()
            return false
        }
        if let payload = document.doc?.shapePayload(index),
           let refusal = ShapeLayerPayload.heldOpsRefusal(
               outline: shape, stroked: !payload.stroke.isEmpty && payload.strokeWidth > 0) {
            presentHeldOpsRefusal(refusal)
            return false
        }
        guard !refuseLockedEdit(layer: index, kind: DR_EDIT_PIXELS) else { return false }
        switch document.commitShapeOutline(shape, layerID: layerID, name: name, note: note) {
        case .committed:
            return true
        case .silent:
            return false
        case .refused:
            if document.doc?.transparencyLockOwner(index) != nil {
                refuseLockedRerender(layer: index)
            } else {
                NSSound.beep()
            }
            return false
        }
    }
}
