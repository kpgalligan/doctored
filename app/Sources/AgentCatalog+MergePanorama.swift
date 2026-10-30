import Foundation

extension AgentServer {
    /// `merge_panorama` (AgentServer+MergePanorama.swift).
    @inline(never) func mergePanoramaCatalog() -> [[String: Any]] {
        let merge = [
            "Merge overlapping photos into ONE new panorama document — File ▸ Automate ▸ ",
            "Merge Panorama…. It runs, as one cancellable job with progress: load_files_into_stack ",
            "(each file one layer, named after the file, bottom first in the order of paths; ",
            "the whole stack is planned and admitted before any file is decoded), ",
            "auto_align_layers over every layer with the canvas grown to hold them, ",
            "auto_blend_layers' panorama method with seamless on and, with fill, its ",
            "Content-Aware Fill Transparent Areas. The reference is the MIDDLE file (index ",
            "(count - 1) / 2); every other file is registered to it, so a ",
            "file that does not overlap the middle one cannot be registered: it is kept, ",
            "hidden and named in align.unregistered and the note, and blended into nothing. ",
            "Fewer than two registered files refuses. layout (default \"auto\"): ",
            "auto (behaves as perspective), perspective, cylindrical, spherical, ",
            "collage or reposition, auto_align_layers' projections; the sampler is bicubic. ",
            "blend (default true) is Blend Images Together: each frame gets a binary layer ",
            "mask on its side of a seam through each overlap, and the frames' tones are ",
            "matched and blended across the seams into their own pixels; false leaves the ",
            "aligned layers as they are. vignette (default false) is Vignette Removal and ",
            "distortion (default false) Geometric Distortion Correction, auto_align_layers' ",
            "vignette and distortion; collage and reposition take neither, and either one ",
            "with them is refused. fill (default false) is Content-Aware Fill Transparent ",
            "Areas: the visible picture is stamped as ONE new layer named \"<topmost ",
            "registered file's name> (merged)\" directly above that frame, and its ",
            "transparent area is filled from the picture around it; fill without blend is ",
            "refused. name (optional) titles the new document, else \"Untitled_Panorama<n>\". ",
            "The new document's history is its initial state; the files are not changed. ",
            "Every refusal opens nothing and starts \"Merge Panorama stopped at <stage>: \" (Load ",
            "Files into Stack, Auto-Align, Blend Images Together or Content-Aware Fill ",
            "Transparent Areas) followed by that stage's own reason, except the argument ",
            "refusals above and fewer than 2 paths. A CMYK stack is refused at Auto-Align. Not ",
            "available inside run_batch. Recordable as an Action step: a replay makes a new ",
            "panorama from the same files. Reply: {ok, document (the new one), depth, profile, ",
            "layout, canvas: {width, height}, layers: load_files_into_stack's rows ",
            "(index, name, path, width, height, source_depth, converted_depth, ",
            "converted_profile, disclosures?), align: {reference, aligned: [{layer, name, ",
            "method, homography, quad, rotation?}], unchanged: [{layer, name}], unregistered: ",
            "[{layer, name, reason}], registration_plan_bytes, focal?, surface?, vignette?, ",
            "distortion?, canvas?: {width, height, origin: {x, y}}} (auto_align_layers' ",
            "shapes), blend? (with blend: {layers: [{layer, name, share, gain?}]}), fill? (with ",
            "fill: {layer: the merged layer's index or null when nothing was transparent, ",
            "covered_pixels}), working_memory_plan_bytes, note? (the hidden files, then what ",
            "the load converted)}.",
        ].joined()
        return [
            tool("merge_panorama", merge,
                 [
                    "paths": [
                        "type": "array", "items": ["type": "string"],
                        "minItems": MergePanorama.minimumFiles,
                        "description": "Absolute file paths, bottom layer first.",
                    ] as [String: Any],
                    "layout": [
                        "type": "string",
                        "enum": MergePanorama.layouts.map(\.projectionName),
                        "default": AutoAlign.Registration.auto.projectionName,
                        "description": "Merge Panorama's Layout.",
                    ] as [String: Any],
                    "blend": [
                        "type": "boolean", "default": true,
                        "description": "Blend Images Together.",
                    ] as [String: Any],
                    "vignette": [
                        "type": "boolean", "default": false,
                        "description": "Vignette Removal (not with collage or reposition).",
                    ] as [String: Any],
                    "distortion": [
                        "type": "boolean", "default": false,
                        "description": "Geometric Distortion Correction (not with collage or "
                            + "reposition).",
                    ] as [String: Any],
                    "fill": [
                        "type": "boolean", "default": false,
                        "description": "Content-Aware Fill Transparent Areas (needs blend).",
                    ] as [String: Any],
                    "name": ["type": "string", "description": "The new document's title."],
                 ], required: ["paths"]),
        ]
    }
}
