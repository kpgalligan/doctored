import AppKit

// The Grayscale document's gate (#1, closed): what a Gray document admits from the
// canvas, the menus and MCP, asked FIRST wherever a depth decides (`DocumentPrecision.refusal`,
// `allows`, `validatePrecisionItem`), before the CMYK gate. The core's own table is
// `precision_capability::GRAY_FAMILIES` and its edit gate `gray_document::admit_edit`; this is
// their host side, written once. Phase 1a opens, views, converts and saves (`.rz`) a Gray document
// and edits its layer STRUCTURE, CMYK phase 1's list; phase 1b adds New Layer, groups, masks,
// clipping, merges and Stamp Visible, Fill Opacity, the blend modes of
// `NativeLayerBlending.grayModes`, Edit ▸ Fill and the one-channel PNG, TIFF and JPEG saves and
// exports; phase 1c adds Export As / Quick Export, `render`'s gray channel, the Channels panel's
// Gray row, the Info panel's K % and `get_precision`'s own list (`support`); phase 2a-ii the
// Brush, Eraser, Paint Bucket and Gradient (their inks greyed by the core), phase 2b the
// adjustments of `GrayAdjust` and phase 2c the Filter menu (`GrayAdjust.allowsFilterAction`), Fade,
// Actions and the retouch tools — Dodge, Burn, Sponge, Blur, Sharpen, Smudge, the History Brush,
// Clone Stamp, Healing, Spot Healing and Patch — on R = G = B through their RGB kernels; Remove
// Red Eye refuses for good (`redEyeRefusal`). Phase 2d adds every layer effect (its colours stored
// as their gray by the core, `gray_style`; its modes `effectModes(for:)`), Blend If on Gray only,
// the global light, the style presets and the alpha channels as structure and as paint, Fill and
// filter targets (`channel:<name>`; the Gray channel target is the layer, and the colour planes
// refuse for good, `planeRefusal`). Phase 3b adds File ▸ Export as PSD (`export_psd`,
// `inspect_psd_export`): a Grayscale PSD, PSB or layered TIFF at the document's declared 8 or 16
// bits. Phase 3c adds 32 bits: a 32-bit Gray PSD or PSB opens and is written (its layered TIFF
// refused, as for RGB float), and a 32-bit document's kept-gray TIFF is a one-channel float TIFF
// (`defaultBitDepth`). Phase 2e adds Stroke…, Content-Aware Fill and Move, the Remove tool, the
// Art History Brush (8 bits; its modes Normal, Darken and Lighten), HDR Toning at 8 and 16 bits
// and the Brush, Clone Stamp and Art History Mode popups narrowed to the Grayscale list. Phase 4
// adds geometry (Image and Canvas Size, Crop, Perspective Crop, Trim, Reveal All, rotation and
// flips, Free Transform, Warp applied at once, Puppet and Perspective Warp, Liquify, the Move
// tool, align/distribute), the selected pixels, copy and paste across modes (`GrayClipboard.swift`;
// the pasteboard's gray image is a one-channel TIFF), Layer Via, Duplicate to Document, Stamp
// Selected, Merge Group and the selections (the wand, Color Range, Similar, Subject and the
// subject tools, Select and Mask, Quick Mask, Save/Load Selection with `plane: "gray"`,
// luminosity masks). Phase 5a adds Edit ▸ Assign Profile and Convert to Profile between Gray
// profiles and Image ▸ Mode ▸ 8/16/32 Bits/Channel (`GrayProfileEdit`), and HDR Toning of a 32-bit
// document into 8 or 16 bits. Phase 5b adds View ▸ Proof Setup, Proof Colors and Gamut Warning
// (`proofSelectors`; the RGB proof of the R = G = B composite in the Gray profile's twin, at 8 and
// 16 bits) and Image ▸ Mode ▸ CMYK Color straight from Gray (`CmykModeConversion`, the RGB → CMYK
// conversion on the twin). Phase 6a-i adds type, shape and fill layers (the Type, shape and Pen
// tools, the fill layer items, Rasterize and Artwork Colors; each colour stored as its gray,
// `GrayArtwork.swift`), the Paths panel, Fill and Stroke Path and the vector masks. Phase 6a-ii
// adds layer comps (`gray_comps`), Link and Unlink Layers, the snapshots (written and reopened as
// the Gray `.rz`) and Edit ▸ Clear (`clear_selection`, the legacy `erase`). Phase 6b adds the
// Histogram (the Gray row, `histogramReport`), Apply Image and Calculations (the Gray, Alpha and
// alpha-channel planes; `NativeChannelMath.applyGray`), Stack Modes and the stack tools, Load Files
// into Stack, Merge to HDR and Merge Panorama (later files converted into the stack's Gray), the model
// selections, and the neural and generative tools (their results folded to their gray by the
// core); Neural Colorize and Sky Replacement refuse for good (`colorizeRefusal`, `skyRefusal`).
// The close lifts the tools no phase named: Arrange, Distort, the layer finders, the views and
// workspaces, the Color Samplers, File Info and metadata, Image Size's resolution, the ruler
// origin, the history budget, recovery, the horizon and lens profiles, Modify Native Plane,
// proof presets, Separate Color, Export Layers, PDF, the contact sheets, the 32-bit tone map, and
// Place and Live Photo layers (the core folds the placed pixels and keeps no source). Camera RAW
// layers (`rawTools`), the 8-bit byte tools (`byteTools`) and linear-light evaluation
// (`evaluationTools`) refuse for good by name.
// The admitted paint, adjustment, filter, retouch, channel, geometry, selection, clipboard, proof,
// artwork, analysis and multi-image tools (`depthRuled`) then answer the depth's own rules as on
// an RGB document; the other lifted tools run their own depth checks. Every other tool refuses
// with the core's RGB-only sentence (`designs/grayscale-mode.md`).

