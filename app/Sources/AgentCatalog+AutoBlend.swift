import Foundation

extension AgentServer {
    /// `auto_blend_layers` (AgentServer+AutoBlend.swift).
    @inline(never) func autoBlendCatalog() -> [[String: Any]] {
        let blend = [
            "Blend raster layers into a panorama or a focus stack — Edit ▸ Auto-Blend Layers…. ",
            "layers names at least 2 layer indices, each a raster layer (not a group, an ",
            "adjustment layer, or a text, shape or fill layer) that has no layer mask and is ",
            "not locked with Lock All. Each layer gets a canvas-sized layer mask: the masks are ",
            "binary (no feather) and together reveal every pixel at least one layer covers ",
            "exactly once, so the stack itself already shows the result, and the masks stay ",
            "editable. A layer never takes a pixel where it is transparent. method (default ",
            "\"stack_images\"): \"panorama\" (the dialog's default) cuts a seam through each ",
            "overlap, the layers taken bottom first, where the layers agree and neither has an ",
            "edge (around a face or a building, through sky); on flat ground the layer below ",
            "keeps the whole overlap and the seam runs along its edge; each mask ",
            "reveals that layer's side. With seamless (default true) Panorama also matches the ",
            "layers' tone and colour (one gain per channel per layer, reported) and writes a ",
            "multi-scale blend across the seams INTO the layers' own pixels: no layer is added, ",
            "each layer keeps its size, position and transparency, and a pixel its mask hides ",
            "holds its tone-matched picture; a layer with Lock Image Pixels is refused. Without ",
            "seamless no pixel changes. \"stack_images\" focus-stacks: each mask reveals the ",
            "pixels where that layer is the sharpest, and seamless also adds ONE merged raster ",
            "layer named \"Auto-Blend\", a multi-scale blend through those masks, directly above ",
            "the top layer in layers and above any layers clipped to that one, not clipped ",
            "itself, transparent where no layer has pixels. The layers' visibility, opacity and ",
            "blend mode are not consulted, and the selection is ignored. align (default false) ",
            "first registers the layers to one another as auto_align_layers does with its ",
            "defaults (the reference is the one position-locked layer, else the lowest index; ",
            "linked layers are refused); with panorama the canvas then grows to hold every ",
            "aligned layer, as auto_align_layers grows it, and the selection moves with the ",
            "canvas. A layer that cannot be registered refuses the whole call by name and ",
            "nothing changes. Pass align true unless the layers already line up. fill (default ",
            "false, panorama only: with stack_images it is refused) is Content-Aware Fill ",
            "Transparent Areas: after the blend, the visible picture is stamped as ONE new ",
            "raster layer named \"<top layer's name> (merged)\" directly above the top layer in ",
            "layers, and its transparent area (where the composite's alpha is below full) is ",
            "filled by Content-Aware Fill from the picture around it, so the layer is opaque; ",
            "the layers and their masks stay as the blend left them. When nothing is ",
            "transparent no layer is added. A fill that cannot run (the area too large for one ",
            "fill) refuses the whole call and nothing changes. One history ",
            "step; the job shows progress and can be cancelled. Reply: {ok, changed: true, ",
            "method, layers: [{layer, name, share, gain?}] (share: the fraction of the canvas ",
            "that layer's mask reveals, 0…1; gain: [r, g, b], Panorama with seamless), ",
            "merged_layer? (Stack Images' merged layer index; layers above it move up by one), ",
            "fill? (with fill: {layer: the filled layer's index or null when nothing was ",
            "transparent, covered_pixels}), ",
            "aligned? (with align: {reference, aligned: [{layer, name, method, homography, ",
            "quad}], unchanged: [{layer, name}], canvas?: {width, height, origin: {x, y}}}, ",
            "auto_align_layers' shapes; canvas when the canvas grew, origin where the old ",
            "canvas's top-left sits), working_memory_plan_bytes, document}. Works at 8, 16 and ",
            "32 bits; a 32-bit float document keeps values above white. A CMYK document is ",
            "refused.",
        ].joined()
        return [
            tool("auto_blend_layers", blend,
                 [
                    "layers": ["type": "array", "items": ["type": "integer"], "minItems": 2],
                    "method": [
                        "type": "string", "enum": AutoBlend.Method.allCases.map(\.rawValue),
                        "default": AutoBlend.Method.stackImages.rawValue,
                        "description": "panorama (seams through the overlaps) or stack_images "
                            + "(focus stacking).",
                    ] as [String: Any],
                    "seamless": [
                        "type": "boolean", "default": true,
                        "description": "Seamless Tones and Colours: Panorama matches tones and "
                            + "blends across the seams; Stack Images adds the merged layer.",
                    ] as [String: Any],
                    "align": [
                        "type": "boolean", "default": false,
                        "description": "Register the layers to one another first.",
                    ] as [String: Any],
                    "fill": [
                        "type": "boolean", "default": false,
                        "description": "Content-Aware Fill Transparent Areas (panorama only): "
                            + "a new layer of the picture with its transparent area filled.",
                    ] as [String: Any],
                    "document_id": Self.docIDProperty,
                 ], required: ["layers"]),
        ]
    }
}
