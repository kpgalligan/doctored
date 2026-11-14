import Foundation

extension AgentServer {
    var focusAreaCatalog: [[String: Any]] {
        [
            tool(
                "select_focus_area",
                "Select > Focus Area…: selects the in-focus part of the picture from its local "
                    + "sharpness (the two-band score Stack Images ranks frames by, over the "
                    + "composite's luma). range 0–100 places the cut along the picture's own "
                    + "sharpness scale, and a lower range selects more; without range, Auto "
                    + "takes Otsu's cut and the reply's range is the value it used (pass it "
                    + "back to repeat the same cut), 0 when the picture has one focus. Flat "
                    + "areas (under one level of detail) are never selected by the measure, "
                    + "but an unselected area a selected one encloses is filled, so a sharp "
                    + "subject keeps its smooth interior; transparent pixels stay outside. "
                    + "soften_edge (default true) feathers the edge. A flat picture selects "
                    + "nothing. Selection-only, one Undo; recorded with the range used. "
                    + "Answers in RGB and Grayscale documents at 8, 16 and 32 bits and in Lab "
                    + "Color, Indexed Color and CMYK documents, reading an 8-bit display picture "
                    + "of the document (at 16 bits its display derivative, at 32 the document's "
                    + "own tone-mapped display, never the window's exposure; Lab and CMYK their "
                    + "Display P3 view, Indexed the palette colours); Duotone and Bitmap refuse "
                    + "it.",
                [
                    "range": ["type": "number", "minimum": 0, "maximum": 100],
                    "soften_edge": ["type": "boolean"],
                    "mode": ["type": "string", "enum": ["replace", "add", "subtract", "intersect"]],
                    "document_id": Self.docIDProperty,
                ]),
        ]
    }
}
