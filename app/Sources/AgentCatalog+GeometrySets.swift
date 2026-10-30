import Foundation

extension AgentServer {
    func geometrySetCatalog() -> [[String: Any]] {
        [tool(
            "transform_layers",
            "Apply one affine matrix or one perspective quad to a layer set, group descendants, "
                + "linked entries and linked masks atomically. Editable artwork stays editable "
                + "under affine geometry; true perspective requires rasterize_artwork:true and "
                + "reports the exact rasterized layer indices. The canvas is unchanged. "
                + "duplicate:true mirrors Layer ▸ Free Transform a Copy (⌥⌘T) with no "
                + "selection: it duplicates layers (unlinked copies, directly above their "
                + "sources), transforms the COPIES with the same matrix or quad and records "
                + "ONE \"Transform Copy\" step; the copies become the selection. The reply "
                + "adds duplicate and copies, and layers lists the copies' subtrees. An identity "
                + "matrix answers changed:false with copies [] and duplicates nothing. It "
                + "refuses a Position lock on an original's subtree. The copies take the "
                + "UI's described rules: at 8 bits ONE copied text or shape "
                + "layer under a matrix or a parallelogram composes into its own "
                + "description and stays editable; otherwise the rules above apply to the "
                + "copies — a true perspective over described copies needs "
                + "rasterize_artwork:true, and rasterized_layers lists the copies whose "
                + "description was dropped (indices in the resulting document). An optional "
                + "pivot [x, y] (only with matrix) records the point the matrix was built "
                + "around; it does not change the result here, and a relative Action replay "
                + "keeps the angle, scale and skew about it.",
            [
                "layers": ["type": "array", "items": ["type": "integer"], "minItems": 1],
                "matrix": ["type": "array", "items": ["type": "number"],
                           "minItems": 6, "maxItems": 6],
                "pivot": ["type": "array", "items": ["type": "number"],
                          "minItems": 2, "maxItems": 2],
                "source_rect": ["type": "array", "items": ["type": "number"],
                                "minItems": 4, "maxItems": 4],
                "corners": ["type": "array", "items": ["type": "array",
                            "items": ["type": "number"], "minItems": 2, "maxItems": 2],
                            "minItems": 4, "maxItems": 4],
                "sampler": ["type": "string",
                            "enum": ["nearest", "bilinear", "bicubic", "lanczos"]],
                "rasterize_artwork": ["type": "boolean"],
                "duplicate": ["type": "boolean", "default": false],
                "document_id": Self.docIDProperty,
            ], required: ["layers"])]
    }
}
