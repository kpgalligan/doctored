import Foundation

extension AgentServer {
    /// `merge_to_hdr` and `merge_layers_to_hdr`
    /// (AgentServer+MergeHDR.swift).
    @inline(never) func mergeHDRCatalog() -> [[String: Any]] {
        let shared = [
            "Each frame is read as linear light and weighted by how well it is exposed there ",
            "(no weight within 2% of black or of clipping, full weight between 10% and 90%) and ",
            "by its exposure, so shadows come from the longer frames and highlights from the ",
            "shorter ones; the result is on the scale of the REFERENCE frame, the middle ",
            "exposure. evs gives each frame's exposure in stops, in the same order (larger = a ",
            "brighter frame; only differences matter; every frame within 15 stops of the ",
            "reference). ",
        ].joined()
        let options = [
            "align (default true) registers every other frame to the reference and warps it ",
            "into place as auto_align_layers does; a frame that cannot be registered is left ",
            "out and reported, a frame already in place is used as it is, and fewer than two ",
            "usable frames is refused (pass align false to merge the frames as they are). ",
            "deghost (default false) gives a pixel where a frame disagrees with the reference ",
            "to the reference alone; it judges only where both are fully weighted and opaque. ",
            "Reply: {ok, document, depth: \"rgba32_float\", profile, canvas: {width, height}, ",
            "frames: [{index, name, path?, ev, ev_source: \"exif\"|\"estimated\"|\"given\", ",
            "alignment?: \"reference\"|\"aligned\"|\"unchanged\"|\"unregistered\" (present when ",
            "aligned is true), used, reason? (why an unused frame was left out)}], reference ",
            "(the reference frame's index), aligned, deghost, all_clipped_pixels (every frame ",
            "clipped: the shortest exposure stands alone), all_dark_pixels (no frame trusted: ",
            "the longest unclipped exposure stands alone), deghosted_pixels, raw_development? ",
            "(present when a frame was a camera RAW), working_memory_plan_bytes (the stack's ",
            "plan plus the merge's, admitted one after the other, so it can exceed the working ",
            "limit on a call that succeeded), note?}. The ",
            "new document is untitled (or takes name), unsaved, with an empty history and one ",
            "layer, \"Merged HDR\"; nothing clips its values: sample_pixel reports them as ",
            "center_rgba32, and save_copy writes them at bit_depth 32 as tiff, exr or hdr, or ",
            "as a .doctored. Not recorded into Actions.",
        ].joined()
        let files = [
            "Merge bracketed exposures of one scene into ONE new 32-bit float document — File ▸ ",
            "Merge to HDR…. paths lists 2 to 16 absolute file paths of any type open_document ",
            "reads; the files are loaded as load_files_into_stack loads them (the first file ",
            "decides the depth, profile and canvas), and a first file that is already 32-bit ",
            "float is refused. A camera RAW is developed at 16 bits with the tone curve and the ",
            "shadow boost off. ",
            shared,
            "Without evs they come from the files' EXIF (exposure time, f-number and ISO) when ",
            "every file has all three, else from an estimate over the pixels; when the estimate ",
            "cannot pair two frames (they share too few well-exposed pixels) the call is ",
            "refused with that reason and opens nothing: pass evs. Two frames within 0.1 EV of ",
            "each other are refused by name unless evs is passed. ",
            options,
        ].joined()
        let layers = [
            "Merge raster layers of an open 8- or 16-bit document, each one exposure of the ",
            "same scene, into ONE new 32-bit float document — Layer ▸ Merge Layers to HDR…. ",
            "layers names 2 to 16 layer indices; each must be a raster layer (not a group, an ",
            "adjustment layer, or a text, shape or fill layer). A layer's mask, opacity, blend ",
            "mode and style are ignored. The source document is not changed: no history step, ",
            "no pixel moves. A 32-bit float document is refused. ",
            shared,
            "Without evs they are estimated from the pixels; when the estimate cannot pair two ",
            "layers the call is refused with that reason and opens nothing: pass evs. Two ",
            "layers within 0.1 EV of each other are refused by name unless evs is passed. ",
            options,
            " The reply adds source_document.",
        ].joined()
        let evs: [String: Any] = [
            "type": "array", "items": ["type": "number"],
            "description": "One exposure value per frame, in stops, in the frames' order.",
        ]
        let align: [String: Any] = ["type": "boolean", "default": true]
        let deghost: [String: Any] = ["type": "boolean", "default": false]
        let name: [String: Any] = ["type": "string", "description": "The new document's title."]
        return [
            tool("merge_to_hdr", files,
                 [
                    "paths": [
                        "type": "array", "items": ["type": "string"],
                        "minItems": MergeHDR.minimumFrames, "maxItems": MergeHDR.maximumFrames,
                        "description": "Absolute file paths, one exposure each.",
                    ] as [String: Any],
                    "evs": evs, "align": align, "deghost": deghost, "name": name,
                 ], required: ["paths"]),
            tool("merge_layers_to_hdr", layers,
                 [
                    "layers": [
                        "type": "array", "items": ["type": "integer"],
                        "minItems": MergeHDR.minimumFrames, "maxItems": MergeHDR.maximumFrames,
                    ] as [String: Any],
                    "evs": evs, "align": align, "deghost": deghost, "name": name,
                    "document_id": Self.docIDProperty,
                 ], required: ["layers"]),
        ]
    }
}
