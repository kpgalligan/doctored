import Foundation

extension AgentServer {
    /// Small explicit catalog amendments preserve the existing tool schemas
    /// while disclosing target/depth restrictions before a command is written.
    @inline(never) func annotatePrecisionCatalog(_ catalog: inout [[String: Any]]) {
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String,
                  var schema = catalog[index]["inputSchema"] as? [String: Any],
                  var properties = schema["properties"] as? [String: Any] else { continue }
            annotateNativePaintCatalog(name, properties: &properties)
            var note: String?
            switch name {
            case "brush_stroke", "fill":
                properties["rgba"] = ["type":"array", "items":["type":"number", "minimum":0, "maximum":1],
                    "minItems":4, "maxItems":4,
                    "description":"16-bit documents only: continuous normalized sRGB RGBA, overriding color. Never a byte-preview sample."]
                properties["color_space"] = ["type":"string", "enum":["srgb","document","display_p3"],
                    "description":"How the colour argument is interpreted, at EITHER depth. Default srgb: an authored sRGB hex (or 16-bit rgba), converted once into the document. document: the hex IS the document's own bytes (sample_color's hex), at 16 bits too, where each hex byte v becomes the exact code v·257 (16-bit numeric rgba is then read as document components). display_p3: the hex (or 16-bit rgba) is Display P3, converted once into the document. Numeric rgba stays 16-bit-only."]
                note = name == "fill"
                    ? "16-bit targets support selected color-match bucket fill. Set fill_mode=color_match explicitly for noncontiguous RGB matching; historical typed contiguous=false without fill_mode retains selection-wide fill."
                        + AgentServer.floatFillNote
                    : "16-bit mode supports every existing paint blend and native scalar targets. stroke_version=1 generates native dabs from points/pressure/timing and records canonical replay; unversioned point calls generate the same native dabs with unit pressure and no timing, under the same two core limits (50,000 dabs, 2 billion work units — about 70 soft 5,000 px dabs on a 100-megapixel canvas); over-cap strokes refuse by name; no partial stroke. On an 8-bit document canonical dabs are accepted for the LAYER target with size > 1 (coverage targets and the size-1 pixel brush refuse by name)."
                    + AgentServer.floatBrushNote
            case "dodge_burn":
                note = String(AgentServer.floatDodgeBurnNote.dropFirst())
            case "select_magic_wand", "select_color_range", "select_similar":
                note = String(AgentServer.floatSelectionNote.dropFirst())
            case "quick_select":
                note = String(AgentServer.floatSelectionNote.dropFirst())
                    + " enhance_edge (Select and Mask's edge stage) refines on the same display codes there."
            case "eraser_stroke":
                note = "On an 8-bit document stroke_version=1 / native_stroke record and replay canonical dabs for the LAYER target with size > 1; a mask, plane or channel target and the size-1 pixel brush refuse by name and keep the historical point footprint."
                    + AgentServer.floatEraserNote
            case "gradient":
                note = "color_space qualifies the hex endpoints and every stop's color at either depth (srgb, document, display_p3); numeric start_rgba/end_rgba stay 16-bit-only."
                    + AgentServer.floatGradientNote
            case "clone_stamp", "heal_stroke":
                properties["source_layer"] = ["type":"integer", "description":"16-bit or 32-bit float mode only: optional authoritative raster source with sample=current; default target layer. Refused with composite sampling."]
                note = "16-bit mode supports captured native current (default), current_and_below or all sampling, ignore_adjustments, fractional source coordinates/source_transform, new-layer output and explicit destination expansion. "
                    + (name == "clone_stamp"
                        ? "blend_mode is honoured at 16 bits as at 8: the cloned pixels composite into the layer through that mode; output new_layer refuses any mode but Normal, because a fresh layer has nothing to blend with. "
                        : "heal_stroke has no blend_mode at either depth: its result is a solved membrane that replaces the covered pixels, so there is no source colour to composite through a mode. ")
                    + "Source RGB never comes from a display preview."
            case "quick_mask_stroke":
                note = "16-bit generation also accepts stroke_version=1 with points/pressure/timing, or canonical native_stroke instead of RLE. Canonical dabs keep the native coverage job bounded and cancellable; opacity and erase describe the final selection deposit."
            case "save_copy":
                properties["bit_depth"] = ["type":"integer", "enum":[8,16,32], "description":"16-bit master flat export: 16 (default) for PNG/TIFF, or explicit 8 for deliberate reduction. 32 is a 32-bit float document's exact copy as tiff, exr or hdr, and the default for exr and hdr, which exist at no other depth. Native retains its mode and refuses this option."]
                properties["dither"] = ["type":"string", "enum":["none","ordered"], "description":"8-bit output RGB dithering; alpha is rounded without dithering. Refused for native output."]
                properties["float_samples"] = ["type":"string", "enum":["f32","f16"], "description":"bit_depth 32 only: f32 (default) or f16, the nearest 16-bit half, for tiff and exr. hdr has one sample form (RGBE) and refuses f16."]
                if var format = properties["format"] as? [String: Any], let names = format["enum"] as? [String] {
                    format["enum"] = names + ["exr", "hdr"]
                    format["description"] = "exr (OpenEXR) and hdr (Radiance) are 32-bit float deliverables: written only from a 32-bit float document, at bit_depth 32."
                    properties["format"] = format
                }
                note = "16-bit master exports keep the master unchanged and report depth/alpha/structure losses. "
                    + "A 32-bit float master's flat copy at 8 or 16 bits is tone-mapped through the document's tone map "
                    + "(the export workspace's float stage) and reports master_bit_depth 32. At bit_depth 32 a float "
                    + "master writes its samples untone-mapped as tiff, OpenEXR (.exr: associated alpha, ZIP, the "
                    + "document profile's primaries as chromaticities) or Radiance (.hdr: RGBE, opaque, values "
                    + "below zero written as zero) — the same bytes export_image writes for those settings — and "
                    + "flat_save_losses names what each container has no place for. save_copy writes no HEIC "
                    + "or AVIF; an HDR photo (hdr_encoding gain_map or pq at bit_depth 32) is export_image's."
            case "apply_filter":
                note = "In Quick Mask, filter blur softens the Quick Mask coverage instead of a layer (sigma 0.65 to 75.5, default 4): with no target while Quick Mask is on, or with target \"quick_mask\", which refuses when it is off. Every other filter refuses in Quick Mask. With target mask, filter invert inverts the mask over the whole canvas, including the area outside its stored rectangle."
            case "add_fill_layer":
                note = "At 16 and 32 bits the fill layer is retained artwork rendered at the document's depth from the same description, at the slot position names; at 32 bits its colors are carried into linear light and a gradient interpolates there. A pattern fill's tile codes are document values: at 32 bits code / 255 is linear light."
            case "add_text_layer", "add_shape_layer":
                note = "In 16-bit mode this creates editable retained artwork at the slot position names, exactly as at 8 bits. Authored colors and generated vector area coverage remain native; intrinsic bitmap glyph assets retain their disclosed byte precision."
            case "sample_color":
                note = "16-bit mode reports schema2 authoritative center_rgba_codes/center_rgba16, exact weighted_sum_rgba_codes/weight_units, mean_rgba_codes/mean_rgba16 and normalized_rgba, with no byte hex approximation, plus srgb_rgba/srgb_in_gamut (continuous sRGB of normalized_rgba; omitted at 32-bit float). Default source is the composite."
            case "sample_pixel":
                note = "16-bit mode reports schema2 authoritative center_rgba_codes/center_rgba16, exact weighted_sum_rgba_codes/weight_units, mean_rgba_codes/mean_rgba16 and normalized_rgba, with no byte hex approximation. Default source is the composite; sample_pixel additionally exposes raw-layer/backdrop scopes and explicit selection weighting."
            case "histogram":
                note = "16-bit mode reads authoritative native codes in every named scope, including raw groups and selection-weighted backdrops. Exact 0/65535 endpoint counts and coverage weights inspect the complete scope independently of display-bin stride."
            case "render":
                note = "Native layer/group and scalar rendering is an explicit byte display derivative. channel=mask shows raw coverage at its canvas placement; effective_mask applies density and feather and ignores the enabled switch for inspection. Neither display becomes editable native source data. region / preview_divisor render the 16-bit or float projection's window at native depth (equal to that crop of the full native render) and then display it like the full canvas; they admit groups (isolated or Pass Through, nested), layer masks, clipping masks, Blend If and layer styles on layers (effects included) and fill-only styles on groups, and layer indices in their refusals are the typed postorder record indices. At 32-bit float a layer, group or colour-plane render (red, green, blue, alpha, luma) goes through the document's tone map like the composite; a plane is read from that display."
            case "get_document": note = "16-bit documents include precision_support and typed record metadata; display previews never replace source samples."
            case "set_layer_style":
                note = "16-bit mode carries the whole style (fill opacity, the nine effects, Blend If) on raster layers, adjustment layers and every group (a styled Pass Through group composites isolated). Effects render from the shape quantized to the byte plane through the byte renderers under the document's global light (set_global_light works at 16 bits); ramps evaluate on 16-bit codes with each 0..255 knee at code 257 * knee. A style whose padded plane would pass 100 MP refuses. The two knockout blending options — knockout and blend_interior_as_group — are CARRIED at this depth since Phase 12E-3b and render exactly as they do at 8 bits; a 32-bit float document still refuses a layer style by name, and both keys are written to a PSD (`knko`/`infx`) at 8 and 16 bits since Phase 12E-3c."
            default: break
            }
            // The seven retouch tools' float sentence, after any note above.
            if FloatRetouch.tools.contains(name) {
                // The note opens with the space that joins it to one before it.
                note = note.map { $0 + AgentServer.floatRetouchNote }
                    ?? String(AgentServer.floatRetouchNote.dropFirst())
            }
            if let note { catalog[index]["description"] = (catalog[index]["description"] as? String ?? "") + " " + note }
            schema["properties"] = properties
            catalog[index]["inputSchema"] = schema
        }
    }
}
