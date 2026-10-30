import AppKit

// A tool whose handler never resolves a target document carries `.documentFree` in its handler
// file's `<prefix>ToolFacts` (so it is in `DocumentPrecision.documentFreeTools`), which
// `refusal(tool:…)` answers nil for first, so it answers at every depth by construction.
// `refusal` asks `isDocumentFree(tool:arguments:)`, which also admits `save_raw_preset` given
// `raw` — the handler's own document-free branch — at every depth, while the tool stays in
// `commonTools` for its document-reading form; and the depth tables list only tools that read a
// document, so the library, job and document-list tools live in `documentFreeTools` alone, and so
// do `new_document`, `get_clipboard`, `new_from_clipboard`, the action library and recorder
// (`list_actions`, `save_action`, `delete_action`, `start_recording`, `stop_recording`) and the
// batch and bundle tools (`run_batch`, `batch_status`, `cancel_batch`, `package_action`,
// `import_action`) — `run_action` reads its target and stays in `commonTools`.
//
// The one part that is easy to get wrong: a family added to `coreRefusal`/`floatCoreRefusal`'s
// `families` map must ALSO be added to the INDEX-MAPPED branch beside it, or `targets` falls to
// `["document"]` and the core refuses every call for a reason the user cannot act on;
// `targetIndices` reads `from` as well as `layers`/`layer`/`index`, because `reorder_layer` names
// the entry it moves that way.
//
// **12F-1b** admitted the EXACT canvas ops at 32-bit float: `crop`, `rotate` and `flip` join
// `FloatDocument.tools` and `floatCoreRefusal`'s `families` map (the DEFAULT `["document"]` branch,
// like `canvas_size`), and
// `rotateCW:`/`rotateCCW:`/`rotate180:`/`flipH:`/`flipV:`/`cropToSelection:` join the FLOAT
// selector whitelist in `EditorViewController+Precision.swift` — `resizeImage:` and
// `freeTransform:` stay out. `RasterDocument.cropped`, `rotated90/180/270` and `flippedH/V` already
// branch on `isHighDepth`, which a float document is, so no wrapper moved. The one thing that needs
// care is that the family table CANNOT tell a straighten from a crop — both are `geometry_crop`,
// and only the ANGLE says whether a sample is resampled — so `DocumentPrecision.cropAngle` is the
// ONE parser (0 when absent, `.nan` when unreadable), and the float branch answers
// `FloatDocument.straightenRefusal` BEFORE the family table.
// (A later change DELETED that refusal, `rotateCanvasRefusal` and the float `transform_layers`
// integer-translation check: `DocumentPrecision.geometryFamily` names every geometry tool's
// family — the angle or the request shape picks it — for both `coreRefusal` and `floatCoreRefusal`;
// the filtered tools, `resizeImage:`/`rotateArbitrary:`/`freeTransform:`/`transformMask:`/`freeTransformCopy:`/`transformAgain:`/`transformAgainWithCopy:`
// and the Crop tool join the float tables. Float `transform_again` and the four
// transform items with a selection transform the selected layer pixels too; only a colour-plane
// target refuses.)
//
// `new_layer` joins `DocumentPrecision.typedTools` and `FloatDocument.tools` and maps to family
// `layer_add` targeting `document` in BOTH `coreRefusal` and `floatCoreRefusal` (the DEFAULT
// branch, not the index-mapped one). `place_image` is admitted at float, and
// so are the artwork tools: the Type and Shape tools are in the float `allows` arm and
// `add_text_layer`/`add_shape_layer` ask family `rasterize` (document) in `floatCoreRefusal` too.

/// The operation/target/depth gate shared by menus, MCP and Actions. A typed
/// owner never reaches a byte edit merely because that method returns nil.
enum DocumentPrecision {
    static let limitation = "16-bit editing supports " + NativeLayerBlending.modeNames.joined(separator: ", ") + " raster layers, adjustment layers and isolated groups, plus pass-through groups, "
        + "tone/color adjustments, masks, clipping masks, layer styles, geometry and all listed paint blending modes. Other layer/group blend "
        + "modes are unavailable. Convert a flattened "
        + "copy to 8-bit to use an unsupported operation; conversion is explicit and undoable."

    /// Built from every `.commonTool` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let commonTools: Set<String> = AgentServer.tools(with: .commonTool)
    /// A handler that never resolves a target document answers at every
    /// depth. Such a tool belongs here, never in `commonTools`, `typedTools`
    /// or `FloatDocument.tools`; `refusal(tool:…)` returns nil for
    /// it before any other gate, through `isDocumentFree`, which also admits
    /// the argument-dependent document-free branch of a tool that stays
    /// in `commonTools`.
    /// Built from every `.documentFree` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let documentFreeTools: Set<String> = AgentServer.tools(with: .documentFree)

    /// True when THESE arguments reach a handler branch that resolves no
    /// document: every `documentFreeTools` member, plus
    /// `save_raw_preset` given authored `raw` settings — the handler's own
    /// branch test. The tool stays in `commonTools` for its
    /// document-reading form.
    static func isDocumentFree(tool: String, arguments: [String: Any]) -> Bool {
        documentFreeTools.contains(tool)
            || (tool == "save_raw_preset" && arguments["raw"] != nil)
    }
    /// Built from every `.typedTool` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let typedTools: Set<String> = AgentServer.tools(with: .typedTool)
    /// Built from every `.rasterTool` entry of the `<prefix>ToolFacts` tables (AgentServer+Registry.swift).
    static let rasterTools: Set<String> = AgentServer.tools(with: .rasterTool)

