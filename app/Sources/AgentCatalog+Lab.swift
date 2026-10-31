import Foundation

/// The Lab Color document's catalog notes: what a Lab document does to the shared layer, style,
/// Fill, save and export tools; the readouts (`get_precision`, `sample_color`, `sample_pixel`,
/// `histogram` and `render`'s channels); the paint tools, the adjustments, the filters, Fade,
/// Auto Tone, Auto Contrast, Equalize and the Lab → RGB refusal of a channel Levels or Curves
/// layer; the colour ops' ProPhoto route, a filter on a mask or alpha channel, the Color Samplers
/// and RGB → Lab carrying adjustment layers; the retouch and content-aware tools, layer effects,
/// the Lightness, a, b and alpha targets, Blend If, patterns and `stroke_selection`; every Lab
/// blend mode composited by its Lab arm (a mode in neither list would be named by `waitingTail`,
/// none today); opening and writing Lab TIFF, PSD, PSB and layered TIFF files; the resamples,
/// transforms, warps, Liquify, Align and Distribute and `move_layers` on the codes; the selected
/// pixels, Layer Via, Stamp Selected, Select ▸ Modify and the Lightness, a and b planes of
/// `load_selection` and `add_channel` (`annotateLabSchema`); `select_color_range` on the codes;
/// the clipboard across modes, `duplicate_to_new_document`, `copy_layers_to_document`, the Magic
/// Wand's Lab rule and Select Similar's and Grow's at 16 bits and refusal at 8 (#5);
/// `convert_mode cmyk` directly, `convert_precision` lab8/lab16, the proof tools on the Display P3
/// view, `convert_profile` as a mode conversion and `assign_profile`'s refusal; the type, shape
/// and fill layer, path and vector mask tools, their colours authored sRGB rendered as L*a*b*
/// codes; the channel math, Stack Modes, Load Files into Stack and Merge to HDR; the layer comp,
/// link, snapshot and Clear tools; Auto-Align, Auto-Blend, Merge Panorama, Quick Selection,
/// Select and Mask, Quick Mask and the model selections; and the neural and generative tools on
/// the Display P3 view and Sky Replacement's refusal (`LabDocument`'s
/// gate, `LabAdjust`, the core's `lab_document::admit_edit`, `lab_adjust`, `lab_filter`,
/// `lab_convert`, `lab_save`, `lab_rgb`, `lab_blend` and `lab_comps`). Behind an explicit
/// non-inlining boundary, for `AgentCatalog.swift`'s reason.
extension AgentServer {
    /// `items` as prose, "a, b and c" (no serial comma), the `ExportCapabilities.list` voice.
    private static func prose(_ items: [String]) -> String {
        guard let last = items.last else { return "" }
        guard items.count > 1 else { return last }
        return items.dropLast().joined(separator: ", ") + " and " + last
    }

