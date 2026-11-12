import Foundation

/// The catalog entries for the brush preset library
/// (`AgentServer+BrushPresets.swift`), in their own file behind an explicit
/// non-inlining boundary for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func brushPresetCatalog() -> [[String: Any]] {
        let preference = "A library write is a preference: no Undo step, no Action step, and "
            + "no document changes."
        let file = "Doctored's own brush preset file (.\(BrushPresetLibrary.fileExtension)): JSON "
            + "{\"\(BrushPresetLibrary.fileFormatKey)\": 1, \"presets\": [{schema_version 1, name, "
            + "hardness, flow, spacing, angle, roundness, airbrush}, …]}"
        let number = { (range: String, what: String) -> [String: Any] in
            ["type": "number", "description": "\(what), \(range) (the options bar's field)."]
        }
        return [
            tool("list_brush_presets",
                 "The paint options bar's Preset popup: every brush tip preset, the built-ins "
                 + "(builtin true) and then the user's library, each {name, builtin, hardness, "
                 + "flow, spacing, angle, roundness, airbrush, file?}. A preset is a TIP — never "
                 + "size, opacity or blend — so LOADING one is passing its six values to a "
                 + "stroke tool (brush_stroke, eraser_stroke, clone_stamp, dodge_burn and the "
                 + "rest take them by these names). No two presets hold the same six values. A "
                 + "file that is not a readable preset is a `broken` row — {file, code, reason, "
                 + "repairable} with code one of unreadable, oversize, not_json, "
                 + "unknown_version, invalid_schema, duplicate_name — instead of refusing the "
                 + "whole library; repairable is always false here. Resolves no document.",
                 [:]),
            tool("save_brush_preset",
                 "The Preset popup's Save Preset…: store a tip under a name. A built-in's name "
                 + "refuses, a name in use refuses unless replace is true, and values another "
                 + "preset (built-in or saved) already holds refuse by that preset's name, since "
                 + "the popup finds a preset by its values. " + preference,
                 ["name": ["type": "string", "description": "Preset name, 1–120 characters."],
                  "hardness": number("0–100", "Hardness percent"),
                  "flow": number("1–100", "Flow percent"),
                  "spacing": number("1–200", "Spacing, percent of the diameter"),
                  "angle": number("-180–180", "Tip angle in degrees"),
                  "roundness": number("1–100", "Roundness percent"),
                  "airbrush": ["type": "boolean", "default": false],
                  "replace": ["type": "boolean", "default": false]],
                 required: ["name", "hardness", "flow", "spacing", "angle", "roundness"]),
            tool("delete_brush_preset",
                 "The Preset popup's Delete Preset…: remove ONE library file. Pass either name "
                 + "(a saved preset; a built-in cannot be deleted) or file alone (any library "
                 + "file, including a broken one that has no name). " + preference,
                 ["name": ["type": "string", "description": "A saved preset's name."],
                  "file": libraryFileProperty("list_brush_presets")]),
            tool("import_brush_presets",
                 "The Preset popup's Import Presets…: read " + file + ", at most "
                 + "\(BrushPresetLibrary.maximumFilePresets) presets and 1 MiB, into the library. "
                 + "Each entry is checked as a library file is; one that fails, or that repeats "
                 + "a listed preset's values, is listed in skipped {name, reason} and the rest "
                 + "import. A name already in use gets \" 2\", \" 3\"…. " + preference,
                 ["path": ["type": "string", "description": "The preset file to read."]],
                 required: ["path"]),
            tool("export_brush_presets",
                 "The Preset popup's Export Presets…: write the user's presets (all, or those "
                 + "names lists) as " + file + ". The built-ins are every copy's, so they are "
                 + "not written; naming one refuses. An empty library refuses. " + preference,
                 ["path": ["type": "string", "description": "Where to write the preset file."],
                  "names": ["type": "array", "items": ["type": "string"],
                            "description": "Saved preset names to export; default all."]],
                 required: ["path"]),
        ]
    }
}
