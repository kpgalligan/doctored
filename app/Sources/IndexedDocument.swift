import AppKit

// The Indexed Color document's gate (#1 Indexed phases 1a to 3c): what an Indexed document
// admits from the canvas, the menus and MCP, asked FIRST wherever a depth decides
// (`DocumentPrecision.refusal`, `allows`, `validatePrecisionItem`), before the Lab, Gray and CMYK
// gates. The core's own table is `precision_capability::INDEXED_FAMILIES` and its edit gate
// `indexed_document::admit_edit`; this is their host side, written once. Phase 1a opens
// (indexed PNG and GIF), converts (Image ▸ Mode ▸ Indexed Color from 8-bit RGB and Grayscale,
// RGB Color and Grayscale back), views, reads (Info's index, `sample_color`, `sample_pixel`)
// and saves (`.rz`) an Indexed document; phase 1b opens palette TIFFs, adds the
// Adaptive palette, and exports and saves PNG, GIF and TIFF keeping the palette (`keptFormats`,
// `saveLosses`); phase 1c edits the table (Image ▸ Mode ▸ Color Table, `get_color_table`,
// `set_color_table`: its own core op, not an edit request), adds the Custom and Previous palettes
// (`options(from:)`) and converts to CMYK and Lab Color directly (the palette expanded, then the
// RGB routes). Phase 2b adds the Brush (`brush_stroke` and the canvas tool): the RGB stroke, then
// re-indexed onto the palette by the core's edge rule (`indexed_paint`, `indexed_edge`); since
// phase 3b in every blend mode. Phase 2c adds Copy, Copy Merged, Cut and Clear,
// Fill, Stroke, the Paint Bucket and the Eraser (the RGB op re-indexed by the same rule; Sample all
// layers is off, since the one layer is the composite), and Image Size, Canvas Size, Trim, Image
// Rotation, flip, Crop, Perspective Crop and Fit Crop (`indexed_geometry`: moves and Nearest
// Neighbor carry every index exactly, the other filters re-index). Phase 2d, which completes
// phase 2, adds Paste and Paste Special (`indexedPasting`: the RGB paste merged into the one layer
// and re-indexed), Free Transform, Transform, Transform Again and the selected pixels' transform
// (exact for a move or a Nearest pick), Warp (applied at once, no binding), the layer row's name,
// label and locks, and Export Layers; a copy transform (`copyRefusal`) refuses. Phase 3a adds the
// Move tool (`move_layers`, `move_selected_pixels` and the canvas tool: whole-pixel moves, every
// index exact, the layer kept canvas-sized at the origin), the row's visibility, offset, order and
// disclosure, the selected pixels of an alpha channel, Reveal All (always no change: the Index
// layer holds no pixel outside the canvas), and Assign Profile and Convert to Profile (the table
// converted, every index kept; `profilePlanBytes`); a pixel an edit leaves clear on a document with
// no transparent index takes the entry nearest white, the background colour. Phase 3b adds the
// Brush, Fill, Stroke and the Paint Bucket in every blend mode, the Select menu's Color Range,
// Similar, Grow, Modify, Transform Selection, Quick Mask and Save and Load Selection (a colour
// selection samples the palette colours), and the background swatch: a UI edit's clear pixel takes
// the entry nearest the current background colour (`swatch`, sent as the core's
// `indexed_background` by `backgroundFields`), an MCP edit's white, or `crop`'s
// `background_color`. Phase 3c adds the Pattern and Noise dithers and Forced Custom
// (`options(from:)`), `render`'s Index and alpha channels (`compositeChannels`, `renderRefusal`),
// the Channels panel's commands, the Magic Wand, Quick Selection and Magnetic Lasso, Clone Stamp
// and the History Brush (re-indexed to each pixel's own nearest entry), and the readouts, history,
// view, path and proof tools; the background key is spliced into a request's text
// (`splicedRequest`). Opacity and blend
// mode (`layerPropertiesRefusal`) refuse. Every other tool refuses with the core's sentence: what
// is not available in Indexed Color (`disabledTools`, since 2b also Auto Tone, Auto
// Contrast, Auto Color and Equalize, read disabled by take1, since 3a Puppet Warp, a mask's
// transform and the layers paste, since 3b HDR Toning, since 3c the Gradient and the retouch
// brushes, `disabledSentence`), what needs more than one layer (`oneLayer`, since 3a, since 3b
// Auto-Align and Auto-Blend, since 3c Link and Arrange), and the rest as offered on RGB documents
// only (`rgbOnly`), all for good. The Indexed Color PSD (#1) adds File ▸ Export as PSD…
// (`export_psd`, `inspect_psd_export`): a colour-mode-2 PSD or PSB of the palette, the
// transparent index and every index, which a PSD's open reads back. take1 checked phase 2a's
// lists against Photoshop's own menus (`scripts/photoshop-probe/indexed_mode_take.py`;
// `designs/indexed-mode.md`).

