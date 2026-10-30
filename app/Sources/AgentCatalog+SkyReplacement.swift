import Foundation

extension AgentServer {
    /// `sky_replacement` and `list_skies` (handlers in AgentServer+SkyReplacement.swift, the
    /// settings and run in SkyReplacement.swift, the skies in SkyLibrary.swift).
    func skyReplacementCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let slider: (Double, Double, Double, String) -> [String: Any] = { low, high, initial, text in
            ["type": "number", "minimum": low, "maximum": high,
             "description": text + " (\(Int(low)) to \(Int(high)), default \(Int(initial)))."]
        }
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let description = [
            "Replaces the sky of a raster layer ",
            "\u{2014} the app's Edit \u{25B8} Sky Replacement\u{2026}. The local sky ",
            "model (Select \u{25B8} Sky's) finds the sky of the document; shift_edge grows (positive) or ",
            "shrinks the mask and fade_edge softens it. The new sky is a library sky or one of the ",
            "user's own skies (sky: an id or name from list_skies), or an image file (sky_path); ",
            "brightness and temperature adjust it, scale, offset_x and offset_y (canvas fractions, ",
            "-1 to 1) and flip place it. The foreground is harmonized toward the sky: ",
            "lighting_mode (multiply or screen) with lighting, color_adjustment and edge_lighting ",
            "(the sky's light spilling over the horizon). output new_layers (the default) ",
            "adds an editable Pass Through group \"Sky Replacement Group\" above the layer ",
            "holding, bottom first, \"Foreground Edge\" (only with decontaminate: the edge's ",
            "decontaminated colour, masked to the edge), \"Foreground Color\" and \"Foreground ",
            "Lighting\" Curves adjustment layers (the lighting one in lighting_mode's blend mode) and ",
            "the masked \"Sky\" layer; decontaminate removes the old sky's colour from the horizon's ",
            "edge in either output; ",
            "output duplicate_layer adds ONE flattened layer named \"Sky Replacement\" directly above ",
            "the layer. The output entry becomes active; the layer itself is not changed. One undo ",
            "step. The reply carries changed, output, decontaminate, output_layer (the group or the new layer), the ",
            "sky used (kind ",
            "library, user or file), the model's id and version, sky_share (the share of the canvas ",
            "the model found sky) and planned_bytes. 8- and 16-bit RGB documents only. ",
            "Refused by name when the sky model or the sky ",
            "library is not installed (the user downloads them in Settings \u{25B8} Models), for a group ",
            "or adjustment layer, an unknown sky, and when the model finds no sky.",
        ].joined()
        var properties: [String: Any] = [
            "sky": ["type": "string",
                    "description": "A library sky id or the name of one of the user's skies (list_skies)."],
            "sky_path": ["type": "string", "description": "An image file to use as the sky, instead of sky."],
            "offset_x": ["type": "number", "minimum": -1, "maximum": 1,
                         "description": "Moves the sky right by this fraction of the canvas width (default 0)."],
            "offset_y": ["type": "number", "minimum": -1, "maximum": 1,
                         "description": "Moves the sky down by this fraction of the canvas height (default 0)."],
            "flip": ["type": "boolean", "description": "Mirror the sky horizontally (default false)."],
            "decontaminate": ["type": "boolean",
                              "description": ["Removes the old sky's colour from the horizon's edge pixels ",
                                              "(default false; the dialog's checkbox is on by default). ",
                                              "With output new_layers it adds a \"Foreground Edge\" ",
                                              "raster layer, masked to the edge, at the bottom of the ",
                                              "group."].joined()],
            "lighting_mode": ["type": "string", "enum": ["multiply", "screen"],
                              "description": "How the sky's light blends into the foreground (default multiply)."],
            "output": ["type": "string", "enum": ["new_layers", "duplicate_layer"],
                       "description": ["Where the result goes: new_layers, the editable Sky Replacement ",
                                       "Group (default), or duplicate_layer, one new flattened layer."]
                           .joined()],
            "layer": schema.index, "document_id": schema.docID,
        ]
        let notes: [String: String] = [
            "shift_edge": "Grows (positive) or shrinks the sky mask",
            "fade_edge": "Softens the mask's edge",
            "brightness": "Brightens or darkens the sky",
            "temperature": "Warms (positive) or cools the sky",
            "scale": "The sky's size in percent of the size that covers the canvas",
            "lighting": "How strongly the sky's light tints the foreground",
            "color_adjustment": "How strongly the sky's colour is carried into the foreground",
            "edge_lighting": "The sky's light spilling over the foreground along the horizon",
        ]
        for entry in SkyReplacementSettings.sliders {
            properties[entry.key] = slider(entry.range.lowerBound, entry.range.upperBound, entry.initial,
                                           notes[entry.key] ?? entry.key)
        }
        let listDescription = [
            "Lists the skies Edit \u{25B8} Sky Replacement\u{2026} offers: the sky library's state ",
            "(not_downloaded, downloading, ready or failed) and version, its skies (id, name, category ",
            "blue, sunset, dramatic or night, source_url and license; empty until the library is ",
            "downloaded in Settings \u{25B8} Models) and the user's own skies (name, width, height), ",
            "added from files in the dialog. Read only.",
        ].joined()
        return [
            tool("sky_replacement", description, properties),
            tool("list_skies", listDescription, [:]),
        ]
    }
}
