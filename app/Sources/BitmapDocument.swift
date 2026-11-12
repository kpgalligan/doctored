import AppKit

// The Bitmap document's gate (#1 Bitmap phases 1a to 3): what a Bitmap document admits from
// the canvas, the menus and MCP, asked FIRST wherever a depth decides (`DocumentPrecision.refusal`,
// `allows`, `validatePrecisionItem`), before the Indexed, Lab, Gray and CMYK gates. A Bitmap
// document is 8-bit Grayscale underneath (the core's `is_gray()` is true on it), but it reports
// `color_mode` "bitmap", so the host's `isGray` is false and only this gate answers it. The core's
// own table is `precision_capability::BITMAP_FAMILIES` and its edit gate
// `bitmap_document::admit_edit`; this is their host side, written once. Phase 1a converts (Image ▸
// Mode ▸ Bitmap… from 8-bit Grayscale, Grayscale back with a Size Ratio), views, reads
// (`sample_color`, `sample_pixel`) and saves (`.rz`) a Bitmap document; phase 1c opens and writes
// its 1-bit files: a 1-bit PNG, TIFF or BMP and a colour-mode-0 PSD or PSB open as a Bitmap
// document, Save a Copy and Export keep the plane in PNG, TIFF and BMP (`keptFormats`,
// `saveLosses`), JPEG goes through the 8-bit Grayscale render (`grayRouteLoss`), and Export as PSD
// writes the 1-bit PSD or PSB. Phase 2a-i paints in black and white and runs the bit-exact
// geometry (`bitmap_edit`): the Brush, the Eraser, Fill, Stroke, the Paint Bucket and Clear, and
// Crop, Canvas Size, the quarter turns and the flips. Phase 2a-ii adds Image Size, arbitrary
// rotation (refused since 2b), the Crop tool's straighten and the Move tool (resampled with
// their own sampler, then thresholded), the selected pixels' moves, Select ▸ Inverse, Modify and
// Transform Selection, and Cut. Each runs on the Gray storage; its ink snaps to black or white
// by its gray, and every pixel goes back on the plane by 50% Threshold's 128 over the background
// (the swatch the UI sends as `indexed_background` by `IndexedDocument.backgroundFields`, else
// white; a past-canvas `crop`'s `background_color`), so area an edit uncovers or vacates, and a
// Cut, takes the background.
// Sample all layers is off: the one layer is the composite. A copy move refuses (`copyRefusal`): it
// makes a second layer. Phase 2b settled the lists: Free Transform, Distort, Transform Again,
// the selected pixels' transforms and Trim refuse for good, the paints take Normal, Darken and
// Lighten (the core's `BLEND_MODES`), and the Magic Wand stays refused. The controller's ruling
// refuses arbitrary rotation for good (Image ▸ Image Rotation ▸ Arbitrary; `rotate`'s quarter
// turns stay) and every filter, Lens Blur by its own sentence (`lensBlurRefusal`). Phase 3 builds
// through the same adapter: the Gradient (its gray ramp thresholded), the Clone Stamp, the History Brush, Liquify,
// Perspective Crop, Copy, Quick Mask, Quick Selection and the Magnetic Lasso, the layer row's name,
// label and lock, the render of the Bitmap channel, and Straighten Layer as the straightened crop
// (`straightenWindow`). Every other tool refuses for good in three tiers, none naming a phase: what
// Bitmap mode disables (`disabledTools`), what needs a second layer (`oneLayer`), and what
// Doctored offers on Grayscale and colour documents only (`unoffered`); the alpha-channel tools and
// Save Selection name the alpha channel a Bitmap document does not hold (`designs/bitmap-mode.md`).

extension RasterDocument {
    /// True for a Bitmap document — read off the captured descriptor (`color_mode`), so a per-row
    /// panel or menu check costs no FFI call.
    var isBitmap: Bool { precisionInfo?["color_mode"] as? String == "bitmap" }
}