extension RasterDocument {
    /// True for an Indexed Color document — read off the captured descriptor (`color_mode`), so a
    /// per-row panel or menu check costs no FFI call.
    var isIndexed: Bool { precisionInfo?["color_mode"] as? String == "indexed" }

    /// An Indexed document's palette size (2…256); nil for every other document.
    var indexedColors: Int? { (precisionInfo?["indexed"] as? [String: Any])?["colors"] as? Int }
}

enum IndexedDocument {
    /// Every MCP tool an Indexed document answers. The painting tools, the clipboard, the
    /// geometry, the transforms and the layer row issue edit requests the core admits
    /// (`indexed_document::admit_edit`), Paste and Warp their own exports; the active
    /// and selected layers are the host's own, and the guides go through their own exports.
    /// `AgentServer+Registry.swift` refuses to start when one is not a registered tool.
    static let tools: Set<String> = [
        "convert_mode", "get_color_profile", "get_document", "render", "sample_color", "sample_pixel",
        "set_active_layer", "set_selected_layers", "select_rect", "select_ellipse", "select_polygon",
        "select_freehand", "select_all", "deselect", "reselect", "get_selection", "undo", "redo",
        "save_copy", "list_guides", "add_guide", "remove_guide", "clear_guides", "list_history",
        "restore_history", "close_document", "get_recovery_status", "write_recovery",
        "export_image", "export_preview", "quick_export", "get_color_table", "set_color_table",
        "brush_stroke",
        // Phase 2c: the clipboard, Fill, Stroke, the Paint Bucket, the Eraser and the Image-menu
        // geometry.
        "copy", "copy_merged", "cut", "clear_selection", "fill", "fill_selection",
        "stroke_selection", "eraser_stroke", "image_size", "canvas_size", "trim", "rotate", "flip",
        "crop", "perspective_crop", "fit_crop",
        // Phase 2d: Paste, the transforms, Warp and the layer row.
        "paste", "transform_layer", "transform_layers", "distort_layer", "transform_selected_pixels",
        "transform_again", "get_warp", "warp_layer", "set_layer_properties", "set_layer_lock",
        "set_layer_label", "export_layers",
        // Phase 3a: the Move tool, Reveal All, the row's order and the profile commands.
        "move_layers", "move_selected_pixels", "reveal_canvas", "reorder_layer", "assign_profile",
        "convert_profile",
        // Phase 3b: the Select menu's edits (Color Range and Similar sample the palette colours;
        // Modify, Transform Selection and Quick Mask change only the selection; Save and Load
        // Selection an alpha channel).
        "select_color_range", "select_similar", "modify_selection", "transform_selection",
        "set_quick_mask", "quick_mask_stroke", "save_selection", "load_selection",
        // Phase 3c: the readouts and document settings, the alpha channels' commands, the Magic
        // Wand, Quick Selection and Magnetic Lasso (palette colours), Clone Stamp and the History
        // Brush (re-indexed to each pixel's own nearest entry), the history and its snapshots,
        // recovery, the proof, the view, the colour samplers, the paths, Straighten and the
        // document-level commands that only read the composite.
        "histogram", "set_resolution", "get_metadata", "get_file_info", "set_file_info",
        "get_precision", "set_ruler_origin", "list_channels", "add_channel", "delete_channel",
        "duplicate_channel", "rename_channel", "set_channel_options", "invert_channel",
        "select_magic_wand", "quick_select", "select_magnetic", "clone_stamp",
        "history_brush_stroke", "auto_select_layer", "run_action", "contact_sheet",
        "contact_sheet_from_sources", "list_snapshots", "create_snapshot", "rename_snapshot",
        "delete_snapshot", "restore_snapshot", "save_snapshot", "load_snapshot",
        "set_history_budget", "recover_document", "discard_recovery", "get_proof_setup",
        "set_proof_setup", "render_proof", "save_proof_preset", "export_pdf", "find_layers",
        "list_layer_comps", "get_view", "set_view", "save_workspace", "restore_workspace",
        "open_second_view", "close_second_view", "list_views", "get_color_samplers",
        "set_color_samplers", "detect_horizon", "straighten_layers", "list_paths", "set_path",
        "rename_path", "duplicate_path", "delete_path", "path_to_selection", "selection_to_path",
        "fill_path", "stroke_path", "set_clipping_path", "list_lens_profiles", "separate_color",
        // #1 Indexed Color PSD: File ▸ Export as PSD… writes a colour-mode-2 PSD or PSB.
        "export_psd", "inspect_psd_export",
    ]

