import Foundation

/// The CMYK document's catalog: `convert_mode` — Image ▸ Mode ▸ CMYK Color… / RGB Color… —
/// and the annotations that teach the shared tools what a CMYK document does to them, with
/// the ink properties (`ink`, `start_ink`, `end_ink`: the Color
/// Picker's C/M/Y/K fields) and Levels' CMYK channel stages. Behind an explicit non-inlining
/// boundary, for `AgentCatalog.swift`'s reason.
extension AgentServer {
    @inline(never) func cmykModeCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        return [
            tool(
                "convert_mode",
                "Converts the document's colour mode — Image ▸ Mode ▸ Grayscale / RGB Color… / "
                    + "CMYK Color… / Lab Color, one undo step. mode \"gray\" discards colour: "
                    + "each pixel's luminance through the document's profile, in a "
                    + "Gray space (gray_space, Edit ▸ Color Settings' working Gray by default); an "
                    + "8- or 16-bit document keeps its depth and a 32-bit one becomes 32-bit "
                    + "Grayscale; from CMYK it converts through RGB. Layers, groups, masks, "
                    + "clipping, Fill Opacity, knockout and the Grayscale blend modes carry; "
                    + "adjustment layers, layer effects and Blend If carry too; a document holding a layer in "
                    + "Hue, Saturation, Color or Luminosity, a placed, "
                    + "warped, type, shape, fill or Generative layer must be flattened first: pass "
                    + "flatten true, else the refusal names the first such layer. mode \"rgb\" on a "
                    + "Grayscale document converts it to RGB through its Gray profile. mode \"cmyk\" converts an 8- or 16-bit RGB document's every "
                    + "layer into a CMYK profile's ink (the document keeps its depth): profile "
                    + "\"working\" (the default: Adobe's U.S. Web Coated (SWOP) v2 when installed, "
                    + "else macOS's Generic CMYK Profile), \"installed\" with name (an installed "
                    + "CMYK profile, list_installed_profiles kinds [\"cmyk\"]) or \"file\" with "
                    + "path. Groups, masks, clipping masks, Fill Opacity and the CMYK blend modes "
                    + "carry, and text, shape and fill layers stay editable with their colours "
                    + "converted (ink toward CMYK, sRGB toward RGB). Layer effects, knockout and "
                    + "a Gray Blend If carry both ways. A document holding an RGB adjustment "
                    + "layer, a Blend If on Red, Green or Blue or a blend mode CMYK does not "
                    + "offer must be flattened first (toward RGB: a CMYK adjustment layer or a "
                    + "Blend If on a plate): pass flatten true, else the "
                    + "refusal names the first such layer. A 32-bit float document refuses. Alpha channels, paths and vector masks "
                    + "carry both ways. mode \"rgb\" converts a CMYK document's ink into RGB "
                    + "(rgb_space \"srgb\" or \"displayP3\", the working space by default) keeping "
                    + "every layer and group. mode \"lab\" converts an 8- or 16-bit RGB, "
                    + "Grayscale or CMYK document to Lab Color (D50, relative colorimetric from the "
                    + "document's profile; CMYK through 16-bit ProPhoto RGB), keeping its depth; it "
                    + "takes only flatten (profile, name, path, rgb_space and gray_space refuse). "
                    + "Layers, groups, masks, clipping, Fill Opacity and knockout carry; a document "
                    + "holding an adjustment layer, a layer effect, Blend If, a blend mode a Lab "
                    + "document does not offer, a placed, warped, type, shape, fill or Generative "
                    + "layer must be flattened first. A 32-bit document "
                    + "refuses (convert to 16 Bits/Channel first). mode \"rgb\" on a Lab document "
                    + "converts it to RGB (rgb_space); mode \"cmyk\" on a Lab document converts it "
                    + "directly through 16-bit ProPhoto RGB, keeping its depth; "
                    + "mode \"gray\" on an 8- or 16-bit Lab document "
                    + "converts it directly, each pixel's L* through the gray_space's tone curve "
                    + "(a* and b* discarded), keeping its depth; an adjustment layer, a Blend If "
                    + "on Lightness, a or b, or a mode Grayscale lacks refuses by name with "
                    + "flatten_required (flatten true converts). "
                    + "intent and black_point_compensation default to Edit ▸ Color Settings' "
                    + "Conversion Options (relative with BPC). "
                    + "mode \"multichannel\" refuses: Multichannel documents are not supported. "
                    + CmykDocument.limitation,
                [
                    "document_id": docID,
                    "mode": ["type": "string", "enum": ["gray", "rgb", "cmyk", "lab"]],
                    "gray_space": ["type": "string",
                                   "enum": GrayWorkingSpace.allCases.map(\.rawValue),
                                   "description": "Toward Grayscale: the Gray space; Edit ▸ Color "
                                       + "Settings' working Gray by default."],
                    "profile": ["type": "string", "enum": ["working", "installed", "file"],
                                "description": "Toward CMYK: which CMYK profile."],
                    "name": ["type": "string", "description": "An installed CMYK profile's name."],
                    "path": ["type": "string", "description": "A CMYK ICC profile file."],
                    "rgb_space": ["type": "string", "enum": ["srgb", "displayP3"],
                                  "description": "Toward RGB: the destination space."],
                    "intent": ["type": "string",
                               "enum": ["perceptual", "relative", "saturation", "absolute"]],
                    "black_point_compensation": ["type": "boolean"],
                    "flatten": ["type": "boolean",
                                "description": "Flatten the image first when it holds a layer a "
                                    + "CMYK (or Grayscale, or Lab Color) document cannot."],
                ], required: ["mode"]),
            // The Info panel's CMYK row and the Color Picker's gamut alarm
            // (AgentServer+CmykColour.swift).
            tool(
                "separate_color",
                "Separates one colour into CMYK ink — the Info panel's CMYK row (\"!\" when out of "
                    + "gamut) and the Color Picker's gamut alarm with its click-to-bring-into-gamut "
                    + "swatch. rgb [r, g, b] (0–255) is in the document's profile on an RGB document "
                    + "and sRGB on a CMYK one; color \"#rrggbb\" is sRGB. The CMYK profile is a CMYK "
                    + "document's own, else Edit ▸ Color Settings' working CMYK; intent and black "
                    + "point compensation are Color Settings'. Answers ink_percent [c, m, y, k], "
                    + "out_of_gamut (the proof's rule: CIE76 ΔE above 5 against the colour's own "
                    + "round trip through the profile, relative colorimetric), delta_e, in_gamut "
                    + "(that ink back in the same RGB space, 0–255) and in_gamut_hex. Reads only.",
                [
                    "document_id": docID,
                    "rgb": ["type": "array", "items": ["type": "number"], "minItems": 3, "maxItems": 3,
                            "description": "Red, green, blue, 0–255."],
                    "color": ["type": "string", "description": "An sRGB colour, \"#rrggbb\"."],
                ]),
        ]
    }

    @inline(never) func annotateCmykCatalog(_ catalog: inout [[String: Any]]) {
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            let existing = (catalog[index]["description"] as? String) ?? ""
            let note: String?
            switch name {
            case "open_document":
                note = " A CMYK TIFF or JPEG (with its own CMYK profile, or untagged while a working "
                    + "CMYK profile is set) opens as a CMYK DOCUMENT keeping its own ink and profile; "
                    + "a TIFF's transparency is kept, its planar layout read and any extra samples "
                    + "named in the import warnings; precision \"rgba8\" keeps the conversion to "
                    + "sRGB. A CMYK PSD, PSB or layered TIFF opens as a CMYK document with either "
                    + "psd_mode: \"layers\" imports its rasters, masks, groups (Normal and Pass "
                    + "Through), clipping, the 27 CMYK modes, opacity, Fill and its Levels, Curves "
                    + "and Hue/Saturation layers; \"composite\" its saved composite (a TIFF has none: "
                    + "without psd_mode it opens flat). Both follow Color Settings' CMYK policy."
            case "inspect_psd":
                note = " A CMYK file (colour mode 4) answers import_precision \"cmyk\" with its "
                    + "layered availability, layered_plan_bytes and the losses its CMYK document "
                    + "cannot hold (the adjustment kinds it does not carry), each named per record; "
                    + "layer effects, knockout and Blend If ranges (Gray, Cyan, Magenta, Yellow, "
                    + "Black) carry on every record, a Pass Through group included."
            case "get_document", "get_color_profile":
                note = " A CMYK document reports color_mode \"cmyk\", its CMYK profile and declared "
                    + "depth; its pixels are shown through a Display P3 view. Its text, shape and "
                    + "fill objects report their colours as ink in percent (ink, fill_ink, "
                    + "stroke_ink, a stop's ink) beside the sRGB appearance hex."
            case "add_text_layer", "edit_text_layer", "add_shape_layer", "edit_shape_layer",
                 "add_fill_layer", "edit_fill_layer", "rasterize_artwork":
                note = " Works on a CMYK document: the layer's colours are stored as ink in "
                    + "the document's profile — pass ink (fill_ink, stroke_ink, a run's or a stop's "
                    + "ink) as C, M, Y, K percent for exact ink, or a colour, which separates "
                    + "through the profile (relative colorimetric with BPC); an edit that passes "
                    + "no colour keeps each ink exactly, and the reply reports the ink in percent "
                    + "beside the appearance hex."
            case "render":
                note = " In a CMYK document channel is \"cyan\", \"magenta\", \"yellow\" or \"black\": "
                    + "that ink plane of the composite on paper, ink dark; clipping \"shadows\" or "
                    + "\"highlights\" is Levels' and Curves' ⌥ display of the composite: black "
                    + "where any ink is at 100 %, or white where every ink is at paper. region with "
                    + "preview_divisor d above 1 is the full view sampled at every d-th pixel of the "
                    + "region (x + i·d, y + j·d), ceil(w/d) × ceil(h/d)."
            case "sample_color", "sample_pixel":
                note = " In a CMYK document the reply adds the composite's ink (ink_percent, ink16, "
                    + "total_ink_percent) beside the view's RGB."
            case "save_copy":
                note = " A CMYK document saves as doctored (or rz), tiff or jpeg (CMYK, its composite on paper, "
                    + "at its declared depth; JPEG 8-bit); export_psd writes its layers as a PSD, "
                    + "PSB or layered TIFF. flat_save_losses is File > Save's own list: the layers, "
                    + "masks, styles, layer properties, bounds, saved channels, guides, paths and "
                    + "transparency the flat file has no place for, empty when it reopens as the "
                    + "document."
            case "export_psd", "inspect_psd_export":
                note = " A CMYK document writes a colour-mode-4 file in every format and both modes "
                    + "at its declared depth (the report's depth \"cmyk\" and bit_depth) with its "
                    + "CMYK profile: layers keeps its rasters, masks, groups, clipping, modes, Fill "
                    + "and its Levels, Curves and Hue/Saturation layers as Photoshop's own records; "
                    + "flattened writes its composite on paper."
            case "brush_stroke", "fill", "fill_selection", "stroke_selection":
                note = " In a CMYK document this paints in INK on a layer (or its mask): pass ink "
                    + "[c, m, y, k] in percent (exactly the Color Picker's C/M/Y/K), or color, which "
                    + "converts through the document's CMYK profile; every blend mode but Pass "
                    + "Through, evaluated on 1 − ink"
                    + (name == "fill" ? "; sample_all_layers grows the region over the composite's "
                        + "ink, as the Magic Wand does." : ".")
                    + " Target cyan, magenta, yellow or black paints that ink plate's stored 1 − ink, "
                    + "and channel:<name> an alpha channel, as a 16-bit colour plane or channel is "
                    + "painted: color's grey is painted (white leaves no ink) and ink refuses."
                    + " On an RGB document ink refuses."
            case "gradient":
                note = " In a CMYK document the ramp is INK: start_ink / end_ink [c, m, y, k] in "
                    + "percent, or start_color / end_color (or stops' colours) converted through the "
                    + "document's CMYK profile; every blend mode but Pass Through, evaluated on 1 − ink."
                    + " Target cyan, magenta, yellow or black paints that ink plate's stored 1 − ink, "
                    + "and channel:<name> an alpha channel, as a 16-bit colour plane or channel is "
                    + "painted: color's grey is painted (white leaves no ink) and start_ink / end_ink refuse."
            case "eraser_stroke", "new_layer", "add_layer_mask", "remove_layer_mask",
                 "set_layer_mask_enabled", "get_mask_properties", "set_mask_properties":
                note = " Works on a CMYK document: a new layer is bare paper, a mask scales "
                    + "its layer's ink coverage, and applying a mask bakes it into the layer's alpha "
                    + "with the ink kept."
            case "select_magic_wand":
                note = " In a CMYK document the wand compares the INK, not the view: each ink as "
                    + "its naive RGB (1 − C)(1 − K), (1 − M)(1 − K), (1 − Y)(1 − K) at the document's "
                    + "depth, a pixel taken when its R, G, B, alpha, R−G, G−B and R−B each lie within "
                    + "tolerance (8-bit codes) of the seed's, four-connected when contiguous, "
                    + "admitting K and GCR twins alike; sample_all_layers false reads the "
                    + "active layer's own ink. sample_size averages that naive RGB over its box."
            case "load_selection":
                note = " In a CMYK document from \"plane\" takes cyan, magenta, yellow or "
                    + "black (the plate's 1 − ink), luma (the "
                    + "composite's luminosity) or alpha; red, green and blue refuse by name."
            case "select_similar", "select_magnetic", "modify_selection", "transform_selection",
                 "set_quick_mask", "quick_mask_stroke", "add_luminosity_masks":
                note = " Works on a CMYK document: Grow and Similar compare the ink as the "
                    + "Magic Wand does, the Magnetic Lasso reads the view, and the rest edit coverage."
            case "select_color_range", "quick_select", "object_select", "select_subject",
                 "inspect_subjects", "select_subjects", "select_people", "inspect_faces",
                 "select_face_parts":
                note = " In a CMYK document this reads the document's Display P3 view of its ink "
                    + "(sample_color's center_rgba16 are that view's codes), and the selection is "
                    + "coverage at document size."
            case "refine_selection":
                note = " Works on a CMYK document: the edge stage reads its Display P3 view, and "
                    + "new_layer / new_layer_with_mask put the composite's ink on the new layer "
                    + "(decontaminate runs on the ink)."
            case "add_adjustment_layer", "edit_adjustment_layer":
                note = " In a CMYK document every op acts on ink: Levels' "
                    + "top-level keys are the CMYK composite stage with optional c, m, y, k stage "
                    + "objects (acting on 1 − ink); Curves takes point lists "
                    + "cmyk, c, m, y, k with points in ink, 0–255 for 0–100 %, or drawn maps "
                    + "cmyk_map, c_map, m_map, y_map, k_map (256 output ink codes each, the composite "
                    + "after the plate's); hue_saturation takes the RGB params (master, bands, "
                    + "colorize) on 1 − C, 1 − M, 1 − Y with K kept (colorize "
                    + "re-separates K from the colorized grey). RGB keys refuse by name; auto (levels "
                    + "and curves) takes Auto Options {algorithm, shadows, highlights}, per_channel "
                    + "keeping black's ink. "
                    + Self.cmykAdjustmentNote
            case "set_layer_properties":
                note = " In a CMYK document blend_mode is one of "
                    + NativeLayerBlending.cmykModes.map { DrBlendMode.displayName(for: $0) }
                        .joined(separator: ", ")
                    + " (evaluated on 1 − ink), or Pass Through on a group."
            case "dodge_burn", "sponge_stroke", "blur_stroke", "sharpen_stroke", "smudge_stroke",
                 "history_brush_stroke":
                note = " Works on a CMYK document's ink: the tool runs on 1 − ink in two "
                    + "passes, C/M/Y together and K alone (Dodge/Burn's Protect Tones holds the C/M/Y "
                    + "ratios; the Sponge moves C/M/Y and keeps K exactly; the History Brush paints "
                    + "from a CMYK state)."
            case "art_history_brush_stroke":
                note = " Works on an 8-bit CMYK document's ink: the strokes take the source "
                    + "state's ink and composite through blend_mode on 1 − ink. A 16-bit CMYK "
                    + "document refuses it by name."
            case "clone_stamp", "heal_stroke", "spot_heal_stroke", "patch_region", "remove_stroke",
                 "content_aware_move", "content_aware_fill", "preview_content_aware_fill":
                note = " Works on a CMYK document's ink: the sample is the ink (current: the "
                    + "layer's; current_and_below / all: the CMYK composite), the clone composites "
                    + "through its blend_mode on 1 − ink, healing and synthesis run on the four "
                    + "inks, and output new_layer adds a CMYK layer."
            case "equalize":
                note = " On a CMYK document one table from the four 1 − ink plates' histograms acts "
                    + "on every plate."
            case "set_layer_style":
                note = " In a CMYK document every effect composites on ink: each colour slot "
                    + "takes ink [c, m, y, k] as fractions 0–1 (ink; a bevel's highlight_ink and "
                    + "shadow_ink; a gradient stop's ink), which wins over color, whose hex is "
                    + "rewritten to the ink's sRGB; a slot without ink converts its color through "
                    + "the document's profile, except a drop shadow, inner shadow, satin or bevel "
                    + "shadow left black, which is K 100 %. knockout and "
                    + "blend_interior_as_group work; blend_if takes channel gray, cyan, magenta, "
                    + "yellow or black, in the primary range and in each extra one (a plate's "
                    + "ramp reads 255 − its ink code, so 0 is 100 % ink; Gray the luminosity of "
                    + "the ink's naive RGB) and refuses red, green and blue by name in any "
                    + "range, even a full one. An adjustment layer takes a style and Blend If too: "
                    + "its shape is its mask over the canvas, its tone applies after its "
                    + "own below effects and This Layer reads the adjusted ink. Converting to RGB "
                    + "keeps each slot's color and drops its ink."
            case "set_global_light":
                note = " Works on a CMYK document: the angle and altitude every effect "
                    + "with use_global_light reads."
            case "group_layers", "ungroup_layers", "set_layer_clipped", "merge_down",
                 "merge_visible", "stamp_visible", "stamp_selected", "merge_group":
                note = " Works on a CMYK document: groups, clipping masks and merges "
                    + "composite in ink."
            case "assign_profile", "convert_profile":
                note = " On a CMYK document profile is working_cmyk (the default: Edit ▸ Color "
                    + "Settings' working CMYK), installed with name or file with path, a CMYK profile: "
                    + "assign keeps every ink number, convert re-separates each layer's ink through the "
                    + "destination at intent and black_point_compensation (Color Settings' by default), "
                    + "keeping masks, adjustment layers and transparency, and flatten true flattens onto "
                    + "paper first to keep the appearance exactly. convert_profile to an RGB profile "
                    + "(srgb, display_p3, or an installed or file RGB profile) converts the document to "
                    + "RGB, as convert_mode rgb does."
            case "convert_precision":
                note = " On a CMYK document mode is cmyk8 or cmyk16 (Image ▸ Mode ▸ 8/16 "
                    + "Bits/Channel): 16 → 8 rounds every ink, alpha, mask and alpha-channel sample to "
                    + "the 8-bit grid and keeps vector masks, 8 → 16 changes only the declared depth."
            case "histogram":
                note = " In a CMYK document the reply is the histogram per ink and composite "
                    + "(channels cmyk, c, m, y, k; 256 bins of L = 1 − ink) "
                    + "of the composite, or of layer's own ink; selection true weights by the selection. "
                    + "source \"scalar\" with target mask, channel:<name> or an ink plate (cyan, magenta, "
                    + "yellow, black: its stored 1 − ink) is that plane's raw scalar histogram, as at "
                    + "16 bits."
            case "get_proof_setup", "set_proof_setup", "render_proof":
                note = " On a CMYK document the proof profile is CMYK (profile "
                    + "\"working_cmyk\" is Color Settings' working CMYK), preserve_numbers sends the "
                    + "ink to it unconverted, plate (cyan, magenta, yellow or black) shows one plate of "
                    + "the proof device as grey, ink dark, and gamut_warning true overlays gamut_color "
                    + "on every ink the proof profile cannot hold (none when preserve_numbers), "
                    + "out_of_gamut counting them; the proof is "
                    + "Display P3 (space \"display-p3\"). On an RGB document plate needs a CMYK proof "
                    + "profile and preserve_numbers refuses."
            case "export_image", "export_preview", "quick_export":
                note = " On a CMYK document profile \"document\" keeps CMYK for jpeg and tiff: "
                    + "the composite on paper, resized on ink by size and filter, output-sharpened "
                    + "per plate, tiff at bit_depth 8 or 16 (16 → 8 rounds), with the metadata "
                    + "policy and its stamp (left at the defaults, the CMYK save's bytes); a tiff "
                    + "bit_depth other than 8 or 16, and dither on an 8-bit tiff of a 16-bit "
                    + "document, refuse by name, and jpeg ignores dither. Every other format, and "
                    + "srgb, display-p3 or file, flattens on paper and converts to sRGB "
                    + "(\"document\"), Display P3 or the file profile at intent and "
                    + "black_point_compensation."
            case "new_document":
                note = " color_mode \"cmyk\" makes a CMYK document at depth 8 or 16 in profile "
                    + "\"working\" (Color Settings' working CMYK) or a CMYK profile path."
            case "warp_layer":
                note = " In a CMYK document a warp is applied at once to the ink "
                    + "(and a linked mask) and keeps no re-editable field: get_warp answers null, "
                    + "clear and bake change nothing, and each warp reshapes the current pixels."
            case "get_warp":
                note = " A CMYK document keeps no warp field: field is null."
            case "move_selected_pixels", "transform_selected_pixels":
                note = " In a CMYK document target is \"layer\" (the ink), \"mask\", "
                    + "\"channel:<name>\" or ONE ink plate, \"cyan\", \"magenta\", \"yellow\" or "
                    + "\"black\": the other "
                    + "plates and the transparency stay, and the vacated area takes background's "
                    + "(a hex, default #000000, as the UI takes the background swatch) "
                    + "0.30/0.59/0.11 gray read as ink, 1 − gray, as an RGB channel takes its gray; "
                    + "red, green, blue and alpha refuse by name."
            case "transform_layer", "transform_layers", "move_layers", "transform_again",
                 "transform_mask", "image_size", "canvas_size", "crop", "perspective_crop",
                 "straighten_layers", "trim", "reveal_canvas", "rotate", "flip", "puppet_warp",
                 "perspective_warp", "liquify", "align_layers", "distribute_layers":
                note = " Works on a CMYK document: the ink's five planes resample "
                    + "premultiplied by alpha as RGB does, and groups, clipping, modes, Fill and masks "
                    + "are kept."
            case "copy", "cut", "copy_merged":
                note = " Works on a CMYK document: the copy is the layer's (or the "
                    + "composite's) INK, a CMYK typed payload (RZCB v4) in the document's CMYK "
                    + "profile and declared depth; a CMYK adjustment layer has no ink to copy "
                    + "(use copy_merged)."
            case "paste":
                note = " CMYK: between CMYK documents of the same profile the ink "
                    + "is kept bit for bit; another CMYK profile converts (relative colorimetric "
                    + "with black-point compensation) or, with profile \"preserve\", keeps the ink. "
                    + "Across modes — an RGB copy or another app's RGB image into a CMYK document, "
                    + "a CMYK copy or a CMYK TIFF from another app into an RGB document — the pixels "
                    + "always convert through the profiles' own tables, and profile \"preserve\" "
                    + "refuses. A copied layers clip of the other mode is converted first (an "
                    + "adjustment layer CMYK cannot hold refuses by name). color_conversion reports "
                    + "what was crossed."
            case "layer_via_copy", "layer_via_cut", "duplicate_to_new_document",
                 "copy_layers_to_document":
                note = " Works on a CMYK document: ink is copied exactly; into an "
                    + "RGB document the layers convert (relative colorimetric with BPC)."
            case "apply_filter":
                note = " In a CMYK document levels (flat, with optional c, m, y, k stages) and "
                    + "curves (params cmyk, c, m, y, k ink points, or the cmyk_map… drawn maps) apply "
                    + "on a layer's ink, and hue_saturation (the RGB params on 1 − C, "
                    + "1 − M, 1 − Y, K kept but under colorize) and every adjustment below on a "
                    + "layer's ink. On one ink plate (target cyan, magenta, yellow or black: its "
                    + "stored 1 − ink), a layer mask or an alpha channel the adjustments and "
                    + "filters run as on a 16-bit colour plane or channel, with its RGB vocabulary "
                    + "(levels' flat keys, "
                    + "curves rgb / rgb_map; a CMYK key refuses by name). " + Self.cmykAdjustmentNote + " shadows_highlights and "
                    + "replace_color apply destructively only. The filters available in "
                    + "CMYK run on the layer's ink as two passes of the RGB filter over 1 − ink "
                    + "(C/M/Y, then K) with their RGB arguments; add_noise's K plate takes its own "
                    + "seed; clouds and difference_clouds take foreground_ink / background_ink "
                    + "([c, m, y, k] percent; default K 100 % over paper). lens_correction, "
                    + "develop_filter, presence and add_grain refuse, "
                    + "as do depth_blur and the red-eye tools (RGB only)."
            case "auto_tone", "auto_contrast", "auto_color":
                note = " In a CMYK document the layer's ink takes CMYK Levels from its tone "
                    + "analysis: per channel for auto_tone and auto_color, monochromatic for "
                    + "auto_contrast, clip (default 0.001, 0.1 %) clipped at each end; one ink plate or "
                    + "an alpha channel takes the 16-bit colour plane's or channel's Auto; a mask "
                    + "refuses."
            case "fill_path", "stroke_path":
                note = " In a CMYK document the path paints in INK on a layer: ink [c, m, y, "
                    + "k] in percent, or color converted through the document's CMYK profile; target "
                    + "cyan, magenta, yellow or black paints that ink plate with color's grey and "
                    + "channel:<name> an alpha channel (ink refuses there)."
            case "fade":
                note = " Works on a CMYK document's layer ink."
            case "auto_align_layers", "reduce_noise_from_stack":
                note = " In a CMYK document the registration estimates on each layer's "
                    + "Display P3 view (its ink through the document's CMYK profile) and the warp "
                    + "moves all four inks; reduce_noise_from_stack then takes its median or mean "
                    + "per ink. auto_align_layers' vignette and distortion refuse by name ("
                    + "\"Vignette Removal does not work in CMYK.\", "
                    + "\"Geometric Distortion does not work in CMYK.\")."
            default:
                note = nil
            }
            if let note { catalog[index]["description"] = existing + note }
            addCmykProperties(&catalog[index], name)
        }
    }

    /// The adjustments batch's CMYK params, one sentence for the adjustment tools.
    static let cmykAdjustmentNote = "bcs (no saturation), invert, posterize and shadows_highlights "
        + "run per plate on 1 − ink; color_balance, color_lookup and photo_filter (params ink "
        + "[c, m, y, k] as fractions 0–1 in place of color) on the C/M/Y triple, K held; "
        + "channel_mixer takes rows cyan, magenta, yellow, black and gray of {c, m, y, k, "
        + "constant} on the ink; selective_color and threshold their RGB params on ink; "
        + "gradient_map stops {position, ink, opacity, midpoint, opacity_midpoint} (method "
        + "classic and smoothness as in RGB; perceptual refuses); replace_color inks (1–8 "
        + "[c, m, y, k] fractions) in place of colors. exposure, vibrance and black_and_white "
        + "refuse, as do hue_rotate, grayscale, sepia and "
        + "white_balance (RGB only)."

    /// The ink properties and Levels' CMYK channel stages, added to the shared tools'
    /// schemas.
    private func addCmykProperties(_ entry: inout [String: Any], _ name: String) {
        let ink: [String: Any] = [
            "type": "array", "items": ["type": "number"], "minItems": 4, "maxItems": 4,
            "description": "CMYK document only: C, M, Y, K ink in percent 0–100.",
        ]
        let stage: [String: Any] = [
            "type": "object",
            "description": "CMYK document only: an ink stage (black, white, gamma, output_black, "
                + "output_white), applied before the composite.",
        ]
        var added: [String: Any] = [:]
        switch name {
        case "brush_stroke", "fill", "fill_selection", "stroke_selection", "fill_path", "stroke_path":
            added = ["ink": ink]
        case "gradient": added = ["start_ink": ink, "end_ink": ink]
        case "render":
            added = ["clipping": ["type": "string", "enum": ["shadows", "highlights"],
                                  "description": "CMYK document only: the ⌥ clipping display."]]
        case "apply_filter":
            added = ["c": stage, "m": stage, "y": stage, "k": stage,
                     "foreground_ink": ink, "background_ink": ink,
                     "auto": ["type": "object",
                              "description": "CMYK document only: levels or curves Auto, "
                                + "{algorithm: per_channel or monochromatic, shadows, highlights: "
                                + "clip percent}, measured on the layer within the selection."]]
        default: return
        }
        guard var schema = entry["inputSchema"] as? [String: Any],
              var properties = schema["properties"] as? [String: Any] else { return }
        for (key, value) in added where properties[key] == nil { properties[key] = value }
        schema["properties"] = properties
        entry["inputSchema"] = schema
    }
}