    /// `crop`'s straighten angle: 0 when the caller named none, and `.nan`
    /// when a value is present but unreadable — so an unparseable angle is
    /// never read as "no straighten" by either depth's gate, nor by the
    /// `crop` handler itself (`AgentServer+Retouch.crop`, Phase 12F-1b2),
    /// which reads this ONE parser too and so refuses a malformed `angle` by
    /// name at 8 and at 16 bits.
    static func cropAngle(_ arguments: [String: Any]) -> Double {
        guard let angle = arguments["angle"] else { return 0 }
        return (angle as? NSNumber)?.doubleValue
            ?? (angle as? String).flatMap(Double.init)
            ?? .nan
    }

    /// `rotate`'s angle, and the ONE parser for it (Phase 12F-1b3): 90 when
    /// the caller named none — today's default, so an argument-less `rotate`
    /// is unchanged — and `.nan` when a value is present but unreadable.
    ///
    /// Both depth gates read this and so does the handler, which is what lets
    /// a malformed `degrees` be refused ONCE, by name, at 8 bits, 16 bits and
    /// float alike. It is a DISCLOSED behaviour change: `degrees: "abc"`
    /// used to be read as 90 and rotate the canvas silently
    /// (`intArg(a, "degrees") ?? 90`), and `90.7` used to truncate to 90.
    static func rotateDegrees(_ arguments: [String: Any]) -> Double {
        guard let degrees = arguments["degrees"] else { return 90 }
        return (degrees as? NSNumber)?.doubleValue
            ?? (degrees as? String).flatMap(Double.init)
            ?? .nan
    }

    /// The tools whose colour argument `color_space` qualifies
    /// (`AgentServer+QualifiedColor.swift`), at every depth the tool itself
    /// runs at (`add_text` is 8-bit only).
    static let colorSpaceTools = [
        "brush_stroke", "fill", "gradient", "fill_selection", "stroke_selection", "add_text", "put_pixels",
    ]
    /// Tools whose own `color_space` means something else and which answer
    /// it with their own sentence (`refine_selection`'s colour-aware kind),
    /// and the two path tools, which hand the argument to the commit
    /// they share: `fill_path` to `stroke_selection`'s band commit and
    /// `stroke_path` to `brush_stroke`, each asked again under that name.
    static let colorSpaceExempt: Set<String> = ["refine_selection", "fill_path", "stroke_path"]
    static let colorSpaceLimitation: String = {
        let tools = colorSpaceTools.dropLast().joined(separator: ", ")
            + " and " + (colorSpaceTools.last ?? "")
        return "color_space qualifies a colour argument on \(tools) only. Text and shape "
            + "layer colours are continuous sRGB numbers (linear light at 32 bits): to reuse a sampled colour there, pass "
            + "sample_color's srgb_rgba as rgba, fill_rgba or stroke_rgba. Layer-style and "
            + "channel overlay colours are authored sRGB hex (sample_color's paint_hex at 8 "
            + "bits). Adjustment colours are already the document's own numbers."
    }()

    /// The ONE `color_space` gate, asked in the float, 8-bit and
    /// 16-bit branches alike: nil when the argument is absent or the tool
    /// takes it (or is exempt), otherwise the refusal by name — so a
    /// `color_space` is never silently ignored at any depth.
    static func colorSpaceGate(tool: String, arguments: [String: Any]) -> String? {
        guard arguments["color_space"] != nil, !colorSpaceTools.contains(tool),
              !colorSpaceExempt.contains(tool) else { return nil }
        return colorSpaceLimitation
    }

    /// Whether a call edits the Quick Mask's coverage (`refusal` below):
    /// `fill_selection`, `gradient` or `apply_filter` blur while in Quick Mask with no target
    /// (`fill_selection` whatever its target), or pinned to it by `target: "quick_mask"`.
    static func quickMaskCoverageEdit(tool: String, arguments: [String: Any],
                                      document: ImageDocument) -> Bool {
        let quickMaskEdit = tool == "fill_selection" || tool == "gradient"
            || (tool == "apply_filter" && arguments["filter"] as? String == "blur")
        return quickMaskEdit
            && ((document.quickMaskActive && (arguments["target"] == nil || tool == "fill_selection"))
                || arguments["target"] as? String == "quick_mask")
    }