    /// The menu items an Indexed document enables. Image ▸ Mode ▸ Indexed Color is listed for its
    /// check mark: `validateCmykModeItem` disables the current mode. View-only and window items are
    /// answered before this list.
    static let selectors: Set<String> = [
        "convertToRGBMode:", "convertToGrayscaleMode:", "convertToIndexedMode:", "convertToCMYKMode:",
        "convertToLabMode:", "showColorTable:",
        "showInfo:", "showHistory:", "toggleLayersPanel:", "showChannels:",
        "undo:", "redo:", "zoomInAction:", "zoomOutAction:", "zoomActualAction:", "zoomFitAction:",
        "selectAll:", "deselect:", "reselect:", "newGuide:", "clearGuides:",
        "showExportWorkspace:", "quickExport:", "exportPSD:",
        // Phase 2c: Edit ▸ Cut, Copy, Copy Merged, Clear, Fill, Stroke; Image ▸ Image Size, Canvas
        // Size, Trim, Image Rotation, Crop.
        "cut:", "copy:", "copyMerged:", "clearSelection:", "fillSelection:",
        "fillSelectionFromDeleteKey:", "fillWithForeground:", "fillWithBackground:",
        "fillWithForegroundPreserving:", "fillWithBackgroundPreserving:", "strokeSelection:",
        "resizeImage:", "showCanvasSize:", "showTrim:", "rotateCW:", "rotateCCW:", "rotate180:",
        "rotateArbitrary:", "flipH:", "flipV:", "cropToSelection:",
        // Phase 2d: Edit ▸ Paste and Paste Special, Free Transform, Transform Again, Warp; the
        // layer row's locks and label; File ▸ Export ▸ Layers to Files. The copy transforms make
        // a second layer, which the mode cannot hold.
        "paste:", "pasteInPlace:", "pasteInto:", "pasteOutside:", "freeTransform:",
        "transformAgain:", "showPersistedWarp:", "lockTransparency:", "lockPixels:",
        "lockPosition:", "lockAll:", "setLayerLabel:", "exportLayers:",
        // Phase 3a: Image ▸ Reveal All; Edit ▸ Assign Profile and Convert to Profile.
        "revealAllCanvas:", "assignProfile:", "convertToProfile:",
        // Phase 3b: Select ▸ Color Range, Transform Selection, Grow, Similar, Inverse, Modify,
        // Save and Load Selection, and Quick Mask.
        "showColorRange:", "transformSelection:", "selectGrow:", "selectSimilar:",
        "invertSelection:", "featherSelection:", "growSelection:", "shrinkSelection:",
        "borderSelection:", "smoothSelection:", "saveSelectionSheet:", "loadSelectionSheet:",
        "toggleQuickMask:",
        // Phase 3c: the Channels panel's commands, File Info, Snapshots, the paths, the colour
        // samplers, a second view and the proof.
        "newChannel:", "deleteChannel:", "duplicateChannel:", "channelOptions:", "invertChannel:",
        "loadChannelAsSelection:", "showFileInfo:", "showSnapshots:", "makeWorkPath:",
        "makeSelectionFromPath:", "deleteCurrentPath:", "duplicateCurrentPath:",
        "fillCurrentPath:", "strokeCurrentPath:", "toggleClippingPath:", "toggleColorSamplers:",
        "clearColorSamplers:",
        "newView:", "showProofSetup:", "toggleProofColors:", "applyWorkingProof:",
        "applyProofPresetFromMenu:",
    ]

    /// `get_precision`'s (and `get_document`'s `precision_support`) answer for an Indexed Color
    /// document: its storage (the descriptor's), colour mode, depth and palette size, and this
    /// gate's tools and menu items.
    static func support(_ doc: RasterDocument) -> [String: Any] {
        [
            "storage": doc.precisionInfo?["storage"] ?? NSNull(), "color_mode": "indexed",
            "bits_per_channel": doc.bitsPerChannel ?? 8,
            "colors": doc.indexedColors.map { $0 as Any } ?? NSNull(),
            "supported_tools": tools.sorted(), "supported_menu_items": selectors.sorted(),
        ]
    }

