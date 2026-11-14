import AppKit

// Every commit entry point takes a `record:` parameter, so a path that records nothing has to say
// so and the gaps are greppable.

/// THE RECORDED VOCABULARY: one factory per user command that has an MCP
/// twin, plus the two "nothing to record" spellings.
///
/// Every commit entry point on `ImageDocument` takes a `record: [ActionStep]`
/// parameter, so every one of the app's ~100 edit sites must name something
/// from this file or say explicitly that it records nothing. That is the
/// whole design: the step is a PARAMETER, not a global slot armed before the
/// call, so (1) a new feature's edit path does not compile until it states a
/// step, (2) a path that refuses AFTER arming — `performLayerEdit` has three
/// such guards — carries its step away with it instead of leaving a stale
/// one for the next unrelated edit to consume, and (3) the gaps are
/// greppable in a diff rather than discovered at runtime.
///
/// Two invariants, both consequences of measured differences between the UI
/// and the agent paths:
///
/// - **A recorded step is fully explicit.** Never rely on a catalog default;
///   they deliberately differ from the UI's (menu Sharpen is amount 1.5, the
///   catalog default 1.0; the blur sheet starts at sigma 2, the catalog at
///   4). A recorded UI Sharpen therefore emits `amount: 1.5`.
/// - **The ambient target is serialised.** Any filter, fill, gradient or
///   stroke step writes `target` explicitly from
///   `ImageDocument.planeEditTarget`, spelled the way `AgentServer.paintTarget`
///   parses it, because the UI reads that ambient value where the agent
///   reads an argument.
///
/// The file is meant to be reviewed against the catalog's argument table in
/// one pass — every factory names its tool, and `ActionStep.init` asserts in
/// debug builds that the name is a live dispatch key.
extension Array where Element == ActionStep {
    /// NOT a user command: an internal re-entry, a helper, an undo/redo
    /// restore, or the agent's own path (which records once, centrally, in
    /// `AgentServer.execute`).
    static var notACommand: [ActionStep] { [] }

    /// A user command with NO MCP twin. Appended as a visible placeholder so
    /// the gap lands in the action — named, listed in the Actions window and
    /// refused at replay — instead of being a silent hole in a recording
    /// that otherwise looks complete.
    static func unrecorded(_ name: String) -> [ActionStep] {
        [ActionStep(tool: ActionStep.unrecordedTool, arguments: ["action": name])]
    }

    /// One step, spelled out. The general form behind every factory below;
    /// used directly only where a command's arguments are too one-off to
    /// deserve a name of their own.
    static func step(_ tool: String, _ arguments: [String: Any] = [:], note: String? = nil)
        -> [ActionStep]
    {
        [ActionStep(tool: tool, arguments: arguments, note: note)]
    }

    // MARK: - Whole-document geometry

    /// Image ▸ Rotate. `degrees` is one of 90/180/270/−90.
    static func rotate(_ degrees: Int) -> [ActionStep] {
        step("rotate", ["degrees": degrees], note: "Image ▸ Rotate")
    }

    /// Image ▸ Rotate Arbitrary… — a FILTERED whole-canvas rotation, which
    /// expands the canvas and resamples (Phase 12F-1b3).
    ///
    /// `degrees` is a JSON NUMBER, not an Int: the whole point of the op is
    /// an angle the four quarter turns cannot express, and a step that
    /// rounded 12.5 to 12 would replay a picture the user never saw.
    /// `sampler` is recorded for `crop`'s reason — the rotation resamples
    /// with the sampler the sheet chose, and without it a replay of a Nearest
    /// rotation comes back smoothed with nothing in the step saying so.
    ///
    /// An EXACT quarter turn never reaches here: the sheet and the MCP
    /// handler both delegate one to today's `applyingDocumentGeometry` path,
    /// which records the INTEGER `rotate` step above.
    static func rotateCanvas(degrees: Double, sampler: String) -> [ActionStep] {
        step(
            "rotate", ["degrees": ActionArgs.number(degrees), "sampler": sampler],
            note: "Image ▸ Rotate Arbitrary…")
    }

    /// Image ▸ Flip Canvas.
    static func flip(_ axis: String) -> [ActionStep] {
        step("flip", ["axis": axis], note: "Image ▸ Flip Canvas")
    }

    /// Image ▸ Crop, and the Crop tool's commit. ABSOLUTE canvas pixels: a
    /// crop is not remapped onto a differently sized document, and
    /// `Action.recordedCanvas` is what lets the player SAY the sizes differ.
    ///
    /// `sampler` is recorded whenever the crop STRAIGHTENS, because that is
    /// the one thing about a crop that resamples: the tool straightens with
    /// the editor's shared Free Transform sampler, so an action recorded
    /// with Nearest selected has to replay through Nearest or it reproduces
    /// smoothed pixels the user never saw. A plain rectangle crop moves the
    /// canvas window and samples nothing, so it records no sampler at all.
    /// `backgroundColor` is a window past the canvas's Background fill,
    /// recorded only for such a window, so every other step keeps its shape.
    /// `fill` is recorded only for Content-Aware Fill; a crop
    /// with Generative Expand is never recorded (it would bill the key again).
    static func crop(_ rect: CGRect, angle: Double = 0, sampler: String? = nil,
                     deleteCropped: Bool = false, backgroundColor: NSColor? = nil,
                     fill: CropFill? = nil) -> [ActionStep]
    {
        var arguments: [String: Any] = [
            "x": Int(rect.minX.rounded()), "y": Int(rect.minY.rounded()),
            "width": Int(rect.width.rounded()), "height": Int(rect.height.rounded()),
        ]
        if angle != 0 {
            arguments["angle"] = ActionArgs.number(angle)
            if let sampler = sampler { arguments["sampler"] = sampler }
        }
        // Only when on, so a step recorded without the option keeps its shape.
        if deleteCropped { arguments["delete_cropped"] = true }
        if let backgroundColor { arguments["background_color"] = ActionArgs.hex(backgroundColor) }
        if fill == .contentAware { arguments["fill"] = CropFill.contentAware.rawValue }
        return step("crop", arguments, note: "Image ▸ Crop")
    }

    /// Image ▸ Image Size…
    static func imageSize(width: Int, height: Int, filter: String) -> [ActionStep] {
        step(
            "image_size", ["width": width, "height": height, "filter": filter],
            note: "Image ▸ Image Size…")
    }

    /// Image ▸ Canvas Size…, from the anchor grid's own cell.
    ///
    /// Row 0 is the TOP row — the sheet's `row / 2` fraction puts none of a
    /// height increase above the old origin there, which is what "top"
    /// means — and the mapping lives here rather than in the sheet so the
    /// catalog's vocabulary stays in the vocabulary file.
    static func canvasSize(
        width: Int, height: Int, anchor: (col: Int, row: Int)
    ) -> [ActionStep] {
        let rows = ["top", "", "bottom"]
        let cols = ["left", "", "right"]
        let row = rows[Swift.min(Swift.max(anchor.row, 0), 2)]
        let col = cols[Swift.min(Swift.max(anchor.col, 0), 2)]
        let name: String
        switch (row.isEmpty, col.isEmpty) {
        case (true, true): name = "center"
        case (true, false): name = col
        case (false, true): name = row
        case (false, false): name = "\(row)-\(col)"
        }
        return step(
            "canvas_size", ["width": width, "height": height, "anchor": name],
            note: "Image ▸ Canvas Size…")
    }

    /// Layer ▸ Flatten Image.
    static var flattenImage: [ActionStep] {
        step("flatten_image", note: "Layer ▸ Flatten Image")
    }

    // MARK: - Layers

    /// Layer ▸ New ▸ Layer.
    /// `position` is recorded only when it is not the default `above` (the
    /// UI's New Layer lands inside an active group).
    static func newLayer(name: String, position: LayerInsertPosition = .above) -> [ActionStep] {
        var arguments: [String: Any] = ["name": name]
        if position != .above { arguments["position"] = position.rawValue }
        return step("new_layer", arguments, note: "Layer ▸ New ▸ Layer")
    }

    /// Layer ▸ Duplicate / Delete / Merge over the panel's whole selection.
    /// The set is symbolic — `{"$layers": "selected"}` — so a replay acts on
    /// whatever the target document has selected.
    static func selectedLayersCommand(_ tool: String, note: String) -> [ActionStep] {
        step(tool, ["layers": ActionArgs.selectedLayers], note: note)
    }

    /// Layer ▸ Duplicate to New Document…. The set is symbolic, so a
    /// replay copies whatever the run target has selected; `name` is written
    /// only when the sheet named the new document.
    static func duplicateToNewDocument(name: String?) -> [ActionStep] {
        var arguments: [String: Any] = ["layers": ActionArgs.selectedLayers]
        if let name, !name.isEmpty { arguments["name"] = name }
        return step("duplicate_to_new_document", arguments,
                    note: "Layer ▸ Duplicate to Document…")
    }

    /// Layer ▸ Merge Down, the single-layer form.
    static var mergeDown: [ActionStep] {
        step("merge_down", ["index": ActionArgs.activeLayer], note: "Layer ▸ Merge Down")
    }

    /// Layer ▸ Merge Visible / Stamp Visible.
    static var mergeVisible: [ActionStep] {
        step("merge_visible", note: "Layer ▸ Merge Visible")
    }

    static func stampVisible(name: String) -> [ActionStep] {
        step("stamp_visible", ["name": name], note: "Layer ▸ Stamp Visible")
    }

    /// #1 phase 4c: Layer ▸ Stamp Selected over the replay's selected
    /// layers, and ⌘E's Merge Group of its active group.
    static func stampSelected(name: String) -> [ActionStep] {
        step("stamp_selected", ["layers": ActionArgs.selectedLayers, "name": name],
             note: "Layer ▸ Stamp Selected")
    }

    static var mergeGroup: [ActionStep] {
        step("merge_group", ["layer": ActionArgs.activeLayer], note: "Layer ▸ Merge Group")
    }

    /// Layer ▸ New ▸ Layer Via Copy / Via Cut.
    static func layerVia(cut: Bool, name: String) -> [ActionStep] {
        step(
            cut ? "layer_via_cut" : "layer_via_copy",
            ["layer": ActionArgs.activeLayer, "name": name],
            note: cut ? "Layer ▸ New ▸ Layer Via Cut" : "Layer ▸ New ▸ Layer Via Copy")
    }

