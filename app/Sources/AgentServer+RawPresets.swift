import AppKit
import CoreFoundation

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let rawPresetsHandlers: [String: ToolHandler] = [
        "list_raw_presets": listRawPresets,
        "save_raw_preset": saveRawPreset,
        "delete_raw_preset": deleteRawPreset,
        "repair_raw_preset": repairRawPreset,
        "apply_raw_preset": applyRawPreset,
        "apply_raw_settings": applyRawSettings,
        "sync_raw_settings": syncRawSettings,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let rawPresetsToolFacts: [String: ToolFacts] = [
        "list_raw_presets": [.nonRecordable, .documentFree],
        "save_raw_preset": [.nonRecordable, .commonTool, .floatTool],
        "delete_raw_preset": [.nonRecordable, .documentFree],
        "repair_raw_preset": [.nonRecordable, .documentFree],
        "apply_raw_preset": [.nonRecordable, .commonTool, .floatTool],
        "apply_raw_settings": [.nonRecordable, .commonTool, .floatTool],
        "sync_raw_settings": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// The encoding is the layer's depth choice, made in Develop ▸
    /// Document depth or Layer ▸ Redevelop RAW… ▸ Development: presets and
    /// Sync never transfer it (`RawSettingsPatch.keys`), so a request that
    /// names it is refused before any other work.
    private func refuseRawEncoding(_ arguments: [String: Any]) throws {
        guard let raw = arguments["raw"] as? [String: Any],
              let value = raw["output_encoding"], !(value is NSNull) else { return }
        throw ToolError(message: "raw.output_encoding is not a preset or Sync control; set it "
            + "with redevelop_raw.")
    }

    /// Mirrors Develop > Presets and Layer > RAW Presets, including the
    /// manager sheet's list of files that are not presets: one bad file is a
    /// `broken` row, never a refusal of the whole library.
    func listRawPresets(_ arguments: [String: Any]) throws -> String {
        let listing = try RawPresetLibrary.listing()
        return try jsonResult(["presets": listing.entries.map(\.json),
                               "broken": listing.broken.map(\.json),
                               "maximum_presets": RawPresetLibrary.maximumPresets,
                               "maximum_file_bytes": RawPresetLibrary.maximumFileBytes])
    }
    /// Mirrors Save Preset's explicit control selection, or accepts a sparse
    /// authored raw patch. Presets never store a camera/decoder recipe.
    func saveRawPreset(_ arguments: [String: Any]) throws -> String {
        try refuseRawEncoding(arguments)
        let settings: RawDevelopSettings
        if arguments["raw"] != nil {
            guard arguments["fields"] == nil, let request = try rawOpenSettings(arguments) else {
                throw ToolError(message: "Provide either raw settings or source fields, not both.")
            }
            settings = try RawSettingsPatch.validated(request)
        } else {
            let document = try rawPresetDocument(arguments)
            settings = try RawSettingsSynchronization.settings(from: .init(document: document,
                layer: rawPresetLayer(arguments, document: document)),
                fields: RawSettingsField.parse(arguments["fields"]))
        }
        let preset = try RawPresetLibrary.save(name: requiredString(arguments, "name"), settings: settings,
                                               replace: boolArg(arguments, "replace") ?? false)
        return try jsonResult(["ok": true, "preset": preset.json])
    }
    /// Mirrors Layer > RAW Presets > Delete and the manager sheet's Delete
    /// File. Library writes are not Actions. A broken file has no name to ask
    /// for, which is why `file` is the second spelling.
    func deleteRawPreset(_ arguments: [String: Any]) throws -> String {
        let hasName = stringArg(arguments, "name") != nil, hasFile = stringArg(arguments, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(message: "delete_raw_preset takes exactly one of name or file — a broken entry has no name.")
        }
        if hasFile {
            let file = try RawPresetLibrary.validatedFileName(requiredString(arguments, "file"))
            try RawPresetLibrary.delete(file: file)
            return try jsonResult(["ok": true, "file": file])
        }
        let preset = try RawPresetLibrary.named(requiredString(arguments, "name"))
        let file = try RawPresetLibrary.listing().entries.first { $0.preset.name == preset.name }?.file
        try RawPresetLibrary.delete(name: preset.name)
        var reply: [String: Any] = ["ok": true]
        if let file { reply["file"] = file }
        return try jsonResult(reply)
    }
    /// Mirrors the RAW Presets manager sheet's Repair button: rewrite ONE bad
    /// file as a canonical schema-4 preset, naming every control dropped.
    func repairRawPreset(_ arguments: [String: Any]) throws -> String {
        let file = try RawPresetLibrary.validatedFileName(requiredString(arguments, "file"))
        let repaired = try RawPresetLibrary.repair(file: file)
        return try jsonResult(["ok": true, "file": repaired.entry.file,
                               "preset": repaired.entry.json, "dropped": repaired.dropped])
    }
    /// Mirrors applying a saved preset. The central edit records resolved
    /// apply_raw_settings, so mutable library names never control Action replay.
    func applyRawPreset(_ arguments: [String: Any]) throws -> String {
        let document = try rawPresetDocument(arguments)
        let preset = try RawPresetLibrary.named(requiredString(arguments, "name"))
        let result = try RawSettingsSynchronization.apply(settings: preset.settings,
            targets: rawSettingsTargets(arguments, document: document), title: "Apply RAW Preset")
        return try jsonResult(["ok": true, "result": result.json, "document": summary(document)])
    }
    /// Mirrors the resolved control application used by presets/synchronization.
    /// This canonical document-local tool is also their stable Action step. `locals` /
    /// `mask_paths` / `source_size` replay Layer > Synchronize RAW Settings' Masking.
    func applyRawSettings(_ arguments: [String: Any]) throws -> String {
        try refuseRawEncoding(arguments)
        let document = try rawPresetDocument(arguments)
        let locals: RawLocalSyncPayload?
        if let value = arguments["locals"], !(value is NSNull) {
            locals = try RawLocalSyncPayload.parse(locals: value, maskPaths: arguments["mask_paths"],
                                                   sourceSize: arguments["source_size"])
        } else {
            guard arguments["mask_paths"] == nil, arguments["source_size"] == nil else {
                throw ToolError(message: "mask_paths and source_size go with locals.")
            }
            locals = nil
        }
        let settings: RawDevelopSettings
        if let value = try rawOpenSettings(arguments) {
            settings = value
        } else if locals != nil {
            settings = RawDevelopSettings()
        } else {
            throw ToolError(message: "raw settings are required.")
        }
        let result = try RawSettingsSynchronization.apply(settings: settings, locals: locals,
            targets: rawSettingsTargets(arguments, document: document))
        return try jsonResult(["ok": true, "result": result.json, "document": summary(document)])
    }
    /// Mirrors Layer > Synchronize RAW Settings, its Masking checkbox included (`locals`).
    /// Explicit document/layer targets are resolved before work; all prepared results precede
    /// any publication.
    func syncRawSettings(_ arguments: [String: Any]) throws -> String {
        let source = try rawPresetDocument(arguments)
        let sourceCapture = try ImageSourceCapture(source)
        let withLocals = boolArg(arguments, "locals") ?? false
        let fields = try arguments["fields"].map { try RawSettingsField.parse($0) }
        guard fields != nil || withLocals else { throw ToolError(message: "Choose fields, locals or both.") }
        let layer = try rawPresetLayer(arguments, document: source)
        let settings = try fields.map {
            try RawSettingsSynchronization.settings(from: .init(document: source, layer: layer), fields: $0)
        } ?? RawDevelopSettings()
        let locals = withLocals ? try RawLocalSyncPayload.capture(source: sourceCapture.source, layer: layer) : nil
        guard let objects = arguments["targets"] as? [[String: Any]], !objects.isEmpty,
              objects.count <= RawSettingsSynchronization.maximumTargets else {
            throw ToolError(message: "targets must contain 1–32 explicit document_id/layer objects.")
        }
        let targets = try objects.map { object -> RawSettingsTarget in
            guard Set(object.keys) == ["document_id", "layer"],
                  object["layer"] != nil, object["document_id"] != nil else {
                throw ToolError(message: "Each RAW target requires only document_id and a nonnegative layer.")
            }
            return RawSettingsTarget(document: try rawPresetDocument(object), layer: try rawPresetInteger(object["layer"], name: "layer"))
        }
        try sourceCapture.requireCurrent(source)
        let result = try RawSettingsSynchronization.apply(settings: settings, locals: locals, targets: targets,
                                                          title: "Synchronize RAW Settings", validateSource: {
            guard NSDocumentController.shared.documents.contains(where: { $0 === source }) else {
                throw DoctoredCoreError(message: "The RAW settings source document closed. No settings were applied.")
            }
            try sourceCapture.requireCurrent(source)
        })
        return try jsonResult(["ok": true, "result": result.json, "raw": settings.jsonObject()])
    }

    private func rawSettingsTargets(_ arguments: [String: Any], document: ImageDocument) throws -> [RawSettingsTarget] {
        if let raw = arguments["layers"] {
            guard arguments["layer"] == nil, let values = raw as? [Any], !values.isEmpty,
                  values.count <= RawSettingsSynchronization.maximumTargets else {
                throw ToolError(message: "Choose layer or 1–32 layers, not both.")
            }
            let indices = try values.map { value -> Int in
                try rawPresetInteger(value, name: "layer")
            }
            return indices.map { .init(document: document, layer: $0) }
        }
        return [.init(document: document, layer: try rawPresetLayer(arguments, document: document))]
    }

    private func rawPresetDocument(_ arguments: [String: Any]) throws -> ImageDocument {
        if let value = arguments["document_id"] {
            return try target(["document_id": rawPresetInteger(value, name: "document_id")])
        }
        return try target(arguments)
    }
    private func rawPresetLayer(_ arguments: [String: Any], document: ImageDocument) throws -> Int {
        if let value = arguments["layer"] { return try rawPresetInteger(value, name: "layer") }
        return document.activeLayerIndex
    }
    private func rawPresetInteger(_ value: Any?, name: String) throws -> Int {
        if let text = value as? String, let integer = Int(text), integer >= 0 { return integer }
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite, number.doubleValue >= 0, number.doubleValue < Double(Int.max),
              number.doubleValue.rounded(.towardZero) == number.doubleValue else {
            throw ToolError(message: "\(name) must be a nonnegative integer, not a boolean or fraction.")
        }
        return number.intValue
    }

    @inline(never) func rawPresetCatalog() -> [[String: Any]] {
        let fields: [String: Any] = ["type": "array", "items": ["type": "string", "enum": RawSettingsField.allCases.map(\.rawValue)],
            "minItems": 1, "uniqueItems": true,
            "description": "Explicit controls to copy. white_balance copies Custom temperature+tint, or requests each target's own captured As Shot balance. Unselected controls keep their saved values."]
        let name: [String: Any] = ["type": "string", "description": "Preset display name, 1–120 characters."]
        let file: [String: Any] = ["type": "string",
            "description": "A library file basename from list_raw_presets (a row's `file`). Pass exactly one of name or file; a broken entry has no name."]
        let layer: [String: Any] = ["type": "integer", "minimum": 0, "description": "RAW layer, default active."]
        let layers: [String: Any] = ["type": "array", "items": ["type": "integer", "minimum": 0], "minItems": 1, "maxItems": 32, "uniqueItems": true]
        // Synchronize RAW Settings' Masking as its recorded step replays it.
        let control: [String: Any] = ["description": "A local control as edit_raw_development's create takes it."]
        var localProperties: [String: Any] = Dictionary(uniqueKeysWithValues:
            RawLocalOperations.controlKeys.sorted().map { ($0, control as Any) })
        localProperties["id"] = ["type": "string", "description": "The local's stable id."]
        localProperties["mask"] = ["type": "object", "description": "{op: coverage, index} names the mask_paths entry holding the drawn coverage; {op: subject|people|sky} is detected again on the target.",
            "properties": ["op": ["type": "string", "enum": ["coverage"] + RawLocalGeneratedKind.allCases.map(\.rawValue)],
                           "index": ["type": "integer", "minimum": 0, "maximum": 15]], "required": ["op"]] as [String: Any]
        let locals: [String: Any] = ["type": "array", "maxItems": 16,
            "description": "Replace the target layers' locals with these, in order. Coverage is resampled per axis in normalised coordinates; feather is scaled by the geometric mean of the target/source_size axis ratios (clamped to 256); a range keeps its Lab. A generated local that finds nothing or whose model is not installed is skipped and named in result.skipped and result.note.",
            "items": ["type": "object", "properties": localProperties, "required": ["id", "mask"]]]
        let maskPaths: [String: Any] = ["type": "array", "maxItems": 16, "items": ["type": "string"],
            "description": "Absolute .drmask paths (RZLM version 1), one per coverage index, in index order."]
        let sourceSize: [String: Any] = ["type": "array", "minItems": 2, "maxItems": 2,
            "items": ["type": "integer", "minimum": 1],
            "description": "[width, height] of the frame each local's feather is stated in; required with locals."]
        return [
            tool("list_raw_presets", "Develop/Layer > RAW Presets: list bounded versioned saved user settings. Every row carries its library `file` and its on-disk `schema_version` (1, 2, 3 or 4). A file that is not a readable preset is reported as a `broken` row — {file, code, reason, repairable} with code one of unreadable, oversize, not_json, unknown_version, invalid_schema, duplicate_name — instead of refusing the whole library, so one bad file never hides the good ones or blocks Develop. Native recipes remain independent; listing never changes documents.", [:]),
            tool("save_raw_preset", "Develop > Save Preset: save explicit sparse raw settings OR copy explicit fields from a source RAW layer. Camera-specific recipe defaults, identity and decoder are never copied. As Shot refers to each receiving camera. Library writes do not enter document Undo or Actions.",
                 ["name": name, "raw": Self.rawDevelopProperty, "fields": fields, "document_id": Self.docIDProperty, "layer": layer, "replace": ["type": "boolean", "default": false]], required: ["name"]),
            tool("delete_raw_preset", "Layer > RAW Presets > Delete: delete ONE library file. Pass exactly one of name (a listed preset) or file (any library file, including a broken one that has no name); passing both or neither is refused. Saved documents and recorded Actions keep their effective settings.", ["name": name, "file": file]),
            tool("repair_raw_preset", "RAW Presets manager > Repair: rewrite ONE broken library file (list_raw_presets `broken` row with repairable: true) as a canonical schema-4 preset. The stored name is trimmed and precomposed, unknown top-level keys are dropped, and each raw control is kept only if it decodes on its own — every dropped key is named in `dropped`. The file moves to its canonical digest name. An oversize, non-JSON, unknown-version or duplicate-name file is refused naming its code; nothing is read past 8 KiB and no unknown future version is ever rewritten.",
                 ["file": file], required: ["file"]),
            tool("apply_raw_preset", "Layer > RAW Presets > Apply: apply the named preset's saved controls to one or multiple RAW layers. Every target is preflighted and prepared first; one Undo per changed document and none for no-ops. Actions record resolved apply_raw_settings, independent of future preset changes.",
                 ["name": name, "document_id": Self.docIDProperty, "layer": layer, "layers": layers], required: ["name"]),
            tool("apply_raw_settings", "Apply explicit sparse user settings to retained RAW layers in one document. Shared preset/synchronization path and their canonical Action step. Other controls, decoder identities, originals, transforms and retouch layers remain target-specific. With locals (Synchronize RAW Settings' Masking; source_size required) each target's locals are REPLACED by those, in order; raw is then optional. All results prepare before one central Undo edit; no-op requests create no history. The result carries skipped (generated locals not made, one line each) and note when any was.",
                 ["raw": Self.rawDevelopProperty, "document_id": Self.docIDProperty, "layer": layer, "layers": layers,
                  "locals": locals, "mask_paths": maskPaths, "source_size": sourceSize]),
            tool("sync_raw_settings", "Layer > Synchronize RAW Settings: copy only selected fields from a RAW source to explicit open-document/layer targets. locals: true (the Masking checkbox) REPLACES each target's local adjustments with the source's, in order, ids, names and controls kept: drawn coverage is resampled per axis in normalised coordinates of each RAW's oriented frame, feather is scaled to the target's size, a range keeps its Lab, and Subject/People/Sky are detected again on each target (brush or invert refinements on them are not carried). A generated local that finds nothing or whose model is not installed is skipped; result.skipped names each and result.note sums them. Give fields, locals: true or both. All target results and projections prepare before any publication. Each changed document gets one independent Undo; a sync that changes nothing adds none. Actions record resolved document-local apply_raw_settings without document IDs. Maximum 32 target layers.",
                 ["document_id": Self.docIDProperty, "layer": layer, "fields": fields,
                  "locals": ["type": "boolean", "default": false, "description": "Masking: replace each target's locals with the source's."],
                  "targets": ["type": "array", "minItems": 1, "maxItems": 32,
                      "items": ["type": "object", "properties": ["document_id": Self.docIDProperty, "layer": layer], "required": ["document_id", "layer"], "additionalProperties": false]]], required: ["targets"])
        ]
    }
}
