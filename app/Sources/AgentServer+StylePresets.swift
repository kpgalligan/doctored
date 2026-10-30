import Foundation

// The five library tools (list, rename, delete, import, export) are `nonRecordable` and `documentFree`: a
// library write is a preference, never a step, and none resolves a document. `save_style_preset`
// reads a layer, so it resolves one; `apply_style_preset` is an edit and a recorded step.

/// The style-preset library and the Styles panel over MCP.
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `listStylePresets` | Window ▸ Styles: the grid, its groups and swatches |
/// | `applyStylePreset` | a swatch click (Shift-click: `add`) over the selected layers |
/// | `saveStylePreset` | the panel's New Style… (`NewStyleSheetController`) |
/// | `renameStylePreset` | a swatch's Rename… and Move to Group… |
/// | `deleteStylePreset` | a swatch's Delete (`name`); `file` clears a broken file |
/// | `importStylePresets` | the panel's Import Styles… (a Photoshop `.asl`) |
/// | `exportStylePresets` | the panel's Export Styles…, and a group header's Export Group… (`group`) |
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let stylePresetsHandlers: [String: ToolHandler] = [
        "list_style_presets": { $0.listStylePresets },
        "apply_style_preset": { $0.applyStylePreset },
        "save_style_preset": { $0.saveStylePreset },
        "rename_style_preset": { $0.renameStylePreset },
        "delete_style_preset": { $0.deleteStylePreset },
        "import_style_presets": { $0.importStylePresets },
        "export_style_presets": { $0.exportStylePresets },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let stylePresetsToolFacts: [String: ToolFacts] = [
        // A preset applies through `withLayerStyle` at every depth and mode
        // (the core refuses a non-float blend at 32 bits by name; inkless
        // slots convert at composite on CMYK).
        "apply_style_preset": [.typedTool, .floatTool, .cmykTool],
        // Reads a layer's style at every depth; the write is a preference.
        "save_style_preset": [.nonRecordable, .typedTool, .floatTool, .cmykTool],
        "list_style_presets": [.nonRecordable, .documentFree],
        "rename_style_preset": [.nonRecordable, .documentFree],
        "delete_style_preset": [.nonRecordable, .documentFree],
        "import_style_presets": [.nonRecordable, .documentFree],
        "export_style_presets": [.nonRecordable, .documentFree],
    ]

    /// Mirrors Window ▸ Styles: every preset, the built-ins first.
    func listStylePresets(_ a: [String: Any]) throws -> String {
        let group = stringArg(a, "group")
        let includeStyle = boolArg(a, "include_style") ?? false
        return try stylePresetLibrary {
            let listing = try StylePresetLibrary.listing()
            let key = { (text: String) in PreferenceLibrary<StylePresetCodec>.key(text) }
            let rows: [[String: Any]] = listing.presets
                .filter { row in group.map { key($0) == row.preset.group.map(key) } ?? true }
                .map { row in
                    var out = stylePresetFields(row.preset)
                    out["file"] = row.file ?? NSNull()
                    if includeStyle { out["style"] = row.preset.styleObject }
                    return out
                }
            return try jsonResult([
                "presets": rows,
                "groups": StylePresetLibrary.groups(listing.presets.map(\.preset)),
                "broken": listing.broken.map(\.json),
                "maximum_presets": StylePresetLibrary.maximumPresets,
                "maximum_file_bytes": StylePresetLibrary.maximumFileBytes,
            ])
        }
    }

    /// Mirrors a swatch click over the selection (Shift-click with `add`):
    /// every listed layer that can hold a style, as ONE undo step; a layer
    /// that cannot (`StylePreset.ineligibility`, the panel's rule) is skipped
    /// and listed, and the call refuses only when none can.
    func applyStylePreset(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image.") }
        let indices = try layerIndices(a, document)
        let name = try requiredString(a, "name")
        let preset = try stylePresetLibrary {
            try StylePresetLibrary.preset(name: name, group: stringArg(a, "group"))
        }
        let add = boolArg(a, "add") ?? false
        var eligible: [Int] = [], skipped: [[String: Any]] = []
        for index in indices {
            if let reason = StylePreset.ineligibility(doc, layer: index) {
                skipped.append(["layer": index, "reason": reason])
            } else {
                eligible.append(index)
            }
        }
        guard !eligible.isEmpty else {
            throw ToolError(message: skipped.first?["reason"] as? String ?? "No layer can hold a style.")
        }
        let updated: RasterDocument?
        do {
            updated = try preset.applied(to: doc, layers: eligible, add: add)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var reply: [String: Any] = ["ok": true, "layers": eligible]
        if !skipped.isEmpty { reply["skipped"] = skipped }
        guard let updated else {
            reply["unchanged"] = true
            return try jsonResult(reply)
        }
        try performGroupedEdit(document, add ? "Add Style" : "Apply Style") { _ in updated }
        return try jsonResult(reply)
    }

    /// Mirrors the panel's New Style…: one layer's style (and, with
    /// `include_blending`, its blending) saved as a preset.
    func saveStylePreset(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image.") }
        let index = try structuralLayerIndex(a, document)
        let name = try requiredString(a, "name")
        return try stylePresetLibrary {
            let preset = try StylePreset.fromLayer(
                doc, layer: index, name: name, group: stringArg(a, "group"),
                includeEffects: boolArg(a, "include_effects") ?? true,
                includeBlending: boolArg(a, "include_blending") ?? false)
            let saved = try StylePresetLibrary.save(preset, replace: boolArg(a, "replace") ?? false)
            return try jsonResult(["ok": true, "preset": stylePresetFields(saved),
                                   "file": stylePresetFile(saved) ?? NSNull()])
        }
    }

    /// Mirrors a swatch's Rename… (`new_name`) and Move to Group…
    /// (`new_group`; "" ungroups).
    func renameStylePreset(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        let group = stringArg(a, "group")
        let newName = stringArg(a, "new_name")
        let newGroup = a["new_group"] == nil ? nil : (stringArg(a, "new_group") ?? "")
        guard newName != nil || newGroup != nil else {
            throw ToolError(message: "rename_style_preset needs new_name, new_group or both.")
        }
        return try stylePresetLibrary {
            let old = try StylePresetLibrary.preset(name: name, group: group)
            let renamed = try StylePresetLibrary.rename(
                name: old.name, group: old.group, to: newName ?? old.name,
                group: newGroup ?? old.group)
            return try jsonResult(["ok": true, "preset": stylePresetFields(renamed),
                                   "file": stylePresetFile(renamed) ?? NSNull()])
        }
    }

    /// Mirrors a swatch's Delete; `file` removes one library file by its
    /// listing basename — the only way to clear a broken one.
    func deleteStylePreset(_ a: [String: Any]) throws -> String {
        let hasName = stringArg(a, "name") != nil, hasFile = stringArg(a, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(message: "delete_style_preset takes exactly one of name (with group) "
                + "or file — a broken entry has no name.")
        }
        return try stylePresetLibrary {
            if hasFile {
                guard a["group"] == nil else {
                    throw ToolError(message: "delete_style_preset: file names one library file "
                        + "by itself; pass group only with name.")
                }
                let file = try PreferenceLibrary<StylePresetCodec>.validatedFileName(
                    requiredString(a, "file"))
                try StylePresetLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            let found = try StylePresetLibrary.preset(name: requiredString(a, "name"),
                                                      group: stringArg(a, "group"))
            let file = try stylePresetFile(found)
            try StylePresetLibrary.delete(name: found.name, group: found.group)
            return try jsonResult(["ok": true, "preset": stylePresetFields(found),
                                   "file": file ?? NSNull()])
        }
    }

    /// Mirrors the panel's Import Styles…: a Photoshop `.asl` into `group`
    /// (default: the file's name).
    func importStylePresets(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else {
            throw ToolError(message: "No file at \(url.path)")
        }
        return try stylePresetLibrary {
            let group = stringArg(a, "group") ?? StylePresetLibrary.defaultGroup(for: url)
            let result = try StylePresetLibrary.importASL(StylePresetLibrary.readASL(url),
                                                          group: group)
            return try jsonResult([
                "ok": true,
                "imported": result.imported.map { preset -> [String: Any] in
                    ["name": preset.name, "group": preset.group.map { $0 as Any } ?? NSNull()]
                },
                "dropped": result.dropped, "skipped": result.skipped, "file": url.path,
            ])
        }
    }

    /// Mirrors the panel's Export Styles… (every preset, built-ins included)
    /// and a group header's Export Group… (`group`, the built-in group's
    /// too): a Photoshop `.asl` written at `path`.
    func exportStylePresets(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        let group = stringArg(a, "group")
        return try stylePresetLibrary {
            let key = { (text: String) in PreferenceLibrary<StylePresetCodec>.key(text) }
            let presets = try StylePresetLibrary.all().presets.filter { preset in
                group.map { key($0) == preset.group.map(key) } ?? true
            }
            guard !presets.isEmpty else {
                throw ToolError(message: group.map { "The group “\($0)” holds no style presets." }
                    ?? "The library holds no style presets.")
            }
            let result = try StylePresetLibrary.exportASL(presets, to: url)
            return try jsonResult(["ok": true, "exported": result.exported,
                                   "skipped": result.skipped, "file": url.path])
        }
    }

    /// One preset as a listing row (without its file).
    private func stylePresetFields(_ preset: StylePreset) -> [String: Any] {
        ["name": preset.name, "group": preset.group ?? NSNull(), "builtin": preset.builtin,
         "effects": preset.effectTypes, "blending": preset.blending?.json ?? NSNull()]
    }

    private func stylePresetFile(_ preset: StylePreset) throws -> String? {
        try StylePresetLibrary.listing().presets.first {
            !$0.preset.builtin && $0.preset.key == preset.key
        }?.file
    }

    /// A library refusal becomes an in-band `ToolError` with its own sentence.
    private func stylePresetLibrary<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