    /// One property of the ACTIVE layer — the panel header's blend mode and
    /// opacity, the eye toggle, an inline rename, a group's disclosure.
    ///
    /// `set_layer_properties` writes ONE layer, and the panel header writes
    /// the whole selection; a multi-entry write therefore has no twin and
    /// records `.unrecorded` at its site rather than pretending here.
    static func layerProperty(_ key: String, _ value: Any, note: String) -> [ActionStep] {
        step("set_layer_properties", ["index": ActionArgs.activeLayer, key: value], note: note)
    }

    /// The same write aimed at a layer the user picked by row rather than at
    /// the ambient active one (the panel's per-row eye, rename and
    /// disclosure controls address their own row).
    static func layerProperty(
        _ key: String, _ value: Any, layer ref: LayerRef?, note: String
    ) -> [ActionStep] {
        step(
            "set_layer_properties",
            ["index": ActionArgs.layer(ref), key: value], note: note)
    }

    /// A property write the layers panel's HEADER fans out across the whole
    /// selection (blend mode, opacity, the lock set).
    ///
    /// One selected layer records the real write; several record the visible
    /// placeholder, because `set_layer_properties` writes ONE layer and there
    /// is no set form of it. Splitting the write into N steps by name was
    /// rejected: two layers may share a name, and a step that silently wrote
    /// the same layer twice would be worse than a stated gap.
    static func selectionProperty(
        _ key: String, _ value: Any, selectionCount: Int, note: String
    ) -> [ActionStep] {
        guard selectionCount == 1 else { return unrecorded(note) }
        return layerProperty(key, value, note: note)
    }

    /// A Move drag or nudge, at either size.
    ///
    /// **Both forms are now RELATIVE**, which is the 12B change: `move_layers`
    /// is the offset op the gesture itself ran
    /// (`RasterDocument.moveLayers`), so the step says exactly what the drag
    /// said — move these layers by this delta. The single-layer form used to
    /// record an ABSOLUTE `set_layer_properties` offset and say so in a note,
    /// which snapped the layer to a fixed canvas position on replay instead
    /// of nudging it; `layerOffset` is gone and this is what replaced it.
    ///
    /// An EMPTY `layers` records the `$layers` symbol — "whatever this
    /// document has selected" — which is what a multi-layer drag means: the
    /// gesture moved the panel selection expanded by subtree and link group,
    /// and a replay should move the replay document's own. A ONE-layer drag
    /// names that layer, so replaying it on a document with a different
    /// selection still moves the layer the user dragged.
    /// An ⌥-drag or ⌥+arrow with no selection adds `"duplicate": true`,
    /// only when true.
    static func moveLayersStep(layers: [LayerRef], dx: Int, dy: Int,
                               duplicate: Bool = false) -> [ActionStep] {
        var args: [String: Any] = [
            "layers": layers.isEmpty ? ActionArgs.selectedLayers : ActionArgs.layers(layers),
            "dx": dx, "dy": dy,
        ]
        if duplicate { args["duplicate"] = true }
        let what = layers.isEmpty ? "the whole layer selection" : "one layer"
        return step(
            "move_layers", args,
            note: duplicate ? "Duplicate: a relative move of a copy of \(what)"
                : "Move: a relative move of \(what)")
    }

    /// A Move drag or nudge over a selection: the one layer whose
    /// selected pixels moved, by reference, and the gesture's delta. An
    /// ⌥-drag adds `"duplicate": true`, only when true. `target` /
    /// `background`: a mask, channel or colour-plane target's
    /// agent name and the vacated area's authored hex (the background
    /// swatch; none for Alpha), recorded only for a plane.
    static func moveSelectedPixelsStep(layer: LayerRef?, dx: Int, dy: Int, duplicate: Bool,
                                       target: String? = nil,
                                       background: String? = nil) -> [ActionStep] {
        var args: [String: Any] = ["layer": ActionArgs.layer(layer), "dx": dx, "dy": dy]
        if duplicate { args["duplicate"] = true }
        if let target { args["target"] = target }
        if let background { args["background"] = background }
        return step(
            "move_selected_pixels", args,
            note: duplicate ? "Duplicate: the selected pixels" : "Move: the selected pixels")
    }

    /// ⌘T over a selection: the one layer whose selected pixels
    /// transformed, by reference, with `transform_layer`'s named parameters,
    /// or the box's four destination corners for a distorted box. ⌥⌘T
    /// adds `"duplicate": true`, only when true.
    /// `target` / `background`: a mask, channel or colour-plane
    /// target's agent name and the vacated area's authored hex (the
    /// background swatch; none for Alpha), recorded only for a plane.
    static func transformSelectedPixelsStep(layer: LayerRef?, transform: LayerTransform?,
                                            corners: [CGPoint]?, sampler: String,
                                            exact: Bool, duplicate: Bool = false,
                                            target: String? = nil,
                                            background: String? = nil) -> [ActionStep] {
        var arguments: [String: Any] = ["layer": ActionArgs.layer(layer), "sampler": sampler]
        if duplicate { arguments["duplicate"] = true }
        if let target { arguments["target"] = target }
        if let background { arguments["background"] = background }
        if let corners {
            arguments["corners"] = exact ? corners.map { [Double($0.x), Double($0.y)] }
                : ActionArgs.points(corners)
        } else if let transform {
            let value: (CGFloat) -> Double = { exact ? Double($0) : ActionArgs.number($0) }
            arguments["pivot_x"] = value(transform.pivot.x)
            arguments["pivot_y"] = value(transform.pivot.y)
            arguments["translate_x"] = value(transform.translation.dx)
            arguments["translate_y"] = value(transform.translation.dy)
            arguments["rotate"] = exact ? transform.degrees : ActionArgs.number(transform.degrees)
            arguments["scale_x"] = value(transform.scaleX)
            arguments["scale_y"] = value(transform.scaleY)
        }
        return step("transform_selected_pixels", arguments,
                    note: duplicate ? "Layer ▸ Free Transform a Copy (selected pixels)"
                        : "Layer ▸ Free Transform (selected pixels)")
    }

    /// A panel drag that moved ONE entry. `to` is in the core's own
    /// remove-then-insert numbering (the stack with this entry's subtree
    /// already taken out), which is what the tool documents and what
    /// `LayerReorder.reorder` computes.
    static func reorderLayer(from: Int, to: Int, depth: Int) -> [ActionStep] {
        step(
            "reorder_layer", ["from": from, "to": to, "depth": depth],
            note: "Layers panel: drag to reorder")
    }

    /// A panel drag that moved SEVERAL entries — the multi-entry twin
    /// `reorder_layer` had no spelling for, so the gesture recorded a visible
    /// placeholder.
    ///
    /// `at` is the insertion index in the PRE-move numbering: the drop line
    /// the panel drew, before anything was taken out. That is the one number
    /// the gesture actually knows — each individual move is expressed in the
    /// numbering the previous one produced, which is why N `reorder_layer`
    /// steps could never be written down — and `LayerReorder.reorder` is the
    /// shared arithmetic that turns it into the chain.
    static func reorderLayers(layers: [LayerRef], at: Int, depth: Int) -> [ActionStep] {
        step(
            "reorder_layers",
            ["layers": ActionArgs.layers(layers), "at": at, "depth": depth],
            note: "Layers panel: drag several rows to reorder")
    }

    /// Layer ▸ Arrange ▸ …
    static func arrangeLayer(to position: String) -> [ActionStep] {
        step(
            "arrange_layer", ["layer": ActionArgs.activeLayer, "to": position],
            note: "Layer ▸ Arrange")
    }

    /// Layer ▸ Align / Distribute, over the panel's selection.
    static func alignLayers(edge: String, to reference: String) -> [ActionStep] {
        step(
            "align_layers",
            ["layers": ActionArgs.selectedLayers, "edge": edge, "to": reference],
            note: "Layer ▸ Align")
    }

    static func distributeLayers(axis: String) -> [ActionStep] {
        step(
            "distribute_layers", ["layers": ActionArgs.selectedLayers, "axis": axis],
            note: "Layer ▸ Distribute")
    }

    /// Edit ▸ Auto-Align Layers…, over the panel's selection, with the
    /// dialog's projection and the menu's own sampler spelled out, and
    /// `"vignette": true` / `"distortion": true` only when Vignette Removal /
    /// Geometric Distortion is on.
    static func autoAlignLayers(projection: String, vignette: Bool = false,
                                distortion: Bool = false) -> [ActionStep] {
        var arguments: [String: Any] = [
            "layers": ActionArgs.selectedLayers, "projection": projection, "sampler": "bicubic",
        ]
        if vignette { arguments["vignette"] = true }
        if distortion { arguments["distortion"] = true }
        return step("auto_align_layers", arguments, note: "Edit ▸ Auto-Align Layers…")
    }

    /// Layer ▸ Stack Mode, over the panel's selection.
    static func stackMode(mode: String) -> [ActionStep] {
        step(
            "stack_mode", ["layers": ActionArgs.selectedLayers, "mode": mode],
            note: "Layer ▸ Stack Mode")
    }

    /// Layer ▸ Reduce Noise from Stack…, over the panel's selection.
    static func reduceNoiseFromStack(mode: String) -> [ActionStep] {
        step(
            "reduce_noise_from_stack", ["layers": ActionArgs.selectedLayers, "mode": mode],
            note: "Layer ▸ Reduce Noise from Stack…")
    }

    /// File ▸ Automate ▸ Merge Panorama…: the dialog's files in
    /// its list's order, its layout and all four boxes, each spelled out.
    /// The step makes a new document; edits on it record as one placeholder
    /// (`ActionRecorder.noteCreatedDocument`).
    static func mergePanorama(paths: [String], layout: String, blend: Bool, vignette: Bool,
                              distortion: Bool, fill: Bool) -> [ActionStep] {
        step(
            "merge_panorama",
            ["paths": paths, "layout": layout, "blend": blend, "vignette": vignette,
             "distortion": distortion, "fill": fill],
            note: "File ▸ Automate ▸ Merge Panorama…")
    }

    /// Edit ▸ Auto-Blend Layers…, over the panel's selection, with the
    /// sheet's method and three checkboxes spelled out. A step recorded before
    /// Panorama has no `method` and replays as Stack Images, the tool's
    /// default.
    /// A recording without `fill` replays as false.
    static func autoBlendLayers(method: String, seamless: Bool, align: Bool, fill: Bool)
        -> [ActionStep]
    {
        step(
            "auto_blend_layers",
            ["layers": ActionArgs.selectedLayers, "method": method, "seamless": seamless,
             "align": align, "fill": fill],
            note: "Edit ▸ Auto-Blend Layers…")
    }

    static func linkLayers(unlink: Bool) -> [ActionStep] {
        step(
            unlink ? "unlink_layers" : "link_layers", ["layers": ActionArgs.selectedLayers],
            note: unlink ? "Layer ▸ Unlink Layers" : "Layer ▸ Link Layers")
    }

