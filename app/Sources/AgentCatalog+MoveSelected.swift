import Foundation

/// The catalog entry for `move_selected_pixels` (with its `target` and
/// `background` arguments and the colour-plane targets), in its own file
/// behind an explicit non-inlining boundary for the reason
/// `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func moveSelectedPixelsCatalog() -> [[String: Any]] {
        [
            tool(
                "move_selected_pixels",
                "Moves the SELECTED PIXELS of one layer by a whole-pixel delta — the Move "
                    + "tool's drag and arrow nudge with a selection active. The layer's pixels "
                    + "× the selection coverage float, move by (dx, dy) and composite back "
                    + "Normal source-over; the hole they leave is transparent (alpha "
                    + "a·(1−coverage), no Background fill), pixels that would land off the "
                    + "canvas are discarded, the layer mask stays put, and the selection "
                    + "moves with the pixels — ONE undo step, \"Move\". Needs an active "
                    + "selection (move_layers moves whole layers). Consecutive calls on the "
                    + "same layer and target, with the document and selection as the last call "
                    + "left them, move ONE floating selection as consecutive Move drags do: "
                    + "each call is one undo step and pixels the float passed over come "
                    + "back; any other edit, a selection or layer change commits it. "
                    + "One pixel layer only: with "
                    + "several layers selected in the panel, name one with `layer`. Groups, "
                    + "adjustment layers, Quick Mask and a retained source refuse by name; "
                    + "Lock Pixels / Lock All refuse, Lock "
                    + "Transparency and Lock Position allow it. A text or shape "
                    + "layer is rasterized and the result says so. dx = dy = 0 is an "
                    + "unchanged no-op. `duplicate: true` is the Move tool's Option-drag "
                    + "and Option-arrow: "
                    + "no hole, the original stays and the COPY lands source-over, the "
                    + "selection moves with the copy — ONE undo step, \"Duplicate\". An "
                    + "empty selected area (no opaque pixel under the selection) refuses. "
                    + "Works at 8 and 16 bits and in a 32-bit float document (f32, nothing "
                    + "quantized). `target` \"mask\" or \"channel:<name>\" "
                    + "moves the selected area of the layer's mask or of an alpha "
                    + "channel instead — at 8, 16 and 32-bit float (masks and channels are "
                    + "16-bit coverage there): the "
                    + "plane's selected coverage floats (feathered edges carry through) and "
                    + "lands at (dx, dy), and the vacated area takes `background` (a hex, "
                    + "default #000000) as coverage gray, the background-colour rule "
                    + "for a plane without transparency; `duplicate` leaves no hole. A mask's "
                    + "stored rect grows to hold what lands on the canvas (a group's mask is "
                    + "clipped instead), a channel stays canvas-sized, and the layer's pixels "
                    + "and any description stay. Lock All refuses a mask (Lock Pixels allows "
                    + "it); a channel has no lock; Quick Mask refuses. The reply names the "
                    + "target and background and carries mask_bounds or channel in place of "
                    + "layer_bounds (no layer for a channel). "
                    + "`target` \"red\", \"green\", \"blue\" or \"alpha\" moves ONE "
                    + "colour channel of the layer's selected pixels instead, at 8, 16 and "
                    + "32-bit float: only that component lands (the other components and the "
                    + "alpha stay, the layer never grows), the red/green/blue vacated area takes "
                    + "`background`'s 0.30/0.59/0.11 luma rounded to an 8-bit code ("
                    + "default #000000), Alpha's is transparent; selected pixels with no stored "
                    + "alpha float nothing, so a selection over only transparent pixels refuses "
                    + "with \"The selection holds no pixels in this colour "
                    + "channel.\" Lock Pixels refuses, and Lock Transparency an Alpha "
                    + "target too; a text or shape layer is rasterized and the reply "
                    + "is the layer target's.",
                [
                    "layer": [
                        "type": "integer",
                        "description": "Layer index whose selected pixels move; default the "
                            + "active layer.",
                    ],
                    "dx": [
                        "type": "integer",
                        "description": "Canvas pixels to move right (negative moves left); "
                            + "|dx| ≤ 1,000,000.",
                    ],
                    "dy": [
                        "type": "integer",
                        "description": "Canvas pixels to move down (negative moves up); "
                            + "|dy| ≤ 1,000,000.",
                    ],
                    "target": [
                        "type": "string",
                        "description": "What moves: \"layer\" (default, the layer's "
                            + "pixels), \"mask\" (the layer's mask), \"channel:<name>\" "
                            + "(an alpha channel) or \"red\", \"green\", \"blue\", "
                            + "\"alpha\" (one colour channel of the layer).",
                    ],
                    "background": [
                        "type": "string",
                        "description": "A mask or channel target's vacated-area colour as a "
                            + "hex (#RRGGBB), read as coverage gray; for a red, green or "
                            + "blue target the background colour whose 0.30/0.59/0.11 luma fills "
                            + "the vacated area; default #000000. Refused with a layer or "
                            + "alpha target.",
                    ],
                    "duplicate": [
                        "type": "boolean",
                        "description": "Move a COPY and leave the original (the Option-drag "
                            + "or Option-arrow); "
                            + "default false. Echoed in the result.",
                    ],
                    "document_id": AgentServer.docIDProperty,
                ], required: ["dx", "dy"]),
        ]
    }
}