    static func refusal(tool: String, arguments: [String: Any] = [:], document: ImageDocument) -> String? {
        if isDocumentFree(tool: tool, arguments: arguments) { return nil }
        if let reason = RetainedImagePolicy.refusal(tool: tool, arguments: arguments, document: document) { return reason }
        guard let doc = document.doc else { return nil }
        // #547 Bitmap phase 1a: a Bitmap document's own closed table (`BitmapDocument`), asked
        // first of all.
        if doc.isBitmap { return BitmapDocument.refusal(tool: tool, arguments: arguments, in: doc) }
        // #547 Duotone phase 1a: `get_duotone`/`set_duotone` answer on a Duotone document only, and
        // a Duotone document's own refusals come before Gray's arm, which then answers unchanged.
        if DuotoneDocument.tools.contains(tool) {
            return doc.isDuotone ? nil : DuotoneDocument.notDuotoneRefusal(tool: tool)
        }
        if doc.isDuotone, let reason = DuotoneDocument.refusal(tool: tool, arguments: arguments) {
            return reason
        }
        // #547 Indexed phase 1a: an Indexed Color document's own closed table (`IndexedDocument`),
        // asked first.
        if doc.isIndexed { return IndexedDocument.refusal(tool: tool, arguments: arguments) }
        // #547 Lab phase 1: a Lab Color document's own table (`LabDocument`), asked first; since
        // phase 2a its paint and adjustment tools (`depthRuled`) then answer the 16-bit rules.
        if doc.isLab {
            if let reason = LabDocument.refusal(tool: tool, arguments: arguments,
                                                depth: doc.labDepth) { return reason }
            guard LabDocument.depthRuled.contains(tool) else { return nil }
        }
        // #547 phase 1a: a Grayscale document's own table (`GrayDocument`), asked first; since
        // phase 2a-ii its paint and adjustment tools (`depthRuled`) then answer the depth's rules.
        if doc.isGray {
            if let reason = GrayDocument.refusal(tool: tool, arguments: arguments, document: doc) {
                return reason
            }
            guard GrayDocument.depthRuled.contains(tool) else { return nil }
        }
        // #547: a CMYK document's own table (`CmykDocument`), asked FIRST — after Quick Mask's
        // coverage edits below (`quickMaskCoverageEdit`), which are colour-free on CMYK too.
        if doc.isCmyk {
            if quickMaskCoverageEdit(tool: tool, arguments: arguments, document: document) { return nil }
            return CmykDocument.refusal(tool: tool, arguments: arguments,
                                        depth: CmykAdjustFilter.depth(of: doc))
        }
        if let reason = CmykDocument.rgbDocumentRefusal(tool: tool, arguments: arguments) { return reason }
        if let reason = FloatPaint.refusal(tool: tool, arguments: arguments, document: document) { return reason }
        if let reason = FloatRetouch.refusal(tool: tool, arguments: arguments, document: document) { return reason }
        // In Quick Mask (or pinned to it by `target:
        // "quick_mask"`, whose handler refuses by name outside Quick Mask)
        // `fill_selection` fills the Quick Mask's native u16 coverage, not
        // document pixels, at 16 bits and at 32-bit float alike. Asked
        // before the float table, whose `fill_stroke` capability check
        // asks about the layer, not the Quick Mask.
        // `gradient` and `apply_filter filter: "blur"` edit the same
        // coverage there (AgentServer+QuickMaskEdits.swift), under the same
        // rule: no target while in Quick Mask, or `target: "quick_mask"`.
        if doc.isHighDepth, quickMaskCoverageEdit(tool: tool, arguments: arguments, document: document) {
            return nil
        }
        // Phase 11F: a 32-bit FLOAT document has its own, narrower table
        // (`FloatDocument.swift`). It is asked FIRST, so a tool outside the
        // set never reaches the u16 gate's wording.
        if doc.isFloat {
            guard FloatDocument.tools.contains(tool) else {
                return FloatDocument.refusal(tool: tool)
            }
            if let reason = colorSpaceGate(tool: tool, arguments: arguments) { return reason }
            if tool == "set_layer_properties", let blend = arguments["blend_mode"] as? String {
                let index = integer(arguments["index"]) ?? integer(arguments["layer"])
                    ?? document.activeLayerIndex
                guard let mode = NativeLayerBlending.mode(named: blend),
                      NativeLayerBlending.allows(mode, document: doc, layer: index) else {
                    return NativeLayerBlending.floatLimitation
                }
            }
            // Only the core's `*_apply` adjustments, on a
            // layer's own pixels; the core refuses an op by name besides.
            // Or a filter of `planeFilterOps` on a mask, an alpha
            // channel or one colour channel.
            if tool == "apply_filter" {
                let filter = arguments["filter"] as? String ?? ""
                let target = arguments["target"] as? String ?? "layer"
                let onLayer = target == "layer" && FloatDocument.appliesDestructively(filter)
                let onPlane = target != "layer" && selectedPlaneFamily(arguments) != nil
                    && FloatDocument.planeFilterOps.contains(filter)
                guard onLayer || onPlane, !document.quickMaskActive else {
                    return FloatDocument.applyRefusal(
                        filter, target: target, quickMask: document.quickMaskActive)
                }
            }
            // Liquify moves a layer's own pixels, never the
            // Quick Mask's coverage.
            if tool == "liquify", document.quickMaskActive {
                return OperationScope.quickMaskRefusal
            }
            return floatCoreRefusal(tool: tool, arguments: arguments, document: document)
        }
        guard doc.isHighDepth else {
            if arguments["start_rgba"] != nil || arguments["end_rgba"] != nil || arguments["fill_mode"] != nil {
                return "Native numeric gradient and fill_mode arguments require a 16-bit document."
            }
            if ["brush_stroke", "fill"].contains(tool), arguments["rgba"] != nil {
                return "Numeric rgba paint is a 16-bit operation. Convert the document explicitly; 8-bit replay never substitutes a hex color."
            }
            // Phase 12A: `color_space` QUALIFIES A HEX colour at either depth
            // on the four colour tools (AgentServer+QualifiedColor.swift), so
            // a wide-gamut sample paints back exactly. The named value itself
            // is validated there; numeric ink stays 16-bit-only above.
            return colorSpaceGate(tool: tool, arguments: arguments)
        }
        guard commonTools.contains(tool) || typedTools.contains(tool) else {
            return "\(tool) is unavailable in 16-bit mode. " + limitation
        }
        if let reason = colorSpaceGate(tool: tool, arguments: arguments) { return reason }
        // The Art History Brush answers its target, its Mode and the depth itself, in one
        // sentence each at every depth (AgentServer+ArtHistoryBrush.swift, the core's plan).
        if tool == "art_history_brush_stroke" { return nil }
        if rasterTools.contains(tool) {
            let target = (arguments["target"] as? String) ?? "layer"
            if target != "layer" {
                // One sentence at every depth (`AgentServer.sampledTargetRefusal`).
                if let refusal = AgentServer.sampledTargetRefusal(arguments) { return refusal }
                guard ["brush_stroke", "eraser_stroke", "fill", "gradient", "clear_selection", "apply_filter", "auto_tone", "auto_contrast", "auto_color", "fill_selection", "stroke_selection"].contains(tool) else {
                    return scalarTargetRefusal(tool)
                }
                return nil // Scalar request validation resolves its exact plane and raw extent.
            }
            if document.quickMaskActive { return OperationScope.quickMaskRefusal }
            let index = integer(arguments["layer"]) ?? integer(arguments["index"]) ?? document.activeLayerIndex
            if ["brush_stroke", "eraser_stroke"].contains(tool), doc.layerIsAdjustment(index), doc.layerHasMask(index) { return nil }
            guard let info = doc.layerInfo(index), !info.isGroup, !doc.layerIsAdjustment(index) else {
                return "\(tool) requires a raster layer in 16-bit mode."
            }
            if tool == "apply_filter", let operation = arguments["filter"] as? String,
               AdjustmentLayerOp(rawValue: operation == "adjust" ? "bcs" : operation) == nil,
               !SelectedAdjustment.spatial.contains(operation) {
                return "\(operation) has no qualified 16-bit implementation. " + limitation
            }
            // The brush, the bucket (`bucket_fill`'s own `blend_mode`) and
            // Fill…/Stroke… blend through `paint_blend`'s modes; the Gradient
            // through `gradient`'s and the Clone Stamp through
            // `retouch_clone`'s.
            if !["brush_stroke", "fill", "fill_selection", "stroke_selection", "gradient", "clone_stamp"].contains(tool),
               let blend = arguments["blend_mode"] as? String,
               blend.caseInsensitiveCompare("Normal") != .orderedSame {
                return "16-bit retouch supports Normal blending only."
            }

        }
        if tool == "set_layer_properties", let blend = arguments["blend_mode"] as? String {
            let index = integer(arguments["index"]) ?? integer(arguments["layer"]) ?? document.activeLayerIndex
            guard let mode = NativeLayerBlending.mode(named: blend),
                  NativeLayerBlending.allows(mode, document: doc, layer: index) else {
                return NativeLayerBlending.limitation
            }
        }
        return coreRefusal(tool: tool, arguments: arguments, document: document)
    }

