import Foundation

/// #1's catalog entry for `equalize` (Image ▸ Adjustments ▸ Equalize), in its own file behind an
/// explicit non-inlining boundary for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func equalizeCatalog() -> [[String: Any]] {
        [
            tool(
                "equalize",
                "Image ▸ Adjustments ▸ Equalize: remaps the target so its levels spread evenly "
                    + "— one table from the summed R, G and B histograms "
                    + "(each pixel weighted by its alpha and the selection's coverage), the darkest "
                    + "occupied level to black and the brightest to white, applied to every "
                    + "channel alike. With a selection, scope answers the Equalize prompt: "
                    + "\"selected\" (default) equalizes the selected area from its own histogram, "
                    + "\"entire\" the whole target from the selected area's histogram; with none "
                    + "the whole target. At 8, 16 and 32 bits (on a 32-bit float document the "
                    + "table works on the display encoding and values above white or below zero "
                    + "are kept). An 8-bit document takes the layer, its mask, a colour plane or "
                    + "a saved channel; a 16-bit, float or CMYK document a raster layer's pixels. "
                    + "The reply is {ok, changed, layer, target, scope} (scope \"whole\" with no "
                    + "selection).",
                [
                    "layer": [
                        "type": "integer",
                        "description": "Layer index; default the active layer.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "layer (default), mask, red, green, blue, alpha or "
                            + "channel:<name> (8-bit documents; typed documents take layer).",
                    ],
                    "scope": [
                        "type": "string", "enum": ["selected", "entire"],
                        "description": "With a selection: the selected area only (default) or "
                            + "the entire target from the selected area's histogram.",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ]),
        ]
    }
}