enum BitmapDocument {
    /// Every MCP tool a Bitmap document answers: Indexed Color phase 1a's list, since phase 1c the
    /// exports (`export_image`, `export_preview`, `quick_export`, `export_psd`,
    /// `inspect_psd_export`), since phase 2a-i the painting tools and the bit-exact geometry, and
    /// since phase 2a-ii the resampled geometry, Move, the selected pixels' moves, the
    /// selection's own edits and Cut, whose edit requests the core admits
    /// (`bitmap_document::admit_edit`), less what phase 2b refused (`disabledTools`). The
    /// active and
    /// selected layers are the host's own, and the guides go through their own exports.
    /// `AgentServer+Registry.swift` refuses to start when one is not a registered tool.
    static let tools: Set<String> = [
        "convert_mode", "get_color_profile", "get_document", "render", "sample_color", "sample_pixel",
        "set_active_layer", "set_selected_layers", "select_rect", "select_ellipse", "select_polygon",
        "select_freehand", "select_all", "deselect", "reselect", "get_selection", "undo", "redo",
        "save_copy", "list_guides", "add_guide", "remove_guide", "clear_guides", "list_history",
        "restore_history", "close_document", "get_recovery_status", "write_recovery",
        "export_image", "export_preview", "quick_export", "export_psd", "inspect_psd_export",
        // Phase 2a-i: the Brush, the Eraser, Fill, Stroke, the Paint Bucket and Clear, and Crop,
        // Canvas Size, Trim, the quarter turns and the flips.
        "brush_stroke", "eraser_stroke", "fill", "fill_selection", "stroke_selection",
        "clear_selection", "crop", "canvas_size", "rotate", "flip",
        // Phase 2a-ii: Image Size and Move (resampled, then thresholded), the selected pixels'
        // moves, Inverse, Modify and Transform Selection, and Cut. Phase 2b took out
        // Free Transform, Distort, Transform Again, the selected pixels' transforms and Trim
        // (`disabledTools`), and `rotate`'s arbitrary angles (`arbitraryRotationRefusal`).
        "image_size", "move_layers", "move_selected_pixels", "modify_selection",
        "transform_selection", "cut",
        // Phase 3: built through the adapter (the Gradient thresholded,
        // Straighten Layer as the straightened crop), and the reads, views, history, paths,
        // samplers, proofs and workspaces that change no pixel.
        "gradient", "copy", "set_quick_mask", "quick_mask_stroke", "liquify", "clone_stamp",
        "history_brush_stroke", "quick_select", "select_magnetic", "perspective_crop", "fit_crop",
        "detect_horizon", "straighten_layers", "reveal_canvas", "set_layer_properties",
        "set_layer_lock", "set_layer_label", "histogram", "set_resolution", "get_metadata",
        "get_file_info", "set_file_info", "get_precision", "set_ruler_origin", "list_channels",
        "get_warp", "auto_select_layer", "run_action", "contact_sheet", "contact_sheet_from_sources",
        "list_snapshots", "create_snapshot", "rename_snapshot", "delete_snapshot",
        "restore_snapshot", "save_snapshot", "load_snapshot", "set_history_budget",
        "recover_document", "discard_recovery", "get_proof_setup", "set_proof_setup",
        "render_proof", "save_proof_preset", "export_pdf", "find_layers", "list_layer_comps",
        "get_view", "set_view", "save_workspace", "restore_workspace", "open_second_view",
        "close_second_view", "list_views", "get_color_samplers", "set_color_samplers",
        "list_paths", "set_path", "rename_path", "duplicate_path", "delete_path",
        "path_to_selection", "selection_to_path", "fill_path", "stroke_path", "list_lens_profiles",
    ]