extension RasterDocument {
    /// True for a Grayscale document — read off the captured descriptor (`color_mode`), so a
    /// per-row panel or menu check costs no FFI call.
    var isGray: Bool { precisionInfo?["color_mode"] as? String == "gray" }

    /// A Grayscale document's declared depth (8, 16 or 32); nil for every other document.
    var grayDepth: Int? { (precisionInfo?["gray"] as? [String: Any])?["depth"] as? Int }

    /// The document's colour mode, Image ▸ Mode's check mark.
    var colorMode: ColorModeTarget {
        if isBitmap { return .bitmap }
        // #1 Duotone phase 1a: a Duotone document is a Gray one with a description.
        if isDuotone { return .duotone }
        if isGray { return .gray }
        if isLab { return .lab }
        if isIndexed { return .indexed }
        return isCmyk ? .cmyk : .rgb
    }
}

enum GrayDocument {
    /// Every MCP tool a Gray document answers: CMYK phase 1's list (`ad8db093`'s `.cmykTool`
    /// set), then phase 1b's layer structure, Fill and export, and phase 3b's PSD export.
    /// `AgentServer+Registry.swift` refuses to start when one is not a registered tool.
    static let tools: Set<String> = [
        "convert_mode", "get_color_profile", "get_document", "render", "sample_color", "sample_pixel",
        "set_active_layer", "flatten_image", "select_rect", "select_ellipse", "select_polygon",
        "select_freehand", "select_all", "deselect", "reselect", "get_selection", "undo", "redo",
        "save_copy", "set_layer_lock", "set_selected_layers", "duplicate_layer", "delete_layer",
        "list_guides", "add_guide", "remove_guide", "clear_guides", "list_history", "restore_history",
        "set_layer_label", "reorder_layer", "set_layer_properties", "reorder_layers",
        "close_document", "get_precision", "get_recovery_status", "write_recovery",
        // Phase 1b.
        "new_layer", "group_layers", "ungroup_layers", "add_layer_mask", "remove_layer_mask",
        "set_layer_mask_enabled", "set_mask_properties", "set_layer_clipped", "merge_down",
        "merge_visible", "stamp_visible", "set_layer_style", "fill_selection", "export_image",
        // Phase 1c: Export As's preview and Quick Export, through the same kept-gray export.
        "export_preview", "quick_export",
        // Phase 2a-ii: painting; phase 2b: the adjustments (`GrayAdjust`).
        "brush_stroke", "eraser_stroke", "fill", "gradient", "apply_filter", "add_adjustment_layer",
        "edit_adjustment_layer", "auto_tone", "auto_contrast", "equalize",
        // Phase 2c: the retouch tools, Fade, Depth Blur and Actions (each step passes this gate).
        "dodge_burn", "sponge_stroke", "blur_stroke", "sharpen_stroke", "smudge_stroke",
        "history_brush_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "patch_region",
        "fade", "depth_blur", "run_action",
        // Phase 2d: the global light, the style presets and the alpha channels.
        "set_global_light", "apply_style_preset", "save_style_preset", "list_channels",
        "add_channel", "duplicate_channel", "delete_channel", "rename_channel",
        "set_channel_options", "invert_channel", "add_spot_channel", "set_spot_channel",
        "merge_spot_channel", "set_channel_kind",
        // Phase 3b: Export as PSD at the declared depth.
        "export_psd", "inspect_psd_export",
        // Phase 2e: Stroke…, the content-aware tools, Remove, the Art History Brush, HDR Toning.
        "stroke_selection", "content_aware_fill", "preview_content_aware_fill",
        "discard_content_aware_fill_preview", "content_aware_move", "remove_stroke",
        "art_history_brush_stroke", "hdr_toning",
        // Phase 4: geometry, transforms and warps, align/distribute and the Move tool, the selected
        // pixels, copy and paste (`GrayClipboard.swift`), Layer Via, Duplicate to Document, Stamp
        // Selected, Merge Group and the selections, on R = G = B through the RGB kernels.
        "image_size", "canvas_size", "crop", "perspective_crop", "fit_crop", "trim",
        "reveal_canvas", "rotate", "flip", "straighten_layers", "transform_layer",
        "transform_layers", "transform_again", "transform_mask", "transform_selection",
        "get_warp", "warp_layer", "puppet_warp", "perspective_warp", "liquify", "align_layers",
        "distribute_layers", "move_layers", "move_selected_pixels", "transform_selected_pixels",
        "copy", "cut", "copy_merged", "paste", "layer_via_copy", "layer_via_cut",
        "duplicate_to_new_document", "copy_layers_to_document", "stamp_selected", "merge_group",
        "select_magic_wand", "select_color_range", "select_similar", "quick_select",
        "select_subject", "select_subjects", "inspect_subjects", "select_people",
        "select_face_parts", "inspect_faces", "object_select", "refine_selection",
        "select_magnetic", "modify_selection", "save_selection", "load_selection",
        "set_quick_mask", "quick_mask_stroke", "add_luminosity_masks",
        // Phase 5a: Assign and Convert to Profile between Gray profiles, 8/16/32 Bits/Channel
        // (`AgentServer+GrayColour.swift`).
        "assign_profile", "convert_profile", "convert_precision",
        // Phase 5b: Proof Setup and Proof Colors (`AgentServer+SoftProof.swift`).
        "get_proof_setup", "set_proof_setup", "render_proof",
        // Phase 6a-i: type, shape and fill layers (each colour stored as its gray,
        // `GrayArtwork.swift`), paths and vector masks.
        "add_text_layer", "edit_text_layer", "add_shape_layer", "edit_shape_layer", "add_fill_layer",
        "edit_fill_layer", "rasterize_artwork", "list_paths", "set_path", "rename_path",
        "duplicate_path", "delete_path", "selection_to_path", "path_to_selection", "fill_path",
        "stroke_path", "get_vector_mask", "set_vector_mask", "set_vector_mask_properties",
        "remove_vector_mask", "set_clipping_path",
        // Replace Missing Fonts (MissingFonts.swift), each colour kept as its gray.
        "list_missing_fonts", "replace_missing_fonts",
        // Phase 6a-ii: links, layer comps, snapshots and Edit ▸ Clear.
        "link_layers", "unlink_layers", "list_layer_comps", "save_layer_comp", "apply_layer_comp",
        "update_layer_comp", "delete_layer_comp", "reorder_layer_comp", "list_snapshots",
        "create_snapshot", "rename_snapshot", "delete_snapshot", "restore_snapshot", "save_snapshot",
        "load_snapshot", "clear_selection",
        // Phase 6b: the Histogram, Apply Image and Calculations, Stack Modes and the stack tools,
        // Merge to HDR, the model selections, the neural and generative tools (their results folded
        // to their gray by the core, `gray_artwork::folded_result`).
        "histogram", "apply_image", "calculations", "stack_mode", "reduce_noise_from_stack",
        "auto_align_layers", "auto_blend_layers", "merge_layers_to_hdr", "select_sky",
        "select_person_parts", "object_finder_select", "super_zoom", "jpeg_artifacts_removal",
        "generative_fill", "generative_expand", "generative_variations",
        "select_generative_variation", "generate_variations",
        // The close: the tools no phase named, on R = G = B through the RGB paths; Place and Live
        // Photo layers land folded to their gray with no retained source (the core's Gray arm).
        "arrange_layer", "distort_layer", "auto_select_layer", "find_layers",
        "get_mask_properties", "list_views", "open_second_view", "close_second_view", "get_view",
        "set_view", "save_workspace", "restore_workspace", "get_color_samplers",
        "set_color_samplers", "get_file_info", "set_file_info", "get_metadata", "set_resolution",
        "set_ruler_origin", "set_history_budget", "recover_document", "discard_recovery",
        "detect_horizon", "list_lens_profiles", "modify_scalar", "save_proof_preset",
        "separate_color", "export_layers", "export_pdf", "contact_sheet",
        "contact_sheet_from_sources", "get_tone_map", "set_tone_map", "place_image",
        "add_live_photo_layer",
        // Edit ▸ Define Pattern… (the gray code at 8 bits, `DefinePattern`).
        "define_pattern",
    ]

