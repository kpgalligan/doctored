import Foundation

extension AgentServer {
    @inline(never)
    func retouchingCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let spacingProperty = schema.spacingProperty
        let angleProperty = schema.angleProperty
        let roundnessProperty = schema.roundnessProperty
        let ringProperty = schema.ringProperty
        let seedProperty = schema.seedProperty
        let pupilSizeProperty = schema.pupilSizeProperty
        let darkenProperty = schema.darkenProperty
        let sampleProperty = schema.sampleProperty
        let ignoreAdjustmentsProperty = schema.ignoreAdjustmentsProperty
        let retouchOutputProperty = schema.retouchOutputProperty
        return [
            tool(
                "heal_stroke",
                "Heals a stroke from a sampled source — the Healing Brush tool. The pixels "
                    + "under the whole footprint are taken from a region (first point − "
                    + "source) away and Poisson-blended into the layer ONCE, at the end of "
                    + "the stroke, so the healed patch keeps the SOURCE's texture and the "
                    + "DESTINATION's illumination and a stroke has no ridges where its dabs "
                    + "overlapped. Use it for a blemish or a wire that sits on a surface you "
                    + "can sample cleanly nearby; use spot_heal_stroke when there is no such "
                    + "source. Coverage below half is not written at all, so hardness SHAPES "
                    + "the footprint rather than fading the heal — the join is already "
                    + "seamless at the edge — and there is no flow: the stroke always "
                    + "deposits full coverage and opacity is the heal's strength. Replace "
                    + "mode is not offered: in this build it would be exactly clone_stamp. "
                    + "Respects the active selection. A dab or a short stroke is a few "
                    + "milliseconds; a stroke enclosing most of a large photograph is "
                    + "seconds (about 16 at the biggest region allowed, and bigger than "
                    + "that is refused with the limit in the message). The solve runs on the "
                    + "shared cancellable worker: the call waits for it by default, and "
                    + "background=true returns a job_id instead. Errors on an "
                    + "adjustment layer (no "
                    + "pixels to rewrite). This rewrites the layer's pixels, so a re-editable "
                    + "text or shape layer drops its description (undo restores "
                    + "it).",
                [
                    "source_x": [
                        "type": "number",
                        "description": "Canvas x of the point healed FROM — its texture lands "
                            + "under the stroke's first point.",
                    ],
                    "source_y": [
                        "type": "number",
                        "description": "Canvas y of the point healed FROM.",
                    ],
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] along the stroke, in canvas px.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 1, "maxItems": 10_000,
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px "
                            + "(\(Int(EditorTool.heal.minimumBrushSize))-\(Int(EditorTool.heal.maximumBrushSize)), "
                            + "default 24). The ceiling stays 200 px where the paint tools reach "
                            + "5,000: the solve runs over the whole covered footprint and its "
                            + "ring, so its cost grows with the brush's area, not per dab.",
                    ],
                    "hardness": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Edge hardness in percent (default 100). Below 100 it "
                            + "SHRINKS the healed footprint inside the brush circle; it does "
                            + "not fade the heal, which is seamless at the edge either way.",
                    ],
                    "opacity": [
                        "type": "number", "minimum": 1, "maximum": 100,
                        "description": "The heal's strength in percent (1-100, default 100): "
                            + "how far the covered pixels move toward the blended result.",
                    ],
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "Legacy spelling of sample: true is all, false is "
                            + "current (default true). Kept so every recorded call replays "
                            + "unchanged; prefer sample, which also offers "
                            + "current_and_below.",
                    ],
                    "sample": sampleProperty,
                    "ignore_adjustments": ignoreAdjustmentsProperty,
                    "output": retouchOutputProperty,
                    "footprint": ["type": "string", "enum": ["automatic", "dabs"],
                        "description": "Brush footprint (default automatic preserves existing Actions). dabs explicitly stamps the tip at its spacing, including a hard default tip; new canvas clone/heal recordings use it for exact replay."],
                    "expand_destination": ["type": "boolean", "description": "Explicitly expand the destination raster to include the canvas (default false). Canvas size is unchanged; transparency-locked targets refuse expansion."],
                    "spacing": spacingProperty,
                    "angle": angleProperty,
                    "roundness": roundnessProperty,
                    "layer": index,
                    "document_id": docID,
                    "source_transform": ["type": "array", "items": ["type": "number"], "minItems": 6, "maxItems": 6,
                        "description": "Optional [a,b,c,d,tx,ty] affine mapping destination canvas pixel centers to source canvas pixel centers; overrides the translation from source_x/source_y. Fractional offsets use the core's alpha-aware bilinear sampler."],
                    "source_layer": ["type": "integer", "description": "Optional distinct native 16-bit source raster with sample current. Refused in other sampling modes and in 8-bit mode."],
                    "background": ["type": "boolean", "description": "Return a job_id immediately for job_status/cancel_job (default false waits; Actions always wait)."],
                ], required: ["source_x", "source_y", "points"]),
            tool(
                "spot_heal_stroke",
                "Heals a stroke with NO sampled source — the Spot Healing Brush tool. With "
                    + "type content_aware (the default) the footprint is inpainted from a "
                    + "ring of valid pixels around it (PatchMatch); proximity_match copies "
                    + "the whole footprint from the nearby patch whose surroundings best "
                    + "match the stroke's own (it refuses when no clean patch the stroke's "
                    + "size fits beside it); create_texture tiles a texture built from the "
                    + "pixels under the stroke, so a second stroke over the same area adds "
                    + "more texture. Each is then Poisson-blended in exactly as "
                    + "heal_stroke's sampled patch is, so it takes the surrounding "
                    + "illumination. Use it for a blemish with no "
                    + "clean nearby source; heal_stroke is better when you can name one. "
                    + "hardness shapes the footprint rather than fading the heal, there is no "
                    + "flow (the stroke always deposits full coverage), and opacity is the "
                    + "heal's strength. Bounded per CALL, counting every separate part of "
                    + "the footprint together: at most 4,000,000 pixels for the footprint "
                    + "plus its sampling ring while the footprint covers up to 1,000,000 "
                    + "pixels. A footprint over 1,000,000 pixels is not refused for its "
                    + "size: its search runs at half scale (a quarter over 4,000,000) and "
                    + "that bound is four times larger for each halving, so large "
                    + "structure is matched and fine texture is copied from the picture "
                    + "without being matched to the footprint's edge. Beyond a bound it "
                    + "refuses with a message naming the limit, in the words of "
                    + "a stroke (there is no selection and no ring to change — use a "
                    + "smaller brush or a shorter stroke). A brush-sized footprint is well "
                    + "under a second; a long stroke on a large canvas takes several "
                    + "seconds, because cost follows the footprint plus the band it heals "
                    + "from, and the app is busy for that whole time. "
                    + "Respects the active selection. Errors on an adjustment layer. This "
                    + "rewrites the layer's pixels, so a re-editable text or shape "
                    + "layer drops its description (undo restores it).",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] along the stroke, in canvas px.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 1, "maxItems": 10_000,
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px "
                            + "(\(Int(EditorTool.spotHeal.minimumBrushSize))-\(Int(EditorTool.spotHeal.maximumBrushSize)), "
                            + "default 24). The ceiling stays 200 px where the paint tools reach "
                            + "5,000: the inpaint and solve run over the whole covered footprint "
                            + "and its ring, so cost grows with the brush's area, not per dab.",
                    ],
                    "hardness": [
                        "type": "number", "minimum": 0, "maximum": 100,
                        "description": "Edge hardness in percent (default 100). Below 100 it "
                            + "shrinks the healed footprint inside the brush circle rather "
                            + "than fading the heal.",
                    ],
                    "opacity": [
                        "type": "number", "minimum": 1, "maximum": 100,
                        "description": "The heal's strength in percent (1-100, default 100).",
                    ],
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "Legacy spelling of sample: true is all, false is "
                            + "current (default false). Kept so every recorded call replays "
                            + "unchanged; prefer sample, which also offers "
                            + "current_and_below.",
                    ],
                    "sample": sampleProperty,
                    "ignore_adjustments": ignoreAdjustmentsProperty,
                    "output": retouchOutputProperty,
                    "footprint": ["type": "string", "enum": ["automatic", "dabs"],
                        "description": "Brush footprint (default automatic preserves existing Actions). dabs explicitly stamps the tip at its spacing, including a hard default tip; new canvas clone/heal recordings use it for exact replay."],
                    "expand_destination": ["type": "boolean", "description": "Explicitly expand the destination raster to include the canvas (default false). Canvas size is unchanged; transparency-locked targets refuse expansion."],
                    "background": ["type": "boolean", "description": "Return a job_id immediately (default false). Use job_status/cancel_job."],
                    "ring": ringProperty,
                    "seed": seedProperty,
                    "type": [
                        "type": "string",
                        "enum": SpotHealType.allCases.map(\.rawValue),
                        "description": "The Spot Healing Brush's Type (default content_aware): "
                            + "content_aware inpaints from the band around the stroke, "
                            + "proximity_match copies the best-matching neighbouring patch, "
                            + "create_texture tiles a texture built from the pixels under the "
                            + "stroke. At 8, 16 and 32 bits and on CMYK ink.",
                    ],
                    "spacing": spacingProperty,
                    "angle": angleProperty,
                    "roundness": roundnessProperty,
                    "layer": index,
                    "document_id": docID,
                ], required: ["points"]),
            tool(
                "remove_stroke",
                "Removes whatever a stroke is painted over — the Remove tool. The stroke "
                    + "(round, size px wide, hard-edged) becomes a hole that is filled from "
                    + "the picture around it, in one undo step. mode \"model\" (the default) "
                    + "fills it with the local inpainting model (MI-GAN, downloaded in Settings "
                    + "▸ Models): the model runs on a square crop around the stroke at 512 px "
                    + "and its fill is composited back by the stroke's coverage, at 8 and 16 "
                    + "bits; it refuses by name when the model is not installed, and in a "
                    + "32-bit or CMYK document (use mode \"content_aware\" there). mode "
                    + "\"content_aware\" makes the stroke the hole of one Content-Aware Fill "
                    + "(seed applies to it alone), at 8, 16 and 32 bits and in CMYK; a stroke "
                    + "too large to fill refuses with the limit in content_aware_fill's words. "
                    + "Paint over the WHOLE object with a little margin; what the stroke "
                    + "leaves uncovered stays and is sampled from (sample / ignore_adjustments "
                    + "choose the picture both modes sample). A hole with nothing but "
                    + "transparency around it refuses, and so does a hole with no picture "
                    + "around it at all (a model-mode use_selection over Select All). "
                    + "strokes [{points, size?}, …] (1-64) removes several strokes as ONE "
                    + "hole and one undo step, the tool with Remove after each stroke off "
                    + "and its Apply (each size defaults to the call's size). use_selection "
                    + "true takes the hole from the live selection instead (points, strokes "
                    + "and use_selection are exclusive; use_selection refuses without a "
                    + "selection). Use it for an object, a wire or a person; "
                    + "spot_heal_stroke suits a small blemish. A stroke is confined to the "
                    + "active selection. output new_layer puts the result on a new layer "
                    + "(\"Remove\" in model mode, \"Content-Aware Fill\" in content_aware "
                    + "mode) and leaves the target's pixels alone. Runs on the shared "
                    + "cancellable worker: the call waits by default, and background=true "
                    + "returns a job_id for job_status/cancel_job. A model reply adds mode, "
                    + "model, crop [x, y, w, h], planned_bytes and covered_pixels; applied "
                    + "adds strokes (their count) when strokes was sent. Layer RGB "
                    + "only: errors on an adjustment layer or a group, and with a mask, colour "
                    + "plane, channel or Quick Mask targeted. In place it rewrites the layer's "
                    + "pixels, so a re-editable text or shape layer drops its description "
                    + "(undo restores it).",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] along the stroke, in canvas px. One "
                            + "point removes a round dab. Required unless strokes is sent or "
                            + "use_selection is true.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 1, "maxItems": 10_000,
                    ],
                    "strokes": [
                        "type": "array",
                        "description": "Several strokes removed together as one hole and one "
                            + "undo step (the Remove tool's Apply with Remove after each stroke "
                            + "off); refuses with points or use_selection.",
                        "items": [
                            "type": "object",
                            "properties": [
                                "points": [
                                    "type": "array",
                                    "description": "[[x, y], …] along this stroke, in canvas px.",
                                    "items": [
                                        "type": "array", "items": ["type": "number"],
                                        "minItems": 2, "maxItems": 2,
                                    ],
                                    "minItems": 1, "maxItems": 10_000,
                                ],
                                "size": [
                                    "type": "number",
                                    "description": "This stroke's width in px (default the "
                                        + "call's size).",
                                ],
                            ],
                            "required": ["points"],
                        ],
                        "minItems": 1, "maxItems": 64,
                    ],
                    "mode": [
                        "type": "string", "enum": ["model", "content_aware"],
                        "description": "The tool's Mode: model (default) runs the local "
                            + "inpainting model; content_aware runs Content-Aware Fill.",
                    ],
                    "use_selection": [
                        "type": "boolean",
                        "description": "Take the hole from the live selection instead of a "
                            + "stroke (default false); refuses with points or strokes.",
                    ],
                    "size": [
                        "type": "number",
                        "description": "Stroke width in px "
                            + "(\(Int(EditorTool.remove.minimumBrushSize))-\(Int(EditorTool.remove.maximumBrushSize)), "
                            + "default 40).",
                    ],
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "The tool's Sample all layers checkbox: true is "
                            + "sample all, false is sample current (default false). Must "
                            + "agree with sample when both are sent.",
                    ],
                    "sample": sampleProperty,
                    "ignore_adjustments": ignoreAdjustmentsProperty,
                    "output": retouchOutputProperty,
                    "seed": seedProperty,
                    "layer": index,
                    "document_id": docID,
                    "background": ["type": "boolean", "description": "Return a job_id immediately for job_status/cancel_job (default false waits; Actions always wait)."],
                ], required: []),
            tool(
                "patch_region",
                "Moves a region's texture from one part of the picture to another and blends "
                    + "it to the destination's light — the Patch tool. direction \"source\" "
                    + "(the default) HEALS the outlined region using the texture at "
                    + "(offset_x, offset_y) away from it: outline the flaw and point the "
                    + "offset at good pixels. direction \"destination\" is the opposite "
                    + "gesture — the outlined region is the good texture, and the region it "
                    + "would land on after the offset is what gets healed. The blend is the "
                    + "same native-depth Poisson join heal_stroke uses, in one undo step. Give points for "
                    + "an explicit polygon, or omit them to patch the current selection "
                    + "(which may be any shape, a magic-wand or Select Subject mask "
                    + "included). Both depths run on the shared cancellable worker (16-bit "
                    + "Patch captures native samples); background=true returns a job_id for "
                    + "job_status/cancel_job instead of waiting. "
                    + "Oversized regions refuse with the limit in the message. Errors on an adjustment "
                    + "layer. This rewrites the layer's "
                    + "pixels, so a re-editable text or shape layer drops its "
                    + "description (undo restores it).",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] outlining the region, in canvas px "
                            + "(closed automatically, 3 points or more). Omit to use the "
                            + "current selection.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 3, "maxItems": 10_000,
                    ],
                    "offset_x": [
                        "type": "number",
                        "description": "How far the region is dragged, in canvas px (x).",
                    ],
                    "offset_y": [
                        "type": "number",
                        "description": "How far the region is dragged, in canvas px (y).",
                    ],
                    "direction": [
                        "type": "string", "enum": ["source", "destination"],
                        "description": "Which end of the drag is healed (default source: the "
                            + "outlined region is).",
                    ],
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "Legacy spelling of sample: true is all, false is "
                            + "current (default true). Kept so every recorded call replays "
                            + "unchanged; prefer sample, which also offers "
                            + "current_and_below.",
                    ],
                    "sample": sampleProperty,
                    "ignore_adjustments": ignoreAdjustmentsProperty,
                    "layer": index,
                    "document_id": docID,
                    "background": ["type": "boolean", "description": "Return a job_id immediately for job_status/cancel_job (default false waits; Actions always wait)."],
                ], required: ["offset_x", "offset_y"]),
            tool(
                "content_aware_move",
                "Moves an outlined region of the picture somewhere else and fills where it "
                    + "was — the Content-Aware Move tool. mode \"move\" (the default) first "
                    + "fills the WHOLE outlined region content-aware, as content_aware_fill "
                    + "does at the automatic ring, then heals the region's original pixels "
                    + "in at (offset_x, offset_y) away, joined to their new surroundings "
                    + "with the Poisson blend patch_region uses, all in one undo step. "
                    + "Outline the object with a little margin: the margin is what the "
                    + "join blends across. The two places may overlap. mode \"extend\" "
                    + "skips the fill, so the object stays where it was and a blended copy "
                    + "lands at the offset (patch_region with direction destination). Give "
                    + "points for an explicit polygon, or omit them to move the current "
                    + "selection's region. Offsets are rounded to whole pixels. Runs on "
                    + "the shared cancellable worker at 8 and 16 bits and in a 32-bit "
                    + "float document: the call waits by "
                    + "default, and background=true returns a job_id for "
                    + "job_status/cancel_job. A region too large to fill refuses with the "
                    + "limit in the message, in content_aware_fill's words. Layer RGB "
                    + "only: errors on an adjustment layer or a group, and with a mask, "
                    + "colour plane, channel or Quick Mask targeted. This rewrites the "
                    + "layer's pixels, so a re-editable text or shape layer drops its "
                    + "description (undo restores it).",
                [
                    "points": [
                        "type": "array",
                        "description": "[[x, y], …] outlining the region, in canvas px "
                            + "(closed automatically, 3 points or more). Omit to use the "
                            + "current selection.",
                        "items": [
                            "type": "array", "items": ["type": "number"],
                            "minItems": 2, "maxItems": 2,
                        ],
                        "minItems": 3, "maxItems": 10_000,
                    ],
                    "offset_x": [
                        "type": "number", "minimum": -100_000, "maximum": 100_000,
                        "description": "How far the region is moved, in canvas px (x).",
                    ],
                    "offset_y": [
                        "type": "number", "minimum": -100_000, "maximum": 100_000,
                        "description": "How far the region is moved, in canvas px (y).",
                    ],
                    "mode": [
                        "type": "string", "enum": ["move", "extend"],
                        "description": "move (default) fills where the region was; extend "
                            + "leaves it and only adds the blended copy.",
                    ],
                    "sample_all_layers": [
                        "type": "boolean",
                        "description": "Legacy spelling of sample: true is all, false is "
                            + "current (default true). Must agree with sample when both "
                            + "are sent.",
                    ],
                    "sample": sampleProperty,
                    "ignore_adjustments": ignoreAdjustmentsProperty,
                    "seed": seedProperty,
                    "layer": index,
                    "document_id": docID,
                    "background": ["type": "boolean", "description": "Return a job_id immediately for job_status/cancel_job (default false waits; Actions always wait)."],
                ], required: ["offset_x", "offset_y"]),
            tool("job_status", "Progress and terminal outcome of a background edit.",
                 ["job_id": ["type": "string"]], required: ["job_id"]),
            tool("cancel_job", "Cancel worker computation before commit. Poll job_status until cancelled.",
                 ["job_id": ["type": "string"]], required: ["job_id"]),
            tool("resource_usage", "App-wide unique retained buffers including undo, jobs and caches; scratch estimates and unaccounted platform memory. Also reports the working-memory limit (working_limit: effective, source, automatic default, preference).",
                 ["evict_caches": ["type": "boolean", "description": "Evict reusable core caches before measuring (default false)."]]),
            tool("content_aware_fill",
                "Edit > Content-Aware Fill: exact native-depth PatchMatch/Poisson synthesis confined to the selection's soft coverage. source_mask edits source eligibility independently; excluded pixels never supply source texture. Current/current-and-below/all sampling and ignore_adjustments match retouch. output=new_layer preserves original layers and descriptions; partial-alpha backdrops can composite differently. In-place edits rasterize artwork. A selection over 1 MP is filled with its search at reduced scale (1/2 to 4 MP selected, 1/4 to 16 MP, 1/8 to 64 MP): structure larger than about 7 px times that reduction is matched, and finer texture is copied from the picture without being matched to the selection's edge. Limits: selection plus ring 4 MP (16 MP for a selection over 1 MP, 64 MP over 4 MP), one part's box 32 MP, 80 MP of component boxes, 1,000,000 separate parts; a refusal names its limit. background=true returns job_id for job_status/cancel_job. Omitted arguments retain legacy defaults. Or pass only preview_id (and optional document_id) to publish the exact reviewed candidate once, with stale source/target/selection refusal. Layer RGB only; scalar targets and Quick Mask refuse.",
                ["background": ["type": "boolean", "description": "Return job_id immediately; default false waits. Actions wait."],
                 "ring": ringProperty, "seed": seedProperty,
                 "sample": sampleProperty, "ignore_adjustments": ignoreAdjustmentsProperty,
                 "sample_all_layers": ["type": "boolean", "description": "Legacy alias: true=all, false=current. Must agree with sample when both are supplied."],
                 "output": ["type": "string", "enum": RetouchOutput.vocabulary, "description": "layer (default) or new_layer."],
                 "source_mask": contentAwareSourceMaskProperty,
                 "preview_id": ["type": "string", "description": "Publish this exact reviewed result. Only document_id may accompany preview_id."],
                 "layer": index, "document_id": docID]),
            tool("preview_content_aware_fill",
                "Edit > Content-Aware Fill > Prepare Final Result and Before/After. Produces the exact final native candidate without changing the document. Returns Before then After PNG display comparisons and a five-minute preview_id; Apply never reruns synthesis. One app-wide ticket; a new preview replaces the old. background=true returns running state immediately; poll this tool with only preview_id and optional document_id for native progress/stage and ready images. 100% review is available in the UI. Any source, target or selection change invalidates acceptance.",
                ["background": ["type": "boolean"], "ring": ringProperty, "seed": seedProperty,
                 "sample": sampleProperty, "ignore_adjustments": ignoreAdjustmentsProperty,
                 "sample_all_layers": ["type": "boolean"],
                 "output": ["type": "string", "enum": RetouchOutput.vocabulary],
                 "source_mask": contentAwareSourceMaskProperty,
                 "preview_id": ["type": "string", "description": "Inspect an existing pending/ready review; do not send new settings with this ID."],
                 "layer": index, "document_id": docID]),
            tool("discard_content_aware_fill_preview",
                "Edit > Content-Aware Fill > Cancel. Discard a review ticket, cancel active native synthesis and drain its worker mailbox before releasing captured inputs; applies no edit.",
                ["preview_id": ["type": "string"], "document_id": docID], required: ["preview_id"]),
            tool(
                "red_eye",
                "Removes flash red inside a rectangle — the Red Eye tool's drag. Pixels are "
                    + "scored by red DOMINANCE (the ratio of red to the average of green and "
                    + "blue, gated by saturation and hue), never a naive R > G test, which "
                    + "every skin tone passes: a specular catchlight scores zero and comes "
                    + "out bit-identical. Drag a tight rectangle over ONE eye. Nothing "
                    + "happens if the rectangle holds no red, to a red region bigger than "
                    + "pupil_size allows, or to one that reaches all four sides of the "
                    + "rectangle — put the rectangle AROUND an eye, not inside something "
                    + "red. In a 32-bit float document the red test and the correction run on "
                    + "the layer's display encoding, so a value above white keeps its light "
                    + "ratio. This rewrites the layer's "
                    + "pixels, so a re-editable text or shape layer drops its "
                    + "description (undo restores it).",
                [
                    "x": [
                        "type": "number", "minimum": -100_000, "maximum": 100_000,
                        "description": "Rectangle left, canvas px.",
                    ],
                    "y": [
                        "type": "number", "minimum": -100_000, "maximum": 100_000,
                        "description": "Rectangle top, canvas px.",
                    ],
                    "width": [
                        "type": "number", "minimum": 1, "maximum": 100_000,
                        "description": "Rectangle width in px.",
                    ],
                    "height": [
                        "type": "number", "minimum": 1, "maximum": 100_000,
                        "description": "Rectangle height in px.",
                    ],
                    "pupil_size": pupilSizeProperty,
                    "darken": darkenProperty,
                    "layer": index,
                    "document_id": docID,
                ], required: ["x", "y", "width", "height"]),
            tool(
                "red_eye_auto",
                "Removes flash red from every eye in the picture — Filters > Remove Red Eye. "
                    + "Vision's face landmarks locate each face's eyes and the same "
                    + "correction red_eye applies runs on each of them, all in ONE undo step. "
                    + "With no face in the picture it reports that and changes nothing — use "
                    + "red_eye with a rectangle, or pass eyes explicitly; eyes also skips the "
                    + "detector entirely, which is how a drawn or synthetic picture can be "
                    + "corrected. In a 32-bit float document each correction runs on the "
                    + "layer's display encoding, as red_eye's does. This rewrites the layer's "
                    + "pixels, so a re-editable text "
                    + "or shape layer drops its description (undo restores it).",
                [
                    "eyes": [
                        "type": "array",
                        "description": "Optional explicit eyes; supplying them skips Vision "
                            + "and corrects exactly these discs. radius is the IRIS radius in "
                            + "canvas px and each eye is scanned over a square six times it, "
                            + "so the eyes together are limited to 100 megapixels of picture "
                            + "— a wrong-unit radius is refused with that number rather than "
                            + "freezing the app.",
                        "items": [
                            "type": "object",
                            "properties": [
                                "x": [
                                    "type": "number", "minimum": -100_000, "maximum": 100_000,
                                ],
                                "y": [
                                    "type": "number", "minimum": -100_000, "maximum": 100_000,
                                ],
                                "radius": [
                                    "type": "number", "exclusiveMinimum": 0, "maximum": 100_000,
                                ],
                            ] as [String: Any],
                            "required": ["x", "y", "radius"],
                        ] as [String: Any],
                        "minItems": 1, "maxItems": 64,
                    ],
                    "pupil_size": pupilSizeProperty,
                    "darken": darkenProperty,
                    "layer": index,
                    "document_id": docID,
                ]),
        ]
    }
}
