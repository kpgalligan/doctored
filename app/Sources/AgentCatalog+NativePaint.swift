import Foundation

extension AgentServer {
    /// What `brush_stroke`, `eraser_stroke` and `gradient` do in a
    /// 32-bit float document, appended to each tool's note
    /// (`annotatePrecisionCatalog`).
    static let floatBrushNote: String = " In a 32-bit float document the stroke composites in "
        + "linear light with no clamp on colour, in "
        + NativeLayerBlending.floatModeNames.joined(separator: ", ")
        + " only; rgba32 replaces color with linear-light numbers, above white included, and the "
        + "reply's storage is \"rgba32_float\". A mask or alpha channel target is painted as at 16 "
        + "bits (storage \"u16\"); a red, green, blue or alpha target refuses there. background "
        + "returns a job for a float layer stroke as it does at 16 bits."
    static let floatEraserNote: String = " In a 32-bit float document it lowers the layer's alpha "
        + "and leaves every colour sample as it is, above white included (storage "
        + "\"rgba32_float\"); a mask or alpha channel target is erased as at 16 bits, and a red, "
        + "green, blue or alpha target refuses there. background returns a job for a float layer "
        + "stroke as it does at 16 bits."
    static let floatGradientNote: String = " In a 32-bit float document the ramp interpolates in "
        + "linear light, in the same blend modes as brush_stroke there; start_rgba32 with "
        + "end_rgba32 give both ends as linear-light numbers, above white included, and the "
        + "reply's storage is \"rgba32_float\". A mask or alpha channel target takes the ramp as "
        + "at 16 bits (storage \"u16\"); a red, green, blue or alpha target refuses there."
    /// What `fill`, `dodge_burn` and the four smart selections do in a
    /// 32-bit float document, appended to each tool's note
    /// (`annotatePrecisionCatalog`); the core's `display_encoding` states
    /// each rule.
    static let floatFillNote: String = " In a 32-bit float document the bucket's tolerance "
        + "compares the display-encoded copy (the document profile's curve over each value "
        + "clamped to 0–1, never the tone map), so a value above white compares as white, and "
        + "the region is filled in linear light in "
        + NativeLayerBlending.floatModeNames.joined(separator: ", ")
        + " only; rgba32 replaces color with linear-light numbers, above white included, on "
        + "every layer fill (color_match, contiguous false, fill_mode selection and "
        + "sample_all_layers), and the reply's storage is \"rgba32_float\". A mask or alpha "
        + "channel target fills as at 16 bits (storage \"u16\") and refuses rgba32; a red, "
        + "green, blue or alpha target refuses there."
    static let floatDodgeBurnNote: String = " In a 32-bit float document the stroke classifies "
        + "on the display-encoded copy and changes linear light: a component v in 0–1 becomes "
        + "D(shaped(E(v))) (E the profile's curve, D its decode), one above white is scaled by "
        + "what the tool does to white and one below zero is shifted by what it does to black, "
        + "held to ±65504, alpha kept (storage \"rgba32_float\"). With target \"mask\" or "
        + "\"channel:<name>\", at 16 and 32 bits, each u16 sample s becomes 65535·shaped(s/65535) "
        + "lerped by stroke × selection coverage — a mask is coverage, not light (storage "
        + "\"u16\"); an 8-bit document refuses target, and a red, green, blue or alpha target "
        + "refuses at every depth."
    static let floatSelectionNote: String = " In a 32-bit float document the tolerance or colour "
        + "distance compares the display-encoded copy (the document profile's curve over each "
        + "value clamped to 0–1, never the tone map), so a value above white compares as white; "
        + "the output is native 65,536-level coverage."

    /// What the seven retouch tools (`FloatRetouch.tools`) do in a
    /// 32-bit float document, appended to each tool's note
    /// (`annotatePrecisionCatalog`).
    static let floatRetouchNote: String = " In a 32-bit float document it works on a layer's "
        + "linear-light pixels with the same arguments as at 16 bits: clone_stamp copies linear "
        + "light with no clamp (a colour above white stays above white) in "
        + NativeLayerBlending.floatModeNames.joined(separator: ", ")
        + " only; the healing tools (heal_stroke, spot_heal_stroke, patch_region, remove_stroke, "
        + "content_aware_fill, content_aware_move) match and blend in a display-like working "
        + "encoding, write linear light back, take Normal blending only and leave every pixel "
        + "they do not change bit for bit. A mask, channel, colour-plane or Quick Mask target "
        + "refuses there."

