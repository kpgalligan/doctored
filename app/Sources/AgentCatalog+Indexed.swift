import Foundation

/// The Indexed Color document's catalog notes (#1 Indexed phases 1a, 1b and 1c): `convert_mode`'s
/// `indexed` mode and the Indexed Color sheet's options, `get_document`'s palette size, the
/// kept-palette export and save (`IndexedDocument`'s gate, the core's `indexed_convert`,
/// `indexed_document` and `indexed_export`), and phase 1c's `get_color_table` and
/// `set_color_table` (`AgentServer+IndexedColorTable.swift`), phase 2b's `brush_stroke` note, and
/// phase 2c's paint, clipboard and geometry notes (the exact and resampled split, the no-transparent
/// refusal, Normal-only painting) and `rotate`'s reply field, and phase 2d's transform, warp, paste,
/// layer-row and Export Layers notes and `transform_layers`' and `paste`'s reply field, and phase
/// 3a's: the background rule in place of the no-transparent refusal, the Move tool, the row's
/// visibility, offset and order, Reveal All, the profile commands and `convert_profile`'s reply
/// field, and the for-good refusals (a mask, the layers paste, Puppet Warp, the one-layer tools),
/// and phase 3b's: every blend mode, the Select menu's edits, the background colour (white over
/// MCP, `crop`'s `background_color` where it pads) and HDR Toning, Auto-Align and Auto-Blend for
/// good, and phase 3c's: the Pattern and Noise dithers (3d's verdicts), Forced Custom and the four
/// unpublished palettes (refused for good since 3d),
/// `render`'s channels, the channel, selection, Clone Stamp and History Brush notes, and the
/// for-good refusals it adds.
/// Behind an explicit non-inlining boundary, for `AgentCatalog.swift`'s reason.
extension AgentServer {
    @inline(never) func indexedColorTableCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        let entry: [String: Any] = ["type": "array", "items": ["type": "integer", "minimum": 0, "maximum": 255],
                                    "minItems": 3, "maxItems": 3]
        return [
            tool(
                "get_color_table",
                "Reads an Indexed Color document's colour table, as Image ▸ Mode ▸ Color Table… shows "
                    + "it (#1 Indexed phase 1c): colors [[r, g, b], …] (8-bit codes in the "
                    + "document's RGB profile, entry i is palette index i), count and "
                    + "transparent_index (null for none). With path it also writes the table as an "
                    + "Adobe Color Table (.act, 772 bytes: 256 triples padded with zeros, then the "
                    + "big-endian count and transparent index, 0xFFFF for none), as the dialog's "
                    + "Save…. Any other document refuses.",
                [
                    "document_id": docID,
                    "path": ["type": "string", "description": "Where to write the table as an .act file."],
                ]),
            tool(
                "set_color_table",
                "Replaces an Indexed Color document's colour table, as Image ▸ Mode ▸ Color Table…'s "
                    + "OK (#1 Indexed phase 1c), one undo step: every pixel keeps its index and takes "
                    + "its entry's new colour. Pass exactly one of colors ([[r, g, b], …], 2–256 "
                    + "entries, with transparent_index or none) and path (an Adobe Color Table .act "
                    + "of 768 bytes, 256 entries and no transparent index, or 772 bytes with its own "
                    + "count and transparent index, as the dialog's Load…). A table shorter than the "
                    + "largest index the image uses plus one refuses, naming both; the same table "
                    + "answers changed false. Any other document refuses.",
                [
                    "document_id": docID,
                    "colors": ["type": "array", "items": entry, "minItems": 2, "maxItems": 256,
                               "description": "The new table, entry i for palette index i."],
                    "transparent_index": ["type": "integer", "minimum": 0, "maximum": 255,
                                          "description": "With colors: the transparent entry; absent "
                                            + "for none."],
                    "path": ["type": "string", "description": "An .act file to load instead of colors."],
                ]),
        ]
    }

    @inline(never) func annotateIndexedCatalog(_ catalog: inout [[String: Any]]) {
        let properties: [String: Any] = [
            "palette": ["type": "string",
                        "enum": ["exact", "web", "uniform", "adaptive", "system_mac", "system_windows",
                                 "custom", "previous", "local_perceptual", "local_selective",
                                 "master_perceptual", "master_selective"],
                        "description": "Toward Indexed Color: the palette (Uniform by default); "
                            + "local_perceptual, local_selective, master_perceptual and "
                            + "master_selective refuse for good (their algorithms are "
                            + "unpublished): use adaptive."],
            "table": ["type": "array",
                      "items": ["type": "array", "items": ["type": "integer", "minimum": 0, "maximum": 255],
                                "minItems": 3, "maxItems": 3],
                      "minItems": 2, "maxItems": 256,
                      "description": "Toward Indexed Color with palette custom (required there, refused "
                        + "elsewhere): the Custom palette's [[r, g, b], …] entries."],
            "colors": ["type": "integer", "minimum": 2, "maximum": 256,
                       "description": "Toward Indexed Color: the colour budget (256 by default); Uniform's "
                        + "cube size, Adaptive's entry count and Exact's limit."],
            "forced": ["type": "string", "enum": ["none", "black_white", "primaries", "web", "custom"],
                       "description": "Toward Indexed Color: forced colours, Uniform and Adaptive only; "
                        + "custom takes forced_table."],
            "forced_table": ["type": "array",
                             "items": ["type": "array",
                                       "items": ["type": "integer", "minimum": 0, "maximum": 255],
                                       "minItems": 3, "maxItems": 3],
                             "minItems": 2, "maxItems": 256,
                             "description": "Toward Indexed Color with forced custom (required there, "
                                + "refused elsewhere): the forced [[r, g, b], …] colours, leading the "
                                + "palette in order, a repeat dropped."],
            "transparency": ["type": "boolean",
                             "description": "Toward Indexed Color: keep transparent pixels as one "
                                + "transparent index (true by default)."],
            "matte": ["type": "string", "enum": ["none"] + IndexedDocument.mattes.keys.sorted(),
                      "description": "Toward Indexed Color: the matte partly transparent pixels are "
                        + "composited over (none by default)."],
            "matte_rgb": ["type": "array", "items": ["type": "integer", "minimum": 0, "maximum": 255],
                          "description": "Toward Indexed Color: the matte as 8-bit [r, g, b] codes "
                            + "(the sheet's Foreground or Background); not with matte."],
            "dither": ["type": "string", "enum": ["none", "diffusion", "pattern", "noise"],
                       "description": "Toward Indexed Color: Floyd–Steinberg diffusion (the default), "
                        + "none, or the ordered pattern or noise (declared; Photoshop's take: "
                        + "pattern 99.2 % of its Web ramp, 62.9 % of its System (Mac OS) one, noise "
                        + "not measurable; with palette exact they change nothing)."],
            "amount": ["type": "integer", "minimum": 1, "maximum": 100,
                       "description": "Toward Indexed Color: the diffusion amount in percent (75); "
                        + "diffusion only."],
            "preserve_exact": ["type": "boolean",
                               "description": "Toward Indexed Color: Preserve Exact Colors; diffusion "
                                + "only."],
        ]
        let note = " mode \"indexed\" (#1 Indexed phase 1a) converts an 8-bit RGB or 8-bit "
            + "Grayscale document to Indexed Color, as Image ▸ Mode ▸ Indexed Color…: one layer of "
            + "at most 256 palette colours, its palette, transparent index and every pixel's index "
            + "kept. palette exact, web, uniform (the default), adaptive (#1 Indexed phase 1b: "
            + "median cut after Heckbert 1982 over the opaque colours), system_mac, "
            + "system_windows, custom (#1 Indexed phase 1c: table "
            + "[[r, g, b], …] of 2–256 entries, each pixel its nearest entry; colors refuses with it; "
            + "a transparent pixel appends the transparent entry, so 256 entries then refuse) or "
            + "previous (custom with the last Indexed Color conversion's palette minus its "
            + "transparent entry; refused before any conversion); colors 2–256; forced (Uniform and "
            + "Adaptive only; Adaptive refuses when no colour is left after the forced and "
            + "transparent ones); transparency (true: an image with a "
            + "transparent pixel gains one transparent entry, the matte else white; System "
            + "palettes make entry 255 transparent); matte or matte_rgb (with no matte alpha "
            + "below 128 is transparent; with one, alpha 0 is and the rest is composited over it); "
            + "dither diffusion (Floyd–Steinberg, amount, preserve_exact), none, or (#1 Indexed "
            + "phase 3c) pattern (the 8×8 Bayer matrix) or noise (a fixed per-pixel hash), each "
            + "pixel moved by (t − ½)·51 codes before its nearest entry, declared candidates kept "
            + "by phase 3d's Photoshop take (pattern matches 99.2 % of Photoshop's Web ramp and "
            + "62.9 % of its System (Mac OS) one; Photoshop refused noise through the descriptor; "
            + "with palette exact they change nothing); forced custom with forced_table (phase 3c) "
            + "leads the palette with those colours; local_perceptual, local_selective, "
            + "master_perceptual and master_selective refuse for good (their algorithms are "
            + "unpublished): use adaptive. A document of "
            + "more than one plain layer must be flattened (flatten true, else the refusal says "
            + "so). 16-bit, 32-bit, CMYK and Lab documents refuse by name. On an Indexed document "
            + "mode \"rgb\" converts at once, exact, and mode \"gray\" converts each palette "
            + "colour; mode \"cmyk\" and \"lab\" (#1 Indexed phase 1c) convert directly, the "
            + "palette expanded exactly and converted through the RGB → CMYK and RGB → Lab routes "
            + "at 8 bits. The reply's indexed holds "
            + "colors and transparent_index."
        let exportNote = " An Indexed Color document (#1 Indexed phase 1b) with profile "
            + "document and format png, gif or tiff keeps Indexed: the palette, the transparent index "
            + "and every index are written as they are (png colour type 3 with PLTE and tRNS; gif89a "
            + "with the colour table padded black to a power of two, no profile or metadata; tiff "
            + "Photometric 3 with the colour map padded black to 256 and a transparent index kept "
            + "only where a pixel uses it), at bit_depth 8 with no size or sharpen (each refuses by "
            + "name). Every other format or profile exports the palette's colours as RGB."
        let paintNote = " On an Indexed Color document (#1 Indexed phase 2c) the edit runs on the "
            + "RGB storage, then each changed pixel goes back onto the palette by the Brush's rule: "
            + "paint takes the entry nearest the colour (an edge pixel when its coverage is at least "
            + "131/255, else it keeps its index), and an erased pixel keeps its index until its alpha "
            + "falls below 128, when it takes the transparent index, or on a document with no "
            + "transparent colour (#1 Indexed phase 3a) the entry nearest white, the background "
            + "colour (in the app, the entry nearest the "
            + "current background colour, phase 3b). Since phase 3b painting takes every blend_mode: "
            + "a changed pixel is compared with the mode's full-strength colour over its old "
            + "colour and takes the entry nearest that colour, by the same 131/255 rule; "
            + "sample_all_layers is off."
        let geometryNote = " On an Indexed Color document (#1 Indexed phase 2c) a move or a "
            + "nearest sampler (crop, canvas size, the quarter turns and flips, nearest resize, "
            + "rotation and perspective crop) keeps every pixel's index exactly; any other sampler "
            + "resamples the colours and maps each pixel to its nearest palette entry (alpha below "
            + "128 as new area). New canvas area is the transparent index, or on a document with no "
            + "transparent colour (phase 3a) the entry nearest white, the background colour (in the "
            + "app, the current background colour, phase 3b); a crop past the canvas takes the entry "
            + "nearest its background_color."
        let transformNote = " On an Indexed Color document (#1 Indexed phase 2d) the one layer "
            + "transforms in place: a whole-pixel translation, or a nearest sampler (and, for "
            + "selected pixels, a selection with no partial coverage), keeps every pixel's index "
            + "exactly, a floated transparent pixel landing nothing; any other edit resamples the "
            + "colours and maps each changed pixel to its nearest palette entry (alpha below 128 as "
            + "uncovered area), unchanged pixels keeping theirs. Uncovered area is the transparent "
            + "index, or on a document with no transparent colour (phase 3a) the entry nearest "
            + "white, the background colour. A warp is applied at once and keeps no editable field "
            + "(get_warp answers null; clear and bake change nothing). duplicate on "
            + "transform_layers or transform_again refuses (a copy makes a second layer). Since "
            + "phase 3a selected pixels of an alpha channel (target {channel: n}) transform as on "
            + "RGB, the layer untouched; a mask target refuses for good (an Indexed layer holds no "
            + "mask), and a red, green, blue or alpha target refuses (the one "
            + "channel is Index)."
        let pasteNote = " On an Indexed Color document (#1 Indexed phase 2d) the paste lands IN "
            + "the one layer (layer 0; Paste Into and Outside mask it by the selection): the "
            + "payload of any mode is converted to the document's RGB, merged and each changed "
            + "pixel mapped to its nearest palette entry (alpha below 128 to the transparent "
            + "index, or with none the entry nearest white), so a paste from an Indexed document "
            + "of the same palette keeps every index. The reply adds working_memory_plan_bytes, "
            + "the paste's admitted working memory. mode layers refuses for good (phase 3a): "
            + "an Indexed Color document holds one layer, so a paste cannot add one."
        let rowNote = " On an Indexed Color document (#1 Indexed phases 2d and 3a) the one layer "
            + "takes a name, a colour label, locks, visibility (hidden, it renders clear, keeps its "
            + "indices and saves hidden in .doctored; a kept-palette export refuses until it is shown) "
            + "and an offset (a whole-pixel move: the layer stays canvas-sized at the origin, so "
            + "the offset reads 0 after and pixels moved past the canvas are dropped); open "
            + "applies to groups only; opacity and blend mode are not available in Indexed Color."
        let moveNote = " On an Indexed Color document (#1 Indexed phase 3a) a move keeps every "
            + "pixel's index exactly: the one layer stays canvas-sized at the origin, pixels moved "
            + "past the canvas are dropped and the uncovered area is the transparent index, or "
            + "with none the entry nearest white, the background colour."
        let oneLayerNote = " On an Indexed Color document (#1 Indexed phase 3a) this refuses for "
            + "good: it needs more than one layer, and an Indexed Color document holds one."
        let selectNote = " On an Indexed Color document (#1 Indexed phase 3b) a colour selection "
            + "samples the palette colours, each pixel its index's colour (so a duplicate entry's "
            + "pixels select with its twin's); the selection, Quick Mask and alpha channels work "
            + "as on RGB and change no index. A red, green, blue or alpha channel, or the "
            + "luminosity, refuses (the one channel is Index)."
        let exportLayersNote = " On an Indexed Color document (#1 Indexed phase 2d) the one "
            + "layer exports untrimmed with profile document as png, gif or tiff keeping the "
            + "palette and every index; trimmed or any other format exports its RGB colours."
        let channelNote = " On an Indexed Color document (#1 Indexed phase 3c) alpha channels "
            + "work as on RGB and change no index; a red, green, blue or alpha target refuses (the "
            + "one colour channel is Index)."
        let retouchNote = " On an Indexed Color document (#1 Indexed phase 3c) the stroke runs on "
            + "the RGB storage, then each changed pixel takes the palette entry nearest its own new "
            + "colour: a hard, full-opacity clone copies the source's entries, and a full History "
            + "Brush stroke restores the earlier state's indices."
        let forGoodNote = " On an Indexed Color document (#1 Indexed phase 3c) this refuses for "
            + "good."
        let forGood3c: Set<String> = [
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
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            let existing = (catalog[index]["description"] as? String) ?? ""
            switch name {
            case "convert_mode":
                catalog[index]["description"] = existing + note
                guard var schema = catalog[index]["inputSchema"] as? [String: Any],
                      var current = schema["properties"] as? [String: Any] else { continue }
                if var mode = current["mode"] as? [String: Any] {
                    var options = (mode["enum"] as? [String]) ?? []
                    if !options.contains("indexed") { options.append("indexed") }
                    mode["enum"] = options
                    current["mode"] = mode
                }
                current.merge(properties) { existing, _ in existing }
                schema["properties"] = current
                catalog[index]["inputSchema"] = schema
            case "get_document":
                catalog[index]["description"] = existing + " An Indexed Color document (#1) "
                    + "reports color_mode indexed and indexed {colors, transparent_index}."
            case "export_image", "export_preview", "quick_export":
                catalog[index]["description"] = existing + exportNote
            case "save_copy":
                catalog[index]["description"] = existing + " An Indexed Color document (#1 "
                    + "Indexed phase 1b) saves doctored (or rz), png, gif or tiff (tif), keeping its palette, "
                    + "transparent index and every index (bit_depth 8 and dither none by default; "
                    + "flat_save_losses names what the copy drops); every other format refuses: "
                    + "use export_image for an RGB copy or convert_mode rgb."
            case "brush_stroke":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 2b) the stroke paints the RGB storage in Normal mode, then each "
                    + "changed pixel goes back onto the palette: full ink takes the entry nearest the "
                    + "colour, and an edge pixel takes it when its coverage is at least 131/255, else "
                    + "keeps its index (a fixed rule, measured against Photoshop, whose own edge "
                    + "varies from stroke to stroke). Since phase 3b every blend_mode paints: the "
                    + "rule compares each changed pixel with the mode's full-strength colour over "
                    + "its old colour and takes the entry nearest that colour."
            case "fill", "fill_selection", "stroke_selection", "eraser_stroke", "clear_selection",
                 "cut", "copy", "copy_merged":
                catalog[index]["description"] = existing + paintNote
            case "image_size", "canvas_size", "trim", "crop", "perspective_crop", "fit_crop":
                catalog[index]["description"] = existing + geometryNote
            case "rotate", "flip":
                catalog[index]["description"] = existing + geometryNote + " On an Indexed Color "
                    + "document the reply adds working_memory_plan_bytes, the edit's admitted "
                    + "working memory."
            case "transform_layers":
                catalog[index]["description"] = existing + transformNote + " On an Indexed Color "
                    + "document a changed reply adds working_memory_plan_bytes, the edit's admitted "
                    + "working memory."
            case "transform_layer", "distort_layer", "transform_selected_pixels", "transform_again",
                 "warp_layer":
                catalog[index]["description"] = existing + transformNote
            case "paste":
                catalog[index]["description"] = existing + pasteNote
            case "set_layer_properties", "set_layer_lock", "set_layer_label":
                catalog[index]["description"] = existing + rowNote
            case "export_layers":
                catalog[index]["description"] = existing + exportLayersNote
            case "move_layers":
                catalog[index]["description"] = existing + moveNote + " duplicate refuses (a copy "
                    + "makes a second layer)."
            case "move_selected_pixels":
                catalog[index]["description"] = existing + moveNote + " A selection with partial "
                    + "coverage maps each changed pixel to its nearest palette entry; target "
                    + "{channel: n} moves an alpha channel as on RGB; a mask target refuses for "
                    + "good, a red, green, blue or alpha target refuses (the one channel is Index)."
            case "reveal_canvas":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 3a) it always answers no change: the Index layer holds no "
                    + "pixel outside the canvas."
            case "reorder_layer":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 3a) the one layer is already in its place: no change."
            case "assign_profile":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 3a), one undo step: every index and the colour table's codes "
                    + "are kept and the table's colours now mean the new RGB profile; a profile "
                    + "that is not RGB refuses."
            case "convert_profile":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 3a), one undo step: the colour table's entries are converted "
                    + "(each through this conversion, at 8-bit codes) and every index and the "
                    + "transparent index are kept; a "
                    + "profile that is not RGB refuses. A changed reply adds "
                    + "working_memory_plan_bytes, the edit's admitted working memory."
            case "align_layers", "distribute_layers", "delete_layer", "flatten_image",
                 "auto_align_layers", "auto_blend_layers", "link_layers", "unlink_layers",
                 "arrange_layer", "reorder_layers", "merge_layers_to_hdr":
                catalog[index]["description"] = existing + oneLayerNote
            case "puppet_warp":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 3a) this refuses for good."
            case "hdr_toning":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 3b) this refuses for good."
            case "select_color_range", "select_similar", "modify_selection", "transform_selection",
                 "set_quick_mask", "quick_mask_stroke", "save_selection", "load_selection",
                 "select_magic_wand", "quick_select", "select_magnetic":
                catalog[index]["description"] = existing + selectNote
            case "render":
                catalog[index]["description"] = existing + " On an Indexed Color document (#1 "
                    + "Indexed phase 3c) channel composite or index draws the composite (the Index "
                    + "channel through the palette) and an alpha channel's name its plane, with no "
                    + "layer; red, green, blue, alpha, mask or a channel with a layer refuses."
            case "list_channels", "add_channel", "delete_channel", "duplicate_channel",
                 "rename_channel", "set_channel_options", "invert_channel":
                catalog[index]["description"] = existing + channelNote
            case "clone_stamp":
                catalog[index]["description"] = existing + retouchNote + " output new_layer "
                    + "refuses for good (an Indexed Color document holds one layer)."
            case "history_brush_stroke":
                catalog[index]["description"] = existing + retouchNote
            case "export_psd", "inspect_psd_export":
                catalog[index]["description"] = existing + " An Indexed Color document (#1) "
                    + "writes an 8-bit colour-mode-2 PSD or PSB in either mode, in Photoshop's own "
                    + "form: the palette, the transparent index and every index, no layer record "
                    + "(a renamed layer's name is a loss), at bit_depth 8; format tiff refuses "
                    + "(save_copy writes its palette TIFF)."
            case "open_document":
                catalog[index]["description"] = existing + " An Indexed Color PSD or PSB (#1) "
                    + "opens from its stored composite (psd_mode composite) as an Indexed Color "
                    + "document, keeping its palette, transparent index and every index."
            case "inspect_psd":
                catalog[index]["description"] = existing + " An Indexed Color PSD or PSB (#1) "
                    + "answers import_precision indexed, layered_available false and its stored "
                    + "composite's availability."
            case let tool where forGood3c.contains(tool):
                catalog[index]["description"] = existing + forGoodNote
            default:
                break
            }
        }
    }
}