    /// The sentence a raster tool that edits layer RGB only carries for a
    /// mask, channel or colour-plane `target`, at 16 bits and (`FloatRetouch`)
    /// at 32-bit float.
    static func scalarTargetRefusal(_ tool: String) -> String {
        "\(tool) requires layer RGB; this operation does not edit scalar targets."
    }

    /// The float twin: the SAME family table, asked at depth 32, so the
    /// core's own `precision_capability` decides and the host never writes a
    /// second copy of the admitted set.
    static func floatCoreRefusal(tool: String, arguments: [String: Any],
                                 document: ImageDocument) -> String? {
        guard let doc = document.doc else { return "Document has no image." }
        // `copy` mode auto asks about the mode it resolves to.
        let arguments = LayerClipboard.resolvingAutoCopy(tool: tool, arguments, in: document)
        // The geometry tools name their family through `geometryFamily`
        // (the angle or the request shape picks it), so they are not here.
        let families: [String: String] = [
            "duplicate_layer": "layer_duplicate", "group_layers": "layer_group",
            "duplicate_to_new_document": "layer_duplicate",
            "reorder_layer": "layer_move", "reorder_layers": "layer_move",
            "ungroup_layers": "layer_ungroup", "delete_layer": "layer_remove",
            "merge_down": "layer_merge", "merge_visible": "layer_merge_visible",
            "set_layer_lock": "layer_lock", "link_layers": "layer_link",
            "unlink_layers": "layer_link", "new_layer": "layer_add",
            "stamp_visible": "layer_stamp", "flatten_image": "layer_flatten",
            // #547 phase 4c: Stamp Selected and Merge Group.
            "stamp_selected": "layer_stamp_selected", "merge_group": "layer_merge",
            // Arrange was missing here while its UI items and typed op worked.
            "arrange_layer": "layer_move",
            // Layer Via Copy on a float layer's f32 pixels.
            "layer_via_copy": "layer_via",
            // Layer Via Cut, the copy then the float Cut's erase.
            "layer_via_cut": "layer_via",
            // Assign Profile relabels the linear samples (document-targeted).
            "assign_profile": "color_assign",
            // Convert to Profile in linear light with no clamp (document-targeted).
            "convert_profile": "color_convert",
            "set_layer_properties": "layer_opacity", "set_layer_clipped": "layer_clipped",
            "add_adjustment_layer": "adjustment_add", "edit_adjustment_layer": "adjustment_set",
            "add_layer_mask": "mask_set", "remove_layer_mask": "mask_remove",
            "set_layer_mask_enabled": "mask_properties",
            "get_mask_properties": "mask_properties", "set_mask_properties": "mask_properties",
            "save_selection": "channel_set", "delete_channel": "channel_remove",
            "sample_color": "sample", "sample_pixel": "sample", "histogram": "histogram",
            "render": "render", "save_copy": "export", "convert_precision": "convert",
            // Phase 12G-1: the typed clipboard. `copy`/`cut` name a record;
            // `copy_merged`/`paste` target the document.
            "copy": "clipboard_copy", "cut": "clipboard_cut",
            "copy_merged": "clipboard_copy_merged", "paste": "clipboard_paste",
            // Fill… / Stroke… on a layer. A mask, channel or colour-plane
            // target is the handler's to decide (a colour plane is the core's
            // `paint_blend` `plane`).
            "fill_selection": "fill_stroke", "stroke_selection": "fill_stroke",
            // Layer ▸ Warp on a layer's pixels or its mask.
            "warp_layer": "geometry_warp",
            // Edit ▸ Puppet Warp on a layer's pixels (or its float).
            "puppet_warp": "geometry_puppet",
            // Edit ▸ Perspective Warp on a layer's pixels (or its float).
            "perspective_warp": "geometry_perspective_warp",
            // The selected area of a mask or channel; a layer target
            // asks `raster`, which float admits.
            "transform_selected_pixels": "selected_pixels_transform",
            // The Move of the same planes; a layer target asks
            // `raster`, which float admits.
            "move_selected_pixels": "selected_pixels_move",
            // A style on a raster or isolated group, and the
            // document's light.
            "set_layer_style": "layer_style", "set_global_light": "global_light",
            // Image ▸ Adjustments on a layer's pixels.
            "apply_filter": "adjustment_apply",
            // Liquify on a layer's pixels asks the same family.
            "liquify": "adjustment_apply",
            // Depth Blur on a layer's pixels asks the same family.
            "depth_blur": "adjustment_apply",
            // A new text or shape layer (document-targeted).
            "add_text_layer": "rasterize", "add_shape_layer": "rasterize",
            // A new fill layer renders through the same family.
            "add_fill_layer": "rasterize",
            // Stack Modes and Reduce Noise from Stack read float layers.
            "stack_mode": "layer_stack_mode", "reduce_noise_from_stack": "layer_stack_mode",
        ]
        if ["fill_selection", "stroke_selection"].contains(tool),
           let target = arguments["target"] as? String, target != "layer" {
            return nil
        }
        // A filter on a mask, channel or colour plane is the core's
        // `scalar_edit` to decide (`float_plane_filter`), refusing by name.
        if tool == "apply_filter", let target = arguments["target"] as? String, target != "layer" {
            return nil
        }
        guard let operation = geometryFamily(tool: tool, arguments: arguments, doc: doc)
                ?? layerCopyFamily(tool: tool, arguments: arguments)
                ?? families[tool] else { return nil }
        var targets: [String]
        if operation == "geometry_warp" {
            targets = [arguments["target"] as? String == "mask" ? "mask" : "raster"]
        }
        else if ["selected_pixels_transform", "selected_pixels_move"].contains(operation) {
            // A mask, channel or colour-plane target asks
            // its own row.
            targets = [selectedPlaneFamily(arguments) ?? "raster"]
        }
        else if ["mask_remove", "mask_properties", "mask_transform"].contains(operation) {
            targets = ["mask"]
        }
        else if operation == "channel_remove" { targets = ["channel"] }
        else if ["layer_duplicate", "layer_opacity", "layer_clipped", "mask_set",
                 "layer_move", "layer_group", "layer_ungroup", "layer_remove",
                 "layer_merge", "layer_lock", "layer_link", "clipboard_copy", "clipboard_cut",
                 // #547 phase 4c: Stamp Selected names its members.
                 "layer_stamp_selected",
                 // Layer Via names the layer it copies.
                 "layer_via",
                 // A Stack Mode names the layers it reads.
                 "layer_stack_mode",
                 "geometry_affine_set", "geometry_perspective_set", "fill_stroke",
                 "layer_style", "adjustment_apply", "geometry_puppet",
                 "geometry_perspective_warp"]
            .contains(operation) {
            targets = targetIndices(arguments, document, operation: operation).map {
                doc.layerIsGroup($0) ? "group"
                    : doc.layerIsAdjustment($0) ? "adjustment" : "raster"
            }
        } else { targets = ["document"] }
        wholeAdjustmentCopy(operation, document: document, targets: &targets)
        do {
            for target in targets {
                let report = try RasterDocument.precisionCapability(
                    operation: operation, target: target, depth: UInt32(32))
                guard report["supported"] as? Bool == true else {
                    return report["reason"] as? String ?? FloatDocument.refusal(tool: tool)
                }
            }
            return nil
        } catch {
            return "Could not validate the 32-bit float operation capability: "
                + error.localizedDescription
        }
    }

