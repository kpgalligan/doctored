import Foundation

/// The catalog entries for the Styles panel's tools
/// (`AgentServer+StylePresets.swift`), in their own file behind an explicit
/// non-inlining boundary for the reason `AgentCatalog.swift`'s header gives.
extension AgentServer {
    @inline(never) func stylePresetCatalog() -> [[String: Any]] {
        let name: [String: Any] = [
            "type": "string",
            "description": "Style preset name, 1–\(StylePreset.maximumNameLength) characters, no "
                + "“/”.",
        ]
        let group: [String: Any] = [
            "type": "string",
            "description": "The preset's group, 1–\(StylePreset.maximumGroupLength) characters, no "
                + "“/”; omit for an ungrouped preset (a lookup with no group falls back to the "
                + "built-in of that name). “\(StylePreset.builtinGroup)” is the read-only "
                + "built-in group.",
        ]
        let preference = "A library write is a preference: no Undo step, no Action step, and no "
            + "document changes."
        return [
            tool("list_style_presets",
                 "Window > Styles: list the style presets — the twelve built-ins (group "
                 + "“\(StylePreset.builtinGroup)”, read-only, builtin true, file null) first, then "
                 + "the user's groups A–Z, then ungrouped presets. Each row is {name, group, "
                 + "builtin, file, effects (the effect types), blending ({mode, opacity} or null: "
                 + "a preset saved with blending options also sets the layer's blend mode and "
                 + "opacity), style (the preset's style JSON, with include_style)}. A file that "
                 + "is not a readable preset is a `broken` row {file, code, reason, repairable}. "
                 + "Also answers `groups`. Resolves no document.",
                 ["group": ["type": "string", "description": "Only this group's presets."],
                  "include_style": ["type": "boolean", "default": false]]),
            tool("apply_style_preset",
                 "A click on a Styles panel swatch: apply a style preset to `layers` as ONE undo "
                 + "step. Replace (default) sets each layer's style to the preset's; a preset "
                 + "without blending keeps each layer's own fill opacity, Blend If, knockout and "
                 + "Blend Interior option, one with blending also sets the layer's blend mode and "
                 + "opacity. add true is Shift-click: the preset's effects join each layer's "
                 + "style (a repeating effect — drop shadow, inner shadow, stroke, colour or "
                 + "gradient overlay — stacks on top, up to 10; any other replaces its kind). "
                 + "Every layer holds a style, a Pass Through group included (it then composites "
                 + "isolated); a layer that cannot take one is skipped and listed in `skipped` "
                 + "[{layer, reason}]; the call refuses only when no listed layer can. Answers {ok, layers, skipped?, "
                 + "unchanged?}: unchanged true adds no undo step. In a recorded Action, layers "
                 + "is {\"$layers\": \"selected\"}.",
                 ["layers": ["type": "array", "items": ["type": "integer"],
                             "description": "Layer indices (0 = bottom)."],
                  "name": name, "group": group,
                  "add": ["type": "boolean", "default": false],
                  "document_id": Self.docIDProperty],
                 required: ["layers", "name"]),
            tool("save_style_preset",
                 "The Styles panel's New Style…: save one layer's style as a preset. "
                 + "include_effects (default true) keeps its effects; include_blending (default "
                 + "false) keeps its fill opacity, Blend If, knockout and Blend Interior option "
                 + "and records the layer's blend mode and opacity. CMYK inks and pattern anchors "
                 + "are never stored. A name taken in the group refuses unless replace is true; "
                 + "saving nothing refuses. " + preference,
                 ["layer": ["type": "integer",
                            "description": "Layer index (0 = bottom); omit for the active layer."],
                  "name": name, "group": group,
                  "include_effects": ["type": "boolean", "default": true],
                  "include_blending": ["type": "boolean", "default": false],
                  "replace": ["type": "boolean", "default": false],
                  "document_id": Self.docIDProperty],
                 required: ["name"]),
            tool("rename_style_preset",
                 "A swatch's Rename… and Move to Group…: give a user preset a new name, a new "
                 + "group (new_group \"\" ungroups), or both. Built-ins cannot be renamed. "
                 + preference,
                 ["name": name, "group": group,
                  "new_name": ["type": "string", "description": "The new name."],
                  "new_group": ["type": "string",
                                "description": "The new group; \"\" for no group."]],
                 required: ["name"]),
            tool("delete_style_preset",
                 "A swatch's Delete: remove ONE user preset by name (and group), or one library "
                 + "file alone (`file`, including a broken one). Built-ins cannot be deleted. "
                 + preference,
                 ["name": name, "group": group,
                  "file": libraryFileProperty("list_style_presets")]),
            tool("import_style_presets",
                 "The Styles panel's Import Styles…: read a Photoshop style library (.asl, at "
                 + "most 64 MiB) into `group` (default: the file's name). Each style keeps the "
                 + "effects Doctored models; every effect or blending option it cannot keep is "
                 + "named in `dropped` (\"<style>: <sentence>\"), and a style left with nothing is "
                 + "`skipped`. A name taken in the group gets \" 2\", \" 3\"…. Answers {ok, "
                 + "imported [{name, group}], dropped, skipped, file}. " + preference,
                 ["path": ["type": "string", "description": "Absolute path (or ~) to a .asl file."],
                  "group": group],
                 required: ["path"]),
            tool("export_style_presets",
                 "The Styles panel's Export Styles… (every preset, the built-ins included) or a "
                 + "group header's Export Group… (`group`, the built-in group's too): write the "
                 + "presets as a Photoshop style library (.asl) at `path`, replacing a file "
                 + "there. Each style is written with its effects at their own stored angles, "
                 + "its blending options (fill opacity, knockout, Blend Interior, Blend If) and, "
                 + "when saved with blending, its blend mode and opacity; colours are sRGB (a wide "
                 + "colour outside sRGB's gamut is clamped). A preset that cannot be written (a "
                 + "setting Photoshop's dialog cannot hold, Blend If on a CMYK plate) is named in "
                 + "`skipped` (\"<name>: <reason>\"). import_style_presets reads the file back. "
                 + "Answers {ok, exported, skipped, file}. Writes no preset and resolves no "
                 + "document.",
                 ["path": ["type": "string",
                           "description": "Absolute path (or ~) of the .asl file to write."],
                  "group": ["type": "string",
                            "description": "Only this group's presets; omit for every preset."]],
                 required: ["path"]),
        ]
    }
}
