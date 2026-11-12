import AppKit

// The Lab Color document's gate (#1 Lab phases 1 to 6b-iii and the close): what a Lab document admits
// from the canvas, the menus and MCP, asked FIRST wherever a depth decides (`DocumentPrecision.refusal`,
// `allows`, `validatePrecisionItem`), before the Gray and CMYK gates. The core's own table is
// `precision_capability::LAB_FAMILIES` and its edit gate `lab_document::admit_edit`; this is
// their host side, written once. Phase 1 makes (New), converts, views and saves (`.rz`) a Lab
// document and edits its layer STRUCTURE, Gray phase 1a's list; phase 1b adds New Layer, groups,
// masks, clipping, merges and Stamp Visible, Fill Opacity and knockout (Blend If since phase 2f, on
// Lightness, a and b: `BlendIf.labChannels`), the
// blend modes of `NativeLayerBlending.labModes`, Edit ▸ Fill (its colour
// converted by Mode ▸ Lab Color's rule in the core), the alpha channels, the Lab TIFF save, and
// Export As, Quick Export and `export_image` through the core's Lab → RGB conversion (the
// document's profile writes sRGB). Phase 2a adds the Brush, Eraser, Paint Bucket, Fill and
// Gradient (their inks Lab codes by Mode ▸ Lab Color's rule, in the core) and phase 2b the
// adjustments of `LabAdjust`, with the Actions that replay them; phase 2c the Filter menu
// (`LabAdjust.allowsFilterAction`), Repeat Last Filter, Fade in `labModes`, the colour
// adjustments, Auto Tone, Auto Contrast and Equalize; phase 2d `histogram source scalar` of
// Lightness, a and b (`scalarReport`), a filter on a layer mask or an alpha channel, and the Color
// Samplers (the tool and its two MCP tools, read in L*a*b*); phase 2e the retouch tools (Dodge
// and Burn on Lightness, the Sponge about a* = b* = 0, the rest per channel on the codes),
// Content-Aware Fill and Move, the layer effects (Pattern Overlay and Pattern strokes since 2f's
// second part, their sRGB tiles converted to Lab codes in the core), Global Light and the style
// presets, the Eyedropper (an exact Lab foreground
// ink, `LabInk.swift`), the Lightness, a, b and alpha planes as paint, edit and read targets, and
// the Brush and Clone Stamp Mode popups narrowed to `labModes`; phase 2f the five luma filters
// (the core's one-channel passes, whose plans `get_precision` reports as `filter_plans`), Blend If
// and the Levels Auto row on Lightness; 2f's second part Edit ▸ Stroke… (`stroke_selection`, its
// colour Lab codes in the core) and the Fill and Gradient tools' Mode popups narrowed to
// `labModes` (`toolBlendChoices`); 2f's third part Remove (the model reads the Display P3 view,
// its fill converted back to Lab codes in the core), the Art History Brush at 8 bits (per channel
// on the codes, strokes along Lightness; its Mode popup `paint.artMode.lab`) and Red Eye (on each
// pixel's 16-bit ProPhoto RGB colour); those tools (`depthRuled`) then answer the 16-bit rules as
// on an RGB document. Phase 3b adds File ▸ Export as PSD, `export_psd` and
// `inspect_psd_export`: a colour-mode-9 PSD, PSB or layered TIFF at the document's own depth
// (`PSDExport.swift`, the core's `psd_export_lab`). Phase 4a adds Image Size, Canvas Size, Crop
// (straighten, and past the canvas: the pad's colour converted by Mode ▸ Lab Color's rule, the
// stored `LabInk` first), Perspective Crop, Trim, Reveal All, Rotate Canvas, the flips, Straighten,
// Free Transform and Transform Again on a layer, a set or a mask and Transform Selection, each the
// RGB kernel on the codes (`depthRuled`). Phase 4a-ii adds Layer ▸ Warp, Puppet Warp, Perspective
// Warp, Liquify, Align and Distribute and the Move tool (its drag, ⌥-drag and nudge of layers),
// each the RGB kernel on the codes; a Lab warp is applied at once and keeps no field, as Gray's
// (`PersistedWarpSheetController.appliedAtOnce`). Phase 4b-i adds the selected pixels of a layer,
// its mask or an alpha channel (⌘T, Transform Again, Puppet and Perspective Warp and the Move tool
// over a selection; a Lightness, a, b or alpha plane's since 6b-i),
// Layer via Copy and via Cut, Stamp Selected, Save and Load Selection (the Lightness, a and b
// planes as the stored codes: `agentPlaneName`, `loadPlaneRefusal`) and Select ▸ Inverse, Feather
// and Modify, each the RGB kernel on the codes. Phase 4b-ii adds Select ▸ Color Range's Sampled
// Colors (`select_color_range` range `sampled`; its samples the L*a*b* codes, its distance the
// largest per-channel L*a*b* difference the take measured: `lab_select` in the core; every other
// range since 6b-i). The Magic Wand and the Paint Bucket at both depths and Select ▸ Similar and
// Grow at 16 bits (#5) run by the rule `lab_select_682_take.py`'s take fitted
// (`lab_select` in the core); Similar and Grow at 8 bits refuse naming #5
// (`selectionRefusals`). Phase 4c, which closes phase 4, adds Copy,
// Cut, Copy Merged and Paste (the payload keeps the Lab codes, `lab_clipboard` in the core; pixels
// pasted across modes convert by the Mode rules, a layers clip through `LabLayerClip`), Duplicate
// to New Document and copying layers to another document. Phase 5a adds
// Image ▸ Mode ▸ CMYK Color (through 16-bit ProPhoto RGB in the core, `lab_cmyk`) and 8/16
// Bits/Channel (`LabDepthEdit`, `convert_precision` mode lab8/lab16; 32 stays disabled). Phase 5b
// adds View ▸ Proof Setup, Proof Colors and Gamut Warning (`proofSelectors`;
// the RGB proof of the Display P3 view, `LabProof.swift`) and Edit ▸ Convert to Profile… (Image ▸
// Mode's conversion into a CMYK or RGB profile, `EditorViewController+LabProfile.swift`); Assign
// Profile… refuses for good (`disabledTools`). Phase 6a-i adds type, shape
// and Solid Color, Gradient and Pattern fill layers,
// Rasterize, Layer Content Options and Artwork Colors, the Text, Shape and Pen tools, the Paths
// panel, Fill and Stroke Path and the vector masks (Gray 6a-i's lists): the colours stay authored
// sRGB and render as L*a*b* codes by Mode ▸ Lab Color's rule in the core (`lab_artwork`), on the
// ×257 grid at 8 bits. Phase 6b-i adds Histogram,
// Apply Image and Calculations (each channel's codes alone, `LabAnalysis.swift`), Stack Modes,
// Load Files into Stack, Merge to HDR (through 16-bit ProPhoto RGB), every Color Range range and
// the selected pixels of the Lightness, a, b and alpha planes.
// Phase 6a-ii adds layer comps (`lab_comps`), Link and Unlink Layers, the snapshots (written and
// reopened as the Lab `.rz`), Edit ▸ Clear (`clear_selection`: the legacy `erase` on a layer, the
// scalar fill 0 on a plane, so a* or b* = −128), the planes' vacated value (the background colour's
// code in that plane, `TransformSelectedPixels.planeBackground`) and Convert to Profile into a
// Gray profile (5a-ii's L* route). Phase 6b-ii adds Auto-Align Layers (Vignette Removal and
// Geometric Distortion on the codes' CIE Y), Auto-Blend Layers and Merge Panorama (L* alone), Quick
// Mask, Quick Selection (ΔE76 on the codes' L*a*b*), Select and Mask (Decontaminate Colors per
// channel on the codes) and Select Subject and its family, Sky, Person Parts and Object Finder (on
// the Display P3 analysis composite). Phase 6b-iii adds Super Zoom, JPEG Artifacts Removal, Neural
// Colorize and Generative Fill, Expand and Variations: the model or provider reads the Display P3
// view (`NeuralFilterRun.modelInput`; Generative Fill's plate in the core) and the core returns its
// result to L*a*b* codes by the Mode rule (`lab_neural`); Sky Replacement refuses for good
// (`skyRefusal`, a Doctored decision). The close (Lab mode closed; `designs/lab-mode.md`) adds the
// remaining layer structure, metadata, view and workspace tools, Distort, Modify Native Plane,
// Invert Channel and the luminosity masks, the Magnetic Lasso and Detect Horizon (the Display P3
// analysis composite), Place and Live Photo (rendered into ProPhoto RGB and published as L*a*b*
// codes in the core), Separate Color, Export Layers, PDF and the contact sheets; Auto-Blend Layers
// refuses for good (`disabledTools`). No refusal names a
// phase: a tool outside `tools` refuses for good by name — `disabledTools`, `skyRefusal`,
// `rawTools`, `byteTools`, `evaluationTools`, `toneMapTools`, else "Doctored offers it on RGB
// documents only" — and a selection of `selectionRefusals` refuses at its depth (#5). Phase 1c
// adds the readouts, never labelling Lab codes as RGB: the Channels panel's Lightness, a and b rows (the R,
// G and B planes of the raw codes, `plane(named:)`, through `labCodeThumbnail`), the Info panel's
// L*a*b* row and Lightness/a/b histogram, `sample_color` / `sample_pixel` (`readingFields`),
// `histogram` (`histogramReport`), `get_precision` (`support`) and `render`'s lightness, a and b
// channels (`labPlaneDisplay`, decided in `planeRenderImage`). The codes' encoding is
// `lab_document`'s module doc (the master): R = L* = 100·v, G = a* and B = b* = 255·v − 128. The
// view is the core's Display P3 conversion of the u16 Lab codes (`viewSpace`, `display(of:)`), and
// a layer thumbnail is fitted in the core before it converts (`thumbnail(of:)`); a plane image never
// goes through the view.