    /// `copy` with `mode: "layers"` copies whole layers — Duplicate
    /// to New Document's one core call — so it asks `layer_duplicate` per
    /// target, as `duplicate_to_new_document` does. nil for every other call.
    /// `mode: "auto"` reaches here already resolved
    /// (`LayerClipboard.resolvingAutoCopy`, both callers).
    static func layerCopyFamily(tool: String, arguments: [String: Any]) -> String? {
        // `copy_layers_to_document` copies whole layers out of its
        // source document the same way.
        tool == "copy_layers_to_document"
            || (tool == "copy" && arguments["mode"] as? String == "layers") ? "layer_duplicate" : nil
    }

    /// The capability family a GEOMETRY tool asks, at either depth. The
    /// family table cannot tell a straighten from a crop (both `crop`), an
    /// arbitrary rotation from a quarter turn (both `rotate`) or a
    /// perspective `transform_layers` from an affine one, so the ANGLE or the
    /// request shape picks the family here. The CORE classifies a rotation
    /// (`CanvasRotation.isFiltered` reads the plan export), so the host
    /// writes no rule; an unreadable `degrees` asks `geometry_rotate` and is
    /// refused once, by the handler (`CanvasRotation.unreadable`). nil for a
    /// tool that is not geometry.
    static func geometryFamily(tool: String, arguments: [String: Any],
                               doc: RasterDocument) -> String? {
        switch tool {
        case "crop": return cropAngle(arguments) != 0 ? "geometry_straighten" : "geometry_crop"
        case "rotate":
            return CanvasRotation.isFiltered(arguments, in: doc)
                ? "geometry_rotate_canvas" : "geometry_rotate"
        case "flip": return "geometry_flip"
        case "canvas_size": return "geometry_canvas_size"
        case "image_size": return "geometry_resize"
        case "transform_layer": return "geometry_affine_set"
        case "transform_layers":
            return arguments["matrix"] == nil ? "geometry_perspective_set" : "geometry_affine_set"
        case "distort_layer": return "geometry_perspective_set"
        // Auto-Align warps each layer through `perspective_set`.
        case "auto_align_layers": return "geometry_perspective_set"
        case "transform_mask": return "mask_transform"
        // Straighten Layer is `transform_layers`' affine set.
        case "straighten_layers": return "geometry_affine_set"
        // Trim commits a crop, Reveal All a canvas resize, and
        // Perspective Crop is a filtered whole-canvas resample like a straighten.
        case "trim": return "geometry_crop"
        case "reveal_canvas": return "geometry_canvas_size"
        case "perspective_crop": return "geometry_straighten"
        default: return nil
        }
    }

