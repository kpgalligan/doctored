import AppKit

/// The per-entry property tools: set_layer_properties, reorder_layer, the
/// layer-mask family and set_layer_clipped.
///
/// They live here rather than in the frozen `AgentServer.swift` because layer
/// groups turned every one of them into structure work — each now looks its
/// target up with `structuralLayerIndex` (a GROUP is renamable, hideable,
/// maskable and clippable), asks `doc_lock` before it edits, and explains the
/// refusals a group or a lock produces instead of falling through to the
/// generic "check the parameters". The dispatch entries are this file's
/// `layerPropertiesHandlers` table.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let layerPropertiesHandlers: [String: ToolHandler] = [
        "reorder_layer": reorderLayer,
        "set_layer_properties": setLayerProperties,
        // Layer masks and clipping
        "add_layer_mask": addLayerMask,
        "remove_layer_mask": removeLayerMask,
        "set_layer_mask_enabled": setLayerMaskEnabled,
        "set_layer_clipped": setLayerClipped,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let layerPropertiesToolFacts: [String: ToolFacts] = [
        // Phase 12E-1: the four pure STRUCTURE tools whose typed twins the
        // core now owns (`precision_structure_ops`). `group_layers` was
        // already here, restricted to "every top-level layer"; that
        // restriction is gone — a subset sharing one parent is what the core
        // takes at either depth.
        // Phase 12E-1: the pure STRUCTURE edits move no pixels — the core's
        // typed ops splice records and keep every plane pointer-equal — so a
        // float document answers them exactly as it answers `group_layers`.
        "reorder_layer": [.typedTool, .floatTool, .cmykTool],
        "set_layer_properties": [.typedTool, .floatTool, .cmykTool],
        "add_layer_mask": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
        "remove_layer_mask": [.typedTool, .floatTool, .cmykTool],
        "set_layer_mask_enabled": [.typedTool, .floatTool, .cmykTool],
        // #1 phase 4a: clipping masks on a CMYK document.
        "set_layer_clipped": [.typedTool, .floatTool, .cmykTool],
    ]

    // MARK: - Order and properties

    func reorderLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let from = intArg(a, "from"), let to = intArg(a, "to") else {
            throw ToolError(message: "reorder_layer requires from and to")
        }
        // Validated here rather than left to the core's nil: an out-of-range
        // index used to come back as the generic "check the parameters"
        // while every other layer tool named the range.
        let count = document.doc?.layerCount ?? 0
        for (key, value) in [("from", from), ("to", to)] where value < 0 || value >= count {
            throw ToolError(
                message: "\(key) \(value) is out of range (0..\(count - 1))")
        }
        // Omitted depth means the destination row's own level, which is what
        // a drag onto a row means and what this tool has always done.
        let depth = intArg(a, "depth") ?? document.doc?.layerDepth(to) ?? 0
        guard
            try editStructure(document, "Reorder Layer", {
                try $0.moveLayerTo(from: from, to: to, depth: depth)
            })
        else {
            throw ToolError(
                message: "Reorder Layer moved nothing. `to` numbers the stack with layer "
                    + "\(from)'s own subtree ALREADY taken out, so a `to` equal to that "
                    + "subtree's start at the same depth puts the layer back where it "
                    + "started — get_document reports each layer's depth and parent. Any "
                    + "other refusal means the (to, depth) pair would not leave a "
                    + "well-formed tree: a layer at depth d needs an enclosing group, and "
                    + "the topmost entry has to sit at depth 0.")
        }
        return try jsonResult(["ok": true, "document": summary(document)])
    }

    func setLayerProperties(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        // Every property here means the same thing on a GROUP as on a layer.
        let index = try structuralLayerIndex(a, document)
        // The ONE call site that accepts "Pass Through": it is a GROUP's
        // default mode, so a group whose blend mode was changed has to be
        // able to get back to it. brush_stroke and clone_stamp keep the
        // narrower vocabulary (AgentServer+Retouch.blendModeArg).
        let blendMode = try blendModeArg(a, allowPassThrough: true)
        let name = stringArg(a, "name")
        let opacity = doubleArg(a, "opacity")
        let visible = boolArg(a, "visible")
        let offsetX = intArg(a, "offset_x")
        let offsetY = intArg(a, "offset_y")
        let open = boolArg(a, "open")
        guard name != nil || opacity != nil || visible != nil || blendMode != nil
            || offsetX != nil || offsetY != nil || open != nil
        else {
            throw ToolError(message: "set_layer_properties: nothing to change")
        }
        if open != nil, document.doc?.layerIsGroup(index) != true {
            throw ToolError(
                message: "open applies to groups only — layer \(index) is a raster layer.")
        }
        // Named here rather than left to the core's NULL: without it the
        // refusal reads "Layer Properties failed — check the parameters",
        // which sends a model looking for the wrong problem.
        if blendMode == DR_BLEND_PASS_THROUGH, document.doc?.layerIsGroup(index) != true {
            throw ToolError(
                message: "Pass Through applies to groups only — layer \(index) is a raster "
                    + "layer. It means \"do not composite this level as a unit\", which has "
                    + "no meaning outside a group; use Normal for a layer.")
        }
        // An offset needs a position to move, at both depths. Only a
        // coordinate that DIFFERS from the reported one is refused, so the
        // read-modify-write an agent performs after get_document, echoing the
        // offset it read, stays a no-op.
        if offsetX != nil || offsetY != nil, let info = document.doc?.layerInfo(index),
           offsetX.map({ $0 != info.offsetX }) == true || offsetY.map({ $0 != info.offsetY }) == true {
            if document.doc?.layerIsAdjustment(index) == true {
                throw ToolError(
                    message: "Layer \(index) is an adjustment layer: it has no position of its own, "
                        + "so offset_x/offset_y cannot move it. move_layers moves its linked mask.")
            }
            if info.isGroup, info.width == 0, info.height == 0 {
                throw ToolError(
                    message: "Group \(index) contains no pixels, so it has no position for "
                        + "offset_x/offset_y to move.")
            }
        }
        // An offset is a MOVE, so it answers to the position lock — over the
        // SUBTREE, because writing a group's offset shifts every raster
        // descendant and the core's `lock_block` ORs their bits in
        // (AgentServer+Groups.rejectLockedOffset).
        if offsetX != nil || offsetY != nil {
            try rejectLockedOffset(document, index)
        }
        // A per-field no-op falls THROUGH instead of nilling the chain. Every
        // setter here answers NULL when the value it is handed is already the
        // stored one (the core's purity rule), so chaining them made ONE such
        // field discard every other property in the call — the read-modify-
        // write an agent performs after get_document, reported as "check the
        // parameters". `ImageDocument.applyToSelectedLayers` reads a nil the
        // same way, and a call in which nothing at all changed answers
        // `changed: false` below rather than an error.
        if document.doc?.isHighDepth == true {
            for (key, parsed) in [("name", name != nil), ("blend_mode", blendMode != nil), ("opacity", opacity != nil),
                ("visible", visible != nil), ("offset_x", offsetX != nil),
                ("offset_y", offsetY != nil), ("open", open != nil)] where a[key] != nil && !parsed {
                throw ToolError(message: "Invalid native layer property: \(key).")
            }
            return try nativeLayerProperties(document, index: index, name: name, opacity: opacity,
                mode: blendMode, visible: visible, x: offsetX, y: offsetY, open: open)
        }
        // `open` is not part of the edit below: a group's disclosure is the
        // panel triangle's no-undo change, applied after it through
        // `applyGroupDisclosure`. So `name` alone is the render-neutral case.
        var changed = false
        var edited = false
        if name != nil || opacity != nil || visible != nil || blendMode != nil
            || offsetX != nil || offsetY != nil {
            // Only the name changed: no rendered pixel moves, so the current
            // projection is carried, as the typed branch and the
            // panel's inline rename do.
            let bookkeepingOnly = opacity == nil && blendMode == nil && visible == nil
                && offsetX == nil && offsetY == nil
            edited = try editStructure(document, "Layer Properties", renderNeutral: bookkeepingOnly, { doc in
                var updated = doc
                func step(_ next: RasterDocument?) {
                    guard let next = next else { return }
                    updated = next
                    changed = true
                }
                if let name = name { step(try updated.withLayerName(index, name)) }
                if let opacity = opacity {
                    step(try updated.withLayerOpacity(index, min(max(opacity, 0), 1)))
                }
                if let mode = blendMode { step(try updated.withLayerBlendMode(index, mode)) }
                if let visible = visible { step(try updated.withLayerVisible(index, visible)) }
                if offsetX != nil || offsetY != nil {
                    let info = doc.layerInfo(index)
                    step(
                        try updated.withLayerOffset(
                            index, offsetX ?? info?.offsetX ?? 0, offsetY ?? info?.offsetY ?? 0))
                }
                return changed ? updated : nil
            })
        }
        var disclosed = false
        if let open = open {
            disclosed = try applyGroupDisclosure(document, index: index, open: open, edited: edited)
        }
        guard edited || disclosed else {
            return try structureNoOp(
                ["layer": index],
                why: "layer \(index) already carries every property this call named.")
        }
        return try jsonResult(["ok": true, "document": summary(document)])
    }

    /// A group's disclosure, the panel triangle's path
    /// (`ImageDocument.applyGroupExpanded`): saved with the document,
    /// so it dirties it, but it changes no pixel and adds no undo step. With
    /// other properties in the call, those were one undo step already, and
    /// this folds the disclosure into that step's current state, so undoing
    /// it restores the disclosure too. `recordAgentCall` records the call, so
    /// the document records nothing here. False when it changed nothing.
    func applyGroupDisclosure(_ document: ImageDocument, index: Int, open: Bool,
                              edited: Bool) throws -> Bool {
        do {
            return try document.applyGroupExpanded(index, open, record: .notACommand)
        } catch {
            let sentence = (error as? ToolError)?.message ?? error.localizedDescription
            throw ToolError(message: edited
                ? "The call's other properties were applied, but the disclosure was refused: " + sentence
                : sentence)
        }
    }

    // MARK: - Layer masks

    /// The mask-owning layer a call targets, verified to actually have a
    /// mask so the failure names the fix instead of a generic edit error.
    /// Internal, not private: AgentServer+ChannelTargets routes a mask
    /// target through it.
    func maskedLayerIndex(
        _ a: [String: Any], _ document: ImageDocument, _ what: String
    ) throws -> Int {
        // A GROUP takes a mask too (a canvas-sized one), so this is the
        // structural lookup, not the pixel one.
        let index = try structuralLayerIndex(a, document)
        guard document.doc?.layerHasMask(index) == true else {
            throw ToolError(
                message: "Layer \(index) has no mask to \(what). Add one with add_layer_mask.")
        }
        // A VECTOR mask with no raster mask beside it: the layer mask
        // tools edit the raster half, and there is none. Applying is the one
        // that takes the whole baked mask, shape included.
        if what != "apply", let vector = document.doc?.vectorMask(index), !vector.hasRaster {
            throw ToolError(
                message: "Layer \(index)'s mask is a vector mask with no raster mask beside "
                    + "it, so there is no layer mask to \(what). Use set_vector_mask, "
                    + "set_vector_mask_properties or remove_vector_mask for the vector mask, "
                    + "or add_layer_mask to give the layer a raster mask as well.")
        }
        return index
    }

    func addLayerMask(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        // A GROUP takes a mask (a canvas-sized one), so this is one of the
        // three tools here that must NOT take the paint helper's refusal.
        let index = try structuralLayerIndex(a, document)
        // A MASK edit, which only Lock All forbids (doc_lock.rs) — named here
        // so the refusal is not the generic "Add Layer Mask failed".
        try rejectLockedEdit(document, index, DR_EDIT_MASK)
        let kindName = stringArg(a, "kind") ?? "reveal_all"
        let kind: DrMaskKind
        var selection: CanvasSelection? = nil
        switch kindName {
        case "reveal_all": kind = DR_MASK_REVEAL_ALL
        case "hide_all": kind = DR_MASK_HIDE_ALL
        case "from_selection":
            // The window's live selection — the same one the marquee shows —
            // as a canvas-sized linked mask at (0, 0), never cropped.
            guard let current = document.canvasSelection else {
                throw ToolError(
                    message: "kind \"from_selection\" needs an active selection, and there is "
                        + "none. Make one with a select_* tool first, or use kind "
                        + "\"reveal_all\" / \"hide_all\".")
            }
            kind = DR_MASK_FROM_SELECTION
            selection = current
        case let other:
            throw ToolError(
                message: "kind must be reveal_all, hide_all, or from_selection (got \"\(other)\")")
        }
        let reply: [String: Any] = ["layer": index, "kind": kindName, "mask_enabled": true]
        if let selection, let doc = document.doc,
           doc.hasLayerMask(index, fromCoverage: selection.maskBytes()) {
            return try structureNoOp(
                reply, why: "layer \(index) already has exactly this selection's mask.")
        }
        try performGroupedEdit(document, "Add Layer Mask") {
            if let selection { return $0.addingLayerMask(index, fromSelection: selection) }
            return $0.addingLayerMask(index, kind: kind)
        }
        return try jsonResult(reply.merging(["ok": true]) { _, new in new })
    }

    func removeLayerMask(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let apply = boolArg(a, "apply") ?? false
        let index = try maskedLayerIndex(a, document, apply ? "apply" : "remove")
        // A MASK edit under Lock All — including with apply, which does write
        // pixels: applying a mask is a mask operation (doc_lock.rs).
        // Applying a mask bakes its coverage into the layer's ALPHA, which is
        // its own edit kind: Lock Transparency refuses it, while deleting the
        // mask (which touches no alpha) stays a plain mask edit.
        try rejectLockedEdit(document, index, apply ? DR_EDIT_MASK_APPLY : DR_EDIT_MASK)
        // Applying bakes the coverage into the layer's own pixels, and a
        // group has none — the core answers NULL for it. Named here, like
        // every other group refusal: a permanent property of the target read
        // back as "check the parameters" sends a model round the same call
        // again with the same arguments.
        if apply, document.doc?.layerIsGroup(index) == true {
            throw ToolError(
                message: "Layer \(index) is a group: applying a mask bakes its coverage into "
                    + "a layer's pixels, and a group has none of its own. Delete the group's "
                    + "mask with apply: false, or apply the mask on the layers inside it.")
        }
        try performGroupedEdit(document, apply ? "Apply Layer Mask" : "Delete Layer Mask") {
            try $0.removingLayerMask(index, apply: apply)
        }
        return try jsonResult(["ok": true, "layer": index, "applied": apply])
    }

    func setLayerMaskEnabled(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let enabled = boolArg(a, "enabled") else {
            throw ToolError(message: "set_layer_mask_enabled requires enabled (true or false)")
        }
        let index = try maskedLayerIndex(a, document, enabled ? "enable" : "disable")
        try rejectLockedEdit(document, index, DR_EDIT_MASK)
        try performGroupedEdit(document, enabled ? "Enable Layer Mask" : "Disable Layer Mask") {
            try $0.withLayerMaskEnabled(index, enabled)
        }
        return try jsonResult(["ok": true, "layer": index, "mask_enabled": enabled])
    }

    // MARK: - Clipping masks

    /// set_layer_clipped: the agent mirror of Layer > Create/Release
    /// Clipping Mask, named the same way so the undo menu reads identically.
    /// The core accepts a clipped BOTTOM layer (it composites as unclipped —
    /// there is nothing below to clip to), so that call succeeds with a
    /// note instead of erroring: the flag is real and matters the moment a
    /// layer is reordered beneath it.
    func setLayerClipped(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        // A group may be clipped, and may be a clip base. Phase 12B: a
        // `layers` SET is what the menu item does over a multi-selection, as
        // ONE undo step.
        let indices = try structuralIndices(a, document)
        guard let clipped = boolArg(a, "clipped") else {
            throw ToolError(message: "set_layer_clipped requires clipped (true or false)")
        }
        let name = clipped ? "Create Clipping Mask" : "Release Clipping Mask"
        // Folded exactly as `set_layer_lock` and the menu twin
        // `ImageDocument.applyToSelectedLayers`: an entry the document will
        // not change (at 16 bits, one already in that state) is SKIPPED and
        // listed, the entries that do change are applied as one undo step,
        // and only a fold where NOTHING changed is refused — naming the
        // entries, because then there is nothing else to report.
        var skipped: [Int] = []
        guard try editStructure(document, name, { start in
            skipped = []
            var current = start
            var changed = false
            for index in indices {
                guard let next = try current.withLayerClipped(index, clipped: clipped) else {
                    skipped.append(index)
                    continue
                }
                current = next
                changed = true
            }
            return changed ? current : nil
        }) else {
            let which = skipped.map(String.init).joined(separator: ", ")
            throw ToolError(
                message: "\(name) failed for layer\(skipped.count == 1 ? "" : "s") \(which) "
                    + "— the document refused the change.")
        }
        var result: [String: Any] = ["ok": true, "clipped": clipped, "layers": indices]
        if !skipped.isEmpty { result["skipped"] = skipped }
        if indices.count == 1, let index = indices.first { result["layer"] = index }
        // Named per entry: a set where only ONE member is baseless has to say
        // which, or the note reads as though the whole call did nothing.
        if clipped, let doc = document.doc {
            let baseless = indices.filter { Self.isBaseless(doc, $0) }
            if !baseless.isEmpty {
                let which = baseless.map(String.init).joined(separator: ", ")
                result["note"] =
                    "Layer \(which) \(baseless.count == 1 ? "is" : "are") the bottom of "
                    + "\(baseless.count == 1 ? "its" : "their") own level — clipping is "
                    + "re-derived WITHIN a level, and there is no unclipped sibling beneath "
                    + "\(baseless.count == 1 ? "it" : "them") there to clip to — so "
                    + "\(baseless.count == 1 ? "it composites" : "they composite") as if "
                    + "unclipped (a group of its own keeps passing through). The flag is "
                    + "stored and takes effect the moment an unclipped sibling sits below."
            }
        }
        return try jsonResult(result)
    }

    /// True when entry `index`'s clipped flag has nothing to act on: every
    /// sibling BELOW it in its own level is clipped too, so the compositor
    /// runs out of level before it finds a base and composites the entry as
    /// if unclipped (`doc_group::composite_level_into`). The bottom layer of
    /// a flat document is the familiar case; a group's bottom-most child is
    /// the one groups added, and an arrange or a panel drag reaches it
    /// without anyone touching the flag.
    private static func isBaseless(_ doc: RasterDocument, _ index: Int) -> Bool {
        doc.layerTree.siblings(of: index)
            .prefix { $0 != index }
            .allSatisfy { doc.layerClipped($0) }
    }
}