    /// Layer ▸ Group Layers / Ungroup Layers.
    static func groupLayers(name: String) -> [ActionStep] {
        step(
            "group_layers", ["layers": ActionArgs.selectedLayers, "name": name],
            note: "Layer ▸ Group Layers")
    }

    static var ungroupLayers: [ActionStep] {
        step(
            "ungroup_layers", ["index": ActionArgs.activeLayer],
            note: "Layer ▸ Ungroup Layers")
    }

    /// Layer ▸ Lock — the resulting lock SET, as the tool's `locks` array.
    ///
    /// `layers: true` is the multi-selection form: the menu item fans the
    /// toggle out over the whole selection, and `set_layer_lock` now takes a
    /// `layers` array, so what used to record a visible placeholder records
    /// the real write over `{"$layers": "selected"}`.
    static func setLayerLock(_ locks: [String], layers: Bool) -> [ActionStep] {
        step(
            "set_layer_lock",
            layers
                ? ["layers": ActionArgs.selectedLayers, "locks": locks]
                : ["layer": ActionArgs.activeLayer, "locks": locks],
            note: "Layer ▸ Lock")
    }

    /// Layer ▸ Label, with the same set form for the same reason: the menu
    /// fans one label out over the whole selection, so the step replays over
    /// `{"$layers": "selected"}` rather than over one index.
    static func setLayerLabel(_ label: String, layers: Bool) -> [ActionStep] {
        step(
            "set_layer_label",
            layers
                ? ["layers": ActionArgs.selectedLayers, "label": label]
                : ["layer": ActionArgs.activeLayer, "label": label],
            note: "Layer ▸ Label")
    }

    /// Window ▸ Layer Comps ▸ New… — a comp records per-layer state by
    /// IDENTITY, so a replay saves the target document's own state under the
    /// same name and flags rather than carrying a table of indices.
    static func saveLayerComp(_ name: String, _ comment: String,
                              _ captures: LayerCompCaptures) -> [ActionStep] {
        var arguments: [String: Any] = ["name": name]
        if !comment.isEmpty { arguments["comment"] = comment }
        arguments.merge(captures.arguments) { _, new in new }
        return step("save_layer_comp", arguments, note: "Window ▸ Layer Comps")
    }

    /// Window ▸ Layer Comps ▸ Apply. By NAME, for `selectLayer`'s reason: an
    /// index means nothing on another document while "the comp called Hero"
    /// is exactly what the user pointed at.
    static func applyLayerComp(_ name: String) -> [ActionStep] {
        step("apply_layer_comp", ["name": name], note: "Window ▸ Layer Comps")
    }

    /// Window ▸ Layer Comps ▸ Update / Options….
    static func updateLayerComp(_ name: String, newName: String?, comment: String?,
                                captures: LayerCompCaptures?, recapture: Bool) -> [ActionStep] {
        var arguments: [String: Any] = ["name": name, "recapture": recapture]
        if let newName { arguments["new_name"] = newName }
        if let comment { arguments["comment"] = comment }
        if let captures { arguments.merge(captures.arguments) { _, new in new } }
        return step("update_layer_comp", arguments, note: "Window ▸ Layer Comps")
    }

    /// Window ▸ Layer Comps ▸ Delete.
    static func deleteLayerComp(_ name: String) -> [ActionStep] {
        step("delete_layer_comp", ["name": name], note: "Window ▸ Layer Comps")
    }

    /// Window ▸ Layer Comps, a row dragged to FINAL index `to`.
    static func reorderLayerComp(_ name: String, to: Int) -> [ActionStep] {
        step("reorder_layer_comp", ["name": name, "to": to], note: "Window ▸ Layer Comps")
    }

    /// Layer ▸ Create/Release Clipping Mask, with the same set form for the
    /// same reason.
    static func setLayerClipped(_ clipped: Bool, layers: Bool) -> [ActionStep] {
        step(
            "set_layer_clipped",
            layers
                ? ["layers": ActionArgs.selectedLayers, "clipped": clipped]
                : ["layer": ActionArgs.activeLayer, "clipped": clipped],
            note: "Layer ▸ Clipping Mask")
    }

    /// A layer picked in the panel, or by the Move tool's Auto-Select.
    ///
    /// By NAME, because "the third row" means nothing on another document
    /// while "the layer called Sky" is exactly what the user pointed at. A
    /// name that is missing at replay fails the step with that name in the
    /// message.
    static func selectLayer(_ ref: LayerRef?) -> [ActionStep] {
        step(
            "set_active_layer", ["index": ActionArgs.layer(ref)],
            note: "Layers panel: select a layer")
    }

    /// A multi-row panel selection: every member by name, with the primary
    /// named too. Duplicate names resolve to the same layer and the tool
    /// then refuses the duplicate, which is a visible failure rather than a
    /// silent wrong answer.
    static func selectLayers(_ refs: [LayerRef?], primary: LayerRef?) -> [ActionStep] {
        step(
            "set_selected_layers",
            [
                "layers": refs.map { ActionArgs.layer($0) },
                "primary": ActionArgs.layer(primary),
            ],
            note: "Layers panel: select several layers")
    }

    /// The Move tool's Auto-Select click — geometric, so it replays on a
    /// different document with no layer name to miss.
    ///
    /// `mode` is `"replace"` for a plain click and `"add"` for the ⇧-click
    /// that builds a selection on the canvas. The add form used to record a
    /// visible placeholder because the tool had no such mode; it has one now.
    static func autoSelectLayer(at point: CGPoint, group: Bool, mode: String) -> [ActionStep] {
        step(
            "auto_select_layer",
            [
                "x": ActionArgs.number(point.x), "y": ActionArgs.number(point.y),
                "group": group, "mode": mode,
            ],
            note: mode == "add" ? "Move tool: ⇧ Auto-Select" : "Move tool: Auto-Select")
    }

    // MARK: - Layer masks, styles and clipping

    static func addLayerMask(kind: String) -> [ActionStep] {
        step(
            "add_layer_mask", ["layer": ActionArgs.activeLayer, "kind": kind],
            note: "Layer ▸ Layer Mask")
    }

    static func removeLayerMask(apply: Bool) -> [ActionStep] {
        step(
            "remove_layer_mask", ["layer": ActionArgs.activeLayer, "apply": apply],
            note: apply ? "Layer ▸ Layer Mask ▸ Apply" : "Layer ▸ Layer Mask ▸ Delete")
    }

    static func setLayerMaskEnabled(_ enabled: Bool) -> [ActionStep] {
        step(
            "set_layer_mask_enabled", ["layer": ActionArgs.activeLayer, "enabled": enabled],
            note: "Layer ▸ Layer Mask ▸ Enable/Disable")
    }

    /// Window ▸ Styles: a swatch click (Shift-click when `add`) over the
    /// selection, so the step replays over `{"$layers": "selected"}`; the
    /// preset by NAME and group, as the library and the panel know it.
    static func applyStylePreset(name: String, group: String?, add: Bool) -> [ActionStep] {
        var arguments: [String: Any] = ["layers": ActionArgs.selectedLayers, "name": name,
                                        "add": add]
        if let group { arguments["group"] = group }
        return step("apply_style_preset", arguments, note: "Window ▸ Styles")
    }