    /// UI tool names map onto one core-owned qualified family table. Record
    /// classes are read from the same immutable facade the operation captures.
    static func coreRefusal(tool: String, arguments: [String: Any], document: ImageDocument) -> String? {
        guard let doc = document.doc else { return "Document has no image." }
        // `copy` mode auto asks about the mode it resolves to.
        let arguments = LayerClipboard.resolvingAutoCopy(tool: tool, arguments, in: document)
        let families: [String: String] = [
            "assign_profile":"color_assign", "convert_profile":"color_convert", "duplicate_layer":"layer_duplicate",
            "duplicate_to_new_document":"layer_duplicate",
            "group_layers":"layer_group", "reorder_layer":"layer_move", "reorder_layers":"layer_move",
            "ungroup_layers":"layer_ungroup", "delete_layer":"layer_remove",
            "merge_down":"layer_merge", "merge_visible":"layer_merge_visible",
            "set_layer_lock":"layer_lock", "link_layers":"layer_link",
            "unlink_layers":"layer_link", "new_layer":"layer_add",
            "stamp_visible":"layer_stamp", "flatten_image":"layer_flatten",
            // #547 phase 4c: Stamp Selected and Merge Group.
            "stamp_selected":"layer_stamp_selected", "merge_group":"layer_merge",
            // Arrange was missing here while its UI items and typed op worked.
            "arrange_layer":"layer_move",
            "set_layer_properties":"layer_opacity", "add_adjustment_layer":"adjustment_add",
            "edit_adjustment_layer":"adjustment_set", "apply_filter":"adjustment_apply",
            // The other geometry tools name their family through `geometryFamily`.
            "warp_layer":"geometry_warp", "brush_stroke":"paint", "eraser_stroke":"erase",
            // Edit ▸ Puppet Warp.
            "puppet_warp":"geometry_puppet",
            // Edit ▸ Perspective Warp.
            "perspective_warp":"geometry_perspective_warp",
            "clone_stamp":"clone", "heal_stroke":"heal", "fill":"paint", "clear_selection":"erase", "add_text_layer":"rasterize",
            // Fill… / Stroke… paint the layer like the brush.
            "fill_selection":"paint", "stroke_selection":"paint",
            "add_shape_layer":"rasterize", "add_fill_layer":"rasterize",
            "add_layer_mask":"mask_set", "remove_layer_mask":"mask_remove",
            "set_layer_mask_enabled":"mask_properties", "get_mask_properties":"mask_properties", "set_mask_properties":"mask_properties",
            "save_selection":"channel_set", "delete_channel":"channel_remove", "set_layer_clipped":"layer_clipped", "set_layer_style":"layer_style", "sample_color":"sample", "sample_pixel":"sample",
            // Document-targeted (the default branch below).
            "set_global_light":"global_light",
            "histogram":"histogram", "render":"render", "save_copy":"export", "convert_precision":"convert",
            // A proof reads the rendered composite, so it asks the
            // render family (DOCUMENT-targeted, the default branch below).
            "render_proof":"render",
            // Phase 12G-1: the typed clipboard.
            "copy":"clipboard_copy", "cut":"clipboard_cut",
            "copy_merged":"clipboard_copy_merged", "paste":"clipboard_paste",
            "move_selected_pixels":"selected_pixels_move",
            "transform_selected_pixels":"selected_pixels_transform",
            // Layer Via Copy / Cut name one raster record.
            "layer_via_copy":"layer_via", "layer_via_cut":"layer_via",
            // Stack Modes and Reduce Noise from Stack name raster records.
            "stack_mode":"layer_stack_mode", "reduce_noise_from_stack":"layer_stack_mode"
        ]
        // The whole-canvas geometry families are DOCUMENT-targeted, so the
        // default `["document"]` branch below covers each of them.
        guard let operation = geometryFamily(tool: tool, arguments: arguments, doc: doc)
                ?? layerCopyFamily(tool: tool, arguments: arguments)
                ?? families[tool] else { return nil }
        var targets: [String]
        if operation == "geometry_warp" { targets = [arguments["target"] as? String == "mask" ? "mask" : "raster"] }
        // The selected area of a mask or channel asks that family.
        else if ["selected_pixels_transform", "selected_pixels_move"].contains(operation),
                let plane = selectedPlaneFamily(arguments) { targets = [plane] }
        else if ["mask_remove", "mask_properties", "mask_transform"].contains(operation) { targets = ["mask"] }
        else if operation == "channel_remove" { targets = ["channel"] }
        else if ["adjustment_apply", "layer_duplicate", "layer_opacity", "layer_clipped", "layer_style", "geometry_affine", "geometry_perspective", "geometry_affine_set", "geometry_perspective_set", "mask_set", "paint", "erase", "clone", "heal", "layer_move", "layer_group", "layer_ungroup", "layer_remove", "layer_merge", "layer_stamp_selected", "layer_lock", "layer_link", "clipboard_copy", "clipboard_cut", "selected_pixels_move", "selected_pixels_transform", "layer_via", "layer_stack_mode", "geometry_puppet", "geometry_perspective_warp"].contains(operation) {
            targets = targetIndices(arguments, document, operation: operation).map {
                doc.layerIsGroup($0) ? "group" : doc.layerIsAdjustment($0) ? "adjustment" : "raster"
            }
        } else { targets = ["document"] }
        wholeAdjustmentCopy(operation, document: document, targets: &targets)
        do {
            for target in targets {
                let report = try RasterDocument.precisionCapability(operation: operation, target: target)
                guard report["supported"] as? Bool == true else {
                    return report["reason"] as? String ?? "Unsupported 16-bit operation target."
                }
            }
            return nil
        } catch { return "Could not validate the 16-bit operation capability: \(error.localizedDescription)" }
    }

