import AppKit

// The four proof preset library tools are `notAStepTool` and `nonRecordable`; `save_proof_preset`
// is in `commonTools` (float refuses), the other three in `documentFreeTools`.

/// The proof-preset library over MCP.
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `listProofPresets` | View ▸ Proof Setup ▸ (the preset rows, Delete Broken File ▸) and the sheet's Preset popup |
/// | `saveProofPreset` | View ▸ Proof Setup ▸ Custom… ▸ Save… |
/// | `deleteProofPreset` | the sheet's Delete (`name`); Proof Setup ▸ Delete Broken File ▸ (`file`) |
/// | `setDefaultProofPreset` | the sheet's "Default for new windows" checkbox |
///
/// Library writes are PREFERENCES: outside document Undo and never Action
/// steps, so all four are `notAStepTool` and `nonRecordable`. Applying a
/// preset is `set_proof_setup {preset}`, which writes VIEW state only.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let proofPresetsHandlers: [String: ToolHandler] = [
        "list_proof_presets": { $0.listProofPresets },
        "save_proof_preset": { $0.saveProofPreset },
        "delete_proof_preset": { $0.deleteProofPreset },
        "set_default_proof_preset": { $0.setDefaultProofPreset },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let proofPresetsToolFacts: [String: ToolFacts] = [
        // The proof-preset library's writes are preferences and its
        // list only reads.
        // The proof-preset library tools that resolve no document.
        "list_proof_presets": [.nonRecordable, .documentFree],
        // Saving a proof preset reads the target window's setup; the
        // other three proof-preset tools read no document (`documentFreeTools`).
        "save_proof_preset": [.nonRecordable, .commonTool],
        "delete_proof_preset": [.nonRecordable, .documentFree],
        "set_default_proof_preset": [.nonRecordable, .documentFree],
    ]

    /// The `preset` argument `set_proof_setup` and `render_proof` take,
    /// resolved into a setup. nil without `preset`; refused beside `profile`,
    /// because a half-overridden preset is a request nobody wrote.
    func proofPresetSetup(_ a: [String: Any]) throws -> ProofSetup? {
        guard let name = stringArg(a, "preset") else { return nil }
        guard a["profile"] == nil else {
            throw ToolError(
                message: "Pass either preset or profile, not both — a preset already names "
                    + "its profile.")
        }
        return try proofLibrary { try ProofPresetLibrary.named(name).resolvedSetup() }
    }

    /// Mirrors the sheet's Preset popup and the View ▸ Proof Setup submenu.
    func listProofPresets(_ a: [String: Any]) throws -> String {
        try proofLibrary {
            let listing = try ProofPresetLibrary.listing()
            let presets: [[String: Any]] = listing.entries.map { entry in
                var row = entry.item.json
                row["file"] = entry.file
                do {
                    _ = try entry.item.resolvedSetup()
                    row["available"] = true
                    row["unavailable_reason"] = NSNull()
                } catch {
                    row["available"] = false
                    row["unavailable_reason"] = error.localizedDescription
                }
                return row
            }
            return try jsonResult([
                "presets": presets,
                "broken": listing.broken.map(\.json),
                "default_preset": try ProofPresetLibrary.defaultPresetName() as Any? ?? NSNull(),
                "maximum_presets": ProofPresetLibrary.maximumPresets,
                "maximum_file_bytes": ProofPresetLibrary.maximumFileBytes,
            ])
        }
    }

    /// Mirrors View ▸ Proof Setup ▸ Custom… ▸ Save…: the window's CURRENT
    /// setup under a name.
    func saveProofPreset(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        let document = try target(a)
        guard let editorVC = editor(document) else {
            throw ToolError(message: "That document has no editor window.")
        }
        guard let setup = editorVC.proofDisplay.setup else {
            throw ToolError(
                message: "This window has no proof setup to save. Call set_proof_setup with a "
                    + "profile first.")
        }
        return try proofLibrary {
            let preset = try ProofPreset(name: name, setup: setup)
            let saved = try ProofPresetLibrary.save(preset, replace: boolArg(a, "replace") ?? false)
            var reply: [String: Any] = ["ok": true, "preset": saved.json]
            if let file = try ProofPresetLibrary.listing().entries
                .first(where: { $0.item.name == saved.name })?.file {
                reply["file"] = file
            }
            if !saved.profileIsInstalled {
                reply["warning"] = "This profile is not installed; the preset will stop working "
                    + "if the file moves."
            }
            return try jsonResult(reply)
        }
    }

    /// Mirrors the sheet's Delete (`name`) and Proof Setup ▸ Delete Broken
    /// File ▸ (`file`). Deleting the default clears it.
    func deleteProofPreset(_ a: [String: Any]) throws -> String {
        let hasName = stringArg(a, "name") != nil, hasFile = stringArg(a, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(
                message: "delete_proof_preset takes exactly one of name or file — a broken "
                    + "entry has no name.")
        }
        return try proofLibrary {
            var reply: [String: Any] = ["ok": true]
            if hasFile {
                let file = try ProofPresetLibrary.validatedFileName(requiredString(a, "file"))
                try ProofPresetLibrary.delete(file: file)
                reply["file"] = file
            } else {
                let preset = try ProofPresetLibrary.named(requiredString(a, "name"))
                let file = try ProofPresetLibrary.listing().entries
                    .first { $0.item.name == preset.name }?.file
                try ProofPresetLibrary.delete(name: preset.name)
                if let file { reply["file"] = file }
            }
            reply["default_preset"] = try ProofPresetLibrary.defaultPresetName() as Any? ?? NSNull()
            return try jsonResult(reply)
        }
    }

    /// Mirrors the sheet's "Default for new windows": null clears it.
    func setDefaultProofPreset(_ a: [String: Any]) throws -> String {
        if let raw = a["name"], !(raw is NSNull), !(raw is String) {
            throw ToolError(message: "name must be a preset name or null.")
        }
        let name = a["name"] is NSNull ? nil : stringArg(a, "name")
        return try proofLibrary {
            try ProofPresetLibrary.setDefault(name: name)
            return try jsonResult([
                "ok": true,
                "default_preset": try ProofPresetLibrary.defaultPresetName() as Any? ?? NSNull(),
            ])
        }
    }

    /// A library refusal becomes an in-band `ToolError` with its own sentence.
    private func proofLibrary<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    @inline(never) func proofPresetCatalog() -> [[String: Any]] {
        let name: [String: Any] = [
            "type": "string", "description": "Proof preset name, 1–120 characters.",
        ]
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document's window; defaults to the frontmost.",
        ]
        return [
            tool("list_proof_presets",
                 "View > Proof Setup > (the saved presets): list the bounded versioned "
                 + "proof-preset library. Each preset reports its fields, its library `file`, "
                 + "and `available` — whether its profile can be read back now (the saved path "
                 + "with the saved sha256, else the installed profile of the same name with "
                 + "it) — with `unavailable_reason` when it cannot. A file that is not a "
                 + "readable preset is a `broken` row — {file, code, reason, repairable} with "
                 + "code one of unreadable, oversize, not_json, unknown_version, "
                 + "invalid_schema, duplicate_name — instead of refusing the whole library; "
                 + "repairable is always false here. default_preset is the preset a window "
                 + "with no proof setup uses (⌘Y, ⇧⌘Y, set_proof_setup and render_proof with "
                 + "no profile), or null.", [:]),
            tool("save_proof_preset",
                 "View > Proof Setup > Custom… > Save…: store this window's CURRENT proof "
                 + "setup — the profile's name, path and sha256 (never its bytes), the intent, "
                 + "black point compensation, the two simulate flags and the warning colour — "
                 + "under a name. Proof Colors and Gamut Warning are not part of a preset. "
                 + "warning is present when the profile is not installed, because the preset "
                 + "stops working if that file moves. A library write is a preference: no "
                 + "Undo step, no Action step. A 32-bit float document has no soft proof and "
                 + "refuses.",
                 ["name": name, "replace": ["type": "boolean", "default": false],
                  "document_id": docID],
                 required: ["name"]),
            tool("delete_proof_preset",
                 "View > Proof Setup > Custom… > Delete (a listed preset) or View > Proof "
                 + "Setup > Delete Broken File (a broken file): remove ONE library file. Pass "
                 + "exactly one of name (a listed preset) or file (any library file, including "
                 + "a broken one that has no name); passing both or neither is refused. "
                 + "Deleting the default preset clears the default.",
                 ["name": name, "file": libraryFileProperty("list_proof_presets")]),
            tool("set_default_proof_preset",
                 "View > Proof Setup > Custom… > Default for new windows: choose the preset a "
                 + "window with no proof setup proofs through, or null to clear it. A library "
                 + "write is a preference: no Undo step, no Action step.",
                 ["name": ["type": ["string", "null"],
                           "description": "Preset name, or null to clear the default."]]),
        ]
    }
}