    /// The admitted tools that then answer the depth's own rules (`DocumentPrecision.refusal`):
    /// the float and 16-bit paint, target and capability checks, as on an RGB document.
    static let depthRuled: Set<String> = [
        "brush_stroke", "eraser_stroke", "fill", "gradient", "apply_filter", "add_adjustment_layer",
        "edit_adjustment_layer", "auto_tone", "auto_contrast", "equalize",
        "dodge_burn", "sponge_stroke", "blur_stroke", "sharpen_stroke", "smudge_stroke",
        "history_brush_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "patch_region",
        "fade", "depth_blur",
        "list_channels", "add_channel", "duplicate_channel", "delete_channel", "rename_channel",
        "set_channel_options", "invert_channel",
        "stroke_selection", "content_aware_fill", "preview_content_aware_fill",
        "discard_content_aware_fill_preview", "content_aware_move", "remove_stroke",
        "art_history_brush_stroke", "hdr_toning",
        // Phase 4: geometry, transforms and warps, align/distribute and the Move tool, the selected
        // pixels, copy and paste (`GrayClipboard.swift`), Layer Via, Duplicate to Document, Stamp
        // Selected, Merge Group and the selections, on R = G = B through the RGB kernels.
        "image_size", "canvas_size", "crop", "perspective_crop", "fit_crop", "trim",
        "reveal_canvas", "rotate", "flip", "straighten_layers", "transform_layer",
        "transform_layers", "transform_again", "transform_mask", "transform_selection",
        "get_warp", "warp_layer", "puppet_warp", "perspective_warp", "liquify", "align_layers",
        "distribute_layers", "move_layers", "move_selected_pixels", "transform_selected_pixels",
        "copy", "cut", "copy_merged", "paste", "layer_via_copy", "layer_via_cut",
        "duplicate_to_new_document", "copy_layers_to_document", "stamp_selected", "merge_group",
        "select_magic_wand", "select_color_range", "select_similar", "quick_select",
        "select_subject", "select_subjects", "inspect_subjects", "select_people",
        "select_face_parts", "inspect_faces", "object_select", "refine_selection",
        "select_magnetic", "modify_selection", "save_selection", "load_selection",
        "set_quick_mask", "quick_mask_stroke", "add_luminosity_masks",
        // Phase 5b: the proof tools refuse a 32-bit document as a float document.
        "get_proof_setup", "set_proof_setup", "render_proof",
        // Phase 6a-i: type, shape and fill layers (each colour stored as its gray,
        // `GrayArtwork.swift`), paths and vector masks.
        "add_text_layer", "edit_text_layer", "add_shape_layer", "edit_shape_layer", "add_fill_layer",
        "edit_fill_layer", "rasterize_artwork", "list_paths", "set_path", "rename_path",
        "duplicate_path", "delete_path", "selection_to_path", "path_to_selection", "fill_path",
        "stroke_path", "get_vector_mask", "set_vector_mask", "set_vector_mask_properties",
        "remove_vector_mask", "set_clipping_path",
        // Replace Missing Fonts (MissingFonts.swift), each colour kept as its gray.
        "list_missing_fonts", "replace_missing_fonts",
        // Phase 6a-ii: Edit ▸ Clear (the legacy `erase`, u16 storage only).
        "clear_selection",
        // Phase 6b: the Histogram, Apply Image and Calculations, Stack Modes and the stack tools,
        // Merge to HDR, the model selections, the neural and generative tools (their results folded
        // to their gray by the core, `gray_artwork::folded_result`).
        "histogram", "apply_image", "calculations", "stack_mode", "reduce_noise_from_stack",
        "auto_align_layers", "auto_blend_layers", "merge_layers_to_hdr", "select_sky",
        "select_person_parts", "object_finder_select", "super_zoom", "jpeg_artifacts_removal",
        "generative_fill", "generative_expand", "generative_variations",
        "select_generative_variation", "generate_variations",
        // The close: Distort, Modify Native Plane and the proof presets.
        "distort_layer", "modify_scalar", "save_proof_preset",
    ]