    @inline(never) func annotateLabCatalog(_ catalog: inout [[String: Any]]) {
        let modes = Self.prose(NativeLayerBlending.labModes.map { DrBlendMode.displayName(for: $0) })
        let notOffered = Self.prose(
            NativeLayerBlending.labNotOffered.map { DrBlendMode.displayName(for: $0) })
        // A mode in neither list, which Doctored does not yet composite in Lab (none
        // now): each clause names them, or says nothing when there are none.
        let waiting = DrBlendMode.allBlendModes.filter {
            !NativeLayerBlending.labModes.contains($0.0)
                && !NativeLayerBlending.labNotOffered.contains($0.0)
        }.map { $0.1 }
        let waitingTail = waiting.isEmpty ? "" : ", and " + Self.prose(waiting)
            + (waiting.count == 1 ? " is" : " are") + " unavailable; Doctored does not yet "
            + "composite " + (waiting.count == 1 ? "it" : "them") + " in Lab"
        let artWaiting = waiting.filter { ["Hue", "Saturation", "Color", "Luminosity"].contains($0) }
        let artWaitingTail = artWaiting.isEmpty ? "" : ", and " + Self.prose(artWaiting)
            + (artWaiting.count == 1 ? " is" : " are") + " unavailable; Doctored does not yet "
            + "composite " + (artWaiting.count == 1 ? "it" : "them") + " in Lab"
        // The adjustment ops by availability; Auto, Equalize and HDR Toning are tools of their own.
        let tools: Set<String> = [
            "auto_tone", "auto_contrast", "auto_color", "equalize", "hdr_toning",
        ]
        let ops = { (wanted: (LabAdjust.Availability) -> Bool) in
            Self.prose(LabAdjust.adjustments.filter { wanted($0.value) && !tools.contains($0.key) }
                .keys.sorted())
        }
        let layered = ops { $0 == .runs(layer: true) }
        let pixelsOnly = ops { $0 == .runs(layer: false) }
        let filters = { (wanted: LabAdjust.Availability) in
            Self.prose(LabAdjust.filters.filter { $0.value == wanted }.keys.sorted())
        }
        let rgb = " A Lab Color document exports through RGB: the composite is converted by "
            + "Image ▸ Mode ▸ RGB Color's rule (the request's intent and black_point_compensation; "
            + "out-of-gamut colours clip) after resampling and before sharpening, and profile "
            + "document writes sRGB."
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            annotateLabSchema(&catalog[index], name: name)
            let existing = (catalog[index]["description"] as? String) ?? ""
            let note: String?
            switch name {
            case "set_layer_properties":
                note = " In a Lab Color document blend_mode takes " + modes + " (each "
                    + "composited on the L*a*b* values, measured against Photoshop's Lab "
                    + "composite), and Pass "
                    + "Through on a group; " + notOffered + " are unavailable"
                    + waitingTail + ". fill_opacity is available."
            case "set_layer_style", "apply_style_preset":
                note = " In a Lab Color document a style takes fill_opacity, knockout and "
                    + "every effect; an effect's blend takes " + modes + " (" + notOffered
                    + " refuse" + waitingTail + "); its colours "
                    + "are sRGB, converted at composite by Image ▸ Mode ▸ Lab Color's rule and "
                    + "stored as given. A pattern_overlay's or pattern stroke's tile (a preset or "
                    + "rgba) is sRGB too: each texel becomes its 8-bit L*a*b* values by that rule "
                    + "before scale and angle, and the tile is stored as given. "
                    + "blend_if takes channel red, green and blue, which are "
                    + "Lightness, a and b (each range on the stored values, a* = 0 at 128; "
                    + "not measured against Photoshop); gray refuses, an identity range "
                    + "included."
            case "fill_selection":
                note = " In a Lab Color document the layer is filled with color converted by "
                    + "Image ▸ Mode ▸ Lab Color's rule (color_space document takes the L*a*b* "
                    + "values as unit codes, exactly); target is layer, mask, lightness, a, b, "
                    + "alpha or channel:<name> (red, green and blue refuse: a Lab document's planes "
                    + "are lightness, a and b), ink refuses, and blend_mode takes the Lab modes."
            case "stroke_selection":
                note = " In a Lab Color document the stroke (a selection's band, or the "
                    + "layer's edge with nothing selected) is painted with color converted by "
                    + "Image ▸ Mode ▸ Lab Color's rule (color_space document takes the L*a*b* "
                    + "values as unit codes, exactly); target is layer, mask, lightness, a, b, "
                    + "alpha or channel:<name> (red, green and blue refuse: a Lab document's planes "
                    + "are lightness, a and b), ink refuses, and blend_mode takes " + modes + "; "
                    + notOffered + " are unavailable" + waitingTail + "."
            case "brush_stroke", "fill", "gradient":
                note = " In a Lab Color document the colour paints its L*a*b* values by Image "
                    + "▸ Mode ▸ Lab Color's rule, at 8 and 16 bits (color_space document takes the "
                    + "L*a*b* values as unit codes, L*/100 and (a* + 128)/255, exactly); target is "
                    + "layer, mask, lightness, a, b, alpha or channel:<name> (red, green and blue "
                    + "refuse: a Lab document's planes are lightness, a and b), ink refuses, and "
                    + "blend_mode takes " + modes + "; " + notOffered + " are unavailable"
                    + waitingTail + "."
            case "eraser_stroke":
                note = " In a Lab Color document target is layer, mask, lightness, a, b, "
                    + "alpha or channel:<name>."
            case "dodge_burn":
                note = " In a Lab Color document Dodge and Burn shape Lightness alone (a* and "
                    + "b* are kept); protect_tones is the plain curve on Lightness."
            case "sponge_stroke":
                note = " In a Lab Color document the Sponge doubles (saturate) or zeroes "
                    + "(desaturate) a* and b* about 0, keeping L*; vibrance changes nothing."
            case "blur_stroke", "sharpen_stroke", "smudge_stroke", "heal_stroke",
                 "spot_heal_stroke", "patch_region", "content_aware_fill",
                 "preview_content_aware_fill", "content_aware_move":
                note = " In a Lab Color document it works on the L*, a* and b* values, each "
                    + "channel alone, at 8 and 16 bits."
            case "clone_stamp":
                note = " In a Lab Color document it clones the L*a*b* values, each channel "
                    + "alone; blend_mode takes " + modes + "; " + notOffered + " are unavailable"
                    + waitingTail + "."
            case "remove_stroke":
                note = " In a Lab Color document the model sees the layer's Display P3 view "
                    + "(the colours the canvas shows) and its fill is converted back to L*a*b* "
                    + "values before it is mixed by coverage on the L*a*b* values; mode "
                    + "content_aware works on the L*, a* and b* values, each channel alone, at 8 "
                    + "and 16 bits."
            case "art_history_brush_stroke":
                note = " In a Lab Color document it runs at 8 bits from a Lab Color state "
                    + "(an RGB state refuses), painting the L*, a* and b* values each channel "
                    + "alone; its strokes follow Lightness, and tolerance compares Lightness, a and "
                    + "b. blend_mode Darken and Lighten are unavailable"
                    + artWaitingTail + ". A 16-bit Lab document refuses."
            case "red_eye", "red_eye_auto":
                note = " In a Lab Color document the red test and the correction run on each "
                    + "pixel's 16-bit ProPhoto RGB colour, so a colour outside sRGB is kept, and a "
                    + "pixel left alone keeps its values"
                    + (name == "red_eye_auto" ? "; detection reads the Lab view." : ".")
            case "history_brush_stroke":
                note = " In a Lab Color document the source state must be a Lab Color state "
                    + "(one from before Image ▸ Mode ▸ Lab Color refuses), and a Lab state paints no "
                    + "other document."
            case "apply_filter", "add_adjustment_layer", "edit_adjustment_layer":
                let what = name == "apply_filter" ? "filter" : "op"
                let adjust = " In a Lab Color document " + what + " " + layered + " run on "
                    + "the L*a*b* values (adjust is bcs), destructively and as a layer, and "
                    + pixelsOnly + " destructively only (as a layer they refuse): "
                    + "levels and curves adjust Lightness with the composite "
                    + "and their r stage, a* with g and b* with b; bcs adjusts Lightness and its "
                    + "saturation scales a* and b* about 0; invert and posterize act on every "
                    + "channel; threshold gives black and white (a* = b* = 0); hue_saturation of "
                    + "saturation -1 alone (Desaturate) keeps L* and sets a* = b* = 0; the other "
                    + "hue_saturation, color_balance, photo_filter, shadows_highlights and "
                    + "replace_color run on each sample's 16-bit ProPhoto RGB colour, so a colour "
                    + "outside sRGB keeps its chroma, and color_lookup and gradient_map on its sRGB "
                    + "colour (a sample the op leaves alone keeps its values); photo_filter's color "
                    + "is sRGB, and replace_color's colors are the document's own 8-bit #LLaabb Lab "
                    + "values (LL = round(255·L*/100), aa and bb = round(a* + 128), round(b* + 128)). "
                    + ops { $0 == .unavailable } + " refuse; "
                    + ops { $0 == .rgbOnly } + " as RGB-only."
                let filter = " Every other filter runs on the L*a*b* values, each channel filtered "
                    + "alone at 8 and 16 bits (add_noise draws a* and b* too; clouds' colours become "
                    + "their Lab values); " + filters(.unavailable) + " refuse; "
                    + filters(.developFilter) + " as Develop Filter controls disabled in "
                    + "Lab; " + filters(.rgbOnly) + " as RGB-only. "
                    + "lens_blur, reduce_noise and smart_sharpen run on the layer per channel, "
                    + "each of L*, a* and b* filtered alone as a gray image; emboss and edge_detect "
                    + "run on Lightness and leave a* = b* = 0 (mixed toward 0 by a partial "
                    + "selection); reduce_noise's color has no effect on Lab (with luminance 0 it "
                    + "changes nothing); offset with undefined_areas background on a lightness, a "
                    + "or b target fills that plane's value of the background colour's L*a*b*. "
                    + "target is layer, mask, lightness, a, b, alpha or channel:<name>: on a mask, "
                    + "an alpha channel or a plane every filter and adjustment runs as on RGB but "
                    + "those that refuse for good and liquify."
                let auto = " A Levels layer's auto {mode tone} sets Lightness alone (params r; a* "
                    + "and b* are kept); mode color refuses."
                note = adjust + (name == "apply_filter" ? filter : auto)
            case "fade":
                note = " In a Lab Color document the fade mode takes " + modes + "; "
                    + notOffered + " are unavailable" + waitingTail + "."
            case "auto_tone", "auto_contrast":
                note = " In a Lab Color document it stretches Lightness alone (a* and b* are "
                    + "kept); target is layer."
            case "equalize":
                note = " In a Lab Color document it equalizes Lightness alone (a* and b* are "
                    + "kept); target is layer."
            case "convert_mode":
                note = " A Lab Color document converted to rgb carries its adjustment layers "
                    + "rebound, a Levels or Curves layer with r, g or b settings too (its "
                    + "Lightness, a and b stages become Red, Green and Blue by channel index); layer "
                    + "styles and a red, green or blue Blend If carry unchanged. An RGB or "
                    + "Grayscale document converted to lab carries its adjustment layers too, each "
                    + "description unchanged and acting in its Lab form (r, g and b settings "
                    + "becoming Lightness, a and b), its layer styles unchanged and a red, green or "
                    + "blue Blend If by index, but an adjustment a Lab document refuses, a "
                    + "shadows_highlights or replace_color layer, an effect in a mode the Lab list "
                    + "lacks and a gray Blend If refuse by name with flatten_required (flatten "
                    + "true converts); a Generative Layer converts as its pixels and its "
                    + "variations detach, as convert_profile detaches them. A Lab "
                    + "document converts to cmyk directly "
                    + "through 16-bit ProPhoto RGB (relative colorimetric with BPC, no 8-bit RGB "
                    + "stop) at its own depth; an adjustment layer, a Blend If on Lightness, a or "
                    + "b, a mode CMYK lacks, a placed or warped layer refuse by name with "
                    + "flatten_required (flatten true converts). mode \"gray\" converts "
                    + "directly, each pixel's L* through the gray_space's tone curve (a* and b* "
                    + "discarded), keeping its depth; an adjustment layer, a Blend If on "
                    + "Lightness, a or b, or a mode Grayscale lacks refuses by name with "
                    + "flatten_required (flatten true converts)."
            case "convert_precision":
                note = " A Lab Color document converts between mode lab8 and lab16 "
                    + "(Image ▸ Mode ▸ 8/16 Bits/Channel): 16 → 8 puts every L*, a*, b*, alpha, "
                    + "mask and alpha-channel value on the 8-bit grid, 8 → 16 keeps every value; "
                    + "dry_run answers the plan. 32 Bits/Channel is unavailable in Lab Color."
            case "open_document", "inspect_psd":
                note = " A Lab TIFF (CIELab or ICCLab, chunky, 8 or 16 bits, no alpha or an "
                    + "unassociated one), or a Lab PSD, PSB or layered TIFF at 8 or 16 bits "
                    + "(inspect_psd answers import_precision \"lab\"), opens as a Lab Color "
                    + "document at its own depth with either psd_mode, keeping its L*a*b* values; "
                    + "an embedded profile is not used."
            case "save_copy":
                note = " A Lab Color document saves as doctored (or rz) or tiff: a TIFF 6.0 CIELab file of "
                    + "the flattened composite at the document's own depth (another bit_depth "
                    + "refuses), no ICC profile; export_psd writes its PSD, and every other format "
                    + "refuses (export_image writes an RGB copy)."
            case "export_psd", "inspect_psd_export":
                note = " A Lab Color document writes a Lab Color (colour mode 9) "
                    + "PSD, PSB or layered TIFF at its own depth (8 or 16 bits), as Photoshop "
                    + "writes Lab: its L*a*b* values as Photoshop stores them (16-bit a* and b* on "
                    + "Photoshop's 15-bit grid), no ICC profile, the composite over Lab white, "
                    + "layer effect colours as Lab; every adjustment layer is written as a pixel "
                    + "layer of the picture up to it, reported in losses."
            case "export_image", "export_preview", "quick_export":
                note = rgb
            case "get_precision":
                note = " A Lab Color document answers its own lists: color_mode lab, mode "
                    + "lab8 or lab16, white_point D50, channels lightness, a and b, and the "
                    + "supported_tools, supported_menu_items and blend_modes of its gate."
            case "sample_color", "sample_pixel":
                note = " In a Lab Color document the codes are L*, a*, b* and alpha in the ICC "
                    + "v4 16-bit Lab encoding, and the reply adds lab {l, a, b} (L* = 100·v, a* and "
                    + "b* = 255·v − 128, D50) with no srgb_rgba: the document has no RGB reading."
            case "histogram":
                note = " In a Lab Color document bins and bin_weight_units hold the Lightness, "
                    + "a and b rows (channels lists them; no luma), each bin round(code·255/65535); "
                    + "source scalar reads a mask, an alpha channel (channel:<name>), or target "
                    + "lightness, a or b: the plane of L*, a* or b* codes, its target named so and "
                    + "lab_value_rule reading a code, or target alpha; target red, green or blue "
                    + "refuses."
            case "get_color_samplers", "set_color_samplers":
                note = " In a Lab Color document each sampler is sample_pixel's own reading: "
                    + "its codes are L*, a*, b* and alpha, and it adds lab {l, a, b} with no RGB "
                    + "reading."
            case "render":
                note = " In a Lab Color document channel lightness, a or b with no layer "
                    + "renders that channel of the composite's codes in grayscale; channel mask "
                    + "takes a layer, an alpha channel is named with no layer, and every other "
                    + "channel refuses."
            case "image_size", "canvas_size", "crop", "perspective_crop", "fit_crop", "trim",
                 "reveal_canvas", "rotate", "flip", "straighten_layers", "transform_layer",
                 "transform_layers", "transform_again", "transform_mask", "transform_selection":
                let geometry = " In a Lab Color document the resample runs the RGB "
                    + "kernel on the L*a*b* codes."
                note = name == "crop"
                    ? geometry + " A crop past the canvas pads the Background with "
                        + "background_color converted by Image ▸ Mode ▸ Lab Color's rule; "
                        + "Generative Expand fills the new area (fill generative_expand), its "
                        + "pictures landing as L*a*b* codes by the same rule."
                    : geometry
            case "warp_layer", "get_warp", "puppet_warp", "perspective_warp", "liquify",
                 "align_layers", "distribute_layers", "move_layers":
                let kernel = " In a Lab Color document it runs the RGB kernel on "
                    + "the L*a*b* codes."
                switch name {
                case "warp_layer", "get_warp":
                    note = kernel + " The warp is applied at once and keeps no field (get_warp "
                        + "answers null), as in a Grayscale, CMYK or 32-bit document."
                default:
                    note = kernel
                }
            case "move_selected_pixels", "transform_selected_pixels":
                note = " In a Lab Color document it runs the RGB kernel "
                    + "on the L*a*b* codes; target is layer, mask, channel:<name>, lightness, a, b "
                    + "or alpha (red, green and blue refuse: a Lab document's planes are lightness, "
                    + "a and b). On lightness, a or b the vacated area takes that plane's code of "
                    + "background (its L*a*b* by Image ▸ Mode ▸ Lab Color's rule: "
                    + "round(L*·255/100), round(a* + 128) or round(b* + 128)); on alpha it is "
                    + "transparent."
            case "layer_via_copy", "layer_via_cut", "stamp_selected":
                note = " In a Lab Color document it runs on the L*a*b* codes, as "
                    + "on RGB."
            case "load_selection", "add_channel":
                note = " In a Lab Color document plane is lightness, a or b (the "
                    + "stored L*, a* and b* codes), luma (= Lightness) or alpha; red, green and blue "
                    + "refuse."
            case "modify_selection":
                note = " In a Lab Color document it is colour-free and runs as on "
                    + "RGB."
            case "select_color_range":
                note = " In a Lab Color document samples and samples16 are the "
                    + "document's L*a*b* codes (as sample_pixel reads them), and tolerance and "
                    + "softness are in L*a*b* units of the largest per-channel difference (|ΔL*|, "
                    + "|Δa*| or |Δb*|), measured against Photoshop's Color Range in "
                    + "Lab, not ΔE76. Every range runs: highlights, midtones, "
                    + "shadows and custom read the codes' own L*; the hue ranges and skin_tones "
                    + "read the codes' 16-bit ProPhoto RGB (Hue/Saturation's space), skin_tones' "
                    + "Lab the codes' own; out_of_gamut proofs the Display P3 view, as the gamut "
                    + "warning does."
            case "apply_image":
                note = " In a Lab Color document each channel's codes blend alone: "
                    + "target is layer (Lightness, a and b each from the source's own), lightness, "
                    + "a, b, alpha or channel:<name>; source_plane is lab (the composite or layer, "
                    + "channel for channel; Lightness onto one plane), lightness, a, b, alpha or "
                    + "an alpha channel's name; blend_mode takes "
                    + Self.prose(LabDocument.channelMathModes.map { DrBlendMode.displayName(for: $0) })
                    + " (a layer set to the same mode composites by its Lab arm instead)."
            case "calculations":
                note = " In a Lab Color document source1_plane and source2_plane "
                    + "are lab (Lightness), lightness, a, b, alpha or an alpha channel's name, and "
                    + "blend_mode takes "
                    + Self.prose(LabDocument.channelMathModes.map { DrBlendMode.displayName(for: $0) })
                    + ", each on the codes."
            case "stack_mode", "reduce_noise_from_stack":
                note = " A Lab Color document runs it per channel on the L*a*b* "
                    + "codes; an 8-bit result can land off the 8-bit grid, as paint does."
            case "load_files_into_stack":
                note = " When the first file opens as Lab Color the stack is Lab: "
                    + "every later file is converted by Image ▸ Mode ▸ Lab Color's rule at its depth."
            case "merge_to_hdr", "merge_layers_to_hdr":
                note = " Lab Color exposures are converted to 16-bit ProPhoto RGB "
                    + "first (black point compensation) and merge into a 32-bit RGB document in "
                    + "ProPhoto RGB, as there is no 32-bit Lab; the source document stays Lab."
            case "copy", "cut", "copy_merged":
                note = " In a Lab Color document the payload keeps the L*a*b* codes "
                    + "(a Lab Color payload); the pasteboard's image for other apps is the RGB "
                    + "rendering (Display P3)."
            case "paste":
                note = " Into or out of a Lab Color document the pixels always "
                    + "convert across modes: into Lab by Image ▸ Mode ▸ Lab Color's rule "
                    + "(relative colorimetric, no black point compensation), out of Lab relative "
                    + "colorimetric with black point compensation, Grayscale and CMYK through "
                    + "16-bit ProPhoto RGB; profile preserve refuses across modes. Between Lab "
                    + "documents both profiles keep the codes, and a 16-bit payload lands on an "
                    + "8-bit document's grid."
            case "get_clipboard":
                note = " mode is \"lab\" for a Lab Color payload; its info carries "
                    + "lab_depth."
            case "duplicate_to_new_document":
                note = " A Lab Color document's duplicate is a Lab Color document "
                    + "of the same depth holding the same codes."
            case "copy_layers_to_document":
                note = " Into or out of a Lab Color document the layers convert "
                    + "by the mode conversions (into Lab by Image ▸ Mode ▸ Lab Color, out of Lab "
                    + "by Image ▸ Mode ▸ RGB Color with black point compensation, Grayscale and "
                    + "CMYK through 16-bit ProPhoto RGB); profile preserve refuses across modes."
            case "select_magic_wand":
                note = " In a Lab Color document it compares 8-bit Lab codes at both "
                    + "depths (measured against Photoshop): L, a and b within ± tolerance of the "
                    + "seed's, and the halved differences (a − L), (b − a) and (L − b) within ± "
                    + "half the tolerance; a 16-bit document's codes are read through "
                    + "Photoshop's 15-bit Lab grid. The Paint Bucket fills the same region."
            case "select_similar":
                note = " In a Lab Color document at 16 bits, global and contiguous (Grow) "
                    + "compare the Magic Wand's Lab rule against the box of the selected pixels' "
                    + "codes (measured against Photoshop). At 8 bits both are unavailable until their "
                    + "Lab tolerance rule is measured (#5)."
            case "get_proof_setup", "set_proof_setup", "render_proof", "save_proof_preset":
                note = " A Lab Color document proofs as RGB: its Display P3 view "
                    + "(what the canvas shows) through the proof profile, so render_proof answers "
                    + "space \"display-p3\", exact true proofs the 16-bit view, and preserve_numbers "
                    + "refuses as on an RGB document."
            case "assign_profile":
                note = " A Lab Color document has no document profile: assign_profile "
                    + "is unavailable in it."
            case "convert_profile":
                note = " A Lab Color document has no document profile: "
                    + "convert_profile converts it, as Image ▸ Mode does, to CMYK Color in a CMYK "
                    + "profile (working_cmyk, the default, or an installed or file CMYK profile) or "
                    + "to RGB Color in an RGB one (srgb, display_p3, or an installed or file RGB "
                    + "profile), at its own depth, with intent, black_point_compensation and "
                    + "flatten as convert_mode takes them. Profile working_gray, "
                    + "a Gray working space (" + GrayWorkingSpace.allCases.map(\.rawValue)
                        .joined(separator: ", ") + ") or an installed or file Gray profile "
                    + "converts it to Grayscale in that profile, each pixel's L* through its tone "
                    + "curve, as convert_mode gray does (intent and black_point_compensation do "
                    + "not apply); the reply's color_mode is gray."
            case "add_text_layer", "edit_text_layer", "add_shape_layer", "edit_shape_layer",
                 "add_fill_layer", "edit_fill_layer":
                note = " In a Lab Color document the colours stay authored sRGB "
                    + "(pass color; ink refuses) and render as L*a*b* codes by Image ▸ Mode ▸ Lab "
                    + "Color's rule (a pattern's tile too, texel by texel; a gradient between its "
                    + "converted stops), an 8-bit layer on the ×257 grid; Image ▸ Mode conversions "
                    + "re-render the layer in the new mode."
            case "list_layer_comps", "save_layer_comp", "apply_layer_comp", "update_layer_comp",
                 "delete_layer_comp", "reorder_layer_comp":
                note = " A Lab Color document holds layer comps, as an RGB "
                    + "document does, and keeps them in .doctored and through Image ▸ Mode between RGB, "
                    + "Grayscale and Lab Color; applying one skips a recorded blend mode or style "
                    + "the mode cannot hold."
            case "link_layers", "unlink_layers", "list_snapshots", "create_snapshot",
                 "rename_snapshot", "delete_snapshot", "restore_snapshot", "save_snapshot",
                 "load_snapshot":
                note = " A Lab Color document takes it as on an RGB document."
            case "clear_selection":
                note = " In a Lab Color document it erases the selected pixels "
                    + "of a layer, as on an RGB document."
            case "auto_align_layers":
                note = " In a Lab Color document Vignette Removal and Geometric "
                    + "Distortion read each layer's CIE Y (its L*), Vignette Removal's gain "
                    + "multiplies the L*a*b* codes' XYZ (the gain any linear RGB would take), and "
                    + "the reprojection and warps resample the codes; an 8-bit result can land off "
                    + "the 8-bit grid, as paint does."
            case "auto_blend_layers":
                note = " A Lab Color document refuses it for good."
            case "merge_panorama":
                note = " Lab Color files stack as Lab Color (Load Files into "
                    + "Stack's rule) and run Auto-Align and Auto-Blend's Lab rules, so the result "
                    + "is a Lab Color document."
            case "quick_select":
                note = " In a Lab Color document the ΔE76 tolerance and edge "
                    + "read the codes' own L*a*b* values."
            case "refine_selection":
                note = " In a Lab Color document the edge stage guides on the "
                    + "L*a*b* codes per channel, and decontaminate solves per channel on the codes "
                    + "(the linear mix Normal compositing uses)."
            case "set_quick_mask", "quick_mask_stroke":
                note = " In a Lab Color document Quick Mask is colour-free "
                    + "coverage, as on an RGB document (fill_selection takes target quick_mask)."
            case "select_subject", "select_subjects", "inspect_subjects", "select_people",
                 "select_face_parts", "inspect_faces", "object_select", "select_sky",
                 "select_person_parts", "object_finder_select":
                note = " In a Lab Color document the model reads the document's "
                    + "Display P3 view (what the canvas shows); the selection is colour-free "
                    + "coverage."
            case "super_zoom", "jpeg_artifacts_removal", "neural_colorize":
                note = " In a Lab Color document the model reads the layer's "
                    + "Display P3 view (what the canvas shows) and its result returns to L*a*b* "
                    + "codes by Image ▸ Mode ▸ Lab Color's rule, on the 8-bit grid at 8 bits"
                    + (name == "jpeg_artifacts_removal" ? "."
                        : "; a new document is Lab Color at the source's depth.")
            case "generative_fill", "generative_expand", "generative_variations",
                 "select_generative_variation", "generate_variations":
                note = " In a Lab Color document the provider sees the "
                    + "composite's Display P3 view and every picture lands as L*a*b* codes by Image "
                    + "▸ Mode ▸ Lab Color's rule, on the 8-bit grid at 8 bits."
            case "sky_replacement":
                note = " A Lab Color document refuses it for good: Doctored "
                    + "offers Sky Replacement on RGB documents only."
            case "place_image", "add_live_photo_layer":
                note = " In a Lab Color document the picture renders into 16-bit ProPhoto "
                    + "RGB and lands as L*a*b* codes by Image ▸ Mode ▸ Lab Color's rule, a plain "
                    + "layer that keeps no linked source; a camera RAW file refuses."
            case "export_layers", "export_pdf", "contact_sheet":
                note = " A Lab Color document exports through its Lab → RGB conversion, as "
                    + "Export As does (a contact sheet cell is its Display P3 view)."
            case "separate_color":
                note = " With a Lab Color document rgb is sRGB, as with a CMYK document."
            case "add_luminosity_masks":
                note = " In a Lab Color document the nine masks read Lightness (the L* "
                    + "codes)."
            case "detect_horizon", "select_magnetic":
                note = " In a Lab Color document it reads the document's Display P3 view "
                    + "(what the canvas shows)."
            case "distort_layer", "modify_scalar", "invert_channel":
                note = " In a Lab Color document it runs on the L*a*b* codes, as on an RGB "
                    + "document."
            default:
                note = nil
            }
            if let note { catalog[index]["description"] = existing + note }
        }
    }

    /// `lightness`, `a` and `b` appended to the `plane` enum of `load_selection` and
    /// `add_channel`, and `lab8` and `lab16` to `convert_precision`'s `mode` enum, each
    /// only when absent — `annotateGraySchema`'s read-modify-write shape.
    private func annotateLabSchema(_ entry: inout [String: Any], name: String) {
        let (key, values, note): (String, [String], String)
        switch name {
        case "load_selection", "add_channel":
            (key, values, note) = ("plane", ["lightness", "a", "b"],
                                   "A Lab Color document's planes are lightness, a and b (its L*, a* "
                                    + "and b* codes), luma (its Lightness) and alpha.")
        case "convert_precision":
            (key, values, note) = ("mode", ["lab8", "lab16"],
                                   "lab8 and lab16 apply to a Lab Color document, and only there.")
        default:
            return
        }
        guard var schema = entry["inputSchema"] as? [String: Any],
              var properties = schema["properties"] as? [String: Any],
              var property = properties[key] as? [String: Any] else { return }
        var options = (property["enum"] as? [String]) ?? []
        for value in values where !options.contains(value) { options.append(value) }
        property["enum"] = options
        let description = (property["description"] as? String) ?? ""
        property["description"] = description.isEmpty ? note : description + " " + note
        properties[key] = property
        schema["properties"] = properties
        entry["inputSchema"] = schema
    }
}
