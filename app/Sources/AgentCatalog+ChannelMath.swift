import Foundation

extension AgentServer {
    @inline(never)
    func channelMathCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let blendNames = schema.blendNames
        let planeBlendNames = schema.planeBlendNames
        return [
            tool(
                "apply_image",
                "Image > Apply Image…: blends ONE source plane onto the target — a layer's "
                    + "pixels (as one colour, plane for plane), one colour plane of them, or "
                    + "an alpha channel — at an opacity. The source "
                    + "is a layer or the flattened composite (\"merged\"), read as its whole "
                    + "colour image, one plane, or a channel; source_plane \"rgb\" against a "
                    + "single-plane target uses the source's luma. Writes layer pixels "
                    + "destructively for a layer or plane target, so a re-editable text "
                    + "or shape layer drops its description. One undo step. Answers at 8 and "
                    + "16 bits; a 32-bit float document refuses it by name (its blend runs on "
                    + "planes clamped to 0…1, which float pixels are not) — use calculations "
                    + "there, which writes a channel or a selection. On a CMYK document the "
                    + "target is the layer (its four ink plates, plate for plate, each a "
                    + "one-plane blend, so the HSL modes refuse), one ink plate (its stored "
                    + "1 − ink) or an alpha channel, and source_plane \"cmyk\" is the CMYK "
                    + "composite: its matching plate onto the layer, its luminosity onto one "
                    + "plate or channel.",
                [
                    "source": [
                        "type": "string",
                        "description": "\"merged\" (default) or a layer index.",
                    ],
                    "source_plane": [
                        "type": "string",
                        "description": "\"rgb\" (default), \"red\", \"green\", \"blue\", "
                            + "\"luma\", \"alpha\", or an alpha channel's name. On a CMYK "
                            + "document: \"cmyk\" (default), \"cyan\"/\"magenta\"/\"yellow\"/"
                            + "\"black\", \"luma\", \"alpha\" or an alpha channel's name.",
                    ],
                    "invert": [
                        "type": "boolean",
                        "description": "Invert the source plane first (default false).",
                    ],
                    "blend_mode": [
                        "type": "string", "enum": blendNames,
                        "description": "Any mode onto \"layer\", which blends the three "
                            + "planes as ONE colour. Onto a single plane or a channel the "
                            + "four HSL modes (Hue, Saturation, Color, Luminosity) are "
                            + "refused: they are defined over an RGB triple and say nothing "
                            + "about one 8-bit plane.",
                    ],
                    "opacity": ["type": "number", "minimum": 0, "maximum": 1],
                    "target": [
                        "type": "string",
                        "description": "\"layer\" (default), \"red\"/\"green\"/\"blue\"/"
                            + "\"alpha\", or \"channel:<name>\"; on a CMYK document \"layer\", "
                            + "\"cyan\"/\"magenta\"/\"yellow\"/\"black\" or \"channel:<name>\".",
                    ],
                    "layer": index,
                    "document_id": docID,
                ]),
            tool(
                "calculations",
                "Image > Calculations…: blends TWO source planes into a new plane, which "
                    + "becomes a new alpha channel (default) or the selection. Source 1 is "
                    + "the blend layer and source 2 the base it is applied to. "
                    + "Each source is a layer or \"merged\", one of its planes "
                    + "or a channel, optionally inverted. A new channel is one undo step; a "
                    + "selection is not an edit at all. Answers at every depth; in a 32-bit "
                    + "float document each colour or luma plane is read clamped to 0…1, and "
                    + "the result is 16-bit coverage as at 16 bits. On a CMYK document each "
                    + "source is the CMYK composite's luminosity (\"cmyk\"), one ink plate "
                    + "(its stored 1 − ink), luma, alpha or a channel.",
                [
                    "source1": [
                        "type": "string",
                        "description": "\"merged\" (default) or a layer index.",
                    ],
                    "source1_plane": [
                        "type": "string",
                        "description": "\"rgb\" (default), a colour plane, or a channel name; "
                            + "on a CMYK document \"cmyk\" (default), an ink plate, \"luma\", "
                            + "\"alpha\" or a channel name.",
                    ],
                    "invert1": ["type": "boolean"],
                    "source2": [
                        "type": "string",
                        "description": "\"merged\" (default) or a layer index.",
                    ],
                    "source2_plane": [
                        "type": "string",
                        "description": "\"rgb\" (default), a colour plane, or a channel name; "
                            + "on a CMYK document \"cmyk\" (default), an ink plate, \"luma\", "
                            + "\"alpha\" or a channel name.",
                    ],
                    "invert2": ["type": "boolean"],
                    "blend_mode": [
                        "type": "string", "enum": planeBlendNames,
                        "description": "The result is ONE plane, so the four HSL modes (Hue, "
                            + "Saturation, Color, Luminosity) are not offered — they are "
                            + "defined over an RGB triple. Every other mode applies. (Onto a "
                            + "whole LAYER, apply_image does take them.)",
                    ],
                    "opacity": ["type": "number", "minimum": 0, "maximum": 1],
                    "result": ["type": "string", "enum": ["new_channel", "selection"]],
                    "name": [
                        "type": "string",
                        "description": "New channel's name (default the next \"Alpha N\").",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "add_luminosity_masks",
                "Select > Add Luminosity Masks: appends nine channels built from the "
                    + "composite's Rec. 709 luma L — \"Lights 1\"..\"Lights 3\" (L, L², L³), "
                    + "\"Darks 1\"..\"Darks 3\" ((1−L), (1−L)², (1−L)³) and \"Midtones "
                    + "1\"..\"Midtones 3\" (the complement of each squared pair, peaking at "
                    + "mid grey) — the photographer's tone-selection set. Load one with "
                    + "load_selection. Refused when nine more channels would not fit. One "
                    + "undo step. Answers at every depth; in a 32-bit float document L is "
                    + "the composite's linear luminance clamped to 0…1.",
                ["document_id": docID]),
        ]
    }
}
