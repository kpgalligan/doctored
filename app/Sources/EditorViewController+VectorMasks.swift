import AppKit

/// Layer ▸ Vector Mask: Current Path, Reveal All, Delete, Enable and Link, on the active
/// layer. The MCP twins are `set_vector_mask`, `remove_vector_mask` and
/// `set_vector_mask_properties` (`AgentServer+Paths.swift`), and each command records its twin.
///
/// A vector mask is a layer mask whose coverage is a path's fill, kept as a shape: the core bakes
/// it, together with the layer's own raster mask when there is one, into the layer's ordinary
/// mask (`core/src/doc_vector_mask.rs`). So every command here changes what composites and goes
/// through `applyEdit`, unlike a path edit. The SHAPE is edited with the Pen: selecting the
/// vector-mask row in the Paths panel makes it the current path
/// (`ImageDocument.commitPenVectorMask`).
extension EditorViewController {
    /// The active layer and its vector mask, if it has one.
    private func activeVectorMask() -> (index: Int, mask: VectorMaskInfo?)? {
        guard let document, let doc = document.doc else { return nil }
        let index = document.activeLayerIndex
        guard index >= 0, index < doc.layerCount else { return nil }
        return (index, doc.vectorMask(index))
    }

    /// The current path target for layer `index`'s vector mask.
    private func vectorMaskTarget(_ index: Int) -> PathTarget? {
        document?.doc?.layerInfo(index).map { .vectorMask(layerID: $0.id) }
    }

    /// Layer ▸ Vector Mask ▸ Current Path, and the Paths panel's Add Vector
    /// Mask: the current path's shape becomes the active layer's vector mask
    /// (replacing the shape of one it already has). The mask keeps its own
    /// copy and becomes the current path, so the Pen goes on editing the mask.
    @objc func addVectorMaskFromCurrentPath(_ sender: Any?) {
        guard let document, let active = activeVectorMask(),
              case .path? = document.currentPath, let current = currentPathStep(),
              !current.shape.isEmpty
        else {
            NSSound.beep()
            return
        }
        guard !refuseLockedEdit(layer: active.index, kind: DR_EDIT_MASK) else { return }
        var arguments = current.arguments
        arguments["layer"] = ActionArgs.activeLayer
        let before = document.doc
        document.applyEdit(
            active.mask == nil ? "Add Vector Mask" : "Edit Vector Mask",
            record: .step("set_vector_mask", arguments, note: "Layer ▸ Vector Mask ▸ Current Path")
        ) { try $0.settingVectorMask(active.index, shape: current.shape, control: nil) }
        if document.doc !== before { setCurrentPath(vectorMaskTarget(active.index)) }
    }

    /// Layer ▸ Vector Mask ▸ Reveal All: an empty vector mask on the active
    /// layer, which hides nothing until the Pen draws in it. It becomes the
    /// current path.
    @objc func addVectorMaskRevealAll(_ sender: Any?) {
        guard let document, let active = activeVectorMask(), active.mask == nil else {
            NSSound.beep()
            return
        }
        guard !refuseLockedEdit(layer: active.index, kind: DR_EDIT_MASK) else { return }
        let before = document.doc
        document.applyEdit(
            "Add Vector Mask",
            record: .step(
                "set_vector_mask", ["layer": ActionArgs.activeLayer, "reveal_all": true],
                note: "Layer ▸ Vector Mask ▸ Reveal All")
        ) { try $0.settingVectorMask(active.index, shape: PathShape(), control: nil) }
        if document.doc !== before { setCurrentPath(vectorMaskTarget(active.index)) }
    }

    /// Layer ▸ Vector Mask ▸ Delete: the raster mask kept beside the shape,
    /// if any, is the layer's mask again.
    @objc func deleteVectorMask(_ sender: Any?) {
        guard let document, let active = activeVectorMask(), active.mask != nil else {
            NSSound.beep()
            return
        }
        guard !refuseLockedEdit(layer: active.index, kind: DR_EDIT_MASK) else { return }
        let wasCurrent = document.currentPath == vectorMaskTarget(active.index)
        let before = document.doc
        document.applyEdit(
            "Delete Vector Mask",
            record: .step(
                "remove_vector_mask", ["layer": ActionArgs.activeLayer],
                note: "Layer ▸ Vector Mask ▸ Delete")
        ) { try $0.removingVectorMask(active.index) }
        if document.doc !== before, wasCurrent { setCurrentPath(nil) }
    }

    /// Layer ▸ Vector Mask ▸ Enable (a toggle).
    @objc func toggleVectorMaskEnabled(_ sender: Any?) {
        setVectorMaskFlags { ($0.enabled ? false : true, $0.linked, "enabled", !$0.enabled) }
    }

    /// Layer ▸ Vector Mask ▸ Link (a toggle): whether the mask moves and
    /// transforms with the layer.
    @objc func toggleVectorMaskLinked(_ sender: Any?) {
        setVectorMaskFlags { ($0.enabled, $0.linked ? false : true, "linked", !$0.linked) }
    }

    /// One flag of the active layer's vector mask, as one undo step recording
    /// `set_vector_mask_properties` with the one key that changed.
    private func setVectorMaskFlags(
        _ change: (VectorMaskInfo) -> (enabled: Bool, linked: Bool, key: String, value: Bool)
    ) {
        guard let document, let active = activeVectorMask(), let mask = active.mask else {
            NSSound.beep()
            return
        }
        guard !refuseLockedEdit(layer: active.index, kind: DR_EDIT_MASK) else { return }
        let next = change(mask)
        document.applyEdit(
            "Vector Mask Properties",
            record: .step(
                "set_vector_mask_properties",
                ["layer": ActionArgs.activeLayer, next.key: next.value],
                note: "Layer ▸ Vector Mask ▸ \(next.key == "enabled" ? "Enable" : "Link")")
        ) {
            try $0.settingVectorMaskProperties(
                active.index, enabled: next.enabled, linked: next.linked, control: nil)
        }
    }

    /// Enablement and check marks for Layer ▸ Vector Mask; nil for anybody
    /// else's item. Reached through `validatePathsItem`.
    func validateVectorMaskItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        let menuItem = item as? NSMenuItem
        switch item.action {
        case #selector(addVectorMaskFromCurrentPath(_:)):
            guard activeVectorMask() != nil, case .path? = document?.currentPath else {
                return false
            }
            return currentPathStep().map { !$0.shape.isEmpty } ?? false
        case #selector(addVectorMaskRevealAll(_:)):
            return activeVectorMask().map { $0.mask == nil } ?? false
        case #selector(deleteVectorMask(_:)):
            return activeVectorMask()?.mask != nil
        case #selector(toggleVectorMaskEnabled(_:)):
            let mask = activeVectorMask()?.mask
            menuItem?.state = mask?.enabled == true ? .on : .off
            return mask != nil
        case #selector(toggleVectorMaskLinked(_:)):
            let mask = activeVectorMask()?.mask
            menuItem?.state = mask?.linked == true ? .on : .off
            return mask != nil
        default:
            return nil
        }
    }
}