    /// The menu items a Bitmap document enables. Image ▸ Mode ▸ Bitmap… is listed for its check
    /// mark and Grayscale for the way back: `validateCmykModeItem` answers every Mode item first.
    /// View-only and window items are answered before this list. Phase 1c adds File ▸ Export and
    /// Quick Export; phase 2a-i Edit ▸ Clear, Fill and Stroke and Image ▸ Canvas Size, the
    /// quarter turns, the flips and Crop; phase 2a-ii Edit ▸ Cut, Image ▸ Image Size, and Select ▸
    /// Inverse, Modify and Transform Selection. Phase 2b took out Image ▸ Trim and Arbitrary
    /// rotation and Edit ▸ Free Transform and Transform Again (refused for good in Bitmap
    /// mode). Phase 3 adds Edit ▸ Copy, Quick Mask, Liquify, the layer locks and label,
    /// Reveal All, File Info, Snapshots, the Paths panel's commands, the colour samplers, New
    /// View and the proof.
    static let selectors: Set<String> = [
        "convertToGrayscaleMode:", "convertToBitmapMode:",
        "showInfo:", "showHistory:", "toggleLayersPanel:", "showChannels:",
        "undo:", "redo:", "zoomInAction:", "zoomOutAction:", "zoomActualAction:", "zoomFitAction:",
        "selectAll:", "deselect:", "reselect:", "newGuide:", "clearGuides:",
        "showExportWorkspace:", "quickExport:",
        "clearSelection:", "fillSelection:", "fillSelectionFromDeleteKey:", "fillWithForeground:",
        "fillWithBackground:", "fillWithForegroundPreserving:", "fillWithBackgroundPreserving:",
        "strokeSelection:", "showCanvasSize:", "rotateCW:", "rotateCCW:", "rotate180:",
        "flipH:", "flipV:", "cropToSelection:",
        "cut:", "resizeImage:",
        "invertSelection:", "featherSelection:", "growSelection:", "shrinkSelection:",
        "borderSelection:", "smoothSelection:", "transformSelection:",
        "copy:", "toggleQuickMask:", "showLiquify:", "lockTransparency:", "lockPixels:",
        "lockPosition:", "lockAll:", "setLayerLabel:", "revealAllCanvas:", "showFileInfo:",
        "showSnapshots:", "makeWorkPath:", "makeSelectionFromPath:", "deleteCurrentPath:",
        "duplicateCurrentPath:", "fillCurrentPath:", "strokeCurrentPath:", "toggleColorSamplers:",
        "clearColorSamplers:", "newView:", "showProofSetup:", "toggleProofColors:",
        "applyWorkingProof:", "applyProofPresetFromMenu:",
    ]

    /// What Bitmap mode disables (a Bitmap document holds one layer, as an Indexed Color one does;
    /// `bitmap_document`'s `DISABLED_OPS` by tool; `IndexedDocument.disabledTools`'s
    /// list): layers beyond the one, merges and stamps, masks, styles, clipping, adjustment layers,
    /// artwork and the auto adjustments, since phase 2a-i Generative Expand (the Crop tool's Fill,
    /// as Generative Fill), and since phase 2a-ii the Magic Wand. Phase 2b added Edit ▸ Free
    /// Transform and Transform (`transform_layer`, `transform_layers`, `distort_layer`,
    /// `transform_again`, `transform_selected_pixels`, `warp_layer`), Image ▸ Trim, Assign and
    /// Convert to Profile, Color Range and Similar/Grow, and took out `liquify` (phase 3 builds
    /// it).
    /// `apply_filter` stays by the controller's ruling, Lens Blur included (`lensBlurRefusal`);
    /// `depth_blur` joins by the same ruling. Phase 3 adds Copy Merged and Paste, Edit ▸ Transform's Puppet Warp and the mask
    /// transform, the Layer Mask and Vector Mask rows, Image ▸ Adjustments' HDR Toning, Mode ▸
    /// 8/16/32 Bits, Select ▸ Subject's tools, Content-Aware and Generative Fill's, the new layers'
    /// contents (text, shape, fill, placed image, Live Photo, another document's layers), the layer
    /// style's global light and presets, and Smart Objects' stack modes. They refuse for good.
    static let disabledTools: Set<String> = [
        "new_layer", "group_layers", "ungroup_layers", "merge_down", "merge_visible",
        "stamp_visible", "stamp_selected", "merge_group", "layer_via_copy", "layer_via_cut",
        "add_layer_mask", "remove_layer_mask", "set_layer_mask_enabled", "set_mask_properties",
        "set_layer_style", "apply_style_preset", "set_layer_clipped", "add_adjustment_layer",
        "edit_adjustment_layer", "add_text_layer", "add_shape_layer",
        "add_fill_layer", "rasterize_artwork", "set_vector_mask",
        "apply_image", "calculations", "content_aware_fill", "preview_content_aware_fill",
        "generative_fill", "duplicate_layer", "duplicate_to_new_document", "perspective_warp",
        "sky_replacement", "select_subject", "select_sky", "refine_selection",
        "super_zoom", "jpeg_artifacts_removal", "neural_colorize",
        "auto_tone", "auto_contrast", "auto_color", "equalize", "generative_expand",
        "select_magic_wand",
        // Phase 2b.
        "transform_layer", "transform_layers", "distort_layer", "transform_again",
        "transform_selected_pixels", "warp_layer", "trim", "assign_profile", "convert_profile",
        "select_color_range", "select_similar", "apply_filter",
        // The controller's ruling (phase 2b): Filter ▸ Neural Filters' Depth Blur.
        "depth_blur",
        // Phase 3.
        "copy_merged", "paste", "puppet_warp", "transform_mask", "get_mask_properties",
        "get_vector_mask", "set_vector_mask_properties", "remove_vector_mask", "hdr_toning",
        "convert_precision", "select_person_parts", "object_finder_select", "inspect_subjects",
        "select_subjects", "select_people", "inspect_faces", "select_face_parts", "object_select",
        "generative_variations", "select_generative_variation", "generate_variations",
        "discard_content_aware_fill_preview", "edit_text_layer", "edit_shape_layer",
        "edit_fill_layer", "set_global_light", "save_style_preset", "place_image",
        "add_live_photo_layer", "copy_layers_to_document", "stack_mode", "reduce_noise_from_stack",
    ]

