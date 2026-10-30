import AppKit

/// The controller half of the Pen tool (PenSession.swift): the tool
/// action, the canvas's path cache, the commit of a finished gesture and the
/// options bar. MCP `set_path` is the twin of the commit: every gesture
/// records the whole path as one `set_path` step — or, when the Pen is
/// editing a layer's vector mask, one `set_vector_mask` step.
extension EditorViewController {
    @objc func selectPenTool(_ sender: Any?) { selectTool(.pen) }

    /// Called once from `loadView`.
    func installPen() {
        canvas.onPenCommit = { [weak self] in self?.penGestureCommitted($0, name: $1) ?? false }
    }

    /// Reads the document's current path into the canvas's session, so a
    /// redraw and a hit test start from a cached shape. Called beside
    /// `refreshCanvasGuides` — at load and on every document change — and by
    /// whatever changes `ImageDocument.currentPath`.
    ///
    /// A LIVE tick is skipped: a Move drag posts one per mouse-moved event,
    /// no live edit can change a path, and the read crosses the FFI boundary
    /// and parses the whole list. With no current path there is no read at
    /// all (`resolvedPath` answers before it asks the core).
    func refreshCanvasPath(_ note: Notification? = nil) {
        guard note?.userInfo?["isLive"] as? Bool != true else { return }
        // MCP `set_active_layer` reaches here through the document-change
        // notification: a current path on another layer's vector mask is
        // dropped, and the panel deselects its row.
        if followActiveLayerVectorMask() { pathsPanel?.activeLayerChanged() }
        let target = document?.currentPath
        var session = canvas.penSession
        session.adopt(
            target: target, shape: document?.resolvedPath(target),
            rubberBand: ToolOptionsStore.shared.pen.rubberBand)
        canvas.penSession = session
    }

    /// A finished Pen gesture: `shape` is the whole path as the gesture left
    /// it. True when it was committed, which is what lets the canvas keep
    /// drawing from the subpath the gesture left open.
    private func penGestureCommitted(_ shape: PathShape, name: String) -> Bool {
        guard let document else {
            NSSound.beep()
            return false
        }
        // A current path left on another layer's vector mask (MCP `new_layer`
        // moves the active layer after its notification) is cleared, and the
        // gesture refused, so the stale mask is never committed to.
        if followActiveLayerVectorMask() {
            refreshCanvasPath()
            pathsPanel?.activeLayerChanged()
            NSSound.beep()
            return false
        }
        // A vector mask is not a path of the list: the gesture reshapes the
        // layer's mask (`ImageDocument.commitPenVectorMask`, `set_vector_mask`).
        if case .vectorMask(let layerID)? = document.currentPath {
            guard document.commitPenVectorMask(shape, layerID: layerID, name: name) else {
                return false
            }
            refreshCanvasPath()
            return true
        }
        guard document.commitPenPath(shape, name: name) else { return false }
        // The edit's own notification ran before a new work path was made
        // current, so the session is read again now that it is — and the
        // Paths panel selects the row it could not find a moment ago.
        refreshCanvasPath()
        pathsPanel?.activeLayerChanged()
        return true
    }

    /// The Pen's options bar.
    func penClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "pen.rubberBand", overflowLabel: "Rubber Band",
                    kind: .checkbox(
                        label: "Rubber Band",
                        get: { ToolOptionsStore.shared.pen.rubberBand },
                        set: { [weak self] value in
                            ToolOptionsStore.shared.pen.rubberBand = value
                            self?.refreshCanvasPath()
                        })),
            ]),
        ]
    }
}
