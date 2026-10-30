import AppKit

/// `new_document` (File ▸ New…) and the bounded document-preset library
/// behind it. Creating a document is `open_document`'s twin: it is
/// `nonRecordable` for the generic recorder for the same reason (a replay
/// would pop a second window beside the run's real target), and the three
/// library tools are preferences — outside Undo, never Action steps.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let newDocumentHandlers: [String: ToolHandler] = [
        "new_document": { $0.newDocument },
        "list_document_presets": { $0.listDocumentPresets },
        "save_document_preset": { $0.saveDocumentPreset },
        "delete_document_preset": { $0.deleteDocumentPreset },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let newDocumentToolFacts: [String: ToolFacts] = [
        // Phase 12C: `new_document` CREATES a document (open_document's
        // reason — a replay would pop a second window beside the run's
        // target), `find_layers` only reads, and the three document-preset
        // tools write a preference library. `set_layer_label` IS a step.
        // None of these handlers resolves a document either — New
        // Document, the clipboard read and New from Clipboard, the action
        // library and recorder, and the batch and bundle tools (a batch opens
        // each file at its own depth and plays through this same gate).
        // Phase 12B's batch and bundle tools read no document, so they are
        // in `documentFreeTools`. `reorder_layers` is a TYPED tool
        // since 12E-1 (`typedTools` below): the core owns the typed reorder.
        // Phase 12C: a colour LABEL is metadata with a typed implementation
        // of its own (the `layer_label` edit), and `find_layers` only reads —
        // neither touches pixel storage. `new_document` and the three
        // document-preset tools read no document, so they are in
        // `documentFreeTools`.
        // Phase 12C: a label is metadata — a float document admits
        // `layer_name` for the same reason — and `find_layers` only reads.
        // `new_document` and the document-preset library read no document,
        // so they are in `DocumentPrecision.documentFreeTools`.
        "new_document": [.nonRecordable, .documentFree],
        // None of these handlers resolves a document.
        "list_document_presets": [.nonRecordable, .documentFree],
        "save_document_preset": [.nonRecordable, .documentFree],
        "delete_document_preset": [.nonRecordable, .documentFree],
    ]

    /// Mirrors File ▸ New…. A preset supplies every field; an explicit field
    /// beside `preset` is refused rather than silently winning, because a
    /// half-overridden preset is a request nobody wrote.
    func newDocument(_ a: [String: Any]) throws -> String {
        let request = try newDocumentRequest(a)
        let document: ImageDocument
        do {
            document = try ImageDocument.makeNew(request)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        var reply = request.json
        reply["ok"] = true
        reply["document"] = summary(document)
        return try jsonResult(reply)
    }

    private func newDocumentRequest(_ a: [String: Any]) throws -> NewDocumentRequest {
        let fields = ["width", "height", "unit", "ppi", "profile", "depth", "background", "color_mode"]
        if let presetName = stringArg(a, "preset") {
            guard fields.allSatisfy({ a[$0] == nil }) else {
                throw ToolError(
                    message: "Pass either `preset` or the explicit fields, not both — a "
                        + "half-overridden preset is a request nobody wrote.")
            }
            do {
                let preset = try NewDocumentPresetLibrary.named(presetName)
                let request = preset.request
                return try NewDocumentRequest(
                    name: stringArg(a, "name"), width: request.width, height: request.height,
                    ppi: request.ppi, profile: request.profile, depth: request.depth,
                    background: request.background, colorMode: request.colorMode)
            } catch let error as ToolError {
                throw error
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
        }
        do {
            let unit = try newDocumentUnit(a)
            let ppi = doubleArg(a, "ppi") ?? 300
            guard let width = doubleArg(a, "width"), let height = doubleArg(a, "height") else {
                throw ToolError(message: "new_document needs width and height.")
            }
            let mode = try newDocumentColorMode(a)
            return try NewDocumentRequest(
                name: stringArg(a, "name"),
                width: NewDocumentRequest.pixels(width, unit: unit, ppi: ppi),
                height: NewDocumentRequest.pixels(height, unit: unit, ppi: ppi),
                ppi: ppi,
                profile: try newDocumentProfile(a, mode: mode),
                depth: intArg(a, "depth") ?? 8,
                background: try NewDocumentBackground(name: stringArg(a, "background") ?? "transparent"),
                colorMode: mode)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// `color_mode`: rgb (the default), cmyk (#547 phase 5), gray (#547 phase 1a) or lab (#547 Lab
    /// phase 1).
    private func newDocumentColorMode(_ a: [String: Any]) throws -> NewDocumentColorMode {
        guard let name = stringArg(a, "color_mode") else { return .rgb }
        guard let mode = NewDocumentColorMode(rawValue: name) else {
            throw ToolError(message: "color_mode must be rgb, cmyk, gray or lab.")
        }
        return mode
    }

    /// `profile` for `mode`: srgb by default for RGB, the working CMYK for CMYK, the working Gray
    /// for Grayscale.
    private func newDocumentProfile(_ a: [String: Any], mode: NewDocumentColorMode) throws -> NewDocumentProfile {
        switch mode {
        case .cmyk: return try NewDocumentProfile.cmyk(name: stringArg(a, "profile") ?? "working")
        case .gray: return try NewDocumentProfile.gray(name: stringArg(a, "profile") ?? "working")
        case .rgb: return try NewDocumentProfile(name: stringArg(a, "profile") ?? "srgb")
        case .lab: return try NewDocumentProfile.lab(name: stringArg(a, "profile") ?? "lab")
        }
    }

    private func newDocumentUnit(_ a: [String: Any]) throws -> NewDocumentUnit {
        guard let name = stringArg(a, "unit") else { return .px }
        guard let unit = NewDocumentUnit(rawValue: name) else {
            throw ToolError(
                message: "unit must be "
                    + NewDocumentUnit.allCases.map(\.rawValue).joined(separator: ", ") + ".")
        }
        return unit
    }

    func listDocumentPresets(_ a: [String: Any]) throws -> String {
        do {
            let listing = try NewDocumentPresetLibrary.listing()
            return try jsonResult([
                "presets": listing.entries.map { $0.item.json },
                "broken": listing.broken.map(\.json),
                "maximum_presets": NewDocumentPresetLibrary.maximumPresets,
                "maximum_file_bytes": NewDocumentPresetLibrary.maximumFileBytes,
            ])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    func saveDocumentPreset(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        do {
            let unit = try newDocumentUnit(a)
            let ppi = doubleArg(a, "ppi") ?? 300
            guard let width = doubleArg(a, "width"), let height = doubleArg(a, "height") else {
                throw ToolError(message: "save_document_preset needs width and height.")
            }
            let mode = try newDocumentColorMode(a)
            let request = try NewDocumentRequest(
                width: NewDocumentRequest.pixels(width, unit: unit, ppi: ppi),
                height: NewDocumentRequest.pixels(height, unit: unit, ppi: ppi),
                ppi: ppi,
                profile: try newDocumentProfile(a, mode: mode),
                depth: intArg(a, "depth") ?? 8,
                background: try NewDocumentBackground(name: stringArg(a, "background") ?? "transparent"),
                colorMode: mode)
            let saved = try NewDocumentPresetLibrary.save(
                NewDocumentPreset(name: name, request: request, unit: unit),
                replace: boolArg(a, "replace") ?? false)
            return try jsonResult(["ok": true, "preset": saved.json])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    func deleteDocumentPreset(_ a: [String: Any]) throws -> String {
        let hasName = stringArg(a, "name") != nil, hasFile = stringArg(a, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(message: "delete_document_preset takes exactly one of name or file — a broken entry has no name.")
        }
        do {
            if hasFile {
                let file = try NewDocumentPresetLibrary.validatedFileName(requiredString(a, "file"))
                try NewDocumentPresetLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            let preset = try NewDocumentPresetLibrary.named(requiredString(a, "name"))
            let file = try NewDocumentPresetLibrary.listing().entries
                .first { $0.item.name == preset.name }?.file
            try NewDocumentPresetLibrary.delete(name: preset.name)
            var reply: [String: Any] = ["ok": true]
            if let file { reply["file"] = file }
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    @inline(never) func newDocumentCatalog() -> [[String: Any]] {
        let name: [String: Any] = ["type": "string", "description": "Preset display name, 1–120 characters."]
        let size: [String: Any] = [
            "type": "number",
            "description": "A length in `unit`; rounded to whole pixels, 1 to 20000 per side.",
        ]
        let unit: [String: Any] = [
            "type": "string", "enum": NewDocumentUnit.allCases.map(\.rawValue), "default": "px",
        ]
        let ppi: [String: Any] = [
            "type": "number", "default": 300,
            "description": "Print resolution, 1 to 30000. Pixels never change with it — only the print size does.",
        ]
        let profile: [String: Any] = [
            "type": "string", "default": "srgb",
            "description": "srgb, display_p3, or the path of an RGB ICC profile file; with "
                + "color_mode cmyk, working (the default: Edit ▸ Color Settings' working CMYK) or "
                + "the path of a CMYK ICC profile; with color_mode gray, working (the default: Edit "
                + "▸ Color Settings' working Gray), a Gray space (dot_gain_10, dot_gain_15, "
                + "dot_gain_20, dot_gain_25, dot_gain_30, gray_gamma_18, gray_gamma_22, sgray) or "
                + "the path of a Gray ICC profile; with color_mode lab, lab (the default and only "
                + "value). A GRAY profile is refused by name for rgb.",
        ]
        let colorMode: [String: Any] = [
            "type": "string", "enum": ["rgb", "cmyk", "gray", "lab"], "default": "rgb",
            "description": "File > New's Color Mode. cmyk (#547) makes a CMYK document of ink at "
                + "depth 8 or 16: white is bare paper, transparent alpha 0, black or a #rrggbb "
                + "colour separated through the document's CMYK profile. gray (#547 phase 1a) "
                + "makes a Grayscale document at depth 8, 16 or 32: the background's luminance in "
                + "the Gray profile. lab (#547) makes a Lab Color document (D50) at depth 8 or "
                + "16: the background converted from sRGB, relative colorimetric.",
        ]
        let depth: [String: Any] = [
            "type": "integer", "enum": [8, 16, 32], "default": 8,
            "description": "8, 16 or 32. 32 is a 32-bit float document holding linear light: the "
                + "16-bit fill converted as convert_precision rgba32 converts it, so white is "
                + "exactly 1.0, black and transparent 0, and a #rrggbb colour its value decoded "
                + "through the profile's tone curve.",
        ]
        let background: [String: Any] = [
            "type": "string", "default": "transparent",
            "description": "transparent, white, black or a #rrggbb colour. A transparent document's "
                + "one layer is named \"Layer 1\"; a filled one's is \"Background\".",
        ]
        return [
            tool("new_document",
                 "File > New…: start a document with no seed image — dimensions in px/in/cm/mm at a "
                 + "PPI, a colour profile, 8, 16 or 32 (float) bits, and a background fill. Pass `preset` to use "
                 + "a saved document preset instead of the explicit fields (mixing the two is "
                 + "refused). Creating a document is not recorded as an Action step, for "
                 + "open_document's reason.",
                 ["width": size, "height": size, "unit": unit, "ppi": ppi, "profile": profile,
                  "depth": depth, "background": background, "color_mode": colorMode,
                  "name": ["type": "string", "description": "The window title; omit for Untitled."],
                  "preset": ["type": "string", "description": "A saved document preset's name."]]),
            tool("list_document_presets",
                 "File > New… > Preset: list the bounded versioned document-preset library. A file "
                 + "that is not a readable preset is reported as a `broken` row — {file, code, "
                 + "reason, repairable} with code one of unreadable, oversize, not_json, "
                 + "unknown_version, invalid_schema, duplicate_name — instead of refusing the whole "
                 + "library; repair is RAW-only, so repairable is always false here.", [:]),
            tool("save_document_preset",
                 "File > New… > Save Preset…: store a size, unit, PPI, profile, depth and background "
                 + "under a name. A library write is a preference: no Undo step, no Action step.",
                 ["name": name, "width": size, "height": size, "unit": unit, "ppi": ppi,
                  "profile": profile, "depth": depth, "background": background,
                  "color_mode": colorMode,
                  "replace": ["type": "boolean", "default": false]],
                 required: ["name", "width", "height"]),
            tool("delete_document_preset",
                 "File > New… > Delete Preset (a listed preset) or Delete File (a broken file): remove "
                 + "ONE library file. Pass exactly one of name "
                 + "(a listed preset) or file (any library file, including a broken one that has no "
                 + "name); passing both or neither is refused.",
                 ["name": name, "file": libraryFileProperty("list_document_presets")]),
        ]
    }
}
