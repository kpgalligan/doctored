import Foundation

extension AgentServer {
    @inline(never)
    func guideChannelCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let selectionMode = schema.selectionMode
        return [
            tool(
                "list_guides",
                "Lists the document's GUIDES: non-printing lines the tools snap to, drawn "
                    + "across the whole canvas and never part of the picture. Each is "
                    + "{index, orientation, position}, sorted by orientation then position — "
                    + "so an index is stable only until the next guide edit. position is an "
                    + "absolute canvas pixel coordinate (x for a vertical guide, y for a "
                    + "horizontal one), measured from the canvas top-left, NOT from the ruler "
                    + "origin. The reply also carries the same ruler block get_document "
                    + "reports — origin_x/origin_y plus the app-wide ruler unit — so placing a "
                    + "guide relative to what a ruler LABELS needs no second call.",
                ["document_id": docID]),
            tool(
                "add_guide",
                "Adds a guide — View ▸ New Guide…, and dragging one out of a ruler strip. "
                    + "position is an absolute canvas coordinate: x for a vertical guide "
                    + "(0 to the canvas width), y for a horizontal one (0 to the canvas "
                    + "height), both ends legal because a guide may sit exactly on an edge. "
                    + "It is NOT measured from the ruler origin, which get_document reports "
                    + "separately. A position another guide of the same orientation already "
                    + "occupies changes nothing and says so. Guides ride along with crop, "
                    + "canvas size, image size, rotate and flip exactly as the pixels do, and "
                    + "persist in .doctored. One undo step.",
                [
                    "orientation": [
                        "type": "string",
                        "enum": ["horizontal", "vertical"],
                        "description": "horizontal is a line of constant y; vertical of "
                            + "constant x.",
                    ],
                    "position": [
                        "type": "number",
                        "description": "Canvas pixels along the guide's own axis.",
                    ],
                    "document_id": docID,
                ], required: ["orientation", "position"]),
            tool(
                "remove_guide",
                "Deletes one guide by its index in list_guides — the same edit as dragging it "
                    + "back into its ruler, or pressing Delete while it is grabbed. Indices "
                    + "shift after any guide edit, so read list_guides immediately before. "
                    + "One undo step.",
                [
                    "guide": [
                        "type": "integer",
                        "description": "Index from list_guides.",
                    ],
                    "document_id": docID,
                ], required: ["guide"]),
            tool(
                "clear_guides",
                "Removes every guide — View ▸ Clear Guides. Works even while guides are "
                    + "locked in the app, because that lock only stops an accidental drag. "
                    + "One undo step, which restores all of them.",
                ["document_id": docID]),
            tool(
                "set_ruler_origin",
                "Moves the rulers' zero point — dragging from the box where the two ruler "
                    + "strips meet (double-clicking it resets to 0, 0). x and y are canvas "
                    + "pixels inside the canvas. This affects ONLY what the on-screen rulers "
                    + "label and what View ▸ New Guide… seeds its field with: the grid, every "
                    + "snap target and every coordinate any tool reports or accepts stay "
                    + "measured from the canvas top-left. Document state, so it persists in "
                    + ".doctored and rides along with the geometry ops. One undo step.",
                [
                    "x": ["type": "number", "description": "Canvas pixels from the left."],
                    "y": ["type": "number", "description": "Canvas pixels from the top."],
                    "document_id": docID,
                ], required: ["x", "y"]),
            tool(
                "list_channels",
                "Lists the document's ALPHA CHANNELS: named canvas-sized coverage planes "
                    + "(0 = out, 255 = in, in between = a soft edge) that are saved "
                    + "selections. A channel is never part of the picture — its overlay "
                    + "colour, opacity and color_indicates only say how the app draws it as "
                    + "a rubylith. Use the index or the name wherever a tool takes "
                    + "`channel`. The rows are get_document's channels array at every "
                    + "depth. spot is a SPOT channel's ink, {lab: D50 [L, a, b], solidity: "
                    + "0–100}, its plane the ink's coverage (add_spot_channel), and null for an "
                    + "alpha channel.",
                ["document_id": docID]),
            tool(
                "add_channel",
                "Adds an alpha channel — the Channels panel's New Channel, and Save "
                    + "Selection as Channel. from: \"empty\" (default) an all-zero channel; "
                    + "\"selection\" the current selection's coverage (errors when nothing "
                    + "is selected); \"layer_alpha\" a layer's transparency; \"layer_mask\" "
                    + "a layer's mask; \"plane\" one colour plane of the flattened "
                    + "composite. One undo step.",
                [
                    "name": [
                        "type": "string",
                        "description": "Channel name (default the next \"Alpha N\").",
                    ],
                    "from": [
                        "type": "string",
                        "enum": ["empty", "selection", "layer_alpha", "layer_mask", "plane"],
                    ],
                    "layer": index,
                    "plane": [
                        "type": "string",
                        "enum": ["red", "green", "blue", "alpha", "luma", "cyan", "magenta", "yellow",
                                 "black"],
                        "description": "Which plane, for from: \"plane\" (default luma). On an "
                            + "8- or 16-bit RGB document luma is the composite's luminosity in Edit ▸ "
                            + "Color Settings' Gray working space (Dot Gain 20% by default: the "
                            + "profile's Y through the space's curve). A CMYK "
                            + "document's planes are cyan, magenta, yellow and black (each loads "
                            + "1 − ink, as a Ctrl-click on the plate does), luma (the composite's "
                            + "luminosity) and alpha.",
                    ],
                    "overlay_color": [
                        "type": "string",
                        "description": "Rubylith colour, #RRGGBB (default #ff0000). Display "
                            + "only.",
                    ],
                    "overlay_opacity": [
                        "type": "number", "minimum": 0, "maximum": 1,
                        "description": "Rubylith opacity 0-1 (default 0.5). Display only.",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "delete_channel",
                "Deletes an alpha channel — the channels row menu's Delete Channel. One "
                    + "undo step.",
                [
                    "channel": [
                        "type": "string",
                        "description": "Channel name or index (list_channels shows both).",
                    ],
                    "document_id": docID,
                ], required: ["channel"]),
            tool(
                "duplicate_channel",
                "Copies an alpha channel — the channels row menu's Duplicate Channel. The "
                    + "copy is named \"<name> copy\" and lands immediately after the "
                    + "original, so the indices below it shift down by one. One undo step.",
                [
                    "channel": [
                        "type": "string",
                        "description": "Channel name or index (list_channels shows both).",
                    ],
                    "document_id": docID,
                ], required: ["channel"]),
            tool(
                "rename_channel",
                "Renames an alpha channel — the channels row's double-click rename. Names "
                    + "need not be unique; the first match wins wherever a name is used.",
                [
                    "channel": [
                        "type": "string",
                        "description": "Channel name or index.",
                    ],
                    "name": ["type": "string", "description": "The new name."],
                    "document_id": docID,
                ], required: ["channel", "name"]),
            tool(
                "set_channel_options",
                "Changes an alpha channel's name and rubylith — Channel Options…. The "
                    + "rubylith is DISPLAY state: it never changes a byte of the channel's "
                    + "coverage. color_indicates \"masked\" (the default, and Quick Mask's "
                    + "polarity) washes where the channel is BLACK; \"selected\" washes "
                    + "where it is white. Omitted properties stay as they are; one undo "
                    + "step.",
                [
                    "channel": [
                        "type": "string",
                        "description": "Channel name or index.",
                    ],
                    "name": ["type": "string"],
                    "overlay_color": [
                        "type": "string", "description": "Rubylith colour, #RRGGBB.",
                    ],
                    "overlay_opacity": ["type": "number", "minimum": 0, "maximum": 1],
                    "color_indicates": ["type": "string", "enum": ["masked", "selected"]],
                    "document_id": docID,
                ], required: ["channel"]),
            tool(
                "invert_channel",
                "Inverts an alpha channel's coverage (255 − v per pixel) — the channels row "
                    + "menu's Invert Channel. What it selected it now excludes. One undo "
                    + "step.",
                [
                    "channel": [
                        "type": "string",
                        "description": "Channel name or index.",
                    ],
                    "document_id": docID,
                ], required: ["channel"]),
            tool(
                "load_selection",
                "Loads a plane as the selection — Select > Load Selection…, and ⌘-clicking "
                    + "a channel row or a layer thumbnail in the app. Sources: an alpha "
                    + "channel (default), a layer's transparency, a layer's mask, or one "
                    + "colour plane of the flattened composite. A selection is not an edit: "
                    + "one transient undo step and no file-dirty change. An all-zero source clears the "
                    + "selection and reports selection_empty. Select > Portrait Matte is from: "
                    + "\"channel\" with the matte's channel name (\"Portrait Matte\", \"Skin\", "
                    + "\"Hair\", \"Teeth\", \"Glasses\" or \"Sky\" on an iPhone photo).",
                [
                    "from": [
                        "type": "string",
                        "enum": ["channel", "layer_alpha", "layer_mask", "plane"],
                    ],
                    "channel": [
                        "type": "string",
                        "description": "Channel name or index, for from: \"channel\".",
                    ],
                    "layer": index,
                    "plane": [
                        "type": "string",
                        "enum": ["red", "green", "blue", "alpha", "luma", "cyan", "magenta", "yellow",
                                 "black"],
                        "description": "Which plane, for from: \"plane\" (default luma). On an "
                            + "8- or 16-bit RGB document luma is the composite's luminosity in Edit ▸ "
                            + "Color Settings' Gray working space (Dot Gain 20% by default: the "
                            + "profile's Y through the space's curve). A CMYK "
                            + "document's planes are cyan, magenta, yellow and black (each loads "
                            + "1 − ink, as a Ctrl-click on the plate does), luma (the composite's "
                            + "luminosity) and alpha.",
                    ],
                    "mode": selectionMode,
                    "invert": [
                        "type": "boolean",
                        "description": "Load the complement instead (default false).",
                    ],
                    "document_id": docID,
                ]),
            tool(
                "save_selection",
                "Writes the current selection into an alpha channel — Select > Save "
                    + "Selection…. With `name` (or neither) it creates a new channel; with "
                    + "`channel` it combines into that existing one, where `mode` says how. "
                    + "Errors when nothing is selected. One undo step.",
                [
                    "name": [
                        "type": "string",
                        "description": "New channel's name (default the next \"Alpha N\"). "
                            + "Ignored when `channel` is given.",
                    ],
                    "channel": [
                        "type": "string",
                        "description": "Existing channel name or index to combine into.",
                    ],
                    // NOT the shared `selectionMode`: this tool writes the
                    // selection INTO a channel, so the mode combines the two
                    // the other way round and an empty result leaves an empty
                    // channel rather than deselecting.
                    "mode": [
                        "type": "string",
                        "enum": ["replace", "add", "subtract", "intersect"],
                        "description": "How the selection combines into the existing "
                            + "`channel` (default replace, which overwrites it). add unions, "
                            + "subtract removes the selection from the channel's coverage, "
                            + "intersect keeps the overlap. Ignored without `channel`. The "
                            + "current selection is never changed.",
                    ],
                    "document_id": docID,
                ]),
        ]
    }
}