extension RasterDocument {
    /// #1 Lab phase 1c: the composite's raw L*a*b* codes as an RGBA8 thumbnail — `projection`,
    /// or a fresh render, quantized and fitted in one core pass (`thumbnailRGBA8`): no view
    /// conversion and no dither, so a Lab channel reads its own codes. With no projection on the
    /// main thread the render runs as captured work under `nativeDisplay(layer: nil)`'s estimate,
    /// which covers this path's 8 B/px.
    func labCodeThumbnail(maxSide: Int, projection: RasterPrecisionImage? = nil) throws -> RasterImage {
        if let projection {
            let context = try PrecisionWorkContext(image: projection)
            return try projection.thumbnailRGBA8(maxSide: maxSide, workingLimit: context.limit,
                                                 otherRetained: context.otherRetained)
        }
        guard precision != nil else {
            throw DoctoredCoreError(message: "A Lab channel requires a typed document.")
        }
        guard Thread.isMainThread else {
            return try labCodeThumbnailWork(maxSide: maxSide, control: try RasterJobControl())
        }
        let scratch = try projectionScratchEstimate() + UInt64(width * height) * 32
        return try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Render Lab Channels") {
            try self.labCodeThumbnailWork(maxSide: maxSide, control: $0)
        }
    }

    /// The render and the thumbnail, each on a child of the single-use `control`.
    private func labCodeThumbnailWork(maxSide: Int, control: RasterJobControl) throws -> RasterImage {
        guard let precision else {
            throw DoctoredCoreError(message: "A Lab channel requires a typed document.")
        }
        let context = try PrecisionWorkContext(source: precision)
        let image = try control.withChildOperation(from: 0, to: 0.7) {
            try precision.render(workingLimit: context.limit, otherRetained: context.otherRetained,
                                 control: $0)
        }
        let thumbnail = try PrecisionWorkContext(image: image)
        return try control.withChildOperation(from: 0.7, to: 1) {
            try image.thumbnailRGBA8(maxSide: maxSide, workingLimit: thumbnail.limit,
                                     otherRetained: thumbnail.otherRetained, control: $0)
        }
    }

    /// One Lab channel (red = Lightness, green = a, blue = b) of the composite's codes as an 8-bit
    /// grayscale image; any other plane refuses with `LabDocument.renderRefusal`.
    func labPlaneDisplay(_ plane: RasterPlane, maxSide: Int,
                         projection: RasterPrecisionImage? = nil) throws -> RasterImage {
        guard [.red, .green, .blue].contains(plane) else {
            throw DoctoredCoreError(message: LabDocument.renderRefusal)
        }
        guard let image = try labCodeThumbnail(maxSide: maxSide, projection: projection)
            .planeImage(plane.dr, maxSide: 0) else {
            throw DoctoredCoreError(message: "Could not extract the \(LabDocument.channelName(plane)) channel.")
        }
        return image
    }

    /// True for a Lab Color document — read off the captured descriptor (`color_mode`), so a
    /// per-row panel or menu check costs no FFI call.
    var isLab: Bool { precisionInfo?["color_mode"] as? String == "lab" }

    /// A Lab document's declared depth (8 or 16); nil for every other document.
    var labDepth: Int? { (precisionInfo?["lab"] as? [String: Any])?["depth"] as? Int }
}