    /// The tools that need a second layer, which a Bitmap document never holds (the core's
    /// `ONE_LAYER`): they refuse for good by the one-layer reason.
    static let oneLayer: Set<String> = [
        "align_layers", "distribute_layers", "delete_layer", "flatten_image", "auto_align_layers",
        "auto_blend_layers", "link_layers", "unlink_layers", "arrange_layer", "reorder_layer",
        "reorder_layers", "merge_layers_to_hdr",
    ]

    /// The alpha-channel tools: a Bitmap document holds no alpha channel (decision 1), so they
    /// refuse for good by `channelRefusal`.
    static let channelTools: Set<String> = [
        "add_channel", "delete_channel", "duplicate_channel", "rename_channel",
        "set_channel_options", "invert_channel", "load_selection", "add_luminosity_masks",
    ]

    /// The core's disabled-in-Bitmap-mode sentence (`bitmap_document::refused_sentence`) for a
    /// tool or op name.
    static func disabledSentence(_ name: String) -> String {
        "\(name) is not available in Bitmap mode."
    }

    /// The core's default sentence: what Doctored offers on Grayscale and colour documents only.
    static func unoffered(_ tool: String) -> String {
        "\(tool) is unavailable in a Bitmap document: Doctored offers it on Grayscale and colour "
            + "documents only; convert to Grayscale first."
    }

    /// An alpha-channel tool's refusal (`channelTools`).
    static func channelRefusal(_ tool: String) -> String {
        "\(tool) needs an alpha channel, which a Bitmap document does not hold; convert to "
            + "Grayscale first."
    }

    /// Select ▸ Save Selection, which needs the alpha channel a Bitmap document does not hold.
    static let saveSelectionRefusal = "Save Selection on a Bitmap document is not built in "
        + "Doctored: a Bitmap document holds no alpha channel; convert to "
        + "Grayscale first."

    /// `render` of a channel outside `compositeChannels`, or of any channel with a `layer`.
    static let renderRefusal = "A Bitmap document renders its composite or its Bitmap channel "
        + "(bitmap), with no layer: it has no colour, alpha or mask plane."

    /// The `render` channels a Bitmap document draws, each as the composite: its one channel is
    /// the plane.
    static let compositeChannels: Set<String> = ["composite", "bitmap"]

    /// `set_layer_properties` of anything but the name: the one layer is the composite.
    static let layerPropertiesRefusal = "A Bitmap document's one layer takes a new name only: no "
        + "opacity, blend mode, visibility, offset or disclosure change."

    /// The refusal of a tool `tools` does not list, in the core's three tiers (Bitmap mode
    /// disables it, it needs a second layer, or Doctored offers it on Grayscale and colour
    /// documents only), the alpha-channel tools and Save Selection by their own sentences.
    static func refusal(tool: String) -> String {
        if disabledTools.contains(tool) { return disabledSentence(tool) }
        if oneLayer.contains(tool) {
            return "\(tool) needs more than one layer, and a Bitmap document holds one."
        }
        if channelTools.contains(tool) { return channelRefusal(tool) }
        if tool == "save_selection" { return saveSelectionRefusal }
        return unoffered(tool)
    }

