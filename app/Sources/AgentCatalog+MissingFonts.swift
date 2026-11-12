import Foundation

extension AgentServer {
    /// `list_missing_fonts` and `replace_missing_fonts` (AgentServer+MissingFonts.swift), the
    /// mirrors of Layer ▸ Replace Missing Fonts….
    @inline(never)
    func missingFontsCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        return [
            tool(
                "list_missing_fonts",
                "Lists the font families this document's text layers name but this Mac does not "
                    + "have: {missing: [{font, layers: [{layer, name}]}], count}. Such a layer "
                    + "keeps its saved pixels, reports font_missing: true in get_document, and "
                    + "cannot re-render through a geometry or colour edit until its family is "
                    + "replaced (replace_missing_fonts). Read-only.",
                ["document_id": docID]),
            tool(
                "replace_missing_fonts",
                "Replaces missing font families on every text layer that names them and "
                    + "re-renders those layers as ONE undo step, the way Layer > Replace Missing "
                    + "Fonts... does. Each layer keeps its text, size, colours (ink in a CMYK "
                    + "document), typography, wrap width, transform, mask and properties; only "
                    + "its family changes, and the description saved in .doctored names the new "
                    + "one. Placement: an axis whose position does not depend on the face keeps "
                    + "the block exactly (the left edge under no rotation or flip; the top too at "
                    + "the natural line height); on any other axis the new raster is centred on "
                    + "the saved one, so naming the family the layer was made in reproduces its "
                    + "saved pixels. All or nothing: a layer that cannot re-render (a lock, a "
                    + "layout past the size cap) is named and nothing changes. Not recorded in "
                    + "Actions. Returns {ok, replaced: [{layer, name, font, with, bounds}]}.",
                [
                    "replacements": [
                        "type": "array", "minItems": 1,
                        "items": [
                            "type": "object",
                            "properties": [
                                "font": [
                                    "type": "string",
                                    "description": "A missing family, as list_missing_fonts "
                                        + "names it.",
                                ],
                                "with": [
                                    "type": "string",
                                    "description": "An installed font FAMILY exactly as the "
                                        + "system lists it (\"Helvetica Neue\"), not a PostScript "
                                        + "face name.",
                                ],
                            ],
                            "required": ["font", "with"],
                        ] as [String: Any],
                        "description": "One pair per family to replace; a missing family left "
                            + "out keeps its layers as they are.",
                    ] as [String: Any],
                    "document_id": docID,
                ], required: ["replacements"]),
        ]
    }
}