    /// The capability target a `transform_selected_pixels` /
    /// `transform_again` / `move_selected_pixels` `target` names —
    /// "mask", "channel" or "red", "green", "blue" or "alpha" — or nil
    /// for the layer.
    static func selectedPlaneFamily(_ arguments: [String: Any]) -> String? {
        guard let raw = arguments["target"] as? String else { return nil }
        if raw == "mask" { return "mask" }
        if raw.hasPrefix("channel:") { return "channel" }
        if let plane = RasterPlane.named(raw), PaintTarget.paintablePlanes.contains(plane) {
            return plane.agentName
        }
        return nil
    }

    /// Phase 12G-3: a whole-layer Copy (no selection) of an adjustment layer
    /// reads no samples — it is a transparent payload carrying the layer's
    /// editable record — so the raster-only `clipboard_copy` family does not
    /// govern that target.
    private static func wholeAdjustmentCopy(_ operation: String, document: ImageDocument,
                                            targets: inout [String]) {
        guard operation == "clipboard_copy", document.canvasSelection == nil else { return }
        targets.removeAll { $0 == "adjustment" }
    }

    /// The entries a tool NAMES. Phase 12B let `set_layer_clipped` and
    /// `set_layer_lock` carry a `layers` SET, and a pre-check that read only
    /// `layer`/`index` would admit or refuse a set on the kind of a layer
    /// that may not even be in it — so the set is asked about entry by entry.
    static func targetIndices(_ arguments: [String: Any], _ document: ImageDocument,
                              operation: String? = nil) -> [Int] {
        if let raw = arguments["layers"] as? [Any] {
            let indices = raw.compactMap { integer($0) }
            if !indices.isEmpty { return indices }
        }
        // `from` is `reorder_layer`'s name for the entry it moves (Phase
        // 12E-1); without it the gate would ask about the ACTIVE layer and
        // admit or refuse a move on a record nobody named. Only `layer_move`
        // spells its target that way, so no other family reads it.
        if operation == "layer_move", let from = integer(arguments["from"]) { return [from] }
        return [integer(arguments["layer"]) ?? integer(arguments["index"])
            ?? document.activeLayerIndex]
    }

    static func integer(_ value: Any?) -> Int? {
        if let number = value as? NSNumber { return number.intValue }
        if let text = value as? String { return Int(text) }
        return nil
    }

    static func refusal(operation: OperationScope, target: PaintTarget, doc: RasterDocument,
                        layerIndex: Int) -> String? {
        guard doc.isHighDepth else { return nil }
        if operation == .layerGeometry { return nil }
        if target != .layer {
            return nil // Shared scope validation below checks mask presence and RGBA target ownership.
        }
        guard !doc.layerIsGroup(layerIndex), !doc.layerIsAdjustment(layerIndex) else {
            return "This operation requires a raster layer in 16-bit mode."
        }
        return nil
    }