    /// A copy move (`duplicate` true on `move_layers`): a second layer, which the one-layer rule
    /// forbids.
    static let copyRefusal = "A copy move makes a second layer, which a Bitmap document cannot "
        + "hold."

    /// `rotate` at an angle that resamples (`CanvasRotation.isFiltered`): Image ▸ Image Rotation ▸
    /// Arbitrary, which Bitmap mode refuses for good since phase 2b.
    static let arbitraryRotationRefusal = "Arbitrary rotation is not available in Bitmap mode."

    /// `apply_filter` with `filter` `lens_blur`, which Doctored does not build for a Bitmap
    /// document.
    static let lensBlurRefusal = "Lens Blur on a Bitmap document is not built in Doctored; "
        + "convert to Grayscale first."

    /// The gate's answer for one MCP call on `doc`: nil to admit it, else the refusal. Admitted
    /// tools whose ARGUMENTS can still reach a path the mode does not hold are narrowed here.
    static func refusal(tool: String, arguments: [String: Any], in doc: RasterDocument) -> String? {
        if tool == "apply_filter", arguments["filter"] as? String == "lens_blur" {
            return lensBlurRefusal
        }
        guard tools.contains(tool) else { return refusal(tool: tool) }
        switch tool {
        case "rotate":
            // The quarter turns and the identity stay; an unreadable angle falls through to the
            // handler's own sentence (`CanvasRotation.unreadable`).
            if CanvasRotation.isFiltered(arguments, in: doc) { return arbitraryRotationRefusal }
        case "save_copy":
            let format = (arguments["format"] as? String)?.lowercased()
                ?? (arguments["path"] as? String).map { ($0 as NSString).pathExtension.lowercased() }
                ?? ImageDocument.nativeFormatName
            if !ImageDocument.isNativeFormatName(format),
               !["png", "tif", "tiff", "bmp", "jpg", "jpeg"].contains(format) { return saveRefusal }
        case "render":
            // The composite and the Bitmap channel draw the composite (`AgentServer.render`);
            // any other channel, or a channel with a layer, has no plane to draw.
            if let channel = arguments["channel"] {
                let name = (channel as? String)?.lowercased() ?? ""
                if !compositeChannels.contains(name) || arguments["layer"] != nil {
                    return renderRefusal
                }
            }
        case "clone_stamp":
            // A clone onto a new layer makes a second layer (the core's `add_layer` refusal).
            if arguments["output"] as? String == "new_layer" { return disabledSentence("add_layer") }
        case "set_layer_properties":
            let changes = ["opacity", "blend_mode", "visible", "offset_x", "offset_y", "open"]
            if changes.contains(where: { arguments[$0] != nil }) { return layerPropertiesRefusal }
        case "crop":
            // The past-canvas fill: only the Background colour (Generative Expand and
            // Content-Aware Fill are disabled in Bitmap mode), before any provider call.
            if let fill = arguments["fill"] as? String, fill != "background" {
                return refusal(tool: fill == "content_aware" ? "content_aware_fill"
                    : "generative_expand")
            }
        case "move_layers":
            // `boolArg`'s spellings: a number or "true"/"1" (`IndexedDocument`'s test).
            let duplicate = arguments["duplicate"]
            let spelled = (duplicate as? String).map { ["true", "1"].contains($0) } ?? false
            if (duplicate as? NSNumber)?.boolValue ?? spelled {
                return copyRefusal
            }
        case "convert_mode":
            if let name = arguments["mode"] as? String, let target = ColorModeTarget(rawValue: name),
               target != .gray, target != .bitmap {
                return modeRefusal(target)
            }
            if arguments["gray_space"] != nil || arguments["rgb_space"] != nil {
                return "gray_space and rgb_space do not apply to a Bitmap document: Grayscale keeps "
                    + "its own Gray profile."
            }
            if arguments["profile"] != nil || arguments["name"] != nil || arguments["path"] != nil {
                return "profile, name and path do not apply to a Bitmap document: Grayscale keeps "
                    + "its own Gray profile."
            }
        default:
            break
        }
        return nil
    }