enum LabDocument {
    /// Every MCP tool a Lab document answers: Gray phase 1a's list (`fa9c7b5b`'s
    /// `GrayDocument.tools`), then phase 1b's layer structure, Fill, exports and alpha channels,
    /// then phase 1c's readouts. `AgentServer+Registry.swift` refuses to start when one is not a
    /// registered tool.
    static let tools: Set<String> = Set<String>([
        "convert_mode", "get_color_profile", "get_document", "render",
        "set_active_layer", "flatten_image", "select_rect", "select_ellipse", "select_polygon",
        "select_freehand", "select_all", "deselect", "reselect", "get_selection", "undo", "redo",
        "save_copy", "set_layer_lock", "set_selected_layers", "duplicate_layer", "delete_layer",
        "list_guides", "add_guide", "remove_guide", "clear_guides", "list_history", "restore_history",
        "set_layer_label", "reorder_layer", "set_layer_properties", "reorder_layers",
        "close_document", "get_recovery_status", "write_recovery",
        // Phase 1b.
        "new_layer", "group_layers", "ungroup_layers", "add_layer_mask", "remove_layer_mask",
        "set_layer_mask_enabled", "set_mask_properties", "set_layer_clipped", "merge_down",
        "merge_visible", "stamp_visible", "set_layer_style", "fill_selection", "export_image",
        "export_preview", "quick_export", "list_channels", "add_channel", "duplicate_channel",
        "delete_channel", "rename_channel", "set_channel_options", "add_spot_channel",
        "set_spot_channel", "merge_spot_channel", "set_channel_kind",
        // Phase 1c: the readouts.
        "sample_color", "sample_pixel", "histogram", "get_precision",
        // Phase 2a: painting; phase 2b: the adjustments (`LabAdjust`); and Actions, whose steps
        // pass this gate one by one.
        "brush_stroke", "eraser_stroke", "fill", "gradient", "apply_filter", "add_adjustment_layer",
        "edit_adjustment_layer", "run_action",
        // Phase 2c: Fade, Image ▸ Auto Tone and Auto Contrast, and Equalize.
        "fade", "auto_tone", "auto_contrast", "equalize",
        // Phase 2d: the Color Samplers.
        "get_color_samplers", "set_color_samplers",
        // Phase 2e: the retouch tools, the content-aware tools (Gray 2e's list), the global light
        // and the style presets.
        "dodge_burn", "sponge_stroke", "blur_stroke", "sharpen_stroke", "smudge_stroke",
        "history_brush_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "patch_region",
        "content_aware_fill", "preview_content_aware_fill", "discard_content_aware_fill_preview",
        "content_aware_move", "set_global_light", "apply_style_preset", "save_style_preset",
        // Phase 2f, second part: Edit ▸ Stroke….
        "stroke_selection",
        // Phase 2f, third part: Remove, the Art History Brush and Red Eye.
        "remove_stroke", "art_history_brush_stroke", "red_eye", "red_eye_auto",
        // Phase 3b: Export as PSD writes a Lab PSD, PSB or layered TIFF.
        "export_psd", "inspect_psd_export",
        // Phase 4a: the resamples and transforms, the RGB kernels on the L*a*b* codes.
        "image_size", "canvas_size", "crop", "perspective_crop", "fit_crop", "trim",
        "reveal_canvas", "rotate", "flip", "straighten_layers", "transform_layer",
        "transform_layers", "transform_again", "transform_mask", "transform_selection",
        // Phase 4a-ii: the warps, Liquify, Align and Distribute and the Move tool, the RGB kernels
        // on the L*a*b* codes.
        "get_warp", "warp_layer", "puppet_warp", "perspective_warp", "liquify", "align_layers",
        "distribute_layers", "move_layers",
        // Phase 4b-i: the selected pixels, Layer Via, Stamp Selected, Save and Load Selection and
        // Select ▸ Modify, the RGB kernels on the L*a*b* codes.
        "move_selected_pixels", "transform_selected_pixels", "layer_via_copy", "layer_via_cut",
        "stamp_selected", "save_selection", "load_selection", "modify_selection",
        // Phase 5a: Image ▸ Mode ▸ 8/16 Bits/Channel (`dispatchLabColour`); `convert_mode cmyk`
        // converts directly.
        "convert_precision",
        // Phase 4b-ii: sampled Color Range by the measured Lab rule (`lab_select` in the core).
        "select_color_range",
        // The Magic Wand and Select ▸ Similar and Grow (#5) by the take's Lab rule
        // (`lab_select` in the core), wherever `selectionRefusals` admits them.
        "select_magic_wand", "select_similar",
        // Phase 4c: the clipboard across modes (`LabClipboard.swift`, `lab_clipboard` in the
        // core), Duplicate to New Document and copying layers to another document.
        "copy", "cut", "copy_merged", "paste", "duplicate_to_new_document",
        "copy_layers_to_document",
        // Edit ▸ Define Pattern… (the composite in sRGB, `DefinePattern`).
        "define_pattern",
        // Phase 5b: Edit ▸ Convert to Profile… (`dispatchLabColour`).
        "convert_profile",
        // Phase 6a-i: type, shape and fill layers (authored sRGB, rendered as L*a*b* codes in the
        // core, `lab_artwork`), paths and vector masks: Gray 6a-i's list.
        "add_text_layer", "edit_text_layer", "add_shape_layer", "edit_shape_layer", "add_fill_layer",
        "edit_fill_layer", "rasterize_artwork", "list_paths", "set_path", "rename_path",
        "duplicate_path", "delete_path", "selection_to_path", "path_to_selection", "fill_path",
        "stroke_path", "get_vector_mask", "set_vector_mask", "set_vector_mask_properties",
        "remove_vector_mask", "set_clipping_path",
        // Replace Missing Fonts (MissingFonts.swift), authored sRGB kept.
        "list_missing_fonts", "replace_missing_fonts",
        // Phase 6b: Apply Image and Calculations (`LabAnalysis.swift`), Stack Modes and Merge to HDR.
        "apply_image", "calculations", "stack_mode", "reduce_noise_from_stack", "merge_layers_to_hdr",
        // Phase 6a-ii: links, layer comps (`lab_comps` in the core), snapshots and Edit ▸ Clear:
        // Gray 6a-ii's list.
        "link_layers", "unlink_layers", "list_layer_comps", "save_layer_comp", "apply_layer_comp",
        "update_layer_comp", "delete_layer_comp", "reorder_layer_comp", "list_snapshots",
        "create_snapshot", "rename_snapshot", "delete_snapshot", "restore_snapshot", "save_snapshot",
        "load_snapshot", "clear_selection",
        // Phase 6b-ii: Auto-Align Layers (the codes' CIE Y in the core, `lab_stack`), Quick
        // Selection, the model selections, Select and Mask and Quick Mask. Auto-Blend Layers
        // refuses for good since the close (`disabledTools`).
        "auto_align_layers",
        "quick_select", "select_subject", "select_subjects", "inspect_subjects", "select_people",
        "select_face_parts", "inspect_faces", "object_select", "refine_selection",
        "set_quick_mask", "quick_mask_stroke",
        "select_sky", "select_person_parts", "object_finder_select",
        // Phase 6b-iii: the neural and generative tools (the model or provider reads the Display
        // P3 view; the result returns to L*a*b* codes by the Mode rule in the core, `lab_neural`).
        "super_zoom", "jpeg_artifacts_removal", "neural_colorize", "generative_fill",
        "generative_expand", "generative_variations", "select_generative_variation",
        "generate_variations",
        // The close: the layer structure, metadata, views and workspaces (colour-free); Distort,
        // Modify Native Plane, Invert Channel and the luminosity masks (the RGB kernels on the
        // codes; the masks read Lightness); the Magnetic Lasso and Detect Horizon (the Display P3
        // analysis composite); Place and Live Photo (rendered into ProPhoto RGB and published as
        // L*a*b* codes in the core); and Separate Color, Export Layers, PDF and the contact sheets
        // (their pixels leave through the Display P3 view or the core's Lab → RGB export).
        "arrange_layer", "auto_select_layer", "find_layers", "get_mask_properties", "merge_group",
        "list_views", "open_second_view", "close_second_view", "get_view", "set_view",
        "save_workspace", "restore_workspace", "get_file_info", "set_file_info", "get_metadata",
        "set_resolution", "set_ruler_origin", "set_history_budget", "recover_document",
        "discard_recovery", "detect_horizon", "list_lens_profiles", "distort_layer",
        "modify_scalar", "invert_channel", "add_luminosity_masks", "select_magnetic",
        "separate_color", "export_layers", "export_pdf", "contact_sheet",
        "contact_sheet_from_sources", "place_image", "add_live_photo_layer",
    ]).union(proofTools) // Phase 5b: the proof tools (`LabProof.swift`).