    /// Layer ▸ Layer Style. `json` is the style's own canonical JSON — what
    /// `LayerStyle.json()` writes and what `set_layer_style` parses — or nil
    /// to clear the layer's style.
    static func setLayerStyle(json: String?, layer ref: LayerRef?, note: String)
        -> [ActionStep]
    {
        // Stored as an OBJECT rather than as the string the handler would
        // also accept, so the recorded step matches the catalog's own schema
        // and reads properly in the JSON editor.
        var parsed = json
            .flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) }
            .flatMap { $0 as? [String: Any] }
        // A replayed pattern fill re-anchors where the layer the step reaches was made:
        // the recorded `anchor` is the recording layer's, meaningless on another one — a pattern
        // overlay's, the one inside each stroke's `pattern_fill` (phase 2), and the bevel's
        // `texture` (phase 3).
        if var style = parsed, let effects = style["effects"] as? [[String: Any]] {
            style["effects"] = effects.map { effect -> [String: Any] in
                var stripped = effect
                switch effect["type"] as? String {
                case LayerStyleEffectKind.patternOverlay.rawValue:
                    stripped.removeValue(forKey: "anchor")
                case LayerStyleEffectKind.stroke.rawValue:
                    if var fill = effect["pattern_fill"] as? [String: Any] {
                        fill.removeValue(forKey: "anchor")
                        stripped["pattern_fill"] = fill
                    }
                case LayerStyleEffectKind.bevelEmboss.rawValue:
                    if var texture = effect["texture"] as? [String: Any] {
                        texture.removeValue(forKey: "anchor")
                        stripped["texture"] = texture
                    }
                default:
                    break
                }
                return stripped
            }
            parsed = style
        }
        let object: Any = parsed ?? NSNull()
        return step(
            "set_layer_style",
            ["layer": ActionArgs.layer(ref), "style": object], note: note)
    }

    static func setGlobalLight(angle: Double, altitude: Double) -> [ActionStep] {
        step(
            "set_global_light",
            ["angle": ActionArgs.number(angle), "altitude": ActionArgs.number(altitude)],
            note: "Layer ▸ Layer Style ▸ Global Light")
    }

    // MARK: - Filters and adjustments

    /// Filter ▸ Neural Filters ▸ Depth Blur…: the settings only — never the depth map or
    /// the model version — so a replay runs the model again on the layer it reaches.
    static func depthBlur(_ settings: DepthBlurSettings) -> [ActionStep] {
        var arguments = settings.arguments
        arguments["layer"] = ActionArgs.activeLayer
        return step("depth_blur", arguments, note: "Filter ▸ Neural Filters ▸ Depth Blur")
    }

    /// Filter ▸ Neural Filters ▸ Super Zoom…: the settings only, so a replay zooms the
    /// layer it reaches into a new document; `note` names the model (`ModelGate.note`).
    static func superZoom(_ settings: SuperZoomSettings, note: String) -> [ActionStep] {
        var arguments = settings.arguments
        arguments["layer"] = ActionArgs.activeLayer
        return step("super_zoom", arguments, note: note)
    }

    /// Filter ▸ Neural Filters ▸ JPEG Artifacts Removal…: the settings only; `note` names
    /// the model (`ModelGate.note`).
    static func jpegArtifactsRemoval(_ settings: JpegArtifactsSettings, note: String) -> [ActionStep] {
        var arguments = settings.arguments
        arguments["layer"] = ActionArgs.activeLayer
        return step("jpeg_artifacts_removal", arguments, note: note)
    }

    /// Filter ▸ Neural Filters ▸ Colorize…: the settings only — never the predicted
    /// chroma — so a replay runs DDColor again on the layer it reaches; `note` names the model
    /// (`ModelGate.note`).
    static func colorize(_ settings: ColorizeSettings, note: String) -> [ActionStep] {
        var arguments = settings.arguments
        arguments["layer"] = ActionArgs.activeLayer
        return step("neural_colorize", arguments, note: note)
    }

    /// Edit ▸ Sky Replacement…: the settings and the sky's library id or user name (`sky`),
    /// or a file's path (`sky_path`) — never the mask — so a replay runs the sky model again;
    /// `note` names the model.
    static func skyReplacement(_ settings: SkyReplacementSettings, sky: SkyChoice,
                               note: String) -> [ActionStep] {
        var arguments = settings.arguments
        arguments["layer"] = ActionArgs.activeLayer
        switch sky {
        case .library(let preset): arguments["sky"] = preset.id
        case .user(let user): arguments["sky"] = user.name
        case .file(let url): arguments["sky_path"] = url.path
        }
        return step("sky_replacement", arguments, note: note)
    }

    /// Filters ▸ … and Image ▸ Adjustments ▸ … (the destructive halves).
    /// `extra` carries the filter's own parameters; `target` is the ambient
    /// plane/channel target, always written out.
    static func filter(_ name: String, _ extra: [String: Any] = [:], target: String)
        -> [ActionStep]
    {
        var arguments: [String: Any] = ["filter": name, "layer": ActionArgs.activeLayer]
        arguments.merge(extra) { _, new in new }
        arguments["target"] = target
        return step("apply_filter", arguments, note: "Filters / Image ▸ Adjustments")
    }

    /// Image ▸ Adjustments ▸ Equalize (#1): the prompt's answer (`scope`, nil when there was no
    /// selection and so no prompt) and the paint target.
    static func equalize(scope: EqualizeScope?, target: String) -> [ActionStep] {
        var arguments: [String: Any] = ["layer": ActionArgs.activeLayer, "target": target]
        if let scope { arguments["scope"] = scope.rawValue }
        return step("equalize", arguments, note: "Image ▸ Adjustments ▸ Equalize")
    }

    /// Image ▸ Auto Tone / Auto Contrast / Auto Color.
    static func autoAdjust(_ tool: String, target: String = "layer", analysisScope: String = "whole_target") -> [ActionStep] {
        step(tool, ["layer": ActionArgs.activeLayer, "target": target, "analysis_scope": analysisScope], note: "Image ▸ Auto…")
    }

    /// Layer ▸ New Adjustment Layer ▸ …, from the very meta the dialog is
    /// about to store, so the recorded parameters cannot drift from the
    /// committed ones.
    /// `position` as in `newLayer`: recorded only when not `above`.
    static func addAdjustmentLayer(
        meta: String, name: String, position: LayerInsertPosition = .above
    ) -> [ActionStep] {
        guard let payload = AdjustmentLayerPayload.decode(meta) else {
            return unrecorded("New \(name) Layer")
        }
        var arguments: [String: Any] = ["op": payload.op, "name": name, "params": payload.params]
        if position != .above { arguments["position"] = position.rawValue }
        return step(
            "add_adjustment_layer", arguments,
            note: lutNote(payload) ?? "Layer ▸ New Adjustment Layer")
    }

    /// Re-editing an existing adjustment layer's parameters.
    static func editAdjustmentLayer(meta: String, layer ref: LayerRef?) -> [ActionStep] {
        guard let payload = AdjustmentLayerPayload.decode(meta) else {
            return unrecorded("Edit Adjustment Layer")
        }
        var params = payload.params
        // A Color Lookup layer's stored LUT is up to ~575 KB of base64, and
        // an EDIT does not need it: `edit_adjustment_layer` carries the
        // layer's own table forward when the call names no replacement
        // (`AgentServer+Adjustments.mergingStoredLut`, which exists for
        // exactly this flow). So the table keys are dropped and only what the
        // dialog actually changes — `strength` — is recorded.
        if payload.op == AdjustmentLayerOp.colorLookup.rawValue {
            for key in AgentServer.lutTableKeys { params.removeValue(forKey: key) }
        }
        return step(
            "edit_adjustment_layer",
            ["layer": ActionArgs.layer(ref), "op": payload.op, "params": params],
            note: "Layer ▸ Adjustment Options…")
    }

    /// The one place a recorded adjustment step is allowed to be large, and
    /// the note that says so.
    ///
    /// A NEW Color Lookup layer has nowhere to get its table from but the
    /// step itself: the layer stores no .cube path (that is stated in
    /// `mergingStoredLut`'s own comment), so there is no `file` form to
    /// record and the honest choice is a big step that replays correctly
    /// over a small one that cannot.
    private static func lutNote(_ payload: AdjustmentLayerPayload) -> String? {
        guard payload.op == AdjustmentLayerOp.colorLookup.rawValue,
              (payload.params["table"] as? String)?.isEmpty == false
        else { return nil }
        return "Layer ▸ New Adjustment Layer — carries the whole colour LUT, "
            + "because the layer keeps no path to the .cube it came from"
    }

    // MARK: - Painting

    /// One paint/retouch stroke, replayed as the SAME polyline through the
    /// SAME `SoftBrush` engine the canvas dabs with.
    ///
    /// Native captures additionally carry versioned canonical dabs, preserving
    /// pressure and timed airbrush deposits. Historical byte point recordings
    /// keep their constant-size and single-deposit hold semantics.
    static func stroke(_ tool: String, _ arguments: [String: Any]) -> [ActionStep] {
        step(tool, arguments, note: "Canvas: a brush stroke")
    }

    /// The paint bucket's click (Fill tool), on the layer or on a plane.
    ///
    /// `space` is the DOCUMENT's colour space when there is a wide-gamut
    /// spelling to record (Phase 12A): the step then carries the document's
    /// own bytes as `color` with `color_space: "document"`, which is exactly
    /// what the tool painted. On an sRGB document `documentHex` answers nil
    /// and the step is byte-for-byte the one this factory always wrote.
    static func fill(
        at point: CGPoint, color: NSColor, tolerance: Int, contiguous: Bool, target: String,
        space: NSColorSpace? = nil
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "x": Int(point.x.rounded(.down)), "y": Int(point.y.rounded(.down)),
            "color": ActionArgs.hex(color), "tolerance": tolerance,
            "contiguous": contiguous, "layer": ActionArgs.activeLayer, "target": target,
        ]
        if let space = space, let hex = ActionArgs.documentHex(color, in: space) {
            arguments["color"] = hex
            arguments["color_space"] = "document"
        }
        return step("fill", arguments, note: "Fill tool: click to fill")
    }

    /// Edit ▸ Fill… and the four ⌥⌫ / ⌘⌫ fills: the RESOLVED
    /// colour (a swatch, a picked Color… or a document constant), never the
    /// coverage gray a plane target turns it into — `fill_selection` makes
    /// that conversion itself. `opacity` is the sheet's 0…100; the step
    /// carries the tool's 0…1. `space` follows `fill`'s rule above.
    /// `rgba32`: the linear ink a 32-bit float fill painted with
    /// (`PrecisionFillStroke.recordedLinear`); the step then carries it, exact,
    /// and no `color` or `color_space`.
    static func fillSelection(
        color: NSColor, blendMode: DrBlendMode, opacity: Double,
        preserveTransparency: Bool, target: String, space: NSColorSpace?, note: String,
        rgba32: [Double]? = nil
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "color": ActionArgs.hex(color),
            "blend_mode": DrBlendMode.displayName(for: blendMode),
            "opacity": ActionArgs.number(opacity / 100),
            "preserve_transparency": preserveTransparency,
            "layer": ActionArgs.activeLayer, "target": target,
        ]
        if let rgba32 {
            arguments["color"] = nil
            arguments["rgba32"] = rgba32
        } else if let space = space, let hex = ActionArgs.documentHex(color, in: space) {
            arguments["color"] = hex
            arguments["color_space"] = "document"
        }
        return step("fill_selection", arguments, note: note)
    }

    /// Edit ▸ Fill… with Pattern contents: the RESOLVED pattern object — a
    /// built-in's `{preset}`, else the tile itself (`{name, width, height,
    /// rgba}`), never a library name — so the step replays without the
    /// library. `opacity` is the sheet's 0…100; the step carries the tool's 0…1.
    static func fillSelection(
        pattern: PatternRef, blendMode: DrBlendMode, opacity: Double,
        preserveTransparency: Bool, target: String, note: String
    ) -> [ActionStep] {
        step("fill_selection", [
            "pattern": pattern.patternObject,
            "blend_mode": DrBlendMode.displayName(for: blendMode),
            "opacity": ActionArgs.number(opacity / 100),
            "preserve_transparency": preserveTransparency,
            "layer": ActionArgs.activeLayer, "target": target,
        ], note: note)
    }

    /// Edit ▸ Fill… and the four ⌥⌫ / ⌘⌫ fills in Quick Mask: the
    /// colour (already quantized to the document's bytes by the caller), the
    /// mode and the opacity, pinned to `target: "quick_mask"` so the step
    /// refuses by name on a document that is not in Quick Mask instead of
    /// painting its layer. No `layer` or `preserve_transparency`. `opacity`
    /// is the sheet's 0…100; the step carries the tool's 0…1. `space`
    /// follows `fill`'s rule above.
    static func quickMaskFill(
        color: NSColor, blendMode: DrBlendMode, opacity: Double, space: NSColorSpace?,
        note: String
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "color": ActionArgs.hex(color),
            "blend_mode": DrBlendMode.displayName(for: blendMode),
            "opacity": ActionArgs.number(opacity / 100),
            "target": "quick_mask",
        ]
        if let space = space, let hex = ActionArgs.documentHex(color, in: space) {
            arguments["color"] = hex
            arguments["color_space"] = "document"
        }
        return step("fill_selection", arguments, note: note)
    }

    /// Edit ▸ Stroke…: `fillSelection`'s colour rules, plus the
    /// band's `width` (pixels, absolute) and `location`. `opacity` is the
    /// sheet's 0…100; the step carries the tool's 0…1. `rgba32` as
    /// `fillSelection`'s.
    static func strokeSelection(
        width: Double, location: StrokeLocation, color: NSColor, blendMode: DrBlendMode,
        opacity: Double, preserveTransparency: Bool, target: String, space: NSColorSpace?,
        rgba32: [Double]? = nil
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "color": ActionArgs.hex(color),
            "blend_mode": DrBlendMode.displayName(for: blendMode),
            "opacity": ActionArgs.number(opacity / 100),
            "preserve_transparency": preserveTransparency,
            "width": ActionArgs.number(width), "location": location.agentName,
            "layer": ActionArgs.activeLayer, "target": target,
        ]
        if let rgba32 {
            arguments["color"] = nil
            arguments["rgba32"] = rgba32
        } else if let space = space, let hex = ActionArgs.documentHex(color, in: space) {
            arguments["color"] = hex
            arguments["color_space"] = "document"
        }
        return step("stroke_selection", arguments, note: "Edit ▸ Stroke…")
    }

    /// The gradient tool's drag. `space` follows `fill`'s rule above; `mode`
    /// is recorded as `blend_mode` only when it is not Normal, so a
    /// Normal drag records the step it always did.
    static func gradient(
        from a: CGPoint, to b: CGPoint, start: NSColor, end: NSColor, radial: Bool,
        target: String, space: NSColorSpace? = nil, mode: DrBlendMode = DR_BLEND_NORMAL
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "x0": ActionArgs.number(a.x), "y0": ActionArgs.number(a.y),
            "x1": ActionArgs.number(b.x), "y1": ActionArgs.number(b.y),
            "start_color": ActionArgs.hex(start), "end_color": ActionArgs.hex(end),
            "shape": radial ? "radial" : "linear",
            "layer": ActionArgs.activeLayer, "target": target,
        ]
        if let space = space,
           let first = ActionArgs.documentHex(start, in: space),
           let last = ActionArgs.documentHex(end, in: space) {
            arguments["start_color"] = first
            arguments["end_color"] = last
            arguments["color_space"] = "document"
        }
        if mode != DR_BLEND_NORMAL {
            arguments["blend_mode"] = DrBlendMode.displayName(for: mode)
        }
        return step("gradient", arguments, note: "Gradient tool: drag")
    }

    /// Edit ▸ Clear.
    static func clearSelection(note: String) -> [ActionStep] {
        step("clear_selection", ["layer": ActionArgs.activeLayer], note: note)
    }

    /// Edit ▸ Cut (Phase 12G-1): the `cut` tool itself, so a replayed Cut
    /// copies to the clipboard AND clears, at every depth.
    static func cut(note: String) -> [ActionStep] {
        step("cut", ["layer": ActionArgs.activeLayer], note: note)
    }

    /// Edit ▸ Copy: the `copy` tool in `mode: "auto"`, with no
    /// layer named, so a replay decides pixels or layers from the replaying
    /// document's own selection and copies its own active or selected
    /// layers (measured against Photoshop's Copy step: its `copyHint` does
    /// not bind the replay). No pixels are stored.
    static func copy(note: String) -> [ActionStep] {
        step("copy", ["mode": "auto"], note: note)
    }

    /// Edit ▸ Copy Merged: the replaying document's own
    /// composite. No pixels are stored.
    static func copyMerged(note: String) -> [ActionStep] {
        step("copy_merged", note: note)
    }

    /// Edit ▸ Paste and its Paste Special kinds. Two kinds of step:
    /// - `payloadPath` nil — the LIVE step every paste records by default:
    ///   no pixels and no position, so a replay pastes whatever the clipboard
    ///   holds then (pixels or layers).
    /// - `payloadPath` set — the saved-payload step (12G-2,
    ///   `ClipboardPayloadStore.pasteStep`): the landed payload as a FILE,
    ///   which `package_action` carries as a `$asset`, so a replay pastes the
    ///   same samples with no pasteboard.
    /// A non-nil `offset` (placement `at`) is written as absolute `x` /
    /// `y`; `name` and `mode` only when given.
    static func paste(payloadPath: String?, placement: PastePlacement, offset: (x: Int, y: Int)?,
                      name: String?, profile: PasteProfile, mode: String? = nil,
                      note: String) -> [ActionStep] {
        var arguments: [String: Any] = [
            "placement": placement.rawValue, "layer": ActionArgs.activeLayer,
        ]
        if let payloadPath { arguments["payload_path"] = payloadPath }
        if let name { arguments["name"] = name }
        if let mode { arguments["mode"] = mode }
        // Only Don't Convert is recorded; Convert is the default, and
        // a step never carries the retired `convert` key.
        if profile == .preserve { arguments["profile"] = profile.rawValue }
        if let offset {
            arguments["x"] = offset.x
            arguments["y"] = offset.y
        }
        return step("paste", arguments, note: note)
    }

    // MARK: - Described layers

    /// The Text tool's commit, from the very payload the layer is rendered
    /// from — so the recorded typography cannot drift from the committed one.
    /// `anchor` is the block's top-left in canvas pixels.
    static func addTextLayer(_ payload: TextLayerPayload, at anchor: CGPoint) -> [ActionStep] {
        var arguments = textArguments(payload)
        arguments["x"] = Double(anchor.x)
        arguments["y"] = Double(anchor.y)
        return step("add_text_layer", arguments, note: "Text tool: new text layer")
    }

    /// Re-editing an existing text layer. No `x`/`y`: a re-edit never moves
    /// the origin (the editor opens at the layer's own anchor), and the tool
    /// has no such argument.
    static func editTextLayer(_ payload: TextLayerPayload, layer ref: LayerRef?)
        -> [ActionStep]
    {
        var arguments = textArguments(payload)
        arguments["layer"] = ActionArgs.layer(ref)
        return step("edit_text_layer", arguments, note: "Text tool: edit a text layer")
    }

    /// The typography both text tools share, spelled with the catalog's key
    /// names. `origin_frac` is deliberately absent: it is written only by the
    /// core's own placement and is not an argument of either tool.
    private static func textArguments(_ payload: TextLayerPayload) -> [String: Any] {
        var arguments: [String: Any] = [
            "text": payload.string,
            "font": payload.font,
            "size": Double(payload.size),
            "color": payload.color,
            "alignment": payload.alignment,
            "weight": payload.weight,
            "italic": payload.italic,
            "underline": payload.underline,
            "strikethrough": payload.strikethrough,
            "tracking": Double(payload.tracking),
            "leading": Double(payload.leading),
            "baseline_shift": Double(payload.baselineShift),
            "transform": payload.transform.array,
        ]
        switch payload.box {
        case .width(let width): arguments["wrap_width"] = width
        case .point: arguments["wrap_width"] = 0
        case .unspecified: break
        }
        if let artwork = payload.artwork, artwork.ink == .cmyk {
            // A CMYK document's text (#1): `ink` in percent beside the appearance hex, whose
            // alpha the replay takes; each run likewise (its hex carries only the alpha).
            arguments["ink"] = AgentServer.inkPercent(artwork.rgba)
            arguments["color_runs"] = artwork.runs.map {
                ["location": $0.location, "length": $0.length, "ink": AgentServer.inkPercent($0.rgba),
                 "color": TextLayer.hex(numbers: [0, 0, 0, ArtworkColor.alpha($0.rgba)])] as [String: Any]
            }
        } else if let artwork = payload.artwork {
            arguments.removeValue(forKey: "color")
            arguments["rgba"] = artwork.rgba
            arguments["color_runs"] = AgentServer.artworkRunFields(artwork.runs)
        }
        return arguments
    }

    /// The Shape tool's commit, from the payload the layer renders from.
    /// `anchor` is the shape box's top-left in canvas pixels.
    /// A path shape (Paths panel ▸ Convert to Shape Layer) records its outline
    /// as `subpaths` in canvas pixels with its `fill_rule`, and none of the box
    /// arguments `add_shape_layer` refuses for it: the outline places itself.
    static func addShapeLayer(_ payload: ShapeLayerPayload, at anchor: CGPoint) -> [ActionStep] {
        var arguments = shapeArguments(payload)
        arguments["kind"] = payload.kind
        if payload.kind == "path", let outline = payload.path {
            for key in ["w", "h", "flipped", "radius"] { arguments.removeValue(forKey: key) }
            arguments["subpaths"] = outline.shape(origin: anchor).subpathsJSON
            arguments["fill_rule"] = outline.fillRule
            return step("add_shape_layer", arguments, note: "Paths panel ▸ Convert to Shape Layer")
        }
        arguments["x"] = Double(anchor.x)
        arguments["y"] = Double(anchor.y)
        return step("add_shape_layer", arguments, note: "Shape tool: new shape layer")
    }

    /// A shape re-edit — the options bar's style, or a drag on the box. `x`
    /// and `y` ride along because a re-edit CAN move the box, unlike a text
    /// re-edit.
    static func editShapeLayer(
        _ payload: ShapeLayerPayload, at anchor: CGPoint, layer ref: LayerRef?
    ) -> [ActionStep] {
        var arguments = shapeArguments(payload)
        arguments["layer"] = ActionArgs.layer(ref)
        arguments["x"] = Double(anchor.x)
        arguments["y"] = Double(anchor.y)
        return step("edit_shape_layer", arguments, note: "Shape tool: edit a shape layer")
    }

    /// The geometry and style both shape tools share. `kind` is absent —
    /// `edit_shape_layer` cannot change it, and `addShapeLayer` adds it.
    private static func shapeArguments(_ payload: ShapeLayerPayload) -> [String: Any] {
        var arguments: [String: Any] = [
            "w": Double(payload.w),
            "h": Double(payload.h),
            "flipped": payload.flipped,
            "fill": payload.fill,
            "stroke": payload.stroke,
            "stroke_width": Double(payload.strokeWidth),
            "radius": Double(payload.radius),
            "transform": payload.transform.array,
        ]
        if let artwork = payload.artwork, artwork.ink == .cmyk {
            // A CMYK document's shape (#1): `fill_ink` / `stroke_ink` in percent beside the
            // appearance hexes, whose alpha the replay takes.
            if let fill = artwork.fill { arguments["fill_ink"] = AgentServer.inkPercent(fill) }
            if let stroke = artwork.stroke { arguments["stroke_ink"] = AgentServer.inkPercent(stroke) }
        } else if let artwork = payload.artwork {
            arguments.removeValue(forKey: "fill"); arguments.removeValue(forKey: "stroke")
            arguments["fill_rgba"] = artwork.fill.map { $0 as Any } ?? NSNull()
            arguments["stroke_rgba"] = artwork.stroke.map { $0 as Any } ?? NSNull()
        }
        // The stroke styles, recorded only when off their defaults.
        if payload.strokeJoin != .miter { arguments["stroke_join"] = payload.strokeJoin.rawValue }
        if payload.strokeDash != .none { arguments["stroke_dash"] = payload.strokeDash.rawValue }
        if payload.strokeAlign != .center { arguments["stroke_align"] = payload.strokeAlign.rawValue }
        return arguments
    }

    // MARK: - Retouching

    /// An outlined polygon is explicit. An adopted mask/ellipse uses the
    /// Action's current selection, exactly like Content-Aware Fill; earlier
    /// recorded selection steps remain responsible for constructing it.
    static func patchRegion(
        region: CanvasSelection, offset: CGVector, direction: String,
        sample: RetouchSampling, ignoreAdjustments: Bool
    ) -> [ActionStep] {
        guard case .polygon(let points) = region.shape else {
            var arguments: [String: Any] = [
                "layer": ActionArgs.activeLayer,
                "offset_x": ActionArgs.number(offset.dx),
                "offset_y": ActionArgs.number(offset.dy), "direction": direction,
            ]
            arguments.merge(ActionArgs.sampling(sample, ignoreAdjustments)) { _, new in new }
            return step("patch_region", arguments, note: "Patch current selection")
        }
        return patchRegion(
            points: points, offset: offset, direction: direction,
            sample: sample, ignoreAdjustments: ignoreAdjustments)
    }

    static func patchRegion(
        points: [CGPoint], offset: CGVector, direction: String,
        sample: RetouchSampling, ignoreAdjustments: Bool
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "layer": ActionArgs.activeLayer, "points": ActionArgs.points(points),
            "offset_x": ActionArgs.number(offset.dx),
            "offset_y": ActionArgs.number(offset.dy),
            "direction": direction,
        ]
        arguments.merge(ActionArgs.sampling(sample, ignoreAdjustments)) { _, new in new }
        return step("patch_region", arguments, note: "Patch tool")
    }

    static func redEye(rect: CGRect, pupilSize: Double, darken: Double) -> [ActionStep] {
        step(
            "red_eye",
            [
                "layer": ActionArgs.activeLayer,
                "x": ActionArgs.number(rect.minX), "y": ActionArgs.number(rect.minY),
                "width": ActionArgs.number(rect.width),
                "height": ActionArgs.number(rect.height),
                "pupil_size": ActionArgs.number(pupilSize),
                "darken": ActionArgs.number(darken),
            ],
            note: "Red Eye tool")
    }

    /// Filters ▸ Remove Red Eye. The eyes are recorded EXPLICITLY, as the
    /// discs Vision found, rather than left for the replay to re-detect: a
    /// recording is a record of what happened, and re-running the detector
    /// on another document would silently correct different pixels. The tool
    /// takes exactly this list.
    static func redEyeAuto(eyes: [RedEye.Eye], pupilSize: Double, darken: Double)
        -> [ActionStep]
    {
        step(
            "red_eye_auto",
            [
                "layer": ActionArgs.activeLayer,
                "eyes": eyes.map {
                    [
                        "x": ActionArgs.number($0.center.x),
                        "y": ActionArgs.number($0.center.y),
                        "radius": ActionArgs.number($0.radius),
                    ]
                },
                "pupil_size": ActionArgs.number(pupilSize),
                "darken": ActionArgs.number(darken),
            ],
            note: "Filters ▸ Remove Red Eye")
    }

    // MARK: - Selection

    /// Feather belongs to the new shape before combination, on every surface.
    static func selectShape(
        _ tool: String, _ arguments: [String: Any], mode: String, feather: Double
    ) -> [ActionStep] {
        var arguments = arguments
        arguments["mode"] = mode
        arguments["feather"] = feather
        return step(tool, arguments, note: "Canvas: a selection gesture")
    }

    /// Select ▸ All — a real tool (added with this phase) precisely so a
    /// recorded Select All is canvas-RELATIVE and batches correctly.
    static func selectAll(mode: String) -> [ActionStep] {
        step("select_all", ["mode": mode], note: "Select ▸ All")
    }

    static var deselect: [ActionStep] {
        step("deselect", note: "Select ▸ Deselect")
    }

    static func modifySelection(operation: String, radius: Double? = nil, width: Double? = nil)
        -> [ActionStep]
    {
        var arguments: [String: Any] = ["operation": operation]
        if let radius = radius { arguments["radius"] = ActionArgs.number(radius) }
        if let width = width { arguments["width"] = ActionArgs.number(width) }
        return step("modify_selection", arguments, note: "Select ▸ Modify")
    }

    /// `sampleAllLayers` false (Sample: Current layer) is recorded as
    /// `sample_all_layers: false`; the default leaves the step as before.
    static func selectMagicWand(
        at point: CGPoint, tolerance: Int, contiguous: Bool, mode: String, feather: Double = 0,
        sampleAllLayers: Bool = true, sampleSize: Int = 1
    ) -> [ActionStep] {
        var arguments: [String: Any] = [
            "x": Int(point.x.rounded(.down)), "y": Int(point.y.rounded(.down)),
            "tolerance": tolerance, "contiguous": contiguous, "mode": mode, "feather": feather,
        ]
        if !sampleAllLayers { arguments["sample_all_layers"] = false }
        // The Sample Size, recorded only off Point Sample.
        if sampleSize != 1 { arguments["sample_size"] = sampleSize }
        return step("select_magic_wand", arguments, note: "Magic Wand")
    }

    static func selectSubject(instance: Int?, mode: String, feather: Double = 0) -> [ActionStep] {
        var arguments: [String: Any] = ["mode": mode, "feather": feather]
        if let instance = instance { arguments["instance"] = instance }
        return step("select_subject", arguments, note: "Select ▸ Select Subject")
    }

    /// Select ▸ Load Selection, and the two ⌘-click gestures that share its
    /// path. Channels travel by NAME — names survive the renumbering every
    /// channel add or delete causes, which an index does not.
    static func loadSelection(
        from source: SelectionSource, mode: String, invert: Bool, in doc: RasterDocument
    ) -> [ActionStep] {
        var arguments: [String: Any] = ["mode": mode, "invert": invert]
        switch source {
        case .channel(let index):
            guard doc.channelInfo(index) != nil else {
                return unrecorded("Load Selection")
            }
            arguments["from"] = "channel"
            arguments["channel"] = ActionArgs.channel(doc.channelRef(index), at: index)
        case .layerAlpha(let index):
            arguments["from"] = "layer_alpha"
            arguments["layer"] = ActionArgs.layer(doc.layerRef(index))
        case .layerMask(let index):
            arguments["from"] = "layer_mask"
            arguments["layer"] = ActionArgs.layer(doc.layerRef(index))
        case .compositePlane(let plane):
            arguments["from"] = "plane"
            // #1 Lab phase 4b-i: a Lab document's planes replay by their Lab names.
            arguments["plane"] = doc.isLab ? LabDocument.agentPlaneName(plane) : plane.agentName
        case .inkPlate(let plate):
            arguments["from"] = "plane"
            arguments["plane"] = plate.key
        }
        return step("load_selection", arguments, note: "Select ▸ Load Selection")
    }

    /// Select ▸ Save Selection. A NEW channel is named; an existing one is
    /// combined into, by name.
    static func saveSelection(
        to destination: SelectionDestination, mode: String, in doc: RasterDocument
    ) -> [ActionStep] {
        switch destination {
        case .newChannel(let name):
            return step(
                "save_selection", ["name": name], note: "Select ▸ Save Selection (new channel)")
        case .channel(let index):
            guard doc.channelInfo(index) != nil else {
                return unrecorded("Save Selection")
            }
            return step(
                "save_selection",
                ["channel": ActionArgs.channel(doc.channelRef(index), at: index), "mode": mode],
                note: "Select ▸ Save Selection")
        }
    }

    // MARK: - Channels

    static func channelCommand(_ tool: String, _ arguments: [String: Any], note: String)
        -> [ActionStep]
    {
        step(tool, arguments, note: note)
    }

    /// Image ▸ Apply Image…
    static func applyImage(_ p: ApplyImageParameters, in doc: RasterDocument) -> [ActionStep] {
        step(
            "apply_image",
            [
                "source": ActionArgs.sourceLayer(p.source, in: doc),
                "source_plane": ActionArgs.planeChoice(p.sourcePlane, in: doc),
                "invert": p.invert,
                "blend_mode": DrBlendMode.displayName(for: p.blend),
                "opacity": ActionArgs.number(p.opacity),
                "target": p.target.agentName(in: doc),
                "layer": ActionArgs.layer(doc.layerRef(p.targetLayer)),
            ],
            note: "Image ▸ Apply Image…")
    }

    /// Image ▸ Calculations… — the new-channel result. The SELECTION result
    /// is recorded at its transient selection commit.
    static func calculations(
        _ p: CalculationsParameters, result: String, in doc: RasterDocument
    ) -> [ActionStep] {
        step(
            "calculations",
            [
                "source1": ActionArgs.sourceLayer(p.source1.layer, in: doc),
                "source1_plane": ActionArgs.planeChoice(p.source1.plane, in: doc),
                "invert1": p.source1.invert,
                "source2": ActionArgs.sourceLayer(p.source2.layer, in: doc),
                "source2_plane": ActionArgs.planeChoice(p.source2.plane, in: doc),
                "invert2": p.source2.invert,
                "blend_mode": DrBlendMode.displayName(for: p.blend),
                "opacity": ActionArgs.number(p.opacity),
                "result": result,
                "name": p.name,
            ],
            note: "Image ▸ Calculations…")
    }

    static var addLuminosityMasks: [ActionStep] {
        step("add_luminosity_masks", note: "Channels ▸ Add Luminosity Masks")
    }

    // MARK: - Guides and rulers

    static func addGuide(orientation: String, position: Double) -> [ActionStep] {
        step(
            "add_guide",
            ["orientation": orientation, "position": ActionArgs.number(position)],
            note: "View ▸ New Guide")
    }

    /// Deleting a guide names it by POSITION, not by list index: an index
    /// would delete a different guide after any earlier guide edit in the
    /// same action.
    static func removeGuide(orientation: String, position: Double) -> [ActionStep] {
        step(
            "remove_guide", ["guide": ActionArgs.guide(orientation, position)],
            note: "Canvas: drag a guide off the canvas")
    }

    /// A guide DRAG is one commit and two calls — the reason the record
    /// parameter is an ARRAY rather than a single step.
    static func moveGuide(orientation: String, from: Double, to: Double) -> [ActionStep] {
        removeGuide(orientation: orientation, position: from)
            + addGuide(orientation: orientation, position: to)
    }

    static var clearGuides: [ActionStep] {
        step("clear_guides", note: "View ▸ Clear Guides")
    }

    static func setRulerOrigin(x: Double, y: Double) -> [ActionStep] {
        step(
            "set_ruler_origin",
            ["x": ActionArgs.number(x), "y": ActionArgs.number(y)],
            note: "Rulers: drag the origin")
    }

    // MARK: - Colour management

    /// Image ▸ Assign Profile… / Convert to Profile…
    ///
    /// A built-in profile records by name. A profile the user LOADED FROM A
    /// FILE records the visible placeholder instead: `ProfileChoice.file`
    /// carries the bytes and a display name but not the path the tool would
    /// need, and inventing one would produce a step that opens the wrong
    /// file — or none — on replay.
    /// The two profile commands. Phase 11C: a FILE destination is now
    /// recordable — the step carries `profile:"file"` plus the absolute
    /// `path`, so a replay converts through the same profile — and the
    /// conversion's own options (`intent`, `black_point_compensation`,
    /// `adjustments`, `rasterize_sources`) ride along only when they differ from the defaults,
    /// so a step recorded before this phase and one recorded now are the
    /// same object for the same request.
    static func profileCommand(_ tool: String, _ choice: ProfileChoice,
                               options: ConversionOptions = .default,
                               rasterizeSources: Bool = false,
                               path: String? = nil, note: String) -> [ActionStep] {
        var args: [String: Any]
        switch choice {
        case .builtin(let space):
            args = ["profile": space == .displayP3 ? "display_p3" : "srgb"]
        case .file:
            guard let path, !path.isEmpty else { return unrecorded(note) }
            args = ["profile": "file", "path": path]
        }
        if tool == "convert_profile" {
            if options.intent != .relative { args["intent"] = options.intent.rawValue }
            if options.blackPointCompensation { args["black_point_compensation"] = true }
            if options.adjustments != .keep { args["adjustments"] = options.adjustments.rawValue }
        }
        if rasterizeSources || (tool == "convert_profile" && options.rasterizeSources) {
            args["rasterize_sources"] = true
        }
        return step(tool, args, note: note)
    }

    static func setResolution(x: Double, y: Double) -> [ActionStep] {
        step(
            "set_resolution",
            ["ppi_x": ActionArgs.number(x), "ppi_y": ActionArgs.number(y)],
            note: "Image ▸ Image Size… (resolution)")
    }

    /// File ▸ File Info… — only the fields whose value changed, so replaying
    /// the step leaves a target document's other fields as they are
    /// (`set_file_info` keeps the value of a key it is not given). No step
    /// when nothing differs.
    static func setFileInfo(from before: FileInfo, to after: FileInfo) -> [ActionStep] {
        let changed = after.changes(from: before)
        return changed.isEmpty ? [] : step("set_file_info", changed, note: "File ▸ File Info…")
    }

    /// Image ▸ Evaluation… — the per-document evaluation policy
    /// (`DocumentEvaluation.swift`).
    static func setEvaluation(_ policy: EvaluationPolicy) -> [ActionStep] {
        step("set_evaluation", ["policy": policy.rawValue], note: "Image ▸ Evaluation…")
    }

    /// Image ▸ Tone Map… — the 32-bit float document's SDR map
    /// (`FloatDocument.swift`). `white` is recorded only where it means
    /// something, exactly as the core's own wire form writes it.
    static func setToneMap(_ map: ToneMap) -> [ActionStep] {
        var args: [String: Any] = ["operator": map.operator.rawValue]
        if map.operator.usesWhite { args["white"] = ActionArgs.number(map.white) }
        return step("set_tone_map", args, note: "Image ▸ Tone Map…")
    }

    // MARK: - Free Transform

    static func persistedWarp(_ request: PersistedWarpRequest) -> [ActionStep] {
        var arguments: [String: Any] = ["layer": ActionArgs.activeLayer]
        switch request {
        case .set(_, let target, let field):
            arguments["operation"] = "set"
            arguments["target"] = target.rawValue
            arguments["columns"] = field.columns
            arguments["rows"] = field.rows
            arguments["displacements"] = field.displacements.map { row in
                row.map { NSDecimalNumber(string: String($0)) }
            }
            arguments["filter"] = field.filter.rawValue
        case .clear:
            arguments["operation"] = "clear"
        case .bake:
            arguments["operation"] = "bake"
        }
        return step("warp_layer", arguments, note: "Layer ▸ Warp…")
    }

    /// `duplicate` (Free Transform a Copy with no selection) writes
    /// `"duplicate": true`, and only when it is true.
    /// `pivot` is the point the matrix was built around, recorded
    /// beside `"matrix"` only (never with `corners`), so a non-uniform
    /// `.relative` replay keeps the angle, scale and skew about it.
    static func transformLayers(matrix: CGAffineTransform, pivot: CGPoint, sourceRect: CGRect,
                                corners: [CGPoint]?, sampler: String,
                                rasterizeArtwork: Bool, duplicate: Bool = false) -> [ActionStep] {
        var arguments: [String: Any] = [
            "layers": ActionArgs.selectedLayers,
            "sampler": sampler,
        ]
        if duplicate { arguments["duplicate"] = true }
        if let corners {
            arguments["source_rect"] = [Double(sourceRect.minX), Double(sourceRect.minY),
                                        Double(sourceRect.width), Double(sourceRect.height)]
            arguments["corners"] = corners.map { [Double($0.x), Double($0.y)] }
            arguments["rasterize_artwork"] = rasterizeArtwork
        } else {
            arguments["matrix"] = [matrix.a, matrix.b, matrix.c,
                                   matrix.d, matrix.tx, matrix.ty].map(Double.init)
            arguments["pivot"] = [Double(pivot.x), Double(pivot.y)]
        }
        return step("transform_layers", arguments,
                    note: duplicate ? "Layer ▸ Free Transform a Copy" : "Layer ▸ Free Transform (layers)")
    }

    /// Layer ▸ Transform Again / Transform Again with Copy:
    /// `transform_again`, with `"duplicate": true` only when true. Replay
    /// repeats the REPLAY document's own memory. `rasterizeArtwork` adds
    /// `"rasterize_artwork": true` (only when true) for a replayed true
    /// perspective, as `transformLayers` does.
    /// `target` / `background`: a plane repeat's mask, channel or
    /// colour plane and its vacated-area hex (none for Alpha),
    /// recorded only for a plane.
    static func transformAgain(duplicate: Bool, rasterizeArtwork: Bool = false,
                               target: String? = nil,
                               background: String? = nil) -> [ActionStep] {
        var args: [String: Any] = duplicate ? ["duplicate": true] : [:]
        if rasterizeArtwork { args["rasterize_artwork"] = true }
        if let target { args["target"] = target }
        if let background { args["background"] = background }
        return step("transform_again", args,
                    note: duplicate ? "Layer ▸ Transform Again with Copy" : "Layer ▸ Transform Again")
    }

    static func transformLayer(_ transform: LayerTransform, sampler: String,
                               exact: Bool = false) -> [ActionStep] {
        let value: (CGFloat) -> Double = { exact ? Double($0) : ActionArgs.number($0) }
        return step(
            "transform_layer",
            [
                "layer": ActionArgs.activeLayer,
                "pivot_x": value(transform.pivot.x),
                "pivot_y": value(transform.pivot.y),
                "translate_x": value(transform.translation.dx),
                "translate_y": value(transform.translation.dy),
                "rotate": exact ? transform.degrees : ActionArgs.number(transform.degrees),
                "scale_x": value(transform.scaleX),
                "scale_y": value(transform.scaleY),
                "sampler": sampler,
            ],
            note: "Layer ▸ Free Transform")
    }

    /// `rasterizeArtwork` adds `"rasterize_artwork": true` (only when true)
    /// for a true perspective the user confirmed, so its replay is not
    /// refused, as `transformLayers` does.
    static func distortLayer(corners: [CGPoint], sampler: String,
                             exact: Bool = false, rasterizeArtwork: Bool = false) -> [ActionStep] {
        var arguments: [String: Any] = [
            "layer": ActionArgs.activeLayer,
            "corners": exact ? corners.map { [Double($0.x), Double($0.y)] }
                : ActionArgs.points(corners),
            "sampler": sampler,
        ]
        if rasterizeArtwork { arguments["rasterize_artwork"] = true }
        return step("distort_layer", arguments, note: "Layer ▸ Free Transform (perspective)")
    }
}

