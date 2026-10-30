import Foundation

extension AgentServer {
    /// `hdr_toning` (AgentServer+HdrToning.swift).
    @inline(never) func hdrToningCatalog() -> [[String: Any]] {
        let description = [
            "Tone map the document into an 8- or 16-bit image — Image ▸ Adjustments ▸ HDR ",
            "Toning… (and Image ▸ Mode ▸ 8/16 Bits/Channel on a 32-bit document). It FLATTENS ",
            "the document, with no prompt: the result is one raster layer, ",
            "\"Background\", holding the composite tone mapped at native depth; one undo step ",
            "restores the layers. method: local_adaptation (the default; measured against ",
            "Photoshop, it differs): the log luminance is split into an edge-aware base (a guided ",
            "filter of radius pixels, 1 to 500, default 16, run at most 192; strength 0.1 to 4, ",
            "default 0.52, the size in stops of a step the base follows; smooth_edges, default ",
            "false, follows more edges) and its detail; the base is compressed into about eight ",
            "stops (gamma 0.1 to 2, default 1, a larger gamma flattens less; exposure -5 to 5 ",
            "stops, default 0), the detail scaled by 1 + detail/100 (detail -100 to 300, default ",
            "30); then vibrance and saturation (-100 to 100, defaults 0 and 20), and shadow and ",
            "highlight (-100 to 100, default 0; positive lifts the shadows or brightens the ",
            "highlights). toning_curve (2 to 16 [in, out] pairs in 0 to 255, default the ",
            "identity [[0,0],[255,255]]) bends the log-luminance range before the split. ",
            "exposure_gamma (exposure in stops, -20 to 20, default ",
            "0, a linear gain; gamma 0.01 to 9.99, default 1, applied as the power 1/gamma in ",
            "linear light, so a larger gamma brightens; then clipped at white), ",
            "highlight_compression (the brightest value rolls down to white along an extended ",
            "Reinhard curve; nothing clips) or equalize_histogram (the image's log tones are ",
            "spread evenly over the whole output range). exposure and gamma are taken by ",
            "exposure_gamma and local_adaptation only, with each method's range; radius, ",
            "strength, smooth_edges, detail, shadow, highlight, vibrance, saturation and ",
            "toning_curve by local_adaptation only. Any other method refuses them. bit_depth: ",
            "for a 32-bit document 8 or 16 (default 16); an 8- or 16-bit document keeps its depth ",
            "(the only value it takes). method is required unless preset is given. preset: ",
            "Default (the built-in preset: local_adaptation at its defaults) or a preset saved ",
            "from the dialog's Preset popup; it sets method and every value, and other ",
            "arguments override it. A recorded call keeps the preset's values, not its ",
            "name. A CMYK document is refused. Recorded into Actions. ",
            "Reply: {ok, method, bit_depth, flattened (whether the source had more than one layer ",
            "or a layer HDR Toning could not keep), working_memory_plan_bytes, document}.",
        ].joined()
        return [
            tool("hdr_toning", description,
                 [
                    "preset": ["type": "string"] as [String: Any],
                    "method": [
                        "type": "string",
                        "enum": HdrToningMethod.allCases.map(\.rawValue),
                    ] as [String: Any],
                    "exposure": ["type": "number", "minimum": -20, "maximum": 20, "default": 0]
                        as [String: Any],
                    "gamma": ["type": "number", "minimum": 0.01, "maximum": 9.99, "default": 1]
                        as [String: Any],
                    "radius": ["type": "number", "minimum": 1, "maximum": 500, "default": 16]
                        as [String: Any],
                    "strength": ["type": "number", "minimum": 0.1, "maximum": 4, "default": 0.52]
                        as [String: Any],
                    "smooth_edges": ["type": "boolean", "default": false] as [String: Any],
                    "detail": ["type": "number", "minimum": -100, "maximum": 300, "default": 30]
                        as [String: Any],
                    "shadow": ["type": "number", "minimum": -100, "maximum": 100, "default": 0]
                        as [String: Any],
                    "highlight": ["type": "number", "minimum": -100, "maximum": 100, "default": 0]
                        as [String: Any],
                    "vibrance": ["type": "number", "minimum": -100, "maximum": 100, "default": 0]
                        as [String: Any],
                    "saturation": ["type": "number", "minimum": -100, "maximum": 100,
                                   "default": 20] as [String: Any],
                    "toning_curve": [
                        "type": "array", "minItems": 2, "maxItems": 16,
                        "items": ["type": "array", "minItems": 2, "maxItems": 2,
                                  "items": ["type": "number", "minimum": 0, "maximum": 255]
                                    as [String: Any]] as [String: Any],
                    ] as [String: Any],
                    "bit_depth": ["type": "integer", "enum": [8, 16]] as [String: Any],
                    "document_id": Self.docIDProperty,
                 ], required: []),
        ]
    }
}