    /// The admitted tools that then answer the depth's own rules (`DocumentPrecision.refusal`):
    /// the 16-bit paint, target and capability checks, as on an RGB document.
    static let depthRuled: Set<String> = [
        "brush_stroke", "eraser_stroke", "fill", "gradient", "apply_filter", "add_adjustment_layer",
        "edit_adjustment_layer", "fade", "auto_tone", "auto_contrast", "equalize",
        // Phase 2e.
        "dodge_burn", "sponge_stroke", "blur_stroke", "sharpen_stroke", "smudge_stroke",
        "history_brush_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "patch_region",
        "content_aware_fill", "preview_content_aware_fill", "discard_content_aware_fill_preview",
        "content_aware_move",
        // Phase 2f, second part.
        "stroke_selection",
        // Phase 2f, third part.
        "remove_stroke", "art_history_brush_stroke", "red_eye", "red_eye_auto",
        // Phase 4a (Gray phase 4's pattern: the same list as `tools`).
        "image_size", "canvas_size", "crop", "perspective_crop", "fit_crop", "trim",
        "reveal_canvas", "rotate", "flip", "straighten_layers", "transform_layer",
        "transform_layers", "transform_again", "transform_mask", "transform_selection",
        // Phase 4a-ii (the same list as `tools`).
        "get_warp", "warp_layer", "puppet_warp", "perspective_warp", "liquify", "align_layers",
        "distribute_layers", "move_layers",
        // Phase 4b-i (the same list as `tools`).
        "move_selected_pixels", "transform_selected_pixels", "layer_via_copy", "layer_via_cut",
        "stamp_selected", "save_selection", "load_selection", "modify_selection",
        // Phase 4b-ii.
        "select_color_range",
        // #5 (the same list as `tools`).
        "select_magic_wand", "select_similar",
        // Phase 4c (the same list as `tools`).
        "copy", "cut", "copy_merged", "paste", "duplicate_to_new_document",
        "copy_layers_to_document",
        // Phase 6a-i (the same list as `tools`).
        "add_text_layer", "edit_text_layer", "add_shape_layer", "edit_shape_layer", "add_fill_layer",
        "edit_fill_layer", "rasterize_artwork", "list_paths", "set_path", "rename_path",
        "duplicate_path", "delete_path", "selection_to_path", "path_to_selection", "fill_path",
        "stroke_path", "get_vector_mask", "set_vector_mask", "set_vector_mask_properties",
        "remove_vector_mask", "set_clipping_path",
        // Replace Missing Fonts (MissingFonts.swift), authored sRGB kept.
        "list_missing_fonts", "replace_missing_fonts",
        // Phase 6b (the same list as `tools`).
        "apply_image", "calculations", "stack_mode", "reduce_noise_from_stack", "merge_layers_to_hdr",
        // Phase 6a-ii: Edit ▸ Clear (the legacy `erase`; a plane, mask or channel the scalar fill).
        "clear_selection",
        // Phase 6b-ii (the same list as `tools`).
        "auto_align_layers",
        "quick_select", "select_subject", "select_subjects", "inspect_subjects", "select_people",
        "select_face_parts", "inspect_faces", "object_select", "refine_selection",
        "set_quick_mask", "quick_mask_stroke",
        "select_sky", "select_person_parts", "object_finder_select",
        // Phase 6b-iii (the same list as `tools`).
        "super_zoom", "jpeg_artifacts_removal", "neural_colorize", "generative_fill",
        "generative_expand", "generative_variations", "select_generative_variation",
        "generate_variations",
        // The close: the lifted tools Gray's list rules by depth too.
        "distort_layer", "modify_scalar", "invert_channel", "add_luminosity_masks",
        "select_magnetic", "merge_group",
    ]