    /// What is not available in Indexed Color, for good (`indexed_document`'s `DISABLED_OPS` by
    /// tool): since phase 2a new layers, Layer via Copy and Cut, Duplicate Layer, New Fill Layer,
    /// Perspective Warp, Content-Aware and Generative Fill, Sky Replacement, Apply Image,
    /// Calculations, Select Subject, Sky and Select and Mask, Liquify and Neural Filters (each
    /// read disabled by take1), by the controller's ruling of 2026-10-22 Auto Tone, Auto
    /// Contrast, Auto Color and Equalize (read disabled), the merges and stamps, masks, styles,
    /// clipping, adjustment layers, filters and artwork (which take1 could not read enabled),
    /// since phase 3a Puppet Warp and a mask's transform (no layer mask), and since phase 3b HDR
    /// Toning (with the other adjustments). Since
    /// phase 3c: the Gradient; the retouch tools (Healing, Spot Healing, Remove, Patch,
    /// Content-Aware Move, Red Eye, Dodge, Burn, Blur, Sharpen, Smudge, Sponge; unmeasured); Depth
    /// Blur (a filter, measured), Generative Expand and Fill's variations and Generate Image
    /// (measured), the subject, person, face and object selections (Select Subject, measured); the
    /// artwork and fill layers' edits, the Global Light and style presets (no layer style), the
    /// mask readouts and edits (no layer mask), Place, Live Photo and the layers copy (no new
    /// layer), Stack Modes and Reduce Noise from a stack (no Smart Object), and Mode ▸ 16 and 32
    /// Bits (measured). They refuse for good.
    static let disabledTools: Set<String> = [
        "new_layer", "group_layers", "ungroup_layers", "merge_down", "merge_visible",
        "stamp_visible", "stamp_selected", "merge_group", "layer_via_copy", "layer_via_cut",
        "add_layer_mask", "remove_layer_mask", "set_layer_mask_enabled", "set_mask_properties",
        "set_layer_style", "apply_style_preset", "set_layer_clipped", "add_adjustment_layer",
        "edit_adjustment_layer", "apply_filter", "add_text_layer", "add_shape_layer",
        "add_fill_layer", "rasterize_artwork", "set_vector_mask",
        "apply_image", "calculations", "content_aware_fill", "preview_content_aware_fill",
        "generative_fill", "duplicate_layer", "duplicate_to_new_document", "perspective_warp",
        "sky_replacement", "select_subject", "select_sky", "refine_selection", "liquify",
        "super_zoom", "jpeg_artifacts_removal", "neural_colorize",
        "auto_tone", "auto_contrast", "auto_color", "equalize",
        "puppet_warp", "transform_mask", "hdr_toning",
        "gradient", "dodge_burn", "heal_stroke", "spot_heal_stroke", "remove_stroke",
        "patch_region", "content_aware_move", "red_eye", "red_eye_auto", "blur_stroke",
        "sharpen_stroke", "smudge_stroke", "sponge_stroke", "depth_blur",
        "discard_content_aware_fill_preview", "generative_expand", "generative_variations",
        "select_generative_variation", "generate_variations", "select_person_parts",
        "object_finder_select", "inspect_subjects", "select_subjects", "select_people",
        "inspect_faces", "select_face_parts", "object_select", "edit_text_layer",
        "edit_shape_layer", "edit_fill_layer", "set_global_light", "save_style_preset",
        "get_mask_properties", "get_vector_mask", "set_vector_mask_properties",
        "remove_vector_mask", "place_image", "add_live_photo_layer", "copy_layers_to_document",
        "stack_mode", "reduce_noise_from_stack", "convert_precision",
    ]

    /// The tools that need more than one layer (phase 3a; the core's `ONE_LAYER`): Align,
    /// Distribute, Layer ▸ Delete and Flatten Image, since phase 3b Auto-Align and Auto-Blend
    /// Layers, and since phase 3c Link and Unlink, Arrange, the order of several layers and Merge
    /// to HDR. An Indexed document holds one, so they refuse for good.
    static let oneLayer: Set<String> = [
        "align_layers", "distribute_layers", "delete_layer", "flatten_image",
        "auto_align_layers", "auto_blend_layers",
        "link_layers", "unlink_layers", "arrange_layer", "reorder_layers", "merge_layers_to_hdr",
    ]