extension GuideOrientation {
    /// The agent's spelling — `add_guide`'s `orientation` argument, and the
    /// `$guide` symbol's. Here for the same reason `SelectionCombineMode`'s
    /// is: it is a fact about the catalog's vocabulary.
    var agentName: String { self == .horizontal ? "horizontal" : "vertical" }
}

extension SelectionCombineMode {
    /// The agent's spelling of a combine mode — the `mode` argument every
    /// `select_*`, `load_selection` and `save_selection` call takes.
    ///
    /// It lives here rather than on the enum because it is a fact about the
    /// CATALOG, not about the selection model, and the vocabulary file is
    /// where the catalog's spellings are reviewed in one pass.
    var agentName: String {
        switch self {
        case .replace: return "replace"
        case .add: return "add"
        case .subtract: return "subtract"
        case .intersect: return "intersect"
        }
    }
}

// MARK: - Argument spellings

/// The small shared spellings every factory above uses: the three symbol
/// forms, points, colours and numbers.
enum ActionArgs {
    /// "whatever layer is active on the replay document" — the default for
    /// the `layer`/`index` argument of 43 tools, and the one symbol that can
    /// never fail to resolve.
    static let activeLayer: [String: Any] = ["$layer": "active"]

    /// "the replay document's own layer selection" — for the nine tools that
    /// take a `layers` array.
    static let selectedLayers: [String: Any] = ["$layers": "selected"]