    /// The menu items a Lab document enables: Gray phase 1a's list with Image ▸ Mode ▸ Lab Color
    /// in place of Grayscale (CMYK Color is then disabled by `validateCmykModeItem`), then Gray
    /// phase 1b's structure, Layer Style and Fill items, Export As, Quick Export and the alpha
    /// channels. View-only and window items are answered before this list.
    static let selectors: Set<String> = Set<String>([
        "convertToRGBMode:", "convertToCMYKMode:", "convertToLabMode:",
        "showInfo:", "showHistory:", "toggleLayersPanel:", "showChannels:",
        "undo:", "redo:", "zoomInAction:", "zoomOutAction:", "zoomActualAction:", "zoomFitAction:",
        "duplicateLayer:", "deleteLayer:", "bringToFront:", "bringForward:", "sendBackward:",
        "sendToBack:", "flattenImage:", "setLayerLabel:",
        "lockTransparency:", "lockPixels:", "lockPosition:", "lockAll:",
        "selectAll:", "deselect:", "reselect:", "newGuide:", "clearGuides:",
        // Phase 1b: the layer structure, Layer Style's blending options and Edit ▸ Fill.
        "newLayer:", "groupLayers:", "ungroupLayers:", "addLayerMaskRevealAll:",
        "addLayerMaskHideAll:", "addLayerMaskFromSelection:", "deleteLayerMask:",
        "toggleLayerMaskEnabled:", "toggleMaskContentLink:", "showMaskProperties:",
        "applyLayerMask:", "toggleClippingMask:", "mergeDown:", "mergeVisible:", "stampVisible:",
        "layerStyle:", "clearLayerStyle:", "fillSelection:", "fillSelectionFromDeleteKey:",
        "fillWithForeground:", "fillWithBackground:", "fillWithForegroundPreserving:",
        "fillWithBackgroundPreserving:",
        // Phase 1b: File ▸ Export As, Quick Export and the alpha channels.
        "showExportWorkspace:", "quickExport:", "newChannel:", "duplicateChannel:",
        "renameChannel:", "channelOptions:", "deleteChannel:",
        // Phase 2b: an adjustment layer's options (the adjustment items are
        // `LabAdjust.allowsAction`'s) and the Actions panel.
        "adjustmentOptions:", "showActions:", "startRecording:", "stopRecording:",
        // Phase 2c: Fade and Equalize (the Filter items are `LabAdjust.allowsFilterAction`'s).
        "showFade:", "equalize:",
        // Phase 2e: the layer effects, Copy/Paste Layer Style and Edit ▸ Content-Aware Fill….
        "layerStyleEffect:", "copyLayerStyle:", "pasteLayerStyle:", "contentAwareFill:",
        // Phase 2f, second part: Edit ▸ Stroke…; third part: Filter ▸ Remove Red Eye.
        "strokeSelection:", "removeRedEye:",
        // Phase 3b: File ▸ Export as PSD….
        "exportPSD:",
        // Phase 4a: Image Size, Canvas Size, Crop, Trim, Reveal All, the rotations and flips,
        // Free Transform, Transform Again, Transform Mask and Transform Selection.
        "resizeImage:", "showCanvasSize:", "cropToSelection:", "showTrim:", "revealAllCanvas:",
        "rotateCW:", "rotateCCW:", "rotate180:", "rotateArbitrary:", "flipH:", "flipV:",
        "freeTransform:", "freeTransformCopy:", "transformAgain:", "transformAgainWithCopy:",
        "transformMask:", "transformSelection:",
        // Phase 4a-ii: Layer ▸ Warp, Puppet Warp, Perspective Warp and Align and Distribute
        // (Liquify is `LabAdjust.allowsFilterAction`'s).
        "showPersistedWarp:", "showPuppetWarp:", "showPerspectiveWarp:", "alignLeft:",
        "alignCenterX:", "alignRight:", "alignTop:", "alignCenterY:", "alignBottom:",
        "distributeHorizontally:", "distributeVertically:",
        // Phase 4b-i: Layer via Copy and Cut, Stamp Selected, Save and Load Selection, the
        // Channels panel's Load Channel as Selection and Select ▸ Inverse, Feather and Modify.
        "layerViaCopy:", "layerViaCut:", "stampSelected:", "saveSelectionSheet:",
        "loadSelectionSheet:", "loadChannelAsSelection:", "invertSelection:", "featherSelection:",
        "growSelection:", "shrinkSelection:", "borderSelection:", "smoothSelection:",
        // Phase 4b-ii: Select ▸ Color Range (Sampled Colors only, `ColorRangeSheetController`).
        "showColorRange:",
        // #5: Select ▸ Grow and Similar, each disabled where `selectionRefusals` refuses it at
        // the document's depth (`EditorViewController+Precision`).
        "selectGrow:", "selectSimilar:",
        // Phase 4c: Copy, Copy Merged, Cut, the Pastes and Duplicate to New Document (Gray's set).
        "copy:", "copyMerged:", "cut:", "paste:", "pasteAsNewLayer:", "pasteInPlace:",
        "definePattern:",
        "pasteInto:", "pasteOutside:", "duplicateToNewDocument:",
        // Phase 5b: Edit ▸ Convert to Profile… (`presentLabProfileSheet`).
        "convertToProfile:",
        // Phase 6a-i: fill layers, Layer Content Options, Rasterize and Artwork Colors, and the
        // Paths panel, Fill and Stroke Path and the vector masks (Gray 6a-i's list).
        "newSolidColorFillLayer:", "newGradientFillLayer:", "newPatternFillLayer:",
        "layerContentOptions:", "rasterizeArtwork:", "editArtworkColors:", "replaceMissingFonts:",
        "makeWorkPath:",
        "makeSelectionFromPath:", "duplicateCurrentPath:", "deleteCurrentPath:", "fillCurrentPath:",
        "strokeCurrentPath:", "toggleClippingPath:", "addVectorMaskFromCurrentPath:",
        "addVectorMaskRevealAll:",
        "deleteVectorMask:", "toggleVectorMaskEnabled:", "toggleVectorMaskLinked:",
        // Phase 6b: Apply Image, Calculations, Stack Modes and Merge to HDR.
        "applyImageSheet:", "calculationsSheet:", "stackMode:", "showReduceNoiseFromStack:",
        "mergeLayersToHDR:",
        // Phase 6a-ii: Link and Unlink Layers, the Snapshots window and Edit ▸ Clear (Layer Comps
        // is a depth-independent window action).
        "linkLayers:", "unlinkLayers:", "showSnapshots:", "clearSelection:",
        // Phase 6b-ii: Auto-Align Layers, Select and Mask, the model selections and Quick Mask
        // (Auto-Blend Layers stays disabled since the close: `disabledTools`).
        "autoAlignLayers:", "refineSelection:", "selectSubject:",
        "showSubjectTargets:", "showPeopleTargets:", "selectPeople:", "showFaceParts:",
        "toggleQuickMask:", "selectSky:", "showPersonParts:",
        // Phase 6b-iii: Super Zoom, JPEG Artifacts Removal, Colorize and Generative Fill, Expand
        // and Variations (Sky Replacement refuses for good, `skyRefusal`).
        "showSuperZoom:", "showJpegArtifactsRemoval:", "showColorize:", "showGenerativeFill:",
        "showGenerativeExpand:", "showGenerativeVariations:",
        // The close: Place Embedded/Linked and Live Photo, Export Layers, the contact sheet, PDF,
        // File Info, Modify Native Plane, Invert Channel and the luminosity masks.
        "placeEmbeddedImage:", "placeLinkedImage:", "placeLivePhoto:", "exportLayers:",
        "showContactSheet:", "exportAsPDF:", "showFileInfo:", "modifyNativePlane:",
        "invertChannel:", "addLuminosityMasks:",
    ]).union(proofSelectors) // Phase 5b: the proof items (`LabProof.swift`).

    /// The camera RAW tools (`GrayDocument.rawTools`), refused for good: Doctored keeps a RAW
    /// layer's development on RGB documents only.
    static let rawTools: Set<String> = GrayDocument.rawTools

    /// A camera RAW tool's permanent refusal (`rawTools`); also Place's for a camera file.
    static func rawRefusal(tool: String) -> String {
        "\(tool) is unavailable in a Lab Color document: Doctored keeps camera RAW layers on RGB "
            + "documents only."
    }

    /// The 8-bit byte tools, refused for good (`byteRefusal`).
    static let byteTools: Set<String> = ["add_text", "put_pixels"]

    /// A byte tool's permanent refusal: its pixels are 8-bit RGB(A).
    static func byteRefusal(tool: String) -> String {
        tool == "add_text"
            ? "add_text is unavailable in a Lab Color document: it draws 8-bit RGB pixels; "
                + "add_text_layer adds editable type."
            : "put_pixels is unavailable in a Lab Color document: it writes 8-bit RGBA bytes, "
                + "which Doctored offers on 8-bit RGB documents only."
    }

    /// Linear-light evaluation, refused for good (Doctored's decision, as on Grayscale).
    static let evaluationTools: Set<String> = ["get_evaluation", "set_evaluation"]