    /// The core's default sentence (`indexed_document::refused_sentence`): every tool outside
    /// `tools`, `disabledTools` and `oneLayer` is offered on RGB documents only (phase 3c),
    /// for good.
    static func rgbOnly(_ tool: String) -> String {
        "\(tool) is unavailable in an Indexed Color document: Doctored offers it on RGB documents "
            + "only."
    }

    /// `render`'s refusal for a colour or mask plane, or a layer (phase 3c).
    static let renderRefusal = "An Indexed Color document renders its composite, its Index channel "
        + "(index) or an alpha channel by name, with no layer: it has no red, green, blue or mask "
        + "plane."

    /// The `render` channel names drawn as the composite (phase 3c): the Index channel is the
    /// composite through the palette.
    static let compositeChannels: Set<String> = ["composite", "index"]

    /// The current background colour (the rail's swatch, `ToolOptionsStore.sharedState`): the
    /// colour a pixel a UI edit leaves clear takes the nearest entry of on a document with no
    /// transparent index (phase 3b). MCP sends none, so its edits take white.
    static var swatch: NSColor {
        TextLayer.color(fromHex: ToolOptionsStore.shared.sharedState.background) ?? .white
    }

    /// The core's `indexed_background` key for `color` on `doc` (`indexed_paint::BACKGROUND_KEY`,
    /// `CropBackground`'s RGB shape): `[:]` unless `doc` is Indexed or (#1 Bitmap phase 2a-i,
    /// `bitmap_edit`, which snaps it to black or white) Bitmap and `color` is given, so every
    /// other document's request is unchanged.
    static func backgroundFields(_ color: NSColor?, in doc: RasterDocument) throws -> [String: Any] {
        guard doc.isIndexed || doc.isBitmap, let color else { return [:] }
        return ["indexed_background": try CropBackground.make(color, cmykInk: nil, in: doc).fields]
    }

    /// `command` with `backgroundFields` spliced into its request text (`splicedRequest`, so the
    /// request's own spelling is kept); `command` itself when there are none.
    static func backgrounded(_ command: PrecisionPixelCommand, _ color: NSColor?,
                             in doc: RasterDocument) throws -> PrecisionPixelCommand {
        let fields = try backgroundFields(color, in: doc)
        guard !fields.isEmpty else { return command }
        return try PrecisionPixelCommand(text: splicedRequest(command.request, fields))
    }

    /// `text` (a JSON object's text) with each of `fields` appended before its last `}` as
    /// `,"key":<value>`, the value sorted-keys JSON, in key order; `text` itself when `fields` is
    /// empty. The one splice of a key into a request already serialized (phase 3c).
    static func splicedRequest(_ text: String, _ fields: [String: Any]) throws -> String {
        guard !fields.isEmpty else { return text }
        guard text.hasSuffix("}") else {
            throw DoctoredCoreError(message: "High-depth pixel request must be a JSON object.")
        }
        var out = String(text.dropLast())
        for key in fields.keys.sorted() {
            let value = try JSONSerialization.data(
                withJSONObject: fields[key] as Any,
                options: [.sortedKeys, .withoutEscapingSlashes, .fragmentsAllowed])
            let name = try JSONSerialization.data(
                withJSONObject: key, options: [.fragmentsAllowed])
            out += (out.hasSuffix("{") ? "" : ",") + String(decoding: name, as: UTF8.self) + ":"
                + String(decoding: value, as: UTF8.self)
        }
        return out + "}"
    }

    /// The for-good sentence (the core's for `DISABLED_OPS`) for `name`, a tool or a core
    /// op (`paste_layers`, which is no registered tool).
    static func disabledSentence(_ name: String) -> String {
        "\(name) is not available in Indexed Color."
    }

    /// The core's sentence (`indexed_document::refused_sentence`) for `tool`.
    static func refusal(tool: String) -> String {
        if disabledTools.contains(tool) { return disabledSentence(tool) }
        if oneLayer.contains(tool) {
            return "\(tool) needs more than one layer, and an Indexed Color document holds one."
        }
        return rgbOnly(tool)
    }

    /// `set_layer_properties` with an opacity or a blend mode (the row's other fields are phase
    /// 3a's).
    static let layerPropertiesRefusal = "An Indexed Color document's one layer takes no opacity or "
        + "blend mode."

    /// A copy transform (`duplicate` true on `transform_layers` or `transform_again`).
    static let copyRefusal = "A transform copy makes a second layer, which an Indexed Color document "
        + "cannot hold; with a selection, transform_selected_pixels with duplicate "
        + "copies the selected pixels."