    /// Phase 5b's View ▸ Proof Setup, Proof Colors and Gamut Warning items, enabled at 8 and 16
    /// bits (`validatePrecisionItem` answers false at 32, as for an RGB float document).
    static let proofSelectors: Set<String> = ["showProofSetup:", "toggleProofColors:",
                                              "toggleGamutWarning:", "applyProofPresetFromMenu:",
                                              "applyWorkingProof:"]

    /// The menu items a Gray document enables: CMYK phase 1's list, plus Image ▸ Mode ▸
    /// Grayscale, plus `proofSelectors`. View-only and window items are answered before this list.
    static let selectors: Set<String> = Set<String>([
        "convertToRGBMode:", "convertToCMYKMode:", "convertToGrayscaleMode:",
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
        // Phase 1c: File ▸ Export As and Quick Export.
        "showExportWorkspace:", "quickExport:",
        // Phase 2b: Equalize and an adjustment layer's options (the adjustment items are
        // `GrayAdjust.allowsAction`'s).
        "equalize:", "adjustmentOptions:",
        // Phase 2c: Fade and Actions (the Filter items are `GrayAdjust.allowsFilterAction`'s).
        "showFade:", "showActions:", "startRecording:", "stopRecording:",
        // Phase 2d: the layer effects, Copy/Paste Layer Style and the alpha channels (Invert
        // Channel at 8 and 16 bits: the menu branch answers false at 32).
        "layerStyleEffect:", "copyLayerStyle:", "pasteLayerStyle:", "newChannel:",
        "duplicateChannel:", "renameChannel:", "channelOptions:", "deleteChannel:", "invertChannel:",
        // Phase 3b: File ▸ Export as PSD.
        "exportPSD:",
        // Phase 2e: Edit ▸ Stroke…, Content-Aware Fill… and HDR Toning… (at 32 bits since 5a).
        "strokeSelection:", "contentAwareFill:", "showHdrToning:",
        // Phase 4: CMYK's phase 4b (geometry, transforms, warps, Liquify, align/distribute) and
        // 4c (copy and paste, Layer Via, Duplicate to Document, Stamp Selected), its selection
        // batch, and the Select menu, Quick Mask, Save/Load Selection and luminosity masks.
        "resizeImage:", "showCanvasSize:", "cropToSelection:", "showTrim:", "revealAllCanvas:",
        "rotateCW:", "rotateCCW:", "rotate180:", "rotateArbitrary:", "flipH:", "flipV:",
        "freeTransform:", "freeTransformCopy:", "transformAgain:", "transformAgainWithCopy:",
        "showPersistedWarp:", "transformMask:", "showPuppetWarp:", "showPerspectiveWarp:",
        "showLiquify:", "alignLeft:", "alignCenterX:", "alignRight:", "alignTop:",
        "alignCenterY:", "alignBottom:", "distributeHorizontally:", "distributeVertically:",
        "copy:", "copyMerged:", "cut:", "paste:", "pasteAsNewLayer:", "pasteInPlace:",
        "definePattern:",
        "pasteInto:", "pasteOutside:", "layerViaCopy:", "layerViaCut:", "duplicateToNewDocument:",
        "stampSelected:",
        "showColorRange:", "refineSelection:", "selectSubject:", "showSubjectTargets:",
        "showPeopleTargets:", "selectPeople:", "showFaceParts:",
        "invertSelection:", "featherSelection:", "growSelection:", "shrinkSelection:",
        "borderSelection:", "smoothSelection:", "selectGrow:", "selectSimilar:",
        "transformSelection:", "toggleQuickMask:", "saveSelectionSheet:", "loadSelectionSheet:",
        "loadChannelAsSelection:", "addLuminosityMasks:",
        // Phase 5a: Edit ▸ Assign Profile… / Convert to Profile… and Image ▸ Mode ▸ 8/16/32
        // Bits/Channel (`validateBitDepthItem` checks the declared depth).
        "assignProfile:", "convertToProfile:", "convertToEightBits:", "convertToSixteenBits:",
        "convertToFloatPrecision:",
        // Phase 6a-i: fill layers, Layer Content Options, Rasterize and Artwork Colors, and the
        // Paths panel, Fill and Stroke Path and the vector masks (CMYK's list).
        "newSolidColorFillLayer:", "newGradientFillLayer:", "newPatternFillLayer:",
        "layerContentOptions:", "rasterizeArtwork:", "editArtworkColors:", "replaceMissingFonts:",
        "makeWorkPath:",
        "makeSelectionFromPath:", "duplicateCurrentPath:", "deleteCurrentPath:", "fillCurrentPath:",
        "strokeCurrentPath:", "toggleClippingPath:", "editClippingPathFlatness:",
        "addVectorMaskFromCurrentPath:", "addVectorMaskRevealAll:",
        "deleteVectorMask:", "toggleVectorMaskEnabled:", "toggleVectorMaskLinked:",
        // Phase 6a-ii: Link and Unlink Layers, the Snapshots window and Edit ▸ Clear (Layer Comps
        // is a depth-independent window action).
        "linkLayers:", "unlinkLayers:", "showSnapshots:", "clearSelection:",
        // Phase 6b: Apply Image, Calculations, Stack Modes, the stack tools, Merge to HDR, Select
        // Sky and Person Parts, and the neural and generative filters (Colorize and Sky
        // Replacement stay disabled: `colorizeRefusal`, `skyRefusal`).
        "applyImageSheet:", "calculationsSheet:", "stackMode:", "showReduceNoiseFromStack:",
        "autoAlignLayers:", "showAutoBlendLayers:", "mergeLayersToHDR:", "selectSky:",
        "showPersonParts:", "showSuperZoom:", "showJpegArtifactsRemoval:", "showGenerativeFill:",
        "showGenerativeExpand:", "showGenerativeVariations:",
        // The close: Place Embedded/Linked and Live Photo, Export Layers, the contact sheet, PDF,
        // File Info, Modify Native Plane and the 32-bit tone map (`floatOnly`).
        "placeEmbeddedImage:", "placeLinkedImage:", "placeLivePhoto:", "exportLayers:",
        "showContactSheet:", "exportAsPDF:", "showFileInfo:", "modifyNativePlane:", "showToneMap:",
    ]).union(proofSelectors)