    /// An evaluation tool's permanent refusal.
    static func evaluationRefusal(tool: String) -> String {
        "\(tool) is unavailable in a Lab Color document: Doctored offers linear-light evaluation "
            + "on RGB documents only."
    }

    /// The 32-bit tone map, refused for good: Lab Color is 8 or 16 bits.
    static let toneMapTools: Set<String> = ["get_tone_map", "set_tone_map"]

    /// A tone-map tool's permanent refusal.
    static func toneMapRefusal(tool: String) -> String {
        "\(tool) is unavailable in a Lab Color document: the tone map is a 32-bit document's, and "
            + "Lab Color is 8 or 16 bits per channel."
    }

    /// The selections a Lab document refuses, one row per (key, fate): the core's
    /// `lab_document::SELECTION_REFUSALS`. `depths` are the bits it refuses at; `issue` names the
    /// issue awaiting a fitted Lab tolerance rule, nil where it is disabled for good. A key absent
    /// here is admitted at both depths. `select_grow` is `select_similar` with `contiguous` true,
    /// a key and never a tool name. `lab_select_682_take.py`'s take fitted the Magic Wand at both
    /// depths and Similar and Grow at 16 bits only.
    static let selectionRefusals: [(key: String, depths: Set<Int>, issue: String?)] = [
        (key: "select_similar", depths: [8], issue: "#5"),
        (key: "select_grow", depths: [8], issue: "#5"),
    ]

    /// `key`'s refusal at `depth` (`selectionRefusals`), or nil where it is admitted: the core's
    /// `lab_document::selection_refusal`, sentence for sentence. A row of one depth names it ("an
    /// 8-bit" or "a 16-bit" Lab Color document).
    static func selectionRefusal(key: String, depth: Int?) -> String? {
        guard let row = selectionRefusals.first(where: {
            $0.key == key && depth.map($0.depths.contains) == true
        }) else { return nil }
        let label = key == "select_grow" ? "select_similar with contiguous true (Grow)" : key
        let document: String
        switch (row.depths.count, depth) {
        case (1, 8): document = "an 8-bit Lab Color document"
        case (1, 16): document = "a 16-bit Lab Color document"
        default: document = "a Lab Color document"
        }
        guard let issue = row.issue else {
            return "\(label) is unavailable in \(document)."
        }
        return "\(label) is unavailable in \(document) until its Lab tolerance rule is measured "
            + "(\(issue))."
    }

    /// The `selectionRefusals` key of an MCP selection call: `select_magic_wand`, or
    /// `select_similar` / `select_grow` by its `contiguous` (false when absent, as
    /// `AgentServer+SelectSimilar.swift` reads it); nil for any other tool.
    static func selectionKey(tool: String, arguments: [String: Any]) -> String? {
        switch tool {
        case "select_magic_wand": return tool
        case "select_similar":
            return (boolArgument(arguments["contiguous"]) ?? false) ? "select_grow" : "select_similar"
        default: return nil
        }
    }

    /// A boolean argument read as `AgentServer.boolArg` reads it (a number, or "true"/"1" and
    /// "false"/"0").
    private static func boolArgument(_ value: Any?) -> Bool? {
        if let number = value as? NSNumber { return number.boolValue }
        switch value as? String {
        case "true", "1": return true
        case "false", "0": return false
        default: return nil
        }
    }

    /// The refusal of a tool outside `tools`, never naming a phase: a tool in `disabledTools`
    /// refuses for good (`disabledRefusal`), then Sky Replacement, the four refused-for-good
    /// sets, and any other tool the core's default sentence (`lab_document::refused_sentence`).
    static func refusal(tool: String) -> String {
        if disabledTools.contains(tool) { return disabledRefusal(tool: tool) }
        if tool == "sky_replacement" { return skyRefusal }
        if rawTools.contains(tool) { return rawRefusal(tool: tool) }
        if byteTools.contains(tool) { return byteRefusal(tool: tool) }
        if evaluationTools.contains(tool) { return evaluationRefusal(tool: tool) }
        if toneMapTools.contains(tool) { return toneMapRefusal(tool: tool) }
        return "\(tool) is unavailable in a Lab Color document: Doctored offers it on RGB "
            + "documents only."
    }

    /// The gate's answer for one MCP call at the document's `depth` (8 or 16): nil to admit it,
    /// else the refusal. A selection of `selectionRefusals` answers first, then an adjustment's
    /// table refusal (`LabAdjust.toolRefusal`). Admitted tools whose ARGUMENTS can still reach a
    /// path the mode does not hold yet are narrowed here.
    static func refusal(tool: String, arguments: [String: Any], depth: Int?) -> String? {
        if let key = selectionKey(tool: tool, arguments: arguments),
           let reason = selectionRefusal(key: key, depth: depth) {
            return reason
        }
        if let reason = LabAdjust.toolRefusal(tool: tool, arguments: arguments) { return reason }
        guard tools.contains(tool) else { return refusal(tool: tool) }
        // A blend mode outside `labModes`; Pass Through is left to the group rule
        // (`NativeLayerBlending.allows`).
        func modeRefusal() -> String? {
            guard let name = arguments["blend_mode"] as? String,
                  let mode = NativeLayerBlending.mode(named: name),
                  mode != DR_BLEND_PASS_THROUGH,
                  !NativeLayerBlending.labModes.contains(mode) else { return nil }
            return NativeLayerBlending.labLimitation
        }
        // The layer, its mask, an alpha channel (`channel:<name>`) and (phase 2e) the Lightness,
        // a, b and alpha planes (`paintTarget`'s Lab names) and (phase 6b-ii) Quick Mask; red,
        // green and blue are not a Lab document's names (`planeTargetRefusal`). A filter on
        // a mask, an alpha channel or a plane runs by the scalar rule
        // (`LabAdjust.scalarFilterRefusal`); an Auto or Equalize on the layer only.
        let painted: Set<String> = [
            "brush_stroke", "eraser_stroke", "fill", "gradient", "fill_selection", "apply_filter",
            "stroke_selection", "clear_selection", "modify_scalar",
        ]
        let layerOnly: Set<String> = ["auto_tone", "auto_contrast", "equalize"]
        if let target = arguments["target"] as? String {
            if painted.contains(tool), ["red", "green", "blue"].contains(target) {
                return planeTargetRefusal
            }
            if painted.contains(tool)
                && !["layer", "mask", "lightness", "a", "b", "alpha", "quick_mask"].contains(target)
                && !target.hasPrefix("channel:") {
                return planeTargetRefusal
            }
            if layerOnly.contains(tool) && target != "layer" {
                return "\(tool) takes target layer in a Lab Color document: it acts on the layer's "
                    + "Lightness."
            }
        }
        switch tool {
        case "set_layer_properties", "clone_stamp":
            if let reason = modeRefusal() { return reason }
        case "fade":
            // Gray's check against the Lab list (`lab_document::blend_refusal` in the core).
            if let name = arguments["mode"] as? String, let mode = NativeLayerBlending.mode(named: name),
               mode != DR_BLEND_PASS_THROUGH, !NativeLayerBlending.labModes.contains(mode) {
                return NativeLayerBlending.labLimitation
            }
        case "fill_selection", "stroke_selection", "brush_stroke", "fill", "gradient":
            if arguments["ink"] != nil {
                return "ink is a CMYK document's colour; a Lab Color document paints with color, "
                    + "converted by Mode ▸ Lab Color's rule."
            }
            if let reason = modeRefusal() { return reason }
        case "move_selected_pixels", "transform_selected_pixels":
            // Phase 4b-i: a layer, its mask or an alpha channel; phase 6b-i: the Lightness, a, b
            // and alpha planes too. Red, green and blue are not a Lab document's names.
            if let target = arguments["target"] as? String,
               ["red", "green", "blue"].contains(target) {
                return planeTargetRefusal
            }
        case "apply_image", "calculations":
            // Phase 6b-i: each channel's codes alone (`LabAnalysis.swift`).
            return channelMathRefusal(arguments: arguments)
        case "save_copy":
            let format = (arguments["format"] as? String)?.lowercased()
                ?? (arguments["path"] as? String).map { ($0 as NSString).pathExtension.lowercased() }
                ?? ImageDocument.nativeFormatName
            // A TIFF's `bit_depth` is checked against the document's by the handler
            // (`savePrecisionCopy`'s Lab branch, `tiffDepthRefusal`).
            if !ImageDocument.isNativeFormatName(format), !["tiff", "tif"].contains(format) {
                return saveRefusal
            }
        case "convert_mode":
            // Phase 5a: cmyk converts directly (the core's ProPhoto stage); since 5a-ii gray converts
            // directly too (the core's `lab_gray`).
            break
        default:
            break
        }
        return nil
    }

