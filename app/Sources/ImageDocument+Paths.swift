import AppKit

// Paths: the document's one path edit and the read behind the Pen and the Paths panel.
// A path changes no pixel, so its edit is `applyGuideEdit`'s — undo, dirty and notify without a
// reprojection, recorded render-neutral — and nothing here registers undo or counts a change
// itself. A VECTOR MASK's shape does change the picture, so the Pen's commit on one is `applyEdit`.

extension ImageDocument {
    /// One edit of the path list as ONE undo step named `name`: `edit` is
    /// handed the current list and answers the next one, or nil for nothing
    /// to do. The core refuses a list that breaks a rule or a cap with its own
    /// sentence, which is presented; nil and an unchanged list beep, as every
    /// refused edit does. `record` is the Action step the edit is (the Pen and
    /// the panel pass theirs; the agent's dispatcher records the call itself
    /// and passes `.notACommand`).
    ///
    /// Off the event path (an agent call) the caller wraps this in
    /// `withUndoGroupDiscardingEmpty`, as `AgentServer+Paths.editPaths` does.
    func applyPathEdit(
        _ name: String, record: [ActionStep], _ edit: (PathSet) -> PathSet?
    ) {
        applyGuideEdit(name, record: record) { doc in
            guard let next = edit(doc.paths) else { return nil }
            return try doc.withPaths(next)
        }
    }

    /// One Pen gesture on a path of the list, as ONE undo step: `shape`
    /// becomes the whole shape of the current path, or — with no current
    /// path, or one that has been deleted or undone away — of the work path,
    /// which is created or replaced and made current. The recorded step is
    /// the `set_path` call that writes the same shape (`path` names a saved
    /// path; the work path is that tool's default).
    ///
    /// False when nothing was committed. The core's refusal (the document's
    /// anchor cap, a coordinate past the wall) BEEPS here instead of
    /// presenting its sentence: it is asked before `applyPathEdit` is, so a
    /// refused anchor does not stop the drawing behind a sheet. A shape the
    /// path already has is no gesture and is silent.
    func commitPenPath(_ shape: PathShape, name: String) -> Bool {
        guard let before = doc else {
            NSSound.beep()
            return false
        }
        let set = before.paths
        var next = set
        var arguments: [String: Any] = [
            "subpaths": shape.subpathsJSON, "fill_rule": shape.fillRule.rawValue,
        ]
        var actionName = name
        var index: Int?
        if case .path(let id)? = currentPath, id != 0 {
            index = set.entries.firstIndex { $0.id == id }
        }
        if let index {
            next.entries[index].shape = shape
            if set.entries[index].kind == .saved { arguments["path"] = set.entries[index].name }
        } else {
            next = set.settingWorkPath(shape)
            actionName = "New Work Path"
        }
        do {
            guard try before.withPaths(next) != nil else { return false }
        } catch {
            NSSound.beep()
            return false
        }
        applyPathEdit(actionName, record: .step("set_path", arguments, note: "Pen tool")) { _ in
            next
        }
        // Unchanged here means a live edit was in flight; `applyGuideEdit`
        // has already beeped.
        guard let after = doc, after !== before else { return false }
        if index == nil, let work = after.paths.workPath { currentPath = .path(work.id) }
        return true
    }

    /// One Pen gesture on a layer's VECTOR MASK, as ONE undo step: `shape`
    /// becomes the mask's whole shape and the layer's mask is baked again.
    /// Unlike a path edit this changes what composites, so it is `applyEdit`,
    /// not `applyGuideEdit`. The recorded step is the `set_vector_mask` call
    /// that writes the same shape.
    ///
    /// False when nothing was committed: the layer or its vector mask is
    /// gone, the mask is locked, or the core refused the shape (the anchor
    /// cap) — each a beep, asked before `applyEdit` is so a refused anchor
    /// does not stop the drawing behind a sheet, as `commitPenPath` does.
    func commitPenVectorMask(_ shape: PathShape, layerID: UInt64, name: String) -> Bool {
        guard let before = doc, let index = before.layerIndex(id: layerID),
              before.vectorMask(index) != nil,
              let next = try? before.settingVectorMask(index, shape: shape, control: nil)
        else {
            NSSound.beep()
            return false
        }
        let arguments: [String: Any] = [
            "layer": ActionArgs.layer(before.layerRef(index)),
            "subpaths": shape.subpathsJSON, "fill_rule": shape.fillRule.rawValue,
        ]
        applyEdit(name, record: .step("set_vector_mask", arguments, note: "Pen tool")) { _ in
            next
        }
        return doc === next
    }

    /// The shape `target` names in the current document, or nil when there is
    /// none — no target, a path that has been deleted or undone away, a
    /// layer that is gone or has no vector mask any more.
    func resolvedPath(_ target: PathTarget?) -> PathShape? {
        guard let target, let doc else { return nil }
        switch target {
        case .path(let id): return doc.paths.entry(id: id)?.shape
        case .vectorMask(let layerID):
            return doc.layerIndex(id: layerID).flatMap { doc.vectorMask($0) }?.shape
        }
    }
}