    /// Phase 6b's items a 32-bit Gray document disables, as an RGB float document does: Apply
    /// Image writes u16 planes, Merge to HDR takes 8- or 16-bit exposures, and the neural and
    /// generative filters refuse float (`validatePrecisionItem` answers false at 32).
    static let floatRefused: Set<String> = [
        "applyImageSheet:", "mergeLayersToHDR:", "showSuperZoom:", "showJpegArtifactsRemoval:",
        "showGenerativeFill:", "showGenerativeExpand:", "showGenerativeVariations:",
        // The close: Modify Native Plane edits u16 planes, as on an RGB float document.
        "modifyNativePlane:",
    ]

    /// The close's items a Gray document enables at 32 bits only, as an RGB document does: the
    /// tone map is a float document's (`validatePrecisionItem` answers false below 32).
    static let floatOnly: Set<String> = ["showToneMap:"]

    /// The camera RAW tools, refused for good: Doctored keeps a RAW layer's development on RGB
    /// documents only. `save_raw_preset`'s `raw` form is document-free and stays admitted.
    static let rawTools: Set<String> = [
        "apply_raw_preset", "apply_raw_settings", "copy_raw_local_settings", "edit_raw_development",
        "get_raw_local_mask", "get_raw_locals", "get_raw_source", "inspect_raw_development",
        "migrate_raw_decoder", "preview_raw_decoder_migration", "redevelop_raw", "save_raw_preset",
        "sync_raw_settings", "get_image_source", "rasterize_image_source",
    ]

    /// A camera RAW tool's permanent refusal (`rawTools`); also Place's for a camera file.
    static func rawRefusal(tool: String) -> String {
        "\(tool) is unavailable in a Grayscale document: Doctored keeps camera RAW layers on RGB "
            + "documents only."
    }