    /// Why a Lab document refuses a save format (#1 phase 1b): its PSD is Export as PSD's
    /// (phase 3b).
    static let saveRefusal = "A Lab Color document saves as .doctored or Lab TIFF; Export as PSD "
        + "(export_psd) writes a Lab PSD, PSB or layered TIFF. Export As writes an RGB copy, or "
        + "convert to RGB Color for other formats."

    /// Why a Lab document refuses Sky Replacement, for good (#1 Lab phase 6b-iii): a Doctored
    /// decision, as on Grayscale (`GrayDocument.skyRefusal`). The core refuses by name too.
    static let skyRefusal = "Sky Replacement is unavailable in a Lab Color document: Doctored "
        + "offers it on RGB documents only."

    /// Why a Lab document refuses a `render` channel (#1 Lab phase 1c; decided in
    /// `AgentServer.planeRenderImage`'s Lab arm).
    static let renderRefusal = "A Lab Color document renders its composite with no channel, channel "
        + "lightness, a or b with no layer, channel mask with a layer, or an alpha channel by name "
        + "with no layer."

    // MARK: - Phase 1c: the readouts

    /// A Lab channel's agent name as the plane of the codes that holds it: L* in red, a* in
    /// green, b* in blue; nil for any other name.
    static func plane(named channel: String) -> RasterPlane? {
        switch channel {
        case "lightness": return .red
        case "a": return .green
        case "b": return .blue
        default: return nil
        }
    }

    /// The Lab name of a plane of the codes ("Lightness", "a", "b"); any other plane keeps its own.
    static func channelName(_ plane: RasterPlane) -> String {
        switch plane {
        case .red: return "Lightness"
        case .green: return "a"
        case .blue: return "b"
        default: return plane.displayName
        }
    }

    /// L*, a*, b* of a normalized sample (`lab_document`'s ICC v4 16-bit encoding).
    static func lab(_ normalized: [Double]) -> (l: Double, a: Double, b: Double) {
        guard normalized.count >= 3 else { return (0, 0, 0) }
        return (100 * normalized[0], 255 * normalized[1] - 128, 255 * normalized[2] - 128)
    }

    /// The Lab fields `sample_color` and `sample_pixel` add to the scoped report: no RGB
    /// reading, since the document's colour space is the Display P3 view.
    static func readingFields(_ normalized: [Double]) -> [String: Any] {
        let value = lab(normalized)
        let rounded = { (v: Double) in (v * 1000).rounded() / 1000 }
        return [
            "lab": ["l": rounded(value.l), "a": rounded(value.a), "b": rounded(value.b)],
            "lab_space": "CIE L*a*b* D50 (the document's own values)",
            "note": "A Lab Color document's codes are L*, a*, b* and alpha in the ICC v4 16-bit Lab "
                + "encoding; it has no RGB reading.",
        ]
    }

    /// The Info panel's Lab row.
    static func readoutText(_ normalized: [Double]) -> String {
        let value = lab(normalized)
        return String(format: "%.1f, %.1f, %.1f", value.l, value.a, value.b)
    }

    /// The 8-bit `#LLaabb` of an L*a*b* colour's codes — round(255·L*/100), round(a* + 128),
    /// round(b* + 128), each clamped to 0…255: Replace Color's colours on a Lab document, the
    /// document's own numbers (#1 phase 2d).
    static func codeHex(_ lab: (l: Double, a: Double, b: Double)) -> String {
        let byte = { (v: Double) in Int(min(max(v.rounded(), 0), 255)) }
        return String(format: "#%02x%02x%02x", byte(255 * lab.l / 100), byte(lab.a + 128),
                      byte(lab.b + 128))
    }

    /// Why `histogram source scalar` refuses a target on a Lab document (#1 phase 2d; alpha
    /// since phase 2e).
    static let scalarTargetRefusal = "A Lab Color document's scalar histogram takes target "
        + "lightness, a, b, alpha, mask or channel:<name>."

    /// Why a paint, fill or filter target refuses on a Lab document (#1 phase 2e): its colour
    /// planes are named lightness, a and b, never red, green and blue.
    static let planeTargetRefusal = "A Lab Color document's targets are layer, mask, lightness, a, "
        + "b, alpha and channel:<name>."

    // MARK: - Phase 4b-i: the selected pixels and the selections

    /// Why `load_selection` and `add_channel` refuse a plane on a Lab document (#1 phase 4b-i):
    /// red, green and blue are not a Lab document's names.
    static let loadPlaneRefusal = "A Lab Color document's planes are lightness, a, b, luma (its "
        + "Lightness) and alpha."

    /// A plane's agent name on a Lab document: the codes' red, green and blue are lightness, a
    /// and b (`plane(named:)`'s inverse); every other plane keeps its `agentName`.
    static func agentPlaneName(_ plane: RasterPlane) -> String {
        switch plane {
        case .red: return "lightness"
        case .green: return "a"
        case .blue: return "b"
        default: return plane.agentName
        }
    }

    /// `report` (a scoped sample: `sample_color`, `sample_pixel`, a Color Samplers row) with
    /// `readingFields` merged in when it holds a four-value `normalized_rgba`.
    static func withReading(_ report: [String: Any]) -> [String: Any] {
        guard let normalized = report["normalized_rgba"] as? [Double], normalized.count == 4 else {
            return report
        }
        return report.merging(readingFields(normalized)) { _, new in new }
    }

