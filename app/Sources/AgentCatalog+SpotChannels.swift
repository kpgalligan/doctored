import Foundation

/// The spot channel tools' catalog: `add_spot_channel`,
/// `set_spot_channel`, `merge_spot_channel` and `set_channel_kind` (`AgentServer+SpotChannels.swift`). Behind an explicit non-inlining boundary, for
/// `AgentCatalog.swift`'s reason.
extension AgentServer {
    @inline(never) func spotChannelCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        let lab: [String: Any] = [
            "type": "array", "items": ["type": "number"], "minItems": 3, "maxItems": 3,
            "description": "The ink colour, D50 CIE Lab: L* 0–100, a* and b* −128–127.",
        ]
        let solidity: [String: Any] = [
            "type": "integer", "minimum": 0, "maximum": 100,
            "description": "Solidity in percent: 0 a transparent ink that shows the "
                + "colours beneath (Multiply), 100 an ink that covers them.",
        ]
        return [
            tool(
                "add_spot_channel",
                "Adds a SPOT channel — Channels ▸ New Spot Channel…: a channel holding one "
                    + "ink's coverage (255 / 65535 = full ink) with the ink's D50 Lab colour and "
                    + "Solidity. Defaults: name the next \"Spot Color N\", lab sRGB red's, solidity 0, "
                    + "from \"selection\" when there is one else \"empty\". Paint it "
                    + "with target \"channel:<name>\" (the brush adds ink); render (full, region or "
                    + "preview_divisor) shows each ink overprinted on the composite and every "
                    + "flattened export at every depth and mode (PNG, JPEG, …; CMYK in ink on paper) "
                    + "composites it (display and export only; the layers never change). The ink "
                    + "is laid on encoded values: a 32-bit document's view on its 8-bit display at "
                    + "the window's or render's exposure, and its exports and merge on its linear "
                    + "samples encoded by its profile's curve (continued above white) and decoded "
                    + "back, so the file keeps what the view shows. PSD and "
                    + "PSB carry them at 8 and 16 bits (not Lab); a TIFF, flat (save_copy tif) or "
                    + "layered (export_psd tiff), keeps them as Photoshop's extra samples from RGB "
                    + "and CMYK at 8 and 16 bits (a Grayscale, Duotone, Lab or 32-bit TIFF "
                    + "refuses); PDF and a Lab or 32-bit PSD/PSB refuse while spot channels exist; "
                    + ".doctored keeps them. Indexed "
                    + "Color and Bitmap have no spot channels. One undo step. Reply "
                    + "{ok, channel, name, lab, solidity, from, channels}.",
                [
                    "document_id": docID,
                    "name": ["type": "string", "description": "Channel name."],
                    "lab": lab,
                    "solidity": solidity,
                    "from": [
                        "type": "string", "enum": ["selection", "empty"],
                        "description": "The coverage: the selection's, or none.",
                    ],
                ]),
            tool(
                "set_spot_channel",
                "Sets a spot channel's name, ink colour and Solidity — Channels ▸ Spot Channel "
                    + "Options…, one undo step. Each omitted field keeps its value; the same "
                    + "values answer changed false. An alpha channel refuses (\"the channel is not a "
                    + "spot channel\"). Reply {ok, channel, name, lab, solidity}.",
                [
                    "document_id": docID,
                    "channel": [
                        "type": "string",
                        "description": "The spot channel's name or index (list_channels shows both).",
                    ],
                    "name": ["type": "string", "description": "A new name."],
                    "lab": lab,
                    "solidity": solidity,
                ],
                required: ["channel"]),
            tool(
                "merge_spot_channel",
                "Merges a spot channel into the colour channels — Channels ▸ Merge Spot Channel"
                    + ": the document is flattened first (hidden layers are "
                    + "discarded and layer comps cleared, as Flatten Image does), the channel's ink is "
                    + "baked in as its overprint (CMYK in ink on paper), and the spot channel is "
                    + "deleted; other channels stay. One undo step. An alpha channel refuses (\"the "
                    + "channel is not a spot channel\"), and so does a Duotone document. Reply {ok, "
                    + "channel, name, layers, channels} (+ working_memory_plan_bytes at 16 and 32 bits "
                    + "and in CMYK, Gray, Lab).",
                [
                    "document_id": docID,
                    "channel": [
                        "type": "string",
                        "description": "The spot channel's name or index (list_channels shows both).",
                    ],
                ],
                required: ["channel"]),
            tool(
                "set_channel_kind",
                "Changes a channel between an alpha channel and a spot channel — Channel "
                    + "Options… ▸ Color Indicates: Spot Color, and Spot Channel Options… ▸ Kind: Alpha "
                    + "Channel — one undo step. The pixels keep what they showed: what showed "
                    + "black (unselected) becomes full ink, so the stored plane is inverted both ways. "
                    + "kind \"spot\" takes lab and solidity, defaulting to the channel's rubylith colour "
                    + "and its opacity in percent; kind \"alpha\" ignores them and keeps the rubylith. "
                    + "Indexed Color and Bitmap refuse spot channels; the same kind answers changed "
                    + "false. Reply {ok, channel, name, kind, spot} (spot null for alpha).",
                [
                    "document_id": docID,
                    "channel": [
                        "type": "string",
                        "description": "The channel's name or index (list_channels shows both).",
                    ],
                    "kind": [
                        "type": "string", "enum": ["spot", "alpha"],
                        "description": "What the channel becomes.",
                    ],
                    "lab": lab,
                    "solidity": solidity,
                ],
                required: ["channel", "kind"]),
        ]
    }
}