    /// The gate's answer for one MCP call: nil to admit it, else the refusal. Admitted tools whose
    /// ARGUMENTS can still reach a path the mode does not hold are narrowed here.
    static func refusal(tool: String, arguments: [String: Any]) -> String? {
        guard tools.contains(tool) else { return refusal(tool: tool) }
        switch tool {
        case "save_copy":
            let format = (arguments["format"] as? String)?.lowercased()
                ?? (arguments["path"] as? String).map { ($0 as NSString).pathExtension.lowercased() }
                ?? ImageDocument.nativeFormatName
            if !ImageDocument.isNativeFormatName(format),
               !["png", "gif", "tiff", "tif"].contains(format) { return saveRefusal }
        case "render":
            // Phase 3c: the composite and the Index channel (the composite through the palette),
            // or an alpha channel by name, never with a layer.
            if let channel = arguments["channel"] as? String {
                let planes: Set<String> = ["red", "green", "blue", "alpha", "mask"]
                let layer = arguments["layer"].flatMap { $0 is NSNull ? nil : $0 }
                if layer != nil || planes.contains(channel.lowercased()) {
                    return renderRefusal
                }
            }
        case "clone_stamp":
            // Layer ▸ New ▸ Layer is disabled, so a clone cannot add one (phase 3c).
            if (arguments["output"] as? String) == "new_layer" { return disabledSentence("add_layer") }
        case "set_layer_properties":
            let keys = ["opacity", "blend_mode"]
            if keys.contains(where: { arguments[$0] != nil }) { return layerPropertiesRefusal }
        case "transform_layers", "transform_again", "move_layers":
            // `boolArg`'s spellings: a number or "true"/"1".
            let duplicate = arguments["duplicate"]
            let spelled = (duplicate as? String).map { ["true", "1"].contains($0) } ?? false
            if (duplicate as? NSNumber)?.boolValue ?? spelled {
                return copyRefusal
            }
        case "paste":
            // Layer ▸ New ▸ Layer is disabled, so a paste cannot add layers (phase 3a).
            if (arguments["mode"] as? String) == "layers" { return disabledSentence("paste_layers") }
        default:
            break
        }
        return nil
    }

    /// Why an Indexed document saves no other format.
    static let saveRefusal = "An Indexed Color document saves as .doctored, PNG, GIF or TIFF, keeping its "
        + "palette; Export (export_image) writes other formats as RGB, or convert to RGB Color."

    /// The formats a copy keeps the palette in (the core's `indexed_export`).
    static let keptFormats: [ExportFormat] = [.png, .gif, .tiff]

    /// What a copy of `doc` in `format` loses: a format not in `keptFormats` (or nil) is the
    /// `.rz`/PNG/GIF/TIFF line alone; a kept format names, each only when it applies, the saved
    /// channels, paths (not in a TIFF that keeps them), colour samplers, guides and ruler origin
    /// (`oneLayerModeCombinedLoss`), the layer's own state (its name,
    /// visibility, lock and label: `oneLayerModeLosses`), the embedded profile, the capture
    /// packets the file strips or has no slot for (`flatPacketLosses`: all in a GIF, IPTC in a
    /// PNG), a GIF's print resolution, the black padding of a GIF or TIFF colour table and a
    /// TIFF's transparent index.
    /// `[]` only when the copy reopens as the same document, which Save As retargets on
    /// (`ImageDocument.canPreserveState`).
    static func saveLosses(_ doc: RasterDocument, format: ExportFormat?, embedProfile: Bool,
                           stripMetadata: Bool) throws -> [String] {
        guard let format, keptFormats.contains(format) else {
            return ["editable Indexed Color document state; an Indexed Color document saves as .doctored, "
                + "PNG, GIF or TIFF"]
        }
        var losses: [String] = []
        if let combined = try doc.oneLayerModeCombinedLoss(format: format) { losses.append(combined) }
        losses += try doc.oneLayerModeLosses()
        let gif = format == .gif
        if doc.iccProfile != nil, !embedProfile || gif { losses.append("embedded color profile") }
        losses += doc.flatPacketLosses(format: format, stripMetadata: stripMetadata)
        let colors = doc.indexedColors ?? 256
        let resolution = doc.resolution
        if gif, resolution.x != 72 || resolution.y != 72 {
            losses.append("the print resolution (a GIF reopens at 72 ppi)")
        }
        if gif, colors & (colors - 1) != 0 {
            losses.append("the palette's size (GIF pads the colour table with black to a power of two)")
        }
        if format == .tiff, colors < 256 {
            losses.append("the palette's size (TIFF pads the colour map with black to 256 entries)")
        }
        let indexed = doc.precisionInfo?["indexed"] as? [String: Any]
        if format == .tiff, indexed?["transparent_index"] is Int {
            losses.append("a transparent index no pixel uses (TIFF records transparency per pixel)")
        }
        return losses
    }

