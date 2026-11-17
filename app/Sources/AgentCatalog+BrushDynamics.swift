import Foundation

/// The catalog's brush dynamics arguments (`AgentServer+BrushDynamics.swift`): `dynamics`,
/// `tilts` and `rotations` on `brush_stroke`, `eraser_stroke` and `quick_mask_stroke`, and
/// `dynamics` on `save_brush_preset`, in their own file behind an explicit non-inlining
/// boundary for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    /// One response's schema (`size` or `opacity`).
    private static func dynamicsResponseSchema(_ what: String) -> [String: Any] {
        ["type": "object", "description": "The \(what) response: factor 1 when control is off "
            + "or the stroke has no sample for it, else minimum + (1 − minimum) · curve(v), v the "
            + "pressure or min(1, hypot(tilt)); jitter shrinks each dab's \(what) by up to that "
            + "fraction at random.",
         "properties": [
            "control": ["type": "string", "enum": ["off", "pressure", "tilt"],
                        "description": "Omit to follow pressure_\(what == "size" ? "size" : "opacity") (pressure when true); passing both refuses."],
            "curve": ["type": "array", "minItems": 2, "maxItems": BrushDynamics.maximumCurvePoints,
                      "items": ["type": "array", "items": ["type": "number", "minimum": 0, "maximum": 1],
                                "minItems": 2, "maxItems": 2],
                      "description": "Natural cubic through [x, y] points, x strictly increasing from 0 to 1; omit for the identity."],
            "minimum": ["type": "number", "minimum": 0, "maximum": 1, "default": 0],
            "jitter": ["type": "number", "minimum": 0, "maximum": 1, "default": 0],
         ] as [String: Any]]
    }

    /// The `dynamics` object: the Dynamics… sheet's values.
    static func dynamicsSchema(seed: Bool) -> [String: Any] {
        var properties: [String: Any] = [
            "size": dynamicsResponseSchema("size"),
            "opacity": dynamicsResponseSchema("opacity"),
            "scatter": ["type": "object", "description": "Each stamp lays count dabs, each offset "
                + "by amount · its nominal diameter · a seeded value in −1…1 across the direction "
                + "of travel (and along it with both_axes; in a seeded direction for a stroke's "
                + "first stamp).",
                "properties": [
                    "amount": ["type": "number", "minimum": 0, "maximum": BrushDynamics.maximumScatter, "default": 0],
                    "both_axes": ["type": "boolean", "default": false],
                    "count": ["type": "integer", "minimum": 1, "maximum": BrushDynamics.maximumCount, "default": 1],
                    "count_jitter": ["type": "number", "minimum": 0, "maximum": 1, "default": 0],
                ] as [String: Any]] as [String: Any],
            "angle": ["type": "object", "description": "The tip shape's angle (the Brush and "
                + "the Eraser): each dab's angle is the tip's angle plus the pen's rotation "
                + "(rotations), the tilt's direction or the direction of travel, plus up to "
                + "±180° × jitter at random, counter-clockwise. Refused with aliased.",
                "properties": [
                    "control": ["type": "string", "enum": ["off", "rotation", "tilt", "direction"],
                                "default": "off"],
                    "jitter": ["type": "number", "minimum": 0, "maximum": 1, "default": 0],
                ] as [String: Any]] as [String: Any],
            "roundness": ["type": "object", "description": "The tip shape's roundness (the "
                + "Brush and the Eraser): each dab's roundness is the tip's times minimum + "
                + "(1 − minimum) · v (v the pressure or min(1, hypot(tilt)); 1 when off or "
                + "unsampled), shrunk by up to jitter at random, floored at 1 %. Refused with "
                + "aliased.",
                "properties": [
                    "control": ["type": "string", "enum": ["off", "pressure", "tilt"], "default": "off"],
                    "minimum": ["type": "number", "minimum": 0, "maximum": 1, "default": 0],
                    "jitter": ["type": "number", "minimum": 0, "maximum": 1, "default": 0],
                ] as [String: Any]] as [String: Any],
            "texture": ["type": "object", "description": "The texture (the Brush and the Eraser): "
                + "each dab's coverage at a canvas pixel is multiplied by 1 − depth · (1 − t) "
                + "before the stroke accumulates it, t the Gray luminosity (0.30, 0.59, 0.11) of "
                + "the pattern's straight colour times its alpha at that pixel, the tile anchored "
                + "at the canvas origin at scale (the Pattern Overlay's sampler at angle 0); a "
                + "white opaque texel or depth 0 leaves the dab as it was. Only the tile's RGBA8 "
                + "raster is read, at every depth and mode. The recorded native_stroke holds the "
                + "tile itself, so a replay never reads the library. Refused with aliased.",
                "properties": [
                    "pattern": ["type": "object", "description": "{\"preset\": id} (list_patterns' "
                        + "built-ins), {\"name\": …} alone (a library pattern, embedded as its "
                        + "tile) or a custom tile {name?, width, height, rgba} (sides 1–"
                        + "\(LibraryPattern.maximumSide), rgba standard base64 of straight RGBA8)."],
                    "scale": ["type": "number", "minimum": BrushDynamics.Texture.scaleRange.lowerBound,
                              "maximum": BrushDynamics.Texture.scaleRange.upperBound, "default": 1],
                    "depth": ["type": "number", "minimum": 0, "maximum": 1, "default": 1],
                ] as [String: Any], "required": ["pattern"]] as [String: Any],
            "dual": ["type": "object", "description": "The dual brush (the Brush and the Eraser): "
                + "a second tip walks the stroke's path (the polyline through its spacing stamps' "
                + "centres) with its own absolute size, spacing in percent of that size and "
                + "scatter, every second dab at full deposit, and the stroke paints the first "
                + "tip's accumulated coverage times the second's, rounded once: only where both "
                + "reach. An airbrush stroke's held ticks deposit the first tip only; they never "
                + "feed the second walk. The second stream is seeded apart from the first's by "
                + "the same seed. The recorded native_stroke holds the second dabs. Refused "
                + "with aliased.",
                "properties": [
                    "size": ["type": "number", "minimum": BrushDynamics.Dual.sizeRange.lowerBound,
                             "maximum": BrushDynamics.Dual.sizeRange.upperBound,
                             "description": "The second tip's diameter in pixels."],
                    "hardness": ["type": "number", "minimum": 0, "maximum": 1, "default": 1],
                    "roundness": ["type": "number", "minimum": 0.01, "maximum": 1, "default": 1],
                    "angle_degrees": ["type": "number", "minimum": -180, "maximum": 180, "default": 0],
                    "spacing": ["type": "number", "minimum": BrushDynamics.Dual.spacingRange.lowerBound,
                                "maximum": BrushDynamics.Dual.spacingRange.upperBound,
                                "default": 25, "description": "Percent of the second tip's size."],
                    "scatter": ["type": "object", "description": "As dynamics.scatter, in "
                        + "second-tip diameters.",
                        "properties": [
                            "amount": ["type": "number", "minimum": 0, "maximum": BrushDynamics.maximumScatter, "default": 0],
                            "both_axes": ["type": "boolean", "default": false],
                            "count": ["type": "integer", "minimum": 1, "maximum": BrushDynamics.maximumCount, "default": 1],
                            "count_jitter": ["type": "number", "minimum": 0, "maximum": 1, "default": 0],
                        ] as [String: Any]] as [String: Any],
                ] as [String: Any], "required": ["size"]] as [String: Any],
        ]
        if seed {
            properties["seed"] = ["type": "integer", "minimum": 0, "maximum": 4_294_967_295,
                "default": 0, "description": "The jitter and scatter seed: the same call paints "
                    + "the same pixels. The recorded step holds the resolved dabs, not the seed."]
        } else {
            // A preset's colour holds the amounts; a stroke's background is the call's.
            properties["color"] = colourSchema(background: false)
        }
        return ["type": "object", "properties": properties]
    }

    /// The `dynamics.color` object (the Brush's Colour rows); `background` on `brush_stroke`.
    static func colourSchema(background: Bool) -> [String: Any] {
        let amount: [String: Any] = ["type": "number", "minimum": 0, "maximum": 1, "default": 0]
        var properties: [String: Any] = [
            "mix": amount.merging(["description": "Foreground/Background Mix: each dab moves "
                + "from the colour toward the background by up to mix (a seeded fraction)."]) { $1 },
            "hue": amount.merging(["description": "Hue Jitter: up to ±180° × hue on the dab's "
                + "HSL hue. RGB documents only."]) { $1 },
            "saturation": amount.merging(["description": "Saturation Jitter: lowers the dab's "
                + "HSL saturation by up to that fraction. RGB documents only."]) { $1 },
            "brightness": amount.merging(["description": "Brightness Jitter: lowers the dab's "
                + "HSL lightness (on Lab, its L*) by up to that fraction."]) { $1 },
        ]
        if background {
            properties["background"] = ["description": "The background colour the mix moves "
                + "toward, in color's forms (a hex string, or [r, g, b, a] as rgba takes it), "
                + "read under color_space; required when mix is above 0."]
        }
        return ["type": "object", "properties": properties,
                "description": "Colour dynamics (the Brush only): each dab's ink is a seeded mix "
                    + "from the stroke's colour toward background, then hue, saturation and "
                    + "brightness jitter, carried per pixel beside the coverage. A layer in Normal "
                    + "mode of an RGB, Grayscale, Duotone or Lab document; Grayscale, Duotone and "
                    + "Lab take mix and brightness alone (Lab's brightness scales L*), and hue or "
                    + "saturation there refuses. Refused by name with aliased, at size 1, on a "
                    + "mask, plane, channel or Quick Mask, with another blend_mode, rgba32 or ink, "
                    + "and on a CMYK, Indexed or Bitmap document. The recorded native_stroke holds "
                    + "each dab's ink (inks), never the seed or the colours."]
    }

    /// Adds `dynamics`, `tilts` and `rotations` to the three dynamics stroke tools.
    @inline(never) func annotateBrushDynamicsCatalog(_ name: String, properties: inout [String: Any]) {
        guard Self.dynamicsStrokeTools.contains(name) else { return }
        var dynamics = Self.dynamicsSchema(seed: true)
        if name == "brush_stroke", var fields = dynamics["properties"] as? [String: Any] {
            fields["color"] = Self.colourSchema(background: true)
            dynamics["properties"] = fields
        }
        dynamics["description"] = "Brush dynamics (mirrors the Brush's, Pencil's and Eraser's "
            + "Dynamics… sheet): pressure or tilt curves on size and opacity, size and opacity "
            + "jitter, scatter, the tip shape's angle and roundness, a texture, a dual brush and "
            + "(brush_stroke) colour. "
            + "Generates stroke_version=1 dabs (stroke_version may be omitted; a tip shape records "
            + "its shapes, a texture its tile and a dual brush its second dabs in native_stroke); "
            + "a stamp's controls read off "
            + "without pressures, tilts "
            + "or rotations. Needs size above 1; refused on every other stroke tool, with "
            + "native_stroke, and on stroke_path."
        properties["dynamics"] = dynamics
        properties["tilts"] = ["type": "array",
            "items": ["type": "array", "items": ["type": "number", "minimum": -1, "maximum": 1],
                      "minItems": 2, "maxItems": 2],
            "description": "With dynamics, one pen tilt [x, y] per point (0, 0 upright), "
                + "interpolated per dab as pressures are."]
        properties["rotations"] = ["type": "array",
            "items": ["type": "number", "minimum": -360, "maximum": 360],
            "description": "With dynamics, one pen barrel rotation per point in degrees "
                + "(counter-clockwise), interpolated per dab on the shortest arc; read by "
                + "dynamics.angle's rotation control."]
    }
}
