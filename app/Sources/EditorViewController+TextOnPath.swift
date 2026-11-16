import AppKit

/// Layer ▸ Place Text on Path and Remove Text from Path (designs/type-on-a-path.md TP-9): each
/// runs `edit_text_layer` with `on_path` through the agent's dispatch (`runPathStep`), so it is
/// one undo step and records its step. The value and its geometry are TextOnPath.swift.
extension EditorViewController {
    /// Places the active one-line text layer on the current path: a path of the list by its
    /// name, the active layer's vector mask or path shape outline by its subpath itself; the
    /// subpath the selection tools selected first, else the first; offset 0.
    @objc func placeTextOnPath(_ sender: Any?) {
        // A text session commits first: the line it holds is what goes on the path.
        guard commitPendingSessions(), let document, let doc = document.doc,
              !document.quickMaskActive,
              let payload = doc.textPayload(document.activeLayerIndex),
              !TextOnPath.hasLineBreak(payload.string),
              let current = currentPathStep(), !current.shape.isEmpty
        else {
            NSSound.beep()
            return
        }
        let index = selectedSubpathsOfCurrentPath()?.indices.first ?? 0
        guard current.shape.subpaths.indices.contains(index) else {
            NSSound.beep()
            return
        }
        var onPath: [String: Any] = ["offset": 0]
        if let name = current.arguments["path"] as? String {
            onPath["path_name"] = name
            onPath["subpath"] = index
        } else {
            onPath["subpaths"] = PathShape(subpaths: [current.shape.subpaths[index]]).subpathsJSON
        }
        runPathStep(
            "edit_text_layer",
            ["layer": ActionArgs.layer(doc.layerRef(document.activeLayerIndex)), "on_path": onPath],
            undoName: "Place Text on Path", note: "Layer ▸ Place Text on Path")
    }

    /// Takes the active text layer off its path: it is laid out in its block again, as point
    /// text.
    @objc func removeTextFromPath(_ sender: Any?) {
        guard commitPendingSessions(), let document, let doc = document.doc,
              doc.textPayload(document.activeLayerIndex)?.onPath != nil
        else {
            NSSound.beep()
            return
        }
        runPathStep(
            "edit_text_layer",
            ["layer": ActionArgs.layer(doc.layerRef(document.activeLayerIndex)),
             "on_path": NSNull()],
            undoName: "Remove Text from Path", note: "Layer ▸ Remove Text from Path")
    }

    /// Enablement for this file's items; nil for anybody else's. Place needs no Quick Mask, the
    /// active layer an unwarped text layer of one line, and a current path with a subpath;
    /// Remove needs the active layer on a path.
    func validateTextOnPathItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard let document else { return nil }
        let payload = document.doc?.textPayload(document.activeLayerIndex)
        switch item.action {
        case #selector(placeTextOnPath(_:)):
            // A warped layer comes off its warp first (designs/warp-text.md, WT-1).
            guard !document.quickMaskActive, let payload, !TextOnPath.hasLineBreak(payload.string),
                  payload.warp == nil
            else { return false }
            return currentPathStep().map { !$0.shape.isEmpty } ?? false
        case #selector(removeTextFromPath(_:)):
            return payload?.onPath != nil
        default:
            return nil
        }
    }
}
