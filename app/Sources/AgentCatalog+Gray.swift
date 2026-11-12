import Foundation

/// The Grayscale document's catalog notes: what a Gray document does to the shared layer,
/// style, Fill, save, paint, adjustment, filter, retouch, channel, PSD open and export,
/// geometry, selection, clipboard, profile, depth, proof, mode, artwork, path, vector-mask,
/// link, comp, snapshot, Clear, analysis, multi-image, neural and generative tools
/// (`GrayDocument`'s gate, `GrayAdjust`, `GrayArtwork`, the core's `gray_document::admit_edit`,
/// `gray_adjust`, `gray_filter`, `gray_style`, `gray_export`, `gray_profile`, `gray_artwork`
/// and `gray_comps`), and the Gray values of `assign_profile` / `convert_profile`'s `profile`
/// and `convert_precision`'s `mode` enums. Behind an explicit non-inlining boundary, for
/// `AgentCatalog.swift`'s reason.
extension AgentServer {
    @inline(never) func annotateGrayCatalog(_ catalog: inout [[String: Any]]) {
        let modes = NativeLayerBlending.grayModes.map { DrBlendMode.displayName(for: $0) }
            .joined(separator: ", ")
        let grayspaces = GrayWorkingSpace.allCases.map(\.rawValue).joined(separator: ", ")
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            annotateGraySchema(&catalog[index], name: name)
            let existing = (catalog[index]["description"] as? String) ?? ""
            let note: String?
            switch name {
            case "set_layer_properties":
                note = " In a Grayscale document blend_mode takes " + modes + " (at 32 bits, "
                    + "those a float document admits), "
                    + "and Pass Through on a group; Darker Color, Lighter Color, Hue, Saturation, "
                    + "Color and Luminosity are unavailable."
            case "set_layer_style":
                note = " In a Grayscale document every effect is available: each colour "
                    + "(hex, linear, gradient stop, a custom pattern's texels) is stored as its gray "
                    + "by Image ▸ Mode ▸ Grayscale's rule and composites R = G = B, effect blend modes "
                    + "take the Grayscale modes, and Blend If takes channel gray only (any other "
                    + "channel refuses)."
            case "set_global_light":
                note = " A Grayscale document takes the global light, which its effects read."
            case "apply_style_preset", "save_style_preset":
                note = " In a Grayscale document a preset's colours are stored as their gray, "
                    + "as set_layer_style stores them."
            case "list_channels", "duplicate_channel", "delete_channel", "rename_channel",
                 "set_channel_options":
                note = " A Grayscale document holds alpha channels, as an RGB document does."
            case "add_channel":
                note = " In a Grayscale document from plane takes plane gray (the Gray row: "
                    + "the composite's own gray codes) or alpha; red, green and blue refuse for good."
            case "load_selection":
                note = " In a Grayscale document from plane takes plane gray (the Gray row: "
                    + "the composite's own gray codes, whatever the working Gray) or alpha; red, "
                    + "green and blue refuse for good."
            case "warp_layer":
                note = " In a Grayscale document a warp is applied at once and keeps no "
                    + "field (get_warp answers null; clear and bake change nothing), as at 32 bits "
                    + "and in CMYK."
            case "move_selected_pixels", "transform_selected_pixels":
                note = " In a Grayscale document target is layer, mask or channel:<name> "
                    + "(the Gray channel is the layer; red, green, blue and alpha refuse for good)."
            case "copy", "cut", "copy_merged":
                note = " Copying from a Grayscale document makes a Grayscale payload (its "
                    + "Gray profile and depth); the pasteboard's TIFF is a one-channel gray image "
                    + "with that profile embedded (two channels with transparency), the PNG an RGB "
                    + "rendering."
            case "get_clipboard":
                note = " mode is \"gray\" for a payload copied from a Grayscale document."
            case "paste":
                note = " Into or out of a Grayscale document the pixels always convert "
                    + "(into Gray by Image ▸ Mode ▸ Grayscale's luminance, a CMYK payload through "
                    + "ProPhoto RGB; out of Gray by Image ▸ Mode ▸ RGB Color's conversion) and profile "
                    + "preserve refuses; color_conversion names the crossing (from or to \"Gray\", "
                    + "intent relative, black_point_compensation false). Between Grayscale documents "
                    + "the same Gray profile keeps the codes, and another one converts under convert "
                    + "and keeps the numbers under preserve. Another app's gray TIFF pastes the same "
                    + "way."
            case "invert_channel":
                note = " A Grayscale document inverts an alpha channel at 8 and 16 bits."
            case "fill_selection":
                note = " In a Grayscale document the layer is filled with color's gray by "
                    + "Image ▸ Mode ▸ Grayscale's rule (its luminance in the Gray profile), R = G = B; "
                    + "target is layer, mask, quick_mask or channel:<name> (the Gray channel is the "
                    + "layer; red, green, blue and alpha refuse for good), ink refuses, and "
                    + "blend_mode takes the Grayscale modes."
            case "export_image":
                note = " A Grayscale document with profile document keeps gray: png, tiff or "
                    + "jpeg written as ONE channel (two with transparency) at bit_depth 8 or 16 "
                    + "(jpeg 8; the default is 16, or 8 for jpeg and an 8-bit document) with its "
                    + "Gray profile embedded; a 32-bit document's tiff also at bit_depth 32 (its "
                    + "default there), a one-channel float TIFF (f32, or f16 with float_samples); "
                    + "another format has no one-channel export, and profile srgb, display-p3 or "
                    + "file exports RGB."
            case "save_copy":
                note = " A Grayscale document saves as doctored (or rz), png, tiff or jpeg: the image "
                    + "formats as ONE channel (two with transparency) with its Gray profile "
                    + "embedded, bit_depth 16 by default (8 for jpeg and an 8-bit document, 32 for "
                    + "a 32-bit document's tiff: a one-channel float TIFF); a PSD is export_psd's, "
                    + "and every other format refuses (convert to RGB Color first). At 8 and 16 "
                    + "bits flat_save_losses is File > Save's own list for the copy's depth: the "
                    + "layers, masks, styles, layer properties, bounds, saved channels, guides, "
                    + "color samplers, paths, layer comps and global light the file has no place "
                    + "for (and transparency in jpeg), empty when the copy reopens as the document."
            case "get_precision":
                note = " A Grayscale document answers color_mode gray, mode gray8, gray16 or "
                    + "gray32, its gray_profile, and its own supported_tools, supported_menu_items "
                    + "and blend_modes; every other tool refuses for good, by name."
            case "render":
                note = " A Grayscale document renders its composite with no channel, channel "
                    + "gray (no layer: its one gray plane, the 8-bit display of its codes; at 32 bits "
                    + "of the tone-mapped display), channel mask with a layer, or an alpha channel's "
                    + "name with no layer; any other channel refuses."
            case "sample_color":
                note = " An 8- or 16-bit Grayscale document also reports gray_k_percent, "
                    + "100 × (1 − gray), the Info panel's K."
            case "brush_stroke", "fill", "gradient":
                note = " In a Grayscale document the colour paints its gray by Image ▸ Mode ▸ "
                    + "Grayscale's rule (its luminance in the Gray profile), R = G = B, at 8, 16 and 32 "
                    + "bits; target is layer, mask or channel:<name> (the Gray channel is the layer; "
                    + "red, green, blue and alpha refuse for good), ink refuses, and blend_mode takes "
                    + "the Grayscale modes."
            case "eraser_stroke":
                note = " In a Grayscale document target is layer, mask or channel:<name>."
            case "apply_filter":
                note = " In a Grayscale document the adjustments bcs/adjust, levels, curves, "
                    + "exposure, invert, posterize, threshold, gradient_map (stops stored as their "
                    + "gray), shadows_highlights and replace_color run on the gray values; Levels' and "
                    + "Curves' r, g and b must be absent or alike; vibrance, hue_saturation, "
                    + "color_balance, black_and_white, photo_filter, channel_mixer, color_lookup and "
                    + "selective_color refuse, and hue_rotate, grayscale, sepia and "
                    + "white_balance as RGB-only. Every filter runs on the gray values through the "
                    + "RGB kernel, a colour it makes (Lens Correction's fringes, develop_filter's "
                    + "temperature and tint) folded to its Rec. 709 luma and add_noise drawing one "
                    + "full-amplitude delta; clouds' foreground_color and background_color are greyed "
                    + "by Image ▸ Mode ▸ Grayscale's rule; target is layer, mask or channel:<name> "
                    + "(red, green, blue and alpha refuse for good)."
            case "dodge_burn", "blur_stroke", "sharpen_stroke", "smudge_stroke", "heal_stroke",
                 "spot_heal_stroke", "patch_region":
                note = " In a Grayscale document it runs on the gray values (R = G = B) at 8, "
                    + "16 and 32 bits on the layer, or its mask where an RGB document takes one."
            case "sponge_stroke":
                note = " In a Grayscale document the Sponge moves gray levels away from "
                    + "(saturate) or toward (desaturate) the middle gray; vibrance changes nothing "
                    + "there."
            case "clone_stamp":
                note = " In a Grayscale document blend_mode takes the Grayscale modes, and the "
                    + "source is the same Grayscale document."
            case "history_brush_stroke":
                note = " In a Grayscale document the state must be from after Image ▸ Mode ▸ "
                    + "Grayscale; an earlier RGB state refuses by name."
            case "fade":
                note = " In a Grayscale document mode takes the Grayscale modes; Darker "
                    + "Color, Lighter Color, Hue, Saturation, Color and Luminosity refuse."
            case "red_eye", "red_eye_auto":
                note = " A Grayscale document refuses it: Doctored offers Remove Red Eye "
                    + "on RGB documents only."
            case "add_adjustment_layer", "edit_adjustment_layer":
                note = " In a Grayscale document op is bcs, levels, curves, exposure, invert, "
                    + "posterize, threshold or gradient_map (Levels' and Curves' r, g and b absent or "
                    + "alike); shadows_highlights and replace_color apply to pixels only (apply_filter), "
                    + "and every other op refuses by name."
            case "auto_tone", "auto_contrast", "equalize":
                note = " In a Grayscale document it runs on the gray values (Auto Tone and Auto "
                    + "Contrast at 8 and 16 bits); auto_color refuses."
            case "export_preview", "quick_export":
                note = " A Grayscale document with profile document keeps gray, as "
                    + "export_image: png, tiff or jpeg as ONE channel at bit_depth 8 or 16 with its "
                    + "Gray profile embedded, and a 32-bit document's tiff at bit_depth 32 too; "
                    + "another format has no one-channel export."
            case "export_psd", "inspect_psd_export":
                note = " A Grayscale document writes a Grayscale (colour mode 1) PSD, PSB or "
                    + "layered TIFF at its declared bit_depth (8 or 16) in Photoshop's gray layout: "
                    + "ONE gray composite channel (no merged-alpha plane; a layered TIFF of one "
                    + "sample), its Gray profile embedded, effect colours as Grsc, Blend If on Gray; "
                    + "color_mode gray in the report. A 32-bit Grayscale document writes a 32-bit "
                    + "Grayscale PSD or PSB (Raw f32 channels, the linear gamma-1.0 twin of its Gray "
                    + "profile embedded, as Photoshop does); its layered TIFF refuses: the 32-bit "
                    + "layered TIFF is RGB only."
            case "inspect_psd":
                note = " A Grayscale file (colour mode 1) at 8 or 16 bits — PSD, PSB or a layered "
                    + "gray TIFF — or a 32-bit PSD or PSB answers import_precision \"gray\": "
                    + "it opens as a Grayscale document with either choice, its effects' Grsc colours "
                    + "carried as gray, Blend If on Gray only, a refused adjustment and every fill "
                    + "layer omitted by name, profile the embedded Gray profile (null when untagged: "
                    + "the working Gray is assigned at open; at 32 bits the linear twin of a built-in "
                    + "names that built-in)."
            case "stroke_selection":
                note = " In a Grayscale document the stroke paints color's gray by Image ▸ "
                    + "Mode ▸ Grayscale's rule, R = G = B, at 8, 16 and 32 bits, with a selection or "
                    + "on the layer's edge; ink refuses, and blend_mode takes the Grayscale modes."
            case "content_aware_fill", "preview_content_aware_fill",
                 "discard_content_aware_fill_preview", "content_aware_move":
                note = " In a Grayscale document it fills from the gray values, R = G = B, at "
                    + "8, 16 and 32 bits."
            case "remove_stroke":
                note = " In a Grayscale document at 8 and 16 bits each removed pixel is folded "
                    + "to one gray value (the model's colour to its Rec. 709 luma), R = G = B."
            case "art_history_brush_stroke":
                note = " A Grayscale document runs it at 8 bits only (16 refuses), "
                    + "from a Grayscale state, with blend_mode Normal, Darken or Lighten; "
                    + "Hue, Saturation, Color and Luminosity refuse."
            case "hdr_toning":
                note = " A Grayscale document at 8 or 16 bits tones its gray values and keeps "
                    + "Grayscale at its own bit_depth, R = G = B; a 32-bit Grayscale document tones "
                    + "into a Grayscale document at bit_depth 8 or 16 (default 16), as Image ▸ Mode ▸ "
                    + "8/16 Bits/Channel does there."
            case "assign_profile":
                note = " On a Grayscale document profile is working_gray (the default: Edit ▸ "
                    + "Color Settings' working Gray), a Gray space (" + grayspaces + "), installed "
                    + "with name or file with path, a Gray profile; every gray value is kept and only "
                    + "the profile changes. An RGB or CMYK profile (srgb, display_p3, an RGB or CMYK "
                    + "file) refuses by name. Replies color_mode gray, profile, "
                    + "previous_profile and depth."
            case "convert_profile":
                note = " On a Grayscale document profile is working_gray (the default), a Gray "
                    + "space (" + grayspaces + "), installed with name or file with path, a Gray "
                    + "profile: every layer's gray values are re-encoded so the picture keeps its "
                    + "appearance, R = G = B, on the 8-bit grid in an 8-bit document; intent and "
                    + "black_point_compensation apply at 8 and 16 bits (a 32-bit document's linear "
                    + "gray is the same in every Gray profile, so only the profile changes). An RGB or "
                    + "CMYK profile refuses by name; flatten and adjustments do not "
                    + "apply. Type, shape and fill layers stay editable: each re-renders from its "
                    + "gray colours in the new profile (assign_profile too)."
            case "convert_precision":
                note = " A Grayscale document takes mode gray8, gray16 or gray32 (Image ▸ Mode ▸ "
                    + "8/16/32 Bits/Channel, one undo step): 16 → 8 rounds every sample to the 8-bit "
                    + "grid, 8 → 16 keeps them, → 32 decodes them to linear gray; from 32 bits it "
                    + "refuses, and hdr_toning with bit_depth 8 or 16 converts. "
                    + "structure and flatten do not apply."
            case "convert_mode":
                note = " RGB to Grayscale carries every adjustment layer a Grayscale document "
                    + "holds (a Gradient Map's stops greyed by the Mode rule); one it cannot hold "
                    + "(Hue/Saturation and the other colour adjustments, Levels or Curves with unlike "
                    + "channels) is named, and flatten converts. Mode cmyk from an 8- or 16-bit "
                    + "Grayscale document converts its gray through its Gray profile into the press "
                    + "profile at the same bits, keeping and refusing records (flatten converts) as "
                    + "from RGB; a 32-bit Grayscale document refuses, as a float document. From a "
                    + "Grayscale document, mode rgb and mode cmyk keep type, shape and fill layers "
                    + "editable, each re-rendered from its gray colours. RGB or CMYK to Grayscale "
                    + "keeps type, shape and fill layers editable, each re-rendered with its colours "
                    + "as their gray, and carries layer comps (their styles greyed). A Generative "
                    + "Layer converts as its pixels: from a document in the Gray profile's RGB twin "
                    + "its variations stay live, from any other profile they are detached, as "
                    + "convert_profile detaches them on RGB."
            case "add_text_layer", "edit_text_layer", "add_shape_layer", "edit_shape_layer",
                 "add_fill_layer", "edit_fill_layer":
                note = " In a Grayscale document every colour — a run's, a stop's, a custom "
                    + "pattern's texels — is stored as its gray by Image ▸ Mode ▸ Grayscale's rule "
                    + "and renders R = G = B; get_document reports the gray; ink refuses."
            case "rasterize_artwork":
                note = " A Grayscale document rasterizes a type, shape or fill layer to its "
                    + "gray pixels, as on an RGB document."
            case "fill_path", "stroke_path":
                note = " In a Grayscale document the path is painted with color's gray by "
                    + "Image ▸ Mode ▸ Grayscale's rule, R = G = B; ink refuses, and its blend mode "
                    + "takes the Grayscale modes."
            case "list_paths", "set_path", "rename_path", "duplicate_path", "delete_path",
                 "selection_to_path", "path_to_selection", "get_vector_mask", "set_vector_mask",
                 "set_vector_mask_properties", "remove_vector_mask", "set_clipping_path":
                note = " A Grayscale document holds paths and vector masks, as an RGB "
                    + "document does, and keeps them in .doctored."
            case "list_layer_comps", "save_layer_comp", "apply_layer_comp", "update_layer_comp",
                 "delete_layer_comp", "reorder_layer_comp":
                note = " A Grayscale document holds layer comps, as an RGB document does, and "
                    + "keeps them in .doctored; applying one skips a recorded blend mode or style the mode "
                    + "cannot hold."
            case "link_layers", "unlink_layers", "list_snapshots", "create_snapshot",
                 "rename_snapshot", "delete_snapshot", "restore_snapshot", "save_snapshot",
                 "load_snapshot":
                note = " A Grayscale document takes it as on an RGB document."
            case "clear_selection":
                note = " In a Grayscale document it erases the selected pixels of a Grayscale "
                    + "layer, mask or alpha channel."
            case "get_proof_setup", "set_proof_setup", "render_proof":
                note = " A Grayscale document at 8 or 16 bits proofs as RGB: its composite, "
                    + "R = G = B, through its Gray profile, so preserve_numbers refuses as on an RGB "
                    + "document; a 32-bit Grayscale document refuses, as a float document."
            case "open_document":
                note = " A Grayscale PSD, PSB or layered TIFF at 8 or 16 bits, or a PSD or PSB at "
                    + "32 (inspect_psd import_precision \"gray\"), opens as a Grayscale document at "
                    + "its depth with either psd_mode, its Gray profile kept (an untagged file takes "
                    + "the working Gray); a one-channel float TIFF with a Gray profile opens "
                    + "as a 32-bit Grayscale document."
            case "histogram":
                note = " A Grayscale document answers channels [\"gray\"]: bins and "
                    + "bin_weight_units hold the one Gray row (R = G = B), no _luma keys; source "
                    + "scalar takes target mask or channel:<name> (red, green, blue and alpha refuse "
                    + "for good)."
            case "apply_image":
                note = " In a Grayscale document target is layer (its Gray, blended as one "
                    + "plane and written R = G = B, one undo step) or channel:<name>; source_plane "
                    + "is gray, alpha or an alpha channel's name; blend_mode takes the Grayscale "
                    + "modes; a 32-bit Grayscale document refuses, as a float document."
            case "calculations":
                note = " In a Grayscale document source1_plane and source2_plane are gray, "
                    + "alpha or an alpha channel's name, and blend_mode takes the Grayscale modes."
            case "stack_mode", "reduce_noise_from_stack", "auto_align_layers", "auto_blend_layers":
                note = " A Grayscale document runs it on the gray values through the RGB "
                    + "kernel, R = G = B, as on an RGB document."
            case "load_files_into_stack", "merge_panorama":
                note = " When the first file opens as Grayscale the stack is Grayscale: every "
                    + "later file is converted into its Gray profile and depth by Image ▸ Mode ▸ "
                    + "Grayscale's rule, R = G = B."
            case "merge_to_hdr", "merge_layers_to_hdr":
                note = " Grayscale exposures merge into a 32-bit Grayscale document in their "
                    + "Gray profile, R = G = B."
            case "select_sky", "select_person_parts", "object_finder_select":
                note = " A Grayscale document is read through its view, R = G = B, as the "
                    + "model sees an RGB document."
            case "super_zoom", "jpeg_artifacts_removal":
                note = " In an 8- or 16-bit Grayscale document the model reads the gray "
                    + "values and its result is folded to its gray by Image ▸ Mode ▸ Grayscale's "
                    + "rule (Super Zoom's new document is Grayscale in the same Gray profile); a "
                    + "32-bit Grayscale document refuses, as a float document."
            case "generative_fill", "generative_expand", "generative_variations",
                 "select_generative_variation", "generate_variations":
                note = " In an 8- or 16-bit Grayscale document the provider sees the gray "
                    + "composite and every picture lands folded to its gray by Image ▸ Mode ▸ "
                    + "Grayscale's rule, R = G = B; a 32-bit Grayscale document refuses, as a float "
                    + "document."
            case "neural_colorize":
                note = " A Grayscale document refuses it for good: a Grayscale layer holds no "
                    + "colour. Convert to RGB Color first."
            case "sky_replacement":
                note = " A Grayscale document refuses it for good: Doctored offers Sky "
                    + "Replacement on RGB documents only."
            case "place_image", "add_live_photo_layer":
                note = " In a Grayscale document the placed pixels land folded to their gray "
                    + "by Image ▸ Mode ▸ Grayscale's rule, R = G = B, as a plain layer with no "
                    + "retained source; a camera RAW file refuses (Doctored keeps RAW layers on RGB "
                    + "documents only)."
            case "export_layers":
                note = " A Grayscale document with profile document writes each layer as a "
                    + "one-channel PNG, TIFF or JPEG with its Gray profile embedded, as export_image."
            case "export_pdf":
                note = " A Grayscale document writes its composite, R = G = B, as on an RGB "
                    + "document."
            case "copy_layers_to_document":
                note = " Into or out of a Grayscale document the layers convert as paste "
                    + "does, and type, shape and fill layers stay editable, each re-rendered from "
                    + "its authored colours (into Gray as their gray)."
            default:
                note = nil
            }
            if let note { catalog[index]["description"] = existing + note }
        }
    }

    /// The Gray values of `assign_profile` / `convert_profile`'s `profile` enum (`installed`,
    /// `working_gray`, every Gray space) and `convert_precision`'s `mode` enum (`gray8`,
    /// `gray16`, `gray32`), each appended only when absent, and `convert_profile`'s `name` when
    /// absent — `annotateOutputColorCatalog`'s read-modify-write shape.
    private func annotateGraySchema(_ entry: inout [String: Any], name: String) {
        guard ["assign_profile", "convert_profile", "convert_precision"].contains(name),
              var schema = entry["inputSchema"] as? [String: Any],
              var properties = schema["properties"] as? [String: Any] else { return }
        func appending(_ key: String, _ values: [String], note: String) {
            guard var property = properties[key] as? [String: Any] else { return }
            var options = (property["enum"] as? [String]) ?? []
            for value in values where !options.contains(value) { options.append(value) }
            property["enum"] = options
            let description = (property["description"] as? String) ?? ""
            property["description"] = description.isEmpty ? note : description + " " + note
            properties[key] = property
        }
        if name == "convert_precision" {
            appending("mode", ["gray8", "gray16", "gray32"],
                      note: "gray8, gray16 and gray32 apply to a Grayscale document, and only there.")
        } else {
            appending("profile", ["installed", "working_gray"] + GrayWorkingSpace.allCases.map(\.rawValue),
                      note: "On a Grayscale document: working_gray (the default), a Gray space "
                        + "by name, installed or file; srgb and display_p3 refuse there.")
            if name == "convert_profile", properties["name"] == nil {
                properties["name"] = [
                    "type": "string",
                    "description": "With profile:\"installed\", the profile's display name.",
                ]
            }
        }
        schema["properties"] = properties
        entry["inputSchema"] = schema
    }
}