    /// The 8-bit byte tools, refused for good (`byteRefusal`).
    static let byteTools: Set<String> = ["add_text", "put_pixels"]

    /// A byte tool's permanent refusal: its pixels are 8-bit RGB(A).
    static func byteRefusal(tool: String) -> String {
        tool == "add_text"
            ? "add_text is unavailable in a Grayscale document: it draws 8-bit RGB pixels; "
                + "add_text_layer adds editable type."
            : "put_pixels is unavailable in a Grayscale document: it writes 8-bit RGBA bytes, "
                + "which Doctored offers on 8-bit RGB documents only."
    }

    /// Linear-light evaluation, refused for good (Doctored's decision: the Gray profile edit never
    /// rebinds the transfer, `gray_profile`).
    static let evaluationTools: Set<String> = ["get_evaluation", "set_evaluation"]

    /// An evaluation tool's permanent refusal.
    static func evaluationRefusal(tool: String) -> String {
        "\(tool) is unavailable in a Grayscale document: Doctored offers linear-light evaluation "
            + "on RGB documents only."
    }

    /// The core's default sentence (`gray_document::refused_sentence`) for `tool`.
    static func refusal(tool: String) -> String {
        "\(tool) is unavailable in a Grayscale document: Doctored offers it on RGB documents only."
    }

    /// The host's colour-plane target sentence, the core's `gray_document::PLANE_REFUSAL` by tool:
    /// The Gray row is the composite row, so the Gray channel target IS the layer.
    static func planeRefusal(tool: String, target: String) -> String {
        "\(tool) target \"\(target)\" is unavailable in a Grayscale document: its one colour "
            + "channel, Gray, is the layer, and Doctored offers the Red, Green, Blue and Alpha "
            + "targets on RGB documents only."
    }

    /// The effect blend modes a Gray document offers: its layer modes at 8 and 16 bits; at 32 bits
    /// those the float document admits on an effect (`FloatDocument.effectModes`).
    static func effectModes(for doc: RasterDocument) -> [DrBlendMode] {
        guard doc.isFloat else { return NativeLayerBlending.grayModes(for: doc) }
        let float = FloatDocument.effectModes
        return NativeLayerBlending.grayModes.filter { float.contains($0) }
    }

    /// The core's Remove Red Eye sentence (`gray_filter::RED_EYE_REFUSAL`): a colour repair, offered
    /// on RGB documents only.
    static let redEyeRefusal = "Remove Red Eye is unavailable in a Grayscale document: Doctored "
        + "offers it on RGB documents only."

    /// Neural Colorize's permanent refusal (#1 phase 6b, Doctored's decision): its colour has
    /// nowhere to go in a one-channel layer.
    static let colorizeRefusal = "Neural Colorize is unavailable in a Grayscale document: a "
        + "Grayscale layer holds no colour. Convert to RGB Color first."

    /// Sky Replacement's permanent refusal (#1 phase 6b, Doctored's decision): its New Layers
    /// output carries colour Curves the Gray adjustment table refuses.
    static let skyRefusal = "Sky Replacement is unavailable in a Grayscale document: Doctored "
        + "offers it on RGB documents only."

    /// Why `histogram source scalar` refuses the Gray or layer target (#1 phase 6b): the Gray
    /// channel is the layer, whose histogram is the composite/layer source's.
    static let scalarTargetRefusal = "A Grayscale document's scalar histogram takes target mask or "
        + "channel:<name>."

    /// The Info panel's histogram menu title of `display` on a Gray document; nil hides the item
    /// (the colour rows and the RGB overlay repeat Gray).
    static func histogramTitle(_ display: HistogramView.Display) -> String? {
        switch display {
        case .luminosity: return "Gray"
        case .red, .green, .blue, .rgb: return nil
        }
    }

    /// `histogram`'s composite/layer/backdrop report on a Gray document: the Gray row only (R = G
    /// = B, so row 0), no luma.
    static func histogramReport(_ report: [String: Any]) -> [String: Any] {
        var out = report.filter { !$0.key.hasSuffix("_luma") }
        for key in ["bins", "bin_weight_units"] {
            if let rows = out[key] as? [Any] { out[key] = Array(rows.prefix(1)) }
        }
        out["channels"] = ["gray"]
        out["note"] = "The _rgba arrays hold Gray in R, G and B, then alpha."
        return out
    }

    /// The bit depth a one-channel save or export of `doc` writes by default: 8 for JPEG or an
    /// 8-bit document, 32 for a TIFF of a 32-bit document (a one-channel float TIFF, #1 phase
    /// 3c), else 16 (a 32-bit document's PNG writes 16: the format has no 32-bit form).
    static func defaultBitDepth(_ doc: RasterDocument, format: ExportFormat) -> Int {
        if format == .jpeg || doc.grayDepth == 8 { return 8 }
        return format == .tiff && doc.grayDepth == 32 ? 32 : 16
    }

