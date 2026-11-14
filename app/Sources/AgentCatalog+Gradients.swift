import Foundation

/// The catalog entries for the user's gradient library (`AgentServer+Gradients.swift`), in their
/// own file behind an explicit non-inlining boundary for the reason `AgentCatalog.swift`'s header
/// gives.
extension AgentServer {
    @inline(never) func gradientPresetCatalog() -> [[String: Any]] {
        let preference = "A library write is a preference: no Undo step, no Action step, and "
            + "no document changes."
        let file = "Doctored's own gradient file (.\(GradientPresetLibrary.fileExtension)): JSON "
            + "{\"\(GradientPresetLibrary.fileFormatKey)\": 1, \"gradients\": [{schema_version "
            + "1, name, stops, smoothness}, …]}"
        let reference = "gradient's stops and add_fill_layer / edit_fill_layer's gradient.stops "
            + "accept {\"name\": <name>} for a library gradient: its stops, and its smoothness "
            + "unless the call gives one. The call is resolved before it runs, so a recorded step "
            + "holds the stops and never depends on the library. Stop colours are authored sRGB "
            + "(a CMYK document converts them as any authored colour; a preset keeps no ink)."
        let stopRow: [String: Any] = [
            "type": "object",
            "properties": [
                "position": ["type": "number", "minimum": 0, "maximum": 1],
                "color": ["type": "string", "description": "#RRGGBB, or #RRGGBBAA whose alpha "
                    + "scales opacity."],
                "opacity": ["type": "number", "minimum": 0, "maximum": 1, "default": 1],
                "midpoint": ["type": "number", "minimum": 0.05, "maximum": 0.95],
                "opacity_midpoint": ["type": "number", "minimum": 0.05, "maximum": 0.95],
            ],
            "required": ["position", "color"],
        ]
        return [
            tool("list_gradients",
                 "Every gradient in the user's library, as the stop editor's Preset popup lists "
                 + "them: {name, stops: [{position, color, opacity, midpoint?, opacity_midpoint?}], "
                 + "smoothness, file}. There are no built-ins. " + reference + " A file that is "
                 + "not a readable gradient is a `broken` row — {file, code, reason, repairable} "
                 + "with code one of unreadable, oversize, not_json, unknown_version, "
                 + "invalid_schema, duplicate_name — instead of refusing the whole library. "
                 + "Resolves no document.",
                 [:]),
            tool("save_gradient",
                 "The stop editor's Save Preset…: save stops (2–32, positions 0–1 never "
                 + "decreasing, opacity 0–1, midpoints 0.05–0.95, colour #RRGGBB) and smoothness "
                 + "(0–1, default 1) in the user's library under name. A name in use refuses "
                 + "unless replace. Answers {gradient: {name, stops, smoothness}}. " + preference,
                 ["name": ["type": "string", "description": "Gradient name, 1–120 characters."],
                  "stops": ["type": "array", "items": stopRow, "minItems": 2, "maxItems": 32],
                  "smoothness": ["type": "number", "minimum": 0, "maximum": 1, "default": 1],
                  "replace": ["type": "boolean", "default": false,
                              "description": "Overwrite the library gradient of that name."]],
                 required: ["name", "stops"]),
            tool("delete_gradient",
                 "Edit ▸ Gradients ▸ Delete Gradient…: remove ONE library file. Pass either name "
                 + "(a library gradient) or file alone (any library file, including a broken one "
                 + "that has no name). Layers and recorded steps keep their own stops. "
                 + preference,
                 ["name": ["type": "string", "description": "A library gradient's name."],
                  "file": libraryFileProperty("list_gradients")]),
            tool("import_gradients",
                 "Edit ▸ Gradients ▸ Import Gradients…: read " + file + ", at most "
                 + "\(GradientPresetLibrary.maximumFileGradients) gradients and "
                 + "\(GradientPresetLibrary.maximumFileBytes / 1_048_576) MiB, into the library. "
                 + "Each entry is checked as a library file is; one that fails is listed in "
                 + "skipped {name, reason} and the rest import. A name already in use gets "
                 + "\" 2\", \" 3\"…. " + preference,
                 ["path": ["type": "string", "description": "The gradient file to read."]],
                 required: ["path"]),
            tool("export_gradients",
                 "Edit ▸ Gradients ▸ Export Gradients…: write the user's gradients (all, or those "
                 + "names lists; at most \(GradientPresetLibrary.maximumFileGradients)) as " + file
                 + ". An empty library refuses. " + preference,
                 ["path": ["type": "string", "description": "Where to write the gradient file."],
                  "names": ["type": "array", "items": ["type": "string"],
                            "description": "Library gradient names to export; default all."]],
                 required: ["path"]),
        ]
    }
}
