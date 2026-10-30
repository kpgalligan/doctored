import Foundation

extension AgentServer {
    /// `select_sky`, `select_person_parts` and `object_finder_select` (handlers in
    /// AgentServer+ModelSelection.swift, the runs in ModelSelection.swift, PersonParts.swift and
    /// ObjectFinder.swift).
    func modelSelectionCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let feather: [String: Any] = ["type": "number", "minimum": 0, "maximum": 250,
                                      "description": "Feather radius in pixels (0 to 250, default 0)."]
        let number: [String: Any] = ["type": "number"]
        // Arrays of literals joined, never a `+` chain (see `adjustmentOps`).
        let common = [
            "The model reads the document's composite (at 16 and 32 bits its display-encoded copy, ",
            "as Vision does) and its low-resolution mask becomes 16-bit coverage, refined against ",
            "the picture by Select and Mask's edge stage with Smart Radius. One selection Undo; ",
            "pixels unchanged; refine: false skips the edge stage and keeps the model's own matte. ",
            "The reply carries ok, bounds, selection_empty and the model's id ",
            "and version. Refused by name when the model is not installed (the user downloads it ",
            "in Settings \u{25B8} Models; downloads stay a user action).",
        ].joined()
        let sky = [
            "Select the sky \u{2014} the app's Select \u{25B8} Sky. ",
            "A sky segmentation model (U\u{00B2}-Net small, Core ML) gives a soft matte over the whole ",
            "image. Refuses \"No sky was found in this image.\" when less than 0.2% of it is sky. ",
            "The reply adds sky_share, the share of the canvas the model covered at half or more. ",
            common,
        ].joined()
        let parts = [
            "Select parts of people \u{2014} hair, face_skin (facial skin), body_skin and clothes \u{2014} ",
            "from MediaPipe's multiclass selfie segmenter, as the app's ",
            "Select \u{25B8} Person Parts\u{2026}. people chooses 1-based person numbers from inspect_subjects ",
            "model \"people\" (left to right); omit it for everyone. Each person is read on their own ",
            "box and clipped to their own coverage. When Vision cannot separate a crowd, omitting ",
            "people reads everyone at once (person_count 0); naming people then refuses. Refuses ",
            "\"No people were found in this image.\" and \"None of the chosen parts was found on the ",
            "chosen people.\" The reply adds parts and person_count. ",
            common,
        ].joined()
        let finder = [
            "Select an object from a prompt with Segment Anything 2.1 (Core ML; Large when ",
            "downloaded, else Tiny), as the Object Selection tool's Object Finder: points are ",
            "clicks in canvas pixels, include true (the default) for the object and false to ",
            "exclude a part of it; box is a dragged rectangle around it. Give points, a box or ",
            "both, at most 14 prompt tokens (a box counts two). A new prompt combines with the ",
            "selection by mode after feather. continue: true refines the selection the previous ",
            "object_finder_select made (still the current selection): points and box are then the ",
            "WHOLE prompt (the earlier points plus the new ones), the result replaces that step's, ",
            "and mode and feather come from the prompt being continued, so passing them refuses. ",
            "continue without such a selection refuses \"continue needs the selection the previous ",
            "object_finder_select made.\" Refuses \"No object was found at the prompt.\" The reply ",
            "adds score (the decoder's own quality estimate) and continued. ",
            common,
        ].joined()
        let refine: [String: Any] = ["type": "boolean",
                                     "description": "Refine the model's matte against the picture's edges (default true)."]
        let point: [String: Any] = [
            "type": "object", "properties": ["x": number, "y": number, "include": ["type": "boolean"]],
            "required": ["x", "y"],
        ]
        return [
            tool("select_sky", sky, ["document_id": schema.docID, "mode": schema.selectionMode,
                                     "feather": feather, "refine": refine]),
            tool("select_person_parts", parts, [
                "document_id": schema.docID,
                "parts": ["type": "array", "minItems": 1,
                          "items": ["type": "string", "enum": PersonPart.allCases.map(\.rawValue)]],
                "people": ["type": "array", "minItems": 1, "items": ["type": "integer", "minimum": 1]],
                "mode": schema.selectionMode, "feather": feather, "refine": refine,
            ], required: ["parts"]),
            tool("object_finder_select", finder, [
                "document_id": schema.docID,
                "points": ["type": "array", "items": point, "maxItems": ObjectFinder.maximumPoints],
                "box": ["type": "object", "properties": ["x": number, "y": number, "width": number, "height": number],
                        "required": ["x", "y", "width", "height"]],
                "mode": schema.selectionMode, "feather": feather, "refine": refine,
                "continue": ["type": "boolean",
                             "description": "Refine the previous object_finder_select's prompt (default false)."],
            ]),
        ]
    }
}