    static func allows(_ tool: EditorTool, document: ImageDocument?) -> Bool {
        guard RetainedImagePolicy.allows(tool, document: document) else { return false }
        // #547 Bitmap phase 1a: a Bitmap document admits its own closed tool list, asked first.
        if document?.doc?.isBitmap == true { return BitmapDocument.allows(tool) }
        // #547 Indexed phase 1a: an Indexed Color document admits its own closed tool list.
        if document?.doc?.isIndexed == true { return IndexedDocument.allows(tool) }
        // #547 Lab phase 1: a Lab Color document admits its own closed tool list.
        if document?.doc?.isLab == true { return LabDocument.allows(tool) }
        // #547 phase 1a: a Grayscale document admits its own closed tool list.
        if document?.doc?.isGray == true { return GrayDocument.allows(tool) }
        // #547: a CMYK document admits its own closed tool list (`CmykDocument.allows`).
        if document?.doc?.isCmyk == true {
            return CmykDocument.allows(tool, depth: CmykAdjustFilter.depth(of: document?.doc))
        }
        // Float Move is an exact integer offset; the Crop tool's
        // straighten resamples at float like any other filtered geometry.
        if document?.doc?.isFloat == true {
            switch tool {
            case .select, .ellipseSelect, .lasso, .freehandLasso, .hand, .zoom, .eyedropper,
                 .colorSampler, .move, .crop:
                return true
            // Vision reads the tone-mapped display at view exposure 0 (`analysisComposite`).
            case .subject, .objectSelect: return true
            // The Ruler measures and its Straighten Layer is a
            // filtered layer transform; Perspective Crop resamples in the core.
            case .ruler, .perspectiveCrop:
                return true
            // Text and shape layers render into linear light.
            case .text, .shapeRect, .shapeEllipse, .shapeLine:
                return true
            // A path is geometry the document carries at every depth.
            case .pen:
                return true
            // The Brush, Eraser and Gradient paint in linear light (`FloatPaint`).
            case .brush, .eraser, .gradient:
                return true
            // The Clone Stamp and the healing tools retouch a float layer (`FloatRetouch`).
            case .clone, .heal, .spotHeal, .patch, .remove, .contentAwareMove: return true
            // The Red Eye tool corrects its rect on the display-encoded copy.
            case .redEye: return true
            // The Blur, Sharpen, Sponge and Smudge brushes and the History Brush rework a
            // float layer in linear light (`FloatRetouch.brushTools`).
            case .blurBrush, .sharpenBrush, .smudge, .historyBrush, .sponge: return true
            // Selectable, so its mouse-down names the refusal.
            case .artHistoryBrush: return true
            // The Paint Bucket and Dodge/Burn (`FloatPaint`), the Magic Wand and Quick
            // Selection on the display copy (`display_encoding`'s R1–R4).
            case .fill, .dodge, .wand, .quickSelect: return true
            // The Magnetic Lasso traces R1 at 8 bits (`edgeTraceComposite`, rule M).
            case .magneticLasso: return true
            default: return false
            }
        }
        guard document?.doc?.isHighDepth == true else { return true }
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .hand, .zoom, .eyedropper, .colorSampler,
             .brush, .eraser, .clone, .heal, .spotHeal, .redEye, .patch, .text, .shapeRect, .shapeEllipse, .shapeLine: return true
        // Object Selection refines at u16 (`refine_edge`); float admits it above, its
        // refinement opt-in there (rule E).
        case .wand, .subject, .quickSelect, .objectSelect: return true
        // The Magnetic Lasso traces the capture's byte display
        // derivative and outputs geometry only; float admits it above (rule M).
        case .magneticLasso: return true
        case .fill, .gradient, .dodge: return true
        // The five retouch brushes run at 16 bits; float admits all five above.
        case .blurBrush, .sharpenBrush, .smudge, .sponge, .historyBrush: return true
        // Selectable at 16 bits so its mouse-down names the refusal; 8-bit CMYK paints.
        case .artHistoryBrush: return true
        // Remove is a typed Content-Aware Fill.
        case .remove: return true
        // Content-Aware Move is that fill and Patch's typed heal.
        case .contentAwareMove: return true
        // Phase 12F-1b2 (S2): the Crop tool is ADMITTED at 16 bits. It is
        // display-only until commit (the canvas rotates the display image
        // behind a `CGRect` box), and the commit is the numeric typed op —
        // `straightenedCrop` at a non-zero angle, `cropped` at angle 0. A
        // float document answers above, before this switch (and admits it).
        case .crop: return true
        case .move: return true
        // Admitted at 16 bits as at float (above).
        case .ruler, .perspectiveCrop: return true
        // The Pen edits the path list, never a pixel.
        case .pen: return true
        }
    }

    static var support: [String: Any] {
        ["storage": "rgba16_unorm", "evaluation": "encoded_rgb_f32_v1", "range": "SDR 0…1",
         "supported_tools": typedTools.sorted(), "shared_tools": commonTools.union(documentFreeTools).sorted(),
         "paint_targets": ["layer", "mask", "red", "green", "blue", "alpha", "saved channel"], "blend_modes": NativeLayerBlending.modeNames + ["Pass Through (groups)"],
         "layer_blend_version": 1, "adjustment_blend_modes": NativeLayerBlending.modeNames,
         "paint_blend_modes": DrBlendMode.allBlendModes.map { $0.1 },
         "coverage": "Authoritative native selections, Quick Mask, masks and channels retain 65,536 coverage levels. Legacy geometric/AppKit inputs and Vision segmentation retain their disclosed byte precision; display derivatives never replace native data.",
         "artwork": NativeArtwork.information, "limitations": limitation]
    }
}