    /// Why a Bitmap document saves no other format (phase 1c).
    static let saveRefusal = "A Bitmap document saves as .doctored, as a 1-bit PNG, TIFF or BMP, or as JPEG "
        + "through its 8-bit Grayscale render; convert to Grayscale for other formats."

    /// The formats a copy keeps the 1-bit plane in (the core's `bitmap_export`).
    static let keptFormats: [ExportFormat] = [.png, .tiff, .bmp]

    /// What a copy through the Gray storage loses: the plane itself, `format` holding the 8-bit
    /// Grayscale render.
    static func grayRouteLoss(_ format: ExportFormat) -> String {
        "the 1-bit Bitmap plane (\(format.displayName) holds the document's 8-bit Grayscale render)"
    }

    /// What a copy of `doc` in `format` loses. A kept format names, each only when it applies, the
    /// saved channels, paths, colour samplers, guides and ruler origin, the layer's own state (its
    /// name, lock and label: `oneLayerModeLosses`), the profile when it is not embedded or the
    /// file is a BMP, and the capture packets the file strips or has no slot for
    /// (`flatPacketLosses`: all in a BMP, IPTC in a PNG): `[]` only when the copy reopens as the
    /// same document, which Save As retargets on (`ImageDocument.canPreserveState`). JPEG is
    /// `grayRouteLoss` and its compression; any other format (or nil) is the `saveRefusal` line.
    static func saveLosses(_ doc: RasterDocument, format: ExportFormat?, embedProfile: Bool,
                           stripMetadata: Bool) throws -> [String] {
        guard let format, keptFormats.contains(format) || format == .jpeg else {
            return [saveRefusal]
        }
        var losses: [String] = []
        if format == .jpeg {
            losses.append(grayRouteLoss(format))
            losses.append("lossless pixel values (JPEG compresses with loss)")
        }
        let origin = doc.rulerOrigin
        if doc.channelCount > 0 || doc.guideCount > 0 || origin.x != 0 || origin.y != 0
            || !doc.paths.entries.isEmpty || !doc.colorSamplers.items.isEmpty {
            losses.append("saved channels, paths, colour samplers, guides and the ruler origin "
                + "(flattened copy)")
        }
        losses += try doc.oneLayerModeLosses()
        let bmp = format == .bmp
        if !embedProfile || bmp { losses.append("embedded color profile") }
        return losses + doc.flatPacketLosses(format: format, stripMetadata: stripMetadata)
    }

    /// Every mode but Grayscale on a Bitmap document.
    static func modeRefusal(_ target: ColorModeTarget) -> String {
        "\(target.title) is unavailable in a Bitmap document; convert to Grayscale "
            + "first."
    }