    /// What a one-channel PNG, TIFF or JPEG of `doc` written at `bitDepth` loses: the core's
    /// editable-state list (guides, channels, samplers, paths, comps, the global light, layers,
    /// masks, styles, layer properties and bounds; transparency in a JPEG, which has no alpha),
    /// the depth the file is written below the document's (a 32-bit document's tone-mapped
    /// values below 32 bits, `PrecisionExportCapture.floatDepthLoss`; its tone map in a 32-bit
    /// TIFF when it is not clip), the profile when it is not embedded and the capture packets it
    /// strips or has no slot for (`flatPacketLosses`: IPTC in a PNG). `[]` only when the file
    /// reopens as the same document, which Save keeps in place on
    /// (`ImageDocument.canPreserveState`). Any other format is the `.doctored`/PNG/TIFF/JPEG
    /// line.
    static func saveLosses(_ doc: RasterDocument, format: ExportFormat?, bitDepth: Int,
                           embedProfile: Bool, stripMetadata: Bool) throws -> [String] {
        guard let format, [.png, .tiff, .jpeg].contains(format) else {
            return ["editable Grayscale document state; a Grayscale document saves as .doctored, PNG, TIFF or JPEG"]
        }
        guard let typed = doc.precision else { throw DoctoredCoreError(message: "No Grayscale document is loaded.") }
        var losses = try typed.flatStateLosses(alphaKept: format != .jpeg, tiff: format == .tiff)
        if let depth = doc.grayDepth, bitDepth < depth {
            losses.append(depth == 32
                ? PrecisionExportCapture.floatDepthLoss(bitDepth: bitDepth)
                    ?? "32-bit float values (written at \(bitDepth) bits)"
                : "\(depth)-bit Grayscale precision (" + (format == .jpeg
                    ? "JPEG holds 8 bits)" : "written at \(bitDepth) bits)"))
        } else if doc.grayDepth == 32, let line = PrecisionExportCapture.toneMapLoss(doc) {
            losses.append(line)
        }
        if !embedProfile { losses.append("embedded color profile") }
        return losses + doc.flatPacketLosses(format: format, stripMetadata: stripMetadata)
    }

    /// The gate's answer for one MCP call: nil to admit it, else the refusal. An adjustment's
    /// permanent refusal comes first (`GrayAdjust.toolRefusal`), then the permanent sets
    /// (`rawTools`, `byteTools`, `evaluationTools`). Admitted tools whose ARGUMENTS can still
    /// reach a path the mode does not hold are narrowed here.
    static func refusal(tool: String, arguments: [String: Any], document: RasterDocument) -> String? {
        if let reason = GrayAdjust.toolRefusal(tool: tool, arguments: arguments) { return reason }
        if ["red_eye", "red_eye_auto"].contains(tool) { return redEyeRefusal }
        if tool == "neural_colorize" { return colorizeRefusal }
        if tool == "sky_replacement" { return skyRefusal }
        if rawTools.contains(tool) { return rawRefusal(tool: tool) }
        if byteTools.contains(tool) { return byteRefusal(tool: tool) }
        if evaluationTools.contains(tool) { return evaluationRefusal(tool: tool) }
        guard tools.contains(tool) else { return refusal(tool: tool) }
        let modes = NativeLayerBlending.grayModes(for: document)
        func modeRefusal() -> String? {
            guard let name = arguments["blend_mode"] as? String,
                  let mode = NativeLayerBlending.mode(named: name),
                  mode != DR_BLEND_PASS_THROUGH, !modes.contains(mode) else { return nil }
            return NativeLayerBlending.grayLimitation(for: document)
        }
        let targeted: Set<String> = [
            "brush_stroke", "eraser_stroke", "fill", "gradient", "apply_filter", "auto_tone",
            "auto_contrast", "equalize", "fill_selection",
            "dodge_burn", "sponge_stroke", "blur_stroke", "sharpen_stroke", "smudge_stroke",
            "history_brush_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "patch_region",
            "stroke_selection", "clear_selection", "apply_image", "modify_scalar",
        ]
        // Phase 2d: the layer, its mask and an alpha channel (`channel:<name>`); phase 4: Quick
        // Mask; every colour plane refuses for good.
        if targeted.contains(tool), let target = arguments["target"] as? String,
           !["layer", "mask", "quick_mask"].contains(target), !target.hasPrefix("channel:") {
            return planeRefusal(tool: tool, target: target)
        }
        let colourPlanes: Set<String> = [
            "red", "green", "blue", "alpha", "cyan", "magenta", "yellow", "black",
        ]
        switch tool {
        case "add_channel", "load_selection":
            // Phase 4: the plane source is the Gray row (`plane: "gray"`), never a colour plane.
            if arguments["from"] as? String == "plane", let plane = arguments["plane"] as? String,
               ["red", "green", "blue"].contains(plane) {
                return planeRefusal(tool: tool, target: plane)
            }
        case "move_selected_pixels", "transform_selected_pixels":
            if let target = arguments["target"] as? String, colourPlanes.contains(target) {
                return planeRefusal(tool: tool, target: target)
            }
        case "set_layer_properties", "clone_stamp", "apply_image", "calculations":
            if let reason = modeRefusal() { return reason }
        case "histogram":
            // Phase 6b: the Gray row is the composite/layer source's; a scalar histogram reads a
            // mask or an alpha channel, and a colour plane refuses for good.
            if arguments["source"] as? String == "scalar", let target = arguments["target"] as? String {
                if ["red", "green", "blue", "alpha"].contains(target) {
                    return planeRefusal(tool: tool, target: target)
                }
                if ["gray", "layer"].contains(target) { return scalarTargetRefusal }
            }
        case "fade":
            if let name = arguments["mode"] as? String, let mode = NativeLayerBlending.mode(named: name),
               mode != DR_BLEND_PASS_THROUGH, !modes.contains(mode) {
                return NativeLayerBlending.grayLimitation(for: document)
            }
        case "fill_selection", "stroke_selection", "brush_stroke", "fill", "gradient":
            if arguments["ink"] != nil {
                return "ink is a CMYK document's colour; a Grayscale document paints with color, "
                    + "as its gray."
            }
            if let reason = modeRefusal() { return reason }
        case "save_copy":
            let format = (arguments["format"] as? String)?.lowercased()
                ?? (arguments["path"] as? String).map { ($0 as NSString).pathExtension.lowercased() }
                ?? ImageDocument.nativeFormatName
            if !ImageDocument.isNativeFormatName(format),
               !["png", "tiff", "tif", "jpeg", "jpg"].contains(format) {
                return saveRefusal
            }
        case "render":
            // The composite is `render` with no channel; the gray plane has no layer, a mask needs
            // one, and an alpha channel (phase 2d) is named with no layer.
            if let channel = arguments["channel"] as? String {
                let hasLayer = arguments["layer"].map { !($0 is NSNull) } ?? false
                let alpha = !hasLayer && RasterPlane.named(channel) == nil
                    && !["gray", "mask", "effective_mask", "composite", "luma"].contains(channel)
                    && document.channelIndex(named: channel) != nil
                if !(channel == "gray" && !hasLayer) && !(channel == "mask" && hasLayer) && !alpha {
                    return renderRefusal
                }
            }
        default:
            break
        }
        return nil
    }