    /// A retouch gesture's sampling arguments, written EXPLICITLY, as every
    /// recorded argument is: the UI's defaults deliberately differ from the
    /// catalog's, so a step that leaned on a default would replay as
    /// something else.
    ///
    /// Both spellings are written whenever the mode has a truthful boolean.
    /// That is not belt-and-braces: it is what lets a step recorded here
    /// replay on a build that predates `sample`, which ignores unknown
    /// arguments and would otherwise fall back to its own default rather than
    /// to what the user actually did. The handlers accept the pair because
    /// they agree — only a CONTRADICTION is refused.
    static func sampling(
        _ sample: RetouchSampling, _ ignoreAdjustments: Bool
    ) -> [String: Any] {
        var out: [String: Any] = ["sample": sample.rawValue]
        if let legacy = sample.legacyAllLayers { out["sample_all_layers"] = legacy }
        // Written only when true: an older build ignores it either way, and a
        // false is the default everywhere, so leaving it out keeps a recorded
        // step to what the gesture actually asked for.
        if ignoreAdjustments { out["ignore_adjustments"] = true }
        return out
    }

    /// A layer the user picked explicitly, as the NAME AND ID pair
    /// (`ActionIdentity`): the id aims the step at the very layer that was
    /// pointed at while the session lasts, the name carries it to every other
    /// document — a batch's next file, a reopened one, someone else's
    /// machine — exactly as the 12A name-only form did.
    ///
    /// A nil ref (a layer that could not be read at record time) degrades to
    /// the active layer rather than recording a symbol that can never
    /// resolve. So does a ref with an empty name: a symbol whose fallback is
    /// unusable is worse than no symbol.
    static func layer(_ ref: LayerRef?) -> [String: Any] {
        guard let ref = ref, !ref.name.isEmpty else { return activeLayer }
        return ["$layer": ["name": ref.name, "id": ref.id, "session": ActionIdentity.session]]
    }