    /// The canvas tools a Bitmap document admits: the selections, the view, (phase 2a-i) the
    /// Brush, the Eraser, the Paint Bucket and Crop, (phase 2a-ii) Move, and (phase 3) the
    /// Gradient, the Clone Stamp, the History Brush, Quick Selection, the Magnetic Lasso, the
    /// Eyedropper, the Color Sampler, the Ruler, the Pen and Perspective Crop. The Magic Wand
    /// stays out for good.
    static func allows(_ tool: EditorTool) -> Bool {
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .hand, .zoom, .brush, .eraser, .fill,
             .crop, .move, .gradient, .clone, .historyBrush, .quickSelect, .magneticLasso,
             .eyedropper, .colorSampler, .ruler, .pen, .perspectiveCrop:
            return true
        default:
            return false
        }
    }

    /// Straighten Layer on a Bitmap document (phase 3): the straightened crop, since the one layer
    /// cannot turn inside the canvas without leaving corners no bit describes. The angle is the
    /// line's `straightenRotation` (nil when the line has no length or lies along an axis, so
    /// nothing turns) and the window the Crop tool's fit of the whole canvas at that angle
    /// (`CropFit.fitted`), so the crop's matrix, `rotated(by: −angle)` about the window's centre,
    /// turns the picture by the angle `straightenMatrix` turns it and shows no wedge.
    static func straightenWindow(_ line: RulerMeasurement, width: Int,
                                 height: Int) -> (rect: CGRect, angle: Double)? {
        guard let angle = line.straightenRotation() else { return nil }
        let canvas = CGSize(width: width, height: height)
        let rect = CropFit.fitted(CGRect(x: 0, y: 0, width: width, height: height), angle: angle,
                                  canvas: canvas)
        return (rect, angle)
    }

    /// Why `doc` cannot become Bitmap, or nil: Bitmap converts 8-bit Grayscale only (the core's
    /// hard refusals, asked before any work).
    /// Bitmap from anything but Grayscale or Duotone.
    static let grayscaleFirst = "Bitmap converts an 8-bit Grayscale document; "
        + "convert to Grayscale first."

    static func sourceRefusal(_ doc: RasterDocument) -> String? {
        // A Bitmap document reports its own mode, so it must answer before the Gray test.
        if doc.isBitmap { return "The document is already in Bitmap mode." }
        // A Duotone document is Gray storage: it converts through its Grayscale rule first
        // (`CmykModeConversion.duotoneToBitmap`).
        guard doc.isGray else { return grayscaleFirst }
        if doc.grayDepth != 8 {
            return "Bitmap converts an 8-bit Grayscale document; convert to 8 "
                + "Bits/Channel first."
        }
        return nil
    }

    /// The methods `convert_mode` names (`bitmap_method`): 50% Threshold, Diffusion Dither, Pattern
    /// Dither, Halftone Screen and Custom Pattern, all five converting since phase 1b.
    static let methods = ["threshold", "diffusion", "pattern", "halftone", "custom"]

    /// A number argument as a Double (an NSNumber, or its string form). A JSON boolean is not a
    /// number; it is told apart by its CFBoolean type, because `is Bool` also holds for the
    /// NSNumbers 0 and 1 (an `angle` of 0 or a `size_ratio` of 1).
    private static func number(_ value: Any) -> Double? {
        if let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() {
            return number.doubleValue
        }
        return (value as? String).flatMap(Double.init)
    }

    /// The core's options (`bitmap_convert`) from `convert_mode`'s arguments: `method` (Diffusion
    /// Dither by default, the sheet's), `resolution` (the Output resolution in pixels per inch;
    /// omitted keeps the document's), Halftone Screen's `frequency` (lines per inch) and `angle`
    /// (degrees), each read as a number (its string form too) and range-checked by the core, and
    /// `shape` and Custom Pattern's `pattern`, passed on as they arrive for the core to check.
    /// Flatten is `CmykConversionOptions.flatten`.
    static func options(from arguments: [String: Any]) throws -> [String: Any] {
        var options: [String: Any] = ["method": "diffusion"]
        if let value = arguments["method"] {
            guard let name = value as? String, methods.contains(name) else {
                throw DoctoredCoreError(message: "method must be threshold, diffusion, pattern, halftone "
                    + "or custom.")
            }
            options["method"] = name
        }
        if let value = arguments["resolution"] {
            guard let ppi = number(value), ppi.isFinite, (1...30000).contains(ppi) else {
                throw DoctoredCoreError(message: "resolution must be 1 to 30000 pixels per inch.")
            }
            options["resolution"] = ppi
        }
        for (key, sentence) in [("frequency", "frequency must be 1 to 999.999 lines per inch."),
                                ("angle", "angle must be -180 to 180 degrees.")] {
            guard let value = arguments[key] else { continue }
            guard let number = number(value), number.isFinite else {
                throw DoctoredCoreError(message: sentence)
            }
            options[key] = number
        }
        for key in ["shape", "pattern"] {
            if let value = arguments[key] { options[key] = value }
        }
        return options
    }

    /// Bitmap → Grayscale's `size_ratio` (1 by default; an integer 1…16).
    static func sizeRatio(from arguments: [String: Any]) throws -> Int {
        guard let value = arguments["size_ratio"] else { return 1 }
        guard let ratio = number(value), ratio.rounded() == ratio, (1...16).contains(ratio) else {
            throw DoctoredCoreError(message: "size_ratio must be an integer from 1 to 16.")
        }
        return Int(ratio)
    }
}