    /// Additive arguments on the existing UI-mirroring tools, with legacy
    /// argument meanings retained for previously saved Actions.
    func annotateNativePaintCatalog(_ name: String, properties: inout [String: Any]) {
        if ["brush_stroke", "eraser_stroke", "clone_stamp", "heal_stroke", "spot_heal_stroke", "dodge_burn", "quick_mask_stroke",
            "blur_stroke", "sharpen_stroke", "smudge_stroke", "sponge_stroke", "history_brush_stroke",
            "art_history_brush_stroke"].contains(name) {
            properties["stroke_version"] = ["type": "integer", "enum": [1],
                "description": "Generate canonical resolved dabs from points and optional pressures/times. Byte and 16-bit stroke tools accept this form, including byte mask/plane/channel, pixel and Quick Mask coverage. At 8 bits, omit for the historical point footprint; at 16 bits unversioned points generate the same native dabs (unit pressure, no timing) and refuse by name at the 50,000-dab and 2-billion-work-unit limits; smudge_stroke has its own 256-million-work-unit bound at both depths (it walks the stroke a pixel at a time) and refuses a longer stroke by name. A versioned call refuses a size outside its tool's range by name (1–\(Int(EditorTool.brush.maximumBrushSize)) px; \(Int(EditorTool.heal.minimumBrushSize))–\(Int(EditorTool.heal.maximumBrushSize)) for heal_stroke and spot_heal_stroke; 1–\(Int(EditorTool.smudge.maximumBrushSize)) for blur_stroke, sharpen_stroke, smudge_stroke, sponge_stroke and history_brush_stroke; 1–\(Int(EditorTool.artHistoryBrush.maximumBrushSize)) for art_history_brush_stroke); an unversioned call clamps it. Maximum 50,000 dabs; oversized strokes refuse by name; no partial stroke."]
            properties["pressures"] = ["type": "array", "items": ["type": "number", "minimum": 0, "maximum": 1],
                "description": "With stroke_version=1, one tablet pressure per point; default 1."]
            properties["times"] = ["type": "array", "items": ["type": "number", "minimum": 0],
                "description": "Elapsed seconds per point, starting at 0 and nondecreasing; used for 20 Hz airbrush deposits."]
            properties["duration"] = ["type": "number", "minimum": 0, "maximum": 2499,
                "description": "Total stroke seconds, including any hold at the final point; defaults to the last time."]
            properties["pressure_size"] = ["type": "boolean", "description": "Scale dab size by pressure, with a one-pixel floor."]
            properties["pressure_opacity"] = ["type": "boolean", "description": "Scale each dab deposit by pressure; stroke opacity still applies once."]
            properties["airbrush"] = ["type": "boolean", "description": "Generate 20 Hz held deposits using the declared sample times/duration."]
            properties["native_stroke"] = ["type": "object",
                "description": "Canonical RZBR version1 replay: version=1, encoding=le_f64_xy_diameter_opacity, dabs=base64 little-endian binary64 quadruples, hardness, roundness, angle_degrees, pixel, and optionally aliased (true is the Pencil's tip, which brush_stroke and quick_mask_stroke take; recordings write it only then). Mutually exclusive with generation options; recorded automatically. Byte commits and replay share one exact dab rasterizer; at 8 bits historical unversioned points keep their prior meaning."]
        }
        // The Pencil (the Brush's slot ▸ Pencil): its tip on the Brush's stroke.
        if AgentServer.aliasedStrokeTools.contains(name) {
            properties["aliased"] = ["type": "boolean",
                "description": "The Pencil: a hard tip that paints each pixel fully or not at all (a pixel at least half inside a dab is painted), at every depth and on every target this tool takes. Generates stroke_version=1 dabs (stroke_version may be omitted): dabs snapped to whole-pixel diameters on the pixel grid, 5% spacing. Takes size, opacity and the pressures (and brush_stroke's colour and blend_mode); hardness and flow other than 100, spacing, angle, roundness and airbrush refuse by name, as does aliased on any other stroke tool."]
        }
        if AgentServer.precisionStrokeJobTools.contains(name) {
            properties["background"] = ["type": "boolean", "description": "16-bit or 32-bit float layer strokes: return a job_id immediately for job_status/cancel_job (default false waits; Actions always wait). Ignored at 8 bits; refused for mask, channel, Quick Mask and adjustment-layer strokes."]
        }
        if name == "quick_mask_stroke" {
            properties["points"] = ["type": "array", "items": ["type": "array", "items": ["type": "number"], "minItems": 2, "maxItems": 2], "maxItems": 10000]
            properties["size"] = ["type": "number", "minimum": 1, "maximum": Int(EditorTool.brush.maximumBrushSize)]
            properties["hardness"] = ["type": "number", "minimum": 0, "maximum": 100]
            properties["flow"] = ["type": "number", "minimum": 1, "maximum": 100]
            properties["opacity"] = ["type": "number", "minimum": 0, "maximum": 1]
            properties["erase"] = ["type": "boolean", "description": "Remove Quick Mask coverage instead of adding white. Byte canonical calls require size and use byte RLE; 16-bit calls use native coverage."]
        }
        // Linear ink for a float layer (`AgentServer.linearInkProperty`'s
        // wording; `FloatPaint` owns the refusals).
        if name == "brush_stroke" {
            var property = AgentServer.linearInkProperty
            property["description"] = (property["description"] as? String ?? "")
                + " Layer target only: a mask, a channel or an adjustment layer's mask is 0–1 "
                + "coverage and refuses it."
            properties["rgba32"] = property
        }
        if name == "gradient" {
            for (key, replaced, other) in [("start_rgba32", "start_color", "end_rgba32"),
                                           ("end_rgba32", "end_color", "start_rgba32")] {
                var property = AgentServer.linearInkProperty
                property["description"] = "32-bit float documents only, instead of \(replaced) "
                    + "and always with \(other): [red, green, blue, alpha] as linear light in the "
                    + "document's primaries (sample_color's center_rgba32 / mean_rgba32), red, "
                    + "green and blue 0–65504 (above 1 is above white) and alpha 0–1, used as they "
                    + "are. Takes no color_space, stops or other endpoint; layer target only; an "
                    + "8- or 16-bit document and Quick Mask refuse it."
                properties[key] = property
            }
        }
        if name == "fill" {
            var property = AgentServer.linearInkProperty
            property["description"] = (property["description"] as? String ?? "")
                + " Layer target only, on every layer fill (color_match, contiguous false, "
                + "fill_mode selection, sample_all_layers): a mask or channel is 0–1 coverage and "
                + "refuses it."
            properties["rgba32"] = property
            properties["opacity"] = ["type": "number", "minimum": 0, "maximum": 1,
                "description": "Explicit native fill_mode only: global opacity, separate from color alpha; default 1."]
            properties["fill_mode"] = ["type": "string", "enum": ["color_match", "selection"],
                "description": "16-bit targets: color_match uses contiguous/noncontiguous target similarity and requires a selected seed. RGB ink can expand raw storage. selection fills the selected area, or the RGB canvas/complete raw scalar target when no selection exists. Historical RGB contiguous=false without this field preserves its selection-wide behavior."]
        }
        if name == "dodge_burn" {
            properties["target"] = ["type": "string",
                "description": "What the stroke dodges or burns: \"layer\" (default), \"mask\" "
                    + "(the layer's mask, which must exist) or \"channel:<name>\" (an alpha "
                    + "channel). 16-bit and 32-bit float documents only: on a mask or channel each "
                    + "u16 sample s becomes 65535·shaped(s/65535), lerped by stroke × selection "
                    + "coverage. An 8-bit document refuses a target other than \"layer\", and a "
                    + "red, green, blue or alpha target refuses at every depth."]
        }
        if name == "gradient" {
            for key in ["start_rgba", "end_rgba"] {
                properties[key] = ["type": "array", "items": ["type": "number", "minimum": 0, "maximum": 1],
                    "minItems": 4, "maxItems": 4, "description": "16-bit continuous normalized RGBA endpoint, including alpha."]
            }
            properties["color_space"] = ["type": "string", "enum": ["srgb", "document", "display_p3"],
                "description": "Endpoint colour interpretation. Default srgb: authored sRGB, converted once into the document. display_p3 reads start_color/end_color (or the 16-bit numeric endpoints) as Display P3 at EITHER depth, converted once into the document. document: start_color/end_color ARE the document's own hex bytes (sample_color's hex) at either depth (exact v·257 at 16 bits); numeric 16-bit endpoints are document components. Every value is converted once and quantized to the document's numbers."]
            properties["opacity"] = ["type": "number", "minimum": 0, "maximum": 1,
                "description": "16-bit gradient opacity; scales interpolated endpoint alpha once."]
        }
    }
}