    /// A set of layers as the same pair each — the explicit form of
    /// `selectedLayers`, for a gesture that acted on entries the user picked
    /// rather than on whatever the replay document has selected.
    static func layers(_ refs: [LayerRef]) -> [Any] {
        refs.map { layer($0) }
    }

    /// A channel the user picked, as the same pair. `nil` — a channel that
    /// could not be read at record time — degrades to the INDEX it sat at,
    /// which `AgentServer.channelArgument` accepts and which is at least
    /// truthful about this document.
    static func channel(_ ref: ChannelRef?, at index: Int) -> Any {
        guard let ref = ref, !ref.name.isEmpty else { return index }
        return ["$channel": ["name": ref.name, "id": ref.id, "session": ActionIdentity.session]]
    }

    /// An Apply Image / Calculations source: "merged", or the layer by
    /// reference (the tools read an integer, and a `$layer` symbol resolves
    /// to one).
    static func sourceLayer(_ index: Int?, in doc: RasterDocument) -> Any {
        guard let index = index else { return "merged" }
        return layer(doc.layerRef(index))
    }

    /// A source-plane choice as `apply_image` / `calculations` spell it.
    static func planeChoice(_ choice: PlaneChoice, in doc: RasterDocument) -> String {
        // #1 Lab phase 6b-i: "lab", "lightness", "a", "b", "alpha" or a channel's name.
        if doc.isLab { return LabDocument.planeChoiceName(choice, in: doc) }
        switch choice {
        // #1: a CMYK document's composite is spelled "cmyk", its plates by their keys.
        // #1 phase 6b: a Grayscale document's composite or layer is spelled "gray".
        case .rgb: return doc.isCmyk ? "cmyk" : doc.isGray ? "gray" : "rgb"
        case .plane(let plane): return plane.agentName
        case .channel(let index): return doc.channelInfo(index)?.name ?? (doc.isCmyk ? "cmyk" : doc.isGray ? "gray" : "rgb")
        case .ink(let plate): return plate.key
        }
    }