    /// `histogram source scalar`'s report of a Lab channel: `target` and `scope.target` carry the
    /// Lab name (`channel`), with the rule that reads a code as L*, a* or b*.
    static func scalarReport(_ report: [String: Any], channel: String) -> [String: Any] {
        var out = report
        out["target"] = channel
        if var scope = out["scope"] as? [String: Any] {
            scope["target"] = channel
            out["scope"] = scope
        }
        out["lab_value_rule"] = "lightness: L* = 100·code/65535; a, b: 255·code/65535 − 128"
        return out
    }

    /// The Info panel's footnote on a Lab document.
    static let infoFootnote = "Lab Color samples: L* 0…100, a* and b* −128…127 (D50), the "
        + "document's own values. The histogram counts the chosen source's Lightness, a and b. RGB "
        + "and HSB are unavailable in this mode."

    /// The Info panel's histogram menu title of `display` on a Lab document; nil hides the item
    /// (luminosity and the RGB overlay say nothing about L*a*b* codes).
    static func histogramTitle(_ display: HistogramView.Display) -> String? {
        switch display {
        case .red: return channelName(.red)
        case .green: return channelName(.green)
        case .blue: return channelName(.blue)
        case .luminosity, .rgb: return nil
        }
    }

    /// `histogram`'s composite/layer/backdrop report on a Lab document: the Lightness, a and b
    /// rows only, no luma.
    static func histogramReport(_ report: [String: Any]) -> [String: Any] {
        var out = report.filter { !$0.key.hasSuffix("_luma") }
        for key in ["bins", "bin_weight_units"] {
            if let rows = out[key] as? [Any] { out[key] = Array(rows.prefix(3)) }
        }
        out["channels"] = ["lightness", "a", "b"]
        out["lab_bin_rule"] = "bin = round(code·255/65535): L* = 100·bin/255; a*, b* = bin − 128"
        out["note"] = "The _rgba arrays hold Lightness, a, b and alpha codes."
        return out
    }

    /// `get_precision`'s (and `get_document`'s `precision_support`) answer for a Lab document:
    /// this gate's own tools and menu items, not the RGB depth's list.
    static func support(_ doc: RasterDocument) -> [String: Any] {
        let blendNames = NativeLayerBlending.labModes.map { DrBlendMode.displayName(for: $0) }
        return [
            "storage": "rgba16_unorm", "color_mode": "lab", "bits_per_channel": doc.labDepth ?? 0,
            "white_point": "D50",
            "supported_tools": tools.sorted(), "supported_menu_items": selectors.sorted(),
            "fill_targets": ["layer", "mask", "lightness", "a", "b", "alpha", "saved channel"],
            "channels": ["lightness", "a", "b"],
            "blend_modes": blendNames + ["Pass Through (groups)"],
            "filter_plans": (doc.precisionInfo?["lab"] as? [String: Any])?["filter_plans"] ?? NSNull(),
        ]
    }

    /// Why a Lab TIFF refuses another depth: it keeps the document's own.
    static func tiffDepthRefusal(_ depth: Int) -> String {
        "A Lab TIFF keeps the document's own depth (\(depth) bits per channel)."
    }

    /// The space a Lab document's display bytes are in and tagged with (`RasterDocument.colorSpace`):
    /// the core's built-in Display P3, a CMYK document's view space. The core converts the u16 Lab
    /// codes into it before any narrowing (`lab_convert::lab_display`), so ColorSync never decodes
    /// the twin for display — it reads every a*/b* half a unit high.
    static let viewSpace: CGColorSpace = ColorProfile.space(for: RasterProfile.builtin(.displayP3))

    /// A Lab projection's display bytes: the core's Display P3 view at u16, then the ordinary
    /// preview. `RasterDocument.display(of:)` routes a Lab document here. `control` is single-use,
    /// so each core call takes a child of it.
    static func display(of image: RasterPrecisionImage, context: PrecisionWorkContext,
                        control: RasterJobControl?) throws -> RasterImage {
        guard let control else {
            let view = try image.labDisplay(workingLimit: context.limit,
                                            otherRetained: context.otherRetained)
            let narrowing = try PrecisionWorkContext(image: view)
            return try view.previewRGBA8(workingLimit: narrowing.limit,
                                         otherRetained: narrowing.otherRetained)
        }
        let view = try control.withChildOperation(from: 0, to: 0.6) {
            try image.labDisplay(workingLimit: context.limit, otherRetained: context.otherRetained,
                                 control: $0)
        }
        let narrowing = try PrecisionWorkContext(image: view)
        return try control.withChildOperation(from: 0.6, to: 1) {
            try view.previewRGBA8(workingLimit: narrowing.limit,
                                  otherRetained: narrowing.otherRetained, control: $0)
        }
    }

    /// A Lab layer's Layers-panel thumbnail: fitted at u16 in the core FIRST, then converted into
    /// the Display P3 view and narrowed (the fit-first rule; no whole-layer view).
    static func thumbnail(of source: RasterPrecisionImage, maxSide: Int,
                          context: PrecisionWorkContext) throws -> RasterImage {
        try source.labThumbnailRGBA8(maxSide: maxSide, workingLimit: context.limit,
                                     otherRetained: context.otherRetained)
    }

    /// The canvas tools a Lab document admits: the selections, the view, (phase 2a) the Brush,
    /// Eraser, Paint Bucket and Gradient, (phase 2d) the Color Sampler, (phase 2e) the
    /// Eyedropper, the retouch tools and Content-Aware Move, and (2f's third part) Remove, the Art
    /// History Brush and Red Eye, (phase 4a) the Crop, Perspective Crop and Ruler tools, and (phase
    /// 4a-ii) the Move tool (its selected pixels since 4b-i; a colour or alpha plane's since 6b-i),
    /// (phase 6a-i) the Text, Shape and Pen tools, (phase 6b-ii) the Quick Selection, Object
    /// Selection and Select Subject tools, (the close) the Magnetic Lasso, whose edges read the
    /// Display P3 analysis composite, and the Magic Wand, by the take's Lab rule at both
    /// depths (`selectionRefusal` refuses no `select_magic_wand`).
    static func allows(_ tool: EditorTool) -> Bool {
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .hand, .zoom, .move,
             .quickSelect, .objectSelect, .subject, .wand,
             .brush, .eraser, .fill, .gradient, .colorSampler, .eyedropper,
             .dodge, .sponge, .blurBrush, .sharpenBrush, .smudge, .historyBrush, .clone, .heal,
             .spotHeal, .patch, .contentAwareMove, .remove, .artHistoryBrush, .redEye,
             .crop, .perspectiveCrop, .ruler, .magneticLasso,
             .text, .shapeRect, .shapeEllipse, .shapeLine, .pen:
            return true
        default:
            return false
        }
    }
}
