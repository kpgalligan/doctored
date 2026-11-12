import Foundation

/// The catalog entries for Edit ▸ Define Pattern… and the user's pattern library
/// (`AgentServer+Patterns.swift`), in their own file behind an explicit non-inlining boundary for
/// the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func patternCatalog() -> [[String: Any]] {
        let preference = "A library write is a preference: no Undo step, no Action step, and "
            + "no document changes."
        let file = "Doctored's own pattern file (.\(PatternLibrary.fileExtension)): JSON "
            + "{\"\(PatternLibrary.fileFormatKey)\": 1, \"patterns\": [{schema_version 1, name, "
            + "width, height, rgba}, …]}"
        let reference = "Every argument that takes a pattern object — fill_selection's pattern, "
            + "add_fill_layer / edit_fill_layer's pattern.pattern, set_layer_style's Pattern "
            + "Overlay, Pattern stroke and bevel texture pattern, and convert_mode's Bitmap "
            + "custom pattern — accepts {\"name\": <name>} for a library pattern; the document "
            + "embeds the tile, so it never depends on the library."
        return [
            tool("define_pattern",
                 "Edit ▸ Define Pattern…: save the document's COMPOSITE inside the selection — a "
                 + "hard-edged rectangle, every pixel in its bounds fully selected (a feathered, "
                 + "anti-aliased or elliptical selection refuses by name) — or the whole canvas "
                 + "with nothing selected, as a pattern in the user's library under name. Each "
                 + "side is at most \(LibraryPattern.maximumSide) px. The tile is straight RGBA8, "
                 + "written so a fill in the same kind of document gives the pixels back as "
                 + "closely as 8 bits allow: 8-bit RGB its bytes; 16-bit RGB and Indexed each code "
                 + "rounded to 8 bits; 32-bit float round(255 × clamp(linear, 0, 1)) — light above "
                 + "white clips; Grayscale, Duotone and Bitmap the gray code; Lab and CMYK the "
                 + "composite in 8-bit sRGB — a CMYK tile keeps no separation (a K-only black comes "
                 + "back as four-colour black). A name in use refuses unless replace; a built-in's "
                 + "name or id refuses. Quick Mask refuses. Answers {pattern: {name, width, "
                 + "height}, source: \"selection\" | \"canvas\"}. " + preference,
                 ["name": ["type": "string", "description": "Pattern name, 1–120 characters."],
                  "replace": ["type": "boolean", "default": false,
                              "description": "Overwrite the library pattern of that name."],
                  "document_id": AgentServer.docIDProperty],
                 required: ["name"]),
            tool("list_patterns",
                 "Every pattern a picker lists: the four built-ins (builtin true, with preset: "
                 + "the id {\"preset\": id} names) and then the user's library (builtin false, "
                 + "with file), each {name, width, height}. " + reference + " A file that is not "
                 + "a readable pattern is a `broken` row — {file, code, reason, repairable} with "
                 + "code one of unreadable, oversize, not_json, unknown_version, invalid_schema, "
                 + "duplicate_name — instead of refusing the whole library; repairable is always "
                 + "false here. Resolves no document.",
                 [:]),
            tool("delete_pattern",
                 "Remove ONE library file. Pass either name (a library pattern; a built-in "
                 + "cannot be deleted) or file alone (any library file, including a broken one "
                 + "that has no name). Documents that use the pattern keep their embedded tile. "
                 + preference,
                 ["name": ["type": "string", "description": "A library pattern's name."],
                  "file": libraryFileProperty("list_patterns")]),
            tool("import_patterns",
                 "Read " + file + ", at most \(PatternLibrary.maximumFilePatterns) patterns and "
                 + "\(PatternLibrary.maximumFileBytes / 1_048_576) MiB, into the library. Each "
                 + "entry is checked as a library file is; one that fails is listed in skipped "
                 + "{name, reason} and the rest import. A name already in use, or a built-in's, "
                 + "gets \" 2\", \" 3\"…. " + preference,
                 ["path": ["type": "string", "description": "The pattern file to read."]],
                 required: ["path"]),
            tool("export_patterns",
                 "Write the user's patterns (all, or those names lists; at most "
                 + "\(PatternLibrary.maximumFilePatterns)) as " + file + ". The built-ins are "
                 + "every copy's, so they are not written; naming one refuses. An empty library "
                 + "refuses. " + preference,
                 ["path": ["type": "string", "description": "Where to write the pattern file."],
                  "names": ["type": "array", "items": ["type": "string"],
                            "description": "Library pattern names to export; default all."]],
                 required: ["path"]),
        ]
    }
}
