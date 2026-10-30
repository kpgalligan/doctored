import Foundation

extension AgentServer {
    /// `generative_fill`, `generative_expand`, `generative_variations`,
    /// `select_generative_variation` and `generate_variations` (the handlers in AgentServer+GenerativeFill.swift, the runs in
    /// GenerativeFill.swift and GenerativeVariations.swift, the providers in CloudProviders.swift).
    func generativeFillCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let description = [
            "Fills the current selection from a cloud image model ",
            "\u{2014} the app's Edit \u{25B8} Generative Fill\u{2026}. The area ",
            "around the selection (a crop twice its size on each axis where the canvas allows, at ",
            "the aspect of the provider's native size nearest the selection's, never past the ",
            "canvas) and the selection itself are sent with prompt (empty or absent: ",
            "fill from the surrounding picture) to provider (openai, the default: OpenAI's image edit ",
            "API; gemini: Google's Gemini image model, one request a variation; fal: fal's FLUX.1 ",
            "Fill [pro], one request a variation) with the USER'S OWN API key, and each call is ",
            "billed to that key. ",
            "variations (1\u{2013}3, default 3) pictures are asked for. The ",
            "first lands on a NEW layer directly above layer (any layer kind; default the active ",
            "layer), named after the prompt (else \"Generative Layer N\"), masked by the selection ",
            "and tagged as generated; every picture is kept on the layer as its variations (saved in ",
            ".doctored) to switch with select_generative_variation. Pixels outside the selection do not ",
            "change. The new layer becomes active. One undo step; one job with progress that can be ",
            "cancelled. Refused before anything is sent unless the user turned on \u{201C}Allow MCP ",
            "agents to use cloud tools\u{201D} in Settings \u{25B8} Cloud (off by default), accepted ",
            "the provider's terms and saved a key there; refused with no selection, under Quick Mask ",
            "and with a mask, colour plane or channel targeted. A provider's refusal (a bad key, a ",
            "content policy) is answered with its own message and adds no layer. Reply: {ok, changed, ",
            "output_layer, provider, model, native_size: [w, h], crop: [x, y, w, h] (the canvas area ",
            "sent), planned_bytes, prompt (the prompt sent), variations (how many the layer keeps), ",
            "selected_variation, variations_requested (the count asked for), variations_received ",
            "(the pictures that came this run), short_reason (only when fewer came than asked: the ",
            "provider's own sentence)}. 8- and 16-bit RGB and 8-bit CMYK documents (32 bits and ",
            "16-bit CMYK are refused). Not recorded into Actions: replaying it would bill ",
            "the key again.",
        ].joined()
        let expand = [
            "Fills the area the picture leaves transparent from a cloud image model ",
            "\u{2014} the app's Edit \u{25B8} Generative Expand\u{2026}. Every ",
            "pixel the composite leaves not fully opaque is filled, with coverage 1 \u{2212} its ",
            "alpha; add that area first with canvas_size (grow the canvas, the picture placed where ",
            "it should stay) or with crop past the canvas (a rectangle beyond its edges, ",
            "delete_cropped false so the new area stays transparent). crop with fill: ",
            "\"generative_expand\" does both as one undo step, the Crop tool's Fill popup. The shared selection is ignored and not changed. The crop around that ",
            "area is sent exactly as generative_fill sends one, with prompt (empty or absent: ",
            "continue the picture) to provider (openai, gemini or fal) with the USER'S OWN API key, ",
            "billed to it; variations (1\u{2013}3, default 3). The first answer lands on a NEW layer ",
            "directly above layer (default the active layer) masked to that area, so the pixels the ",
            "picture already covers do not change; every answer is kept as its variations. One undo ",
            "step; one job. Gated exactly as generative_fill. Refused when nothing is transparent ",
            "(\"the picture has no transparent area to expand into: enlarge the canvas first\"), ",
            "under Quick Mask and with a mask, colour plane or channel targeted. Reply: ",
            "generative_fill's. 8- and 16-bit RGB and 8-bit CMYK documents (32-bit and 16-bit ",
            "CMYK are refused). Not recorded into Actions.",
        ].joined()
        let read = [
            "Reports a Generative Layer's kept variations, as the app's Layer \u{25B8} Generative ",
            "Variations\u{2026} shows \"Variation k of N\". layer defaults to the active layer. Reply: ",
            "{ok, layer, count, selected (0-based), native_size: [w, h], crop: [x, y, w, h], prompt ",
            "(the selected variation's)}. A layer whose pixels or position were edited since (a ",
            "paint, a filter, a move, a transform, Convert or Assign Profile), or whose mask was ",
            "deleted or moved, has dropped its variations (a mask edit keeps them) and is refused (\"generative layer N holds no variations\"). ",
            "Sends nothing.",
        ].joined()
        let select = [
            "Switches a Generative Layer to its kept variation index (0-based), as the app's Layer ",
            "\u{25B8} Generative Variations\u{2026} Previous / Next: the layer is re-rendered from that ",
            "picture inside the area the fill covered, its mask is kept and its tag becomes that ",
            "picture's. One undo step; selecting the current one changes nothing (changed false). ",
            "Refused for a layer holding no variations, an index out of range and a pixel lock. ",
            "Sends nothing. Reply: {ok, changed, layer, selected, count}.",
        ].joined()
        let more = [
            "Asks the provider for a NEW set of variations of a Generative Layer, as the app's Layer ",
            "\u{25B8} Generative Variations\u{2026} \u{25B8} Generate: the same area the fill sent (the ",
            "picture with that layer hidden, its own mask as the selection) with prompt (default the ",
            "selected variation's) to provider (default the one that made it), variations (1\u{2013}3, ",
            "default 3) pictures, with the USER'S OWN API key, billed to it. They are appended to ",
            "the layer's variations (24 at most) and the first new one is selected. Gated exactly ",
            "as generative_fill. One undo step. Reply: generative_fill's.",
        ].joined()
        let prompt: [String: Any] = [
            "type": "string", "maxLength": GenerativeFillSettings.maximumPromptLength,
            "description": "What to generate in the selection; empty fills from the surroundings.",
        ]
        let provider: [String: Any] = [
            "type": "string", "enum": CloudProviderID.allCases.map(\.rawValue),
            "description": "The cloud provider (default openai).",
        ]
        let expandPrompt: [String: Any] = [
            "type": "string", "maxLength": GenerativeFillSettings.maximumPromptLength,
            "description": "What to add in the transparent area; empty continues the picture.",
        ]
        let variations: [String: Any] = [
            "type": "integer", "minimum": GenerativeFillSettings.variationRange.lowerBound,
            "maximum": GenerativeFillSettings.variationRange.upperBound,
            "description": "How many pictures to ask for (default 3).",
        ]
        let index: [String: Any] = [
            "type": "integer", "minimum": 0,
            "description": "The variation to show, 0-based (generative_variations reports count).",
        ]
        return [
            tool("generative_fill", description, [
                "prompt": prompt, "provider": provider, "variations": variations,
                "layer": schema.index, "document_id": schema.docID,
            ]),
            tool("generative_expand", expand, [
                "prompt": expandPrompt, "provider": provider, "variations": variations,
                "layer": schema.index, "document_id": schema.docID,
            ]),
            tool("generative_variations", read, ["layer": schema.index, "document_id": schema.docID]),
            tool("select_generative_variation", select, [
                "index": index, "layer": schema.index, "document_id": schema.docID,
            ], required: ["index"]),
            tool("generate_variations", more, [
                "prompt": prompt, "provider": provider, "variations": variations,
                "layer": schema.index, "document_id": schema.docID,
            ]),
        ]
    }
}