    /// The canvas tools an Indexed document admits: the selections, the view, (phase 2b) the
    /// Brush, (phase 2c) the Eraser, the Paint Bucket, Crop and Perspective Crop, and (phase 3a)
    /// the Move tool; since phase 3b the painting tools take every blend mode; since phase 3c the
    /// Magic Wand, Quick Selection, Magnetic Lasso, Eyedropper, Color Sampler, Ruler, Pen, Clone
    /// Stamp and History Brush.
    static func allows(_ tool: EditorTool) -> Bool {
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .hand, .zoom, .brush, .eraser, .fill,
             .crop, .perspectiveCrop, .move, .wand, .quickSelect, .magneticLasso, .eyedropper,
             .colorSampler, .ruler, .pen, .clone, .historyBrush:
            return true
        default:
            return false
        }
    }

    /// The geometry edit's admitted working memory (`scratch_bytes` of its core plan, the storage
    /// op's plus the adapter's) on an Indexed or (#1 Bitmap phase 2a-i) Bitmap document; nil for
    /// every other document or when the plan refuses. The `rotate` and `flip` replies carry it as
    /// `working_memory_plan_bytes` (`scripts/indexed-admission/measure.py`,
    /// `scripts/bitmap-admission/measure.py`).
    static func geometryPlanBytes(_ doc: RasterDocument, _ command: PrecisionPixelCommand) -> UInt64? {
        guard doc.isIndexed || doc.isBitmap,
              let plan = try? doc.geometryEditPlan(request: command.request) else { return nil }
        return (plan["scratch_bytes"] as? NSNumber)?.uint64Value
    }

    /// `geometryPlanBytes` of an exact canvas op (the quarter turns and flips).
    static func geometryPlanBytes(_ doc: RasterDocument, _ op: DocumentGeometry) -> UInt64? {
        let name: String
        switch op {
        case .rotate90: name = "rotate90"
        case .rotate180: name = "rotate180"
        case .rotate270: name = "rotate270"
        case .flipHorizontal: name = "flip_horizontal"
        case .flipVertical: name = "flip_vertical"
        case .resize: return nil
        }
        guard let command = try? PrecisionPixelCommand(["op": name]) else { return nil }
        return geometryPlanBytes(doc, command)
    }

    /// Convert to Profile's admitted working memory on an Indexed document (phase 3a): the
    /// `scratch_bytes` of its core conversion plan (`dr_precision_doc_conversion_plan`: the
    /// storage's assign plus the colour table's re-render) for `profile` under `options`; nil for
    /// every other document or when the plan refuses. `convert_profile`'s reply carries it as
    /// `working_memory_plan_bytes` (`scripts/indexed-admission/measure.py`).
    static func profilePlanBytes(_ doc: RasterDocument, profile: Data,
                                 options: ConversionOptions) -> UInt64? {
        guard doc.isIndexed, let precision = doc.precision,
              let plan = try? precision.conversionPlan(
                profile: profile, options: options,
                workingLimit: RasterResources.shared.workingLimit, otherRetained: 0)
        else { return nil }
        return (plan["scratch_bytes"] as? NSNumber)?.uint64Value
    }

    /// Why `doc` cannot become Indexed Color, or nil: Indexed Color converts 8-bit RGB and 8-bit
    /// Grayscale only (the core's hard refusals, asked before any work).
    static func sourceRefusal(_ doc: RasterDocument) -> String? {
        // An Indexed document's storage is typed, so it must answer before the depth test.
        // `CmykModeConversion.init`'s sentence for the same condition.
        if doc.isIndexed { return "The document is already in Indexed Color mode." }
        if doc.isCmyk || doc.isLab {
            return "Indexed Color converts an 8-bit RGB or Grayscale document; "
                + "convert to RGB Color first."
        }
        let eightBit = doc.isGray ? doc.grayDepth == 8 : !doc.isHighDepth
        if !eightBit {
            return "Indexed Color converts an 8-bit RGB or Grayscale document; "
                + "convert to 8 Bits/Channel first."
        }
        return nil
    }

    /// Palette `previous` before any Indexed Color conversion (or after one that left fewer than 2
    /// opaque entries).
    static let noPrevious = "There is no previous Indexed Color palette: convert a document to Indexed "
        + "Color first."

    /// The named mattes `convert_mode` takes, as 8-bit codes (the sheet's Foreground and Background
    /// resolve to the current colours instead).
    static let mattes: [String: [Int]] = [
        "white": [255, 255, 255], "black": [0, 0, 0], "gray50": [128, 128, 128],
        "netscape_gray": [192, 192, 192],
    ]

    /// The core's options (`indexed_convert`) from `convert_mode`'s arguments: `palette` (Uniform by
    /// default, the sheet's), `colors`, `forced`, `transparency`, `matte` by name (`none` or one of
    /// `mattes`) or `matte_rgb` [r, g, b], `dither`, `amount`, `preserve_exact` and `flatten`.
    /// Since phase 1c `table` ([[r, g, b], …], 2…256 entries) is the Custom palette's, and palette
    /// `previous` is Custom with the last conversion's palette (`IndexedPalettes.previous`); since
    /// phase 3c `forced_table` is Forced Custom's.
    static func options(from arguments: [String: Any]) throws -> [String: Any] {
        var options: [String: Any] = ["palette": "uniform"]
        for key in ["palette", "forced", "dither"] {
            if let value = arguments[key] {
                guard let text = value as? String else {
                    throw DoctoredCoreError(message: "\(key) must be a string.")
                }
                options[key] = text
            }
        }
        // `table` (the Custom palette's) and, since phase 3c, `forced_table` (Forced Custom's),
        // each validated the same way.
        for key in ["table", "forced_table"] {
            if let table = arguments[key] {
                guard let entries = table as? [Any],
                      case let rows = entries.compactMap({ $0 as? [Any] }),
                      rows.count == entries.count,
                      case let codes = rows.map({ $0.compactMap(IndexedColorTable.integralCode) }),
                      zip(rows, codes).allSatisfy({ $0.count == 3 && $1.count == 3 }),
                      codes.allSatisfy({ $0.allSatisfy { (0...255).contains($0) } }) else {
                    throw DoctoredCoreError(
                        message: "\(key) must be a list of [r, g, b] codes from 0 to 255.")
                }
                options[key] = codes
            }
        }
        if options["palette"] as? String == "previous" {
            guard let previous = IndexedPalettes.previous else { throw DoctoredCoreError(message: noPrevious) }
            if options["table"] != nil {
                throw DoctoredCoreError(message: "table applies to the Custom palette; Previous is the last "
                    + "conversion's palette.")
            }
            options["palette"] = "custom"
            options["table"] = previous.colors.map { $0.map { Int($0) } }
        }
        if let colors = arguments["colors"] {
            guard let count = (colors as? NSNumber)?.intValue ?? (colors as? String).flatMap(Int.init) else {
                throw DoctoredCoreError(message: "colors must be a number from 2 to 256.")
            }
            options["colors"] = count
        }
        if let amount = arguments["amount"] {
            guard let value = (amount as? NSNumber)?.intValue ?? (amount as? String).flatMap(Int.init) else {
                throw DoctoredCoreError(message: "amount must be a number from 1 to 100.")
            }
            options["amount"] = value
        }
        for key in ["transparency", "preserve_exact", "flatten"] {
            if let value = arguments[key] {
                if let flag = value as? Bool {
                    options[key] = flag
                } else if let text = value as? String, let flag = Bool(text.lowercased()) {
                    options[key] = flag
                } else {
                    throw DoctoredCoreError(message: "\(key) must be a boolean.")
                }
            }
        }
        if let rgb = arguments["matte_rgb"] {
            guard arguments["matte"] == nil else {
                throw DoctoredCoreError(message: "Pass matte or matte_rgb, not both.")
            }
            guard let codes = rgb as? [Any], codes.count == 3,
                  case let values = codes.compactMap(IndexedColorTable.integralCode), values.count == 3,
                  values.allSatisfy({ (0...255).contains($0) }) else {
                throw DoctoredCoreError(message: "matte_rgb must be three codes from 0 to 255.")
            }
            options["matte"] = values
        } else if let matte = arguments["matte"] {
            guard let name = matte as? String else {
                throw DoctoredCoreError(message: "matte must be none, white, black, gray50 or netscape_gray.")
            }
            if name != "none" {
                guard let codes = mattes[name] else {
                    throw DoctoredCoreError(message: "matte must be none, white, black, gray50 or netscape_gray.")
                }
                options["matte"] = codes
            }
        }
        return options
    }
}