    static func guide(_ orientation: String, _ position: Double) -> [String: Any] {
        ["$guide": ["orientation": orientation, "position": number(position)]]
    }

    /// A colour as the AUTHORED sRGB hex both `AgentServer.parseColor` and
    /// the app's colour wells mean — never the document's own numbers, which
    /// are what the same colour looks like in whatever profile is assigned.
    static func hex(_ color: NSColor) -> String {
        let srgb = ColorProfile.converted(color, into: .sRGB) ?? color
        let byte: (CGFloat) -> UInt8 = { UInt8(max(0, min(255, ($0 * 255).rounded()))) }
        return RasterImage.hexString(
            (
                r: byte(srgb.redComponent), g: byte(srgb.greenComponent),
                b: byte(srgb.blueComponent), a: byte(srgb.alphaComponent)
            ))
    }

    /// A colour as the hex the DOCUMENT's own bytes spell — the numbers the
    /// tool itself paints (`ColorProfile.bytes`) — for the profile-qualified
    /// `color_space: "document"` form a wide-gamut recording carries.
    ///
    /// nil on an sRGB document, deliberately: there the authored hex already
    /// IS the document's spelling, so an sRGB recording stays byte-for-byte
    /// what it has always been and the new argument never appears in one. Also
    /// nil when ColorSync refuses the conversion, which leaves the caller with
    /// today's authored `hex` rather than a made-up colour.
    static func documentHex(_ color: NSColor, in space: NSColorSpace) -> String? {
        guard space != .sRGB, let bytes = ColorProfile.bytes(color, in: space) else {
            return nil
        }
        return RasterImage.hexString(
            (r: bytes[0], g: bytes[1], b: bytes[2], a: bytes[3]))
    }

    /// A canvas polyline as the `[[x, y], …]` every stroke tool parses.
    static func points(_ points: [CGPoint]) -> [[Double]] {
        points.map { [number($0.x), number($0.y)] }
    }

    /// A coordinate or amount, rounded to 4 decimals.
    ///
    /// Two reasons, both about the FILE rather than the pixels: a raw
    /// `CGFloat` writes 17 significant digits into the JSON, which makes a
    /// recorded stroke unreadable in the editor and enormous on disk; and a
    /// quarter of a thousandth of a pixel is far below anything a gesture or
    /// a slider can mean. It is the same quantization the layer-style writer
    /// applies for the same reason.
    static func number<T: BinaryFloatingPoint>(_ value: T) -> Double {
        let double = Double(value)
        guard double.isFinite else { return 0 }
        return (double * 10000).rounded() / 10000
    }
}

// MARK: - Facts about the catalog the Actions feature needs

/// The three small classifications of catalog tools that the recorder, the
/// player and the menus ask for, plus the one-line display name the Actions
/// window and the Repeat Last menu item show.
///
/// Each set is built from the `ToolFacts` entry beside each tool's handler
/// (`<prefix>ToolFacts`, merged in `AgentServer+Registry.swift`), so a new tool
/// states its facts in its own file.
enum ActionCatalogFacts {
    /// What Filters ▸ Repeat Last (⌃F) can repeat: the destructive filter
    /// and adjustment family, and nothing else.
    ///
    /// Adjustment LAYERS are excluded on purpose — they are non-destructive
    /// and they stack, so "repeat" for them would mean "make a second one",
    /// which is not what the command promises.
    /// Built from every `.repeatable` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let repeatableTools: Set<String> = AgentServer.tools(with: .repeatable)

    /// Repeat Last re-aims every destructive filter/auto command at the
    /// current target. Analysis reads that whole target; writes use selection.
    static let targetedRepeatableTools = repeatableTools

    /// Tools a recording deliberately drops: they read, they undo, they
    /// write a file, they OPEN one, or they are the Actions machinery
    /// itself. Recording any of them would put a step in the action that
    /// changes no picture — and recording `run_action` inside a recording
    /// would nest a replay.
    ///
    /// `open_document` is here because a step that CREATES a document is
    /// meaningless in an action: an action is by definition replayed on a
    /// document that already exists and, in Batch, on a different file every
    /// time. A session that began by opening a file — the normal shape, and
    /// the one `start_recording`'s own catalog text advertises — otherwise
    /// baked that absolute path in as step 1, so every replay popped the
    /// recorded file open as a second visible document beside the run's real
    /// target, and every Batch file did it again; once the recorded file had
    /// moved, step 1 threw and the default `on_error: "stop"` failed all 200
    /// files without writing anything. A deliberate open inside an action is
    /// still possible — `Action.decode` accepts the step — it just may not
    /// arrive as a recording artefact.
    /// Built from every `.nonRecordable` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let nonRecordable: Set<String> = AgentServer.tools(with: .nonRecordable)

    /// ADVISORY only, and never a failure: tools whose result depends on
    /// there being a selection. The Actions window annotates these rows so a
    /// reader can see why a step behaved differently on another document.
    ///
    /// It is not a pre-check. The four tools that genuinely REQUIRE a
    /// selection — `clear_selection`, `modify_selection`, `save_selection`,
    /// `content_aware_fill` — and `stroke_selection` on a mask, colour plane
    /// or alpha channel (with nothing selected it outlines a layer's own
    /// pixels) already throw their own in-band error with a better
    /// message than the player could invent, and that error becomes the
    /// step's failure through the ordinary path. `layer_via_copy` and
    /// `layer_via_cut` are here for the opposite reason: with no selection
    /// they copy the WHOLE layer and say so in a note, so annotating them is
    /// the only warning a reader gets.
    /// Built from every `.selectionSensitive` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let selectionSensitive: Set<String> = AgentServer.tools(with: .selectionSensitive)

    /// Whether a call bills the user's cloud key though its tool is recordable: a `crop` with
    /// `fill: "generative_expand"`. A recording drops it, as it drops
    /// `generative_expand` itself, because a replay would bill the key again.
    static func billsCloud(_ tool: String, _ arguments: [String: Any]) -> Bool {
        tool == "crop" && arguments["fill"] as? String == CropFill.generativeExpand.rawValue
    }

    /// The Actions window's word for WHAT a `paste` step pastes: the
    /// clipboard at replay, or the payload the step saved. nil for
    /// every other tool.
    static func pasteSource(tool: String, arguments: [String: Any]) -> String? {
        guard tool == "paste" else { return nil }
        return arguments["payload_path"] == nil
            ? "pastes the clipboard at replay" : "pastes its saved payload"
    }

    /// A one-line human name for a step: the tool, plus the one argument
    /// that says WHICH one it is. Shown in the Actions window's rows and in
    /// the Repeat Last menu item's title.
    static func displayName(for tool: String, arguments: [String: Any]) -> String {
        if tool == ActionStep.unrecordedTool {
            return arguments["action"] as? String ?? "an unrecorded command"
        }
        // The tools whose identity lives in an argument rather than in their
        // name — a bare "apply_filter" tells a reader nothing.
        let discriminator: String?
        switch tool {
        case "apply_filter": discriminator = arguments["filter"] as? String
        case "add_adjustment_layer", "edit_adjustment_layer": discriminator =
            arguments["op"] as? String
        case "modify_selection": discriminator = (arguments["operation"] as? String)
            .map { SelectionMorph.displayName(for: $0).lowercased() }
        case "rotate": discriminator = (arguments["degrees"] as? NSNumber).map { "\($0)°" }
        case "flip", "arrange_layer": discriminator =
            (arguments["axis"] ?? arguments["to"]) as? String
        case "quick_export": discriminator = arguments["preset"] as? String
        case "export_layers": discriminator = arguments["items"] as? String
        case "contact_sheet": discriminator = (arguments["output"] as? [String: Any])?["kind"] as? String
        default: discriminator = nil
        }
        let title = tool.replacingOccurrences(of: "_", with: " ").capitalized
        guard let discriminator = discriminator else { return title }
        return "\(title): \(discriminator.replacingOccurrences(of: "_", with: " "))"
    }
}