    /// Why a Gray document refuses a `render` channel (#1 phase 1c).
    static let renderRefusal = "A Grayscale document renders its composite with no channel, channel "
        + "gray with no layer, channel mask with a layer, or an alpha channel by name with no layer."

    /// The K % of a Gray sample: ink is the complement of the normalized gray value.
    static func kPercent(_ normalizedGray: Double) -> Double {
        100 * (1 - min(max(normalizedGray, 0), 1))
    }

    /// `get_precision`'s (and `get_document`'s `precision_support`) answer for a Gray document:
    /// this gate's own tools and menu items, not the RGB depth's list.
    static func support(_ doc: RasterDocument) -> [String: Any] {
        let blendNames = NativeLayerBlending.grayModes(for: doc).map { DrBlendMode.displayName(for: $0) }
        return [
            "storage": doc.isFloat ? "rgba32_float" : "rgba16_unorm",
            "color_mode": "gray", "bits_per_channel": doc.grayDepth ?? 0,
            "gray_profile": doc.profileName,
            "supported_tools": tools.sorted(), "supported_menu_items": selectors.sorted(),
            "paint_targets": ["layer", "mask", "saved channel"],
            "blend_modes": blendNames + ["Pass Through (groups)"], "paint_blend_modes": blendNames,
            "effect_blend_modes": effectModes(for: doc).map { DrBlendMode.displayName(for: $0) },
            "blend_if_channels": ["gray"],
        ]
    }

    /// Why a Gray document refuses a save format.
    static let saveRefusal = "A Grayscale document saves as .doctored, PNG, TIFF or JPEG (one channel, its "
        + "Gray profile embedded); Export as PSD (export_psd) writes a PSD, PSB or layered "
        + "TIFF. Convert to RGB Color for other formats."

    /// The open's profile sentence of a Gray document: an untagged file's assignment of the working
    /// Gray (`gray_io::untagged_sentence`), carried in its import warnings; nil otherwise.
    static func assignedSentence(_ info: [String: Any]?) -> String? {
        (info?["import_warnings"] as? [String])?.first {
            $0.hasPrefix("The image is grayscale with no usable Gray profile")
        }
    }

    /// The canvas tools a Gray document admits: selections, the view, the eyedropper, (phase
    /// 2a-ii) the Brush, Eraser, Paint Bucket and Gradient, (phase 2c) the retouch tools, (phase
    /// 2e) Content-Aware Move, Remove and the Art History Brush, and (phase 4) the Magic Wand,
    /// Quick Selection, Object Selection, Select Subject, the Move tool, Crop, Perspective Crop,
    /// the Ruler and the Magnetic Lasso, (phase 6a-i) the Type, shape and Pen tools, and (the
    /// close) the Color Sampler.
    static func allows(_ tool: EditorTool) -> Bool {
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .hand, .zoom, .eyedropper,
             .brush, .eraser, .fill, .gradient,
             .dodge, .sponge, .blurBrush, .sharpenBrush, .smudge, .historyBrush, .clone, .heal,
             .spotHeal, .patch, .contentAwareMove, .remove, .artHistoryBrush,
             .wand, .quickSelect, .objectSelect, .subject, .move, .crop, .perspectiveCrop, .ruler,
             .magneticLasso, .text, .shapeRect, .shapeEllipse, .shapeLine, .pen, .colorSampler:
            return true
        default:
            return false
        }
    }
}
