import AppKit
import CryptoKit

// `export_layers` and `contact_sheet` are Action steps (so they are `nonRecordable` for the generic
// recorder, which would otherwise add a second copy); `contact_sheet_from_sources`,
// `save_metadata_policy` and `delete_metadata_policy` are `notAStepTool`.

/// Phase 11B's MCP half: the twins of File ▸ Export Layers…, File ▸ Contact
/// Sheet… and the metadata-policy library.
///
/// None of these changes a document. The layer exports and the contact sheet
/// read the captured immutable handle and write files; the policy library is a
/// preference, exactly like the export presets beside it.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let productionOutputsHandlers: [String: ToolHandler] = [
        // Production outputs (AgentServer+ProductionOutputs.swift): the twins
        // of File > Export Layers…, File > Contact Sheet… and the
        // metadata-policy library.
        "export_layers": { $0.exportLayers },
        "contact_sheet": { $0.contactSheet },
        "contact_sheet_from_sources": { $0.contactSheetFromSources },
        "list_metadata_policies": { $0.listMetadataPolicies },
        "save_metadata_policy": { $0.saveMetadataPolicy },
        "delete_metadata_policy": { $0.deleteMetadataPolicy },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let productionOutputsToolFacts: [String: ToolFacts] = [
        // Phase 11B: `export_layers` and `contact_sheet` ARE steps, but
        // `ImageDocument.exportLayers` / `.contactSheet` record the resolved
        // step themselves, so the generic recorder must not add a second copy.
        // The other four write files or a preference library.
        // Phase 11B production outputs: a layer export renders each item at
        // the document's own depth, and a contact sheet reduces to 8-bit
        // deliberately (ContactSheet.bitDepthRefusal says so). The policy
        // library reads no document, so its three tools are in
        // `documentFreeTools`.
        // File ▸ Export Layers… — the float whitelist's `exportLayers:`
        // — exports each item canvas-framed or trimmed; the core's float trim
        // copies the box's samples exactly (an exact 32-bit TIFF by default).
        "export_layers": [.nonRecordable, .commonTool, .floatTool],
        // A float document's cells are its tone-mapped 8-bit rendering
        // (each item's 8-bit sRGB export, or the display projection of a
        // whole document), so both sheets answer at 32 bits.
        "contact_sheet": [.nonRecordable, .commonTool, .floatTool],
        "contact_sheet_from_sources": [.nonRecordable, .commonTool, .floatTool],
        // None of these handlers resolves a document.
        "list_metadata_policies": [.nonRecordable, .documentFree],
        "save_metadata_policy": [.nonRecordable, .documentFree],
        "delete_metadata_policy": [.nonRecordable, .documentFree],
    ]

    // MARK: - Layer exports

    /// Mirrors File ▸ Export Layers…: every selected (or visible, or every)
    /// top-level item as its own file. Partial results are NOT an error —
    /// `ok` is false and every item names its path or its refusal, so a model
    /// can fix the one that failed instead of re-running the whole batch.
    func exportLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        let mode = try itemsMode(a, default: .selected)
        let request: LayerExportRequest
        do {
            if let name = stringArg(a, "preset"), a["settings"] == nil {
                let preset = try ExportPresetLibrary.named(name)
                try AgentPathGuard.shared.check(
                    exportPreset: preset, argument: "export preset \"\(preset.name)\"")
                var settings = try preset.resolvedSettings()
                if let policy = stringArg(a, "metadata_policy") {
                    settings.metadata = try MetadataPolicyLibrary.named(policy).policy
                }
                request = LayerExportRequest(
                    items: mode, trimmed: boolArg(a, "trimmed") ?? false, settings: settings,
                    destination: preset.destination, pattern: preset.pattern, preset: preset.name)
            } else {
                guard var object = a["settings"] as? [String: Any] else {
                    throw ToolError(message: "export_layers needs a preset name or a settings object.")
                }
                if object["format"] == nil {
                    object["format"] = ExportSettings.defaults(for: document).format.requestName
                }
                var settings = try ExportSettings.decode(object)
                if let policy = stringArg(a, "metadata_policy") {
                    settings.metadata = try MetadataPolicyLibrary.named(policy).policy
                }
                request = LayerExportRequest(
                    items: mode, trimmed: boolArg(a, "trimmed") ?? false, settings: settings,
                    destination: try requiredString(a, "destination"),
                    pattern: try requiredString(a, "pattern"),
                    preset: stringArg(a, "preset"))
            }
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        // #1 Duotone phase 1b: a Duotone document's layers export as RGB through its inks.
        let source = document.doc
        do {
            let reply = try document.exportLayers(request).json
            return try jsonResult(DuotoneDocument.noting(reply, for: source))
        }
        catch let error as ToolError { throw error }
        catch { throw ToolError(message: error.localizedDescription) }
    }

    // MARK: - Contact sheets

    /// Mirrors File ▸ Contact Sheet… on ONE document's own items — the
    /// recordable form.
    func contactSheet(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        let mode = try itemsMode(a, default: .all)
        let layout = try sheetLayout(a)
        let output = try sheetOutput(a, document: document)
        do {
            let source = document.doc
            let outcome = try document.contactSheet(items: mode, layout: layout, output: output)
            return try jsonResult(DuotoneDocument.noting(sheetResult(outcome), for: source))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors the same sheet driven from FILES or other open documents.
    /// There is no single document to replay it against, so it is deliberately
    /// not an Action step.
    func contactSheetFromSources(_ a: [String: Any]) throws -> String {
        let layout = try sheetLayout(a)
        let source: ContactSheetSource
        var files: [URL] = []
        if let paths = a["files"] as? [Any] {
            let urls = paths.compactMap { $0 as? String }.map {
                URL(fileURLWithPath: ($0 as NSString).expandingTildeInPath)
            }
            guard urls.count == paths.count, !urls.isEmpty else {
                throw ToolError(message: "files must be a non-empty array of paths.")
            }
            files = urls
            source = .files(urls)
        } else if let ids = a["documents"] as? [Any] {
            let documents = try ids.map { value -> ImageDocument in
                guard let number = value as? NSNumber else {
                    throw ToolError(message: "documents must be an array of document ids.")
                }
                return try target(["document_id": number])
            }
            guard !documents.isEmpty else {
                throw ToolError(message: "documents must be a non-empty array of document ids.")
            }
            // The sources are read-only (no UI path commits them), so an open
            // canvas session refuses by name rather than being committed.
            if let pending = documents.first(where: { $0.hasPendingCanvasSession }) {
                throw ToolError(message: "“\(pending.displayName ?? "Untitled")” has an "
                    + "uncommitted text, shape or Free Transform edit on the canvas; commit or "
                    + "cancel it first. Nothing was written.")
            }
            source = .documents(documents)
        } else {
            throw ToolError(message: "contact_sheet_from_sources needs files or documents.")
        }
        // The 8-bit rule and the destination refusal are checked BEFORE any
        // cell is built, so a 16-bit preset costs nothing and reports the
        // sheet's own reason.
        let output = try sheetOutput(a, document: nil)
        if let refusal = ContactSheet.outputRefusal(output) {
            throw ToolError(message: refusal)
        }
        do {
            let cells = try ContactSheet.cells(source)
            let (sheet, planBytes) = try ContactSheet.compose(cells, layout: layout)
            let outcome = try ContactSheet.deliver(sheet, output: output,
                                                   baseName: "Contact Sheet", date: Date())
            var result = sheetResult(ContactSheetOutcome(
                document: outcome.document, url: outcome.url, result: outcome.result,
                cells: cells.count, width: outcome.width, height: outcome.height,
                planBytes: planBytes))
            // Each file whose core open converted from or ignored its embedded
            // profile, in file order; `files` is empty for documents.
            let refusals: [[String: Any]] = zip(files, cells).compactMap { url, cell in
                cell.profileRefusal.map { ["file": url.path, "sentence": $0] }
            }
            if !refusals.isEmpty { result["profile_refusals"] = refusals }
            return try jsonResult(result)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    // MARK: - The metadata policy library

    /// Mirrors File ▸ Export… ▸ Metadata policy ▸ Manage….
    func listMetadataPolicies(_ a: [String: Any]) throws -> String {
        do {
            let listing = try MetadataPolicyLibrary.listing()
            return try jsonResult([
                "policies": listing.entries.map { $0.item.json },
                "broken": listing.broken.map(\.json),
                "maximum_policies": MetadataPolicyLibrary.maximumPolicies,
                "maximum_file_bytes": MetadataPolicyLibrary.maximumFileBytes,
                "rules": "Each of exif, xmp, iptc and density is default|keep|drop. default defers "
                    + "to settings.strip_metadata. There is no orientation rule: the rotation is "
                    + "baked into the pixels at open, so every packet written states Orientation 1.",
                "refusals": "A stamp needs the xmp packet written; density: drop needs exif and "
                    + "xmp dropped too, because each restates the ppi.",
            ])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors the Manage… sheet's Save As….
    func saveMetadataPolicy(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        guard let object = a["policy"] as? [String: Any] else {
            throw ToolError(message: "save_metadata_policy needs a policy object.")
        }
        do {
            let saved = try MetadataPolicyLibrary.save(
                NamedMetadataPolicy(name: name, policy: try MetadataPolicy.decode(object)),
                replace: boolArg(a, "replace") ?? false)
            return try jsonResult(["ok": true, "policy": saved.json])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors the Manage… sheet's Delete.
    func deleteMetadataPolicy(_ a: [String: Any]) throws -> String {
        let hasName = stringArg(a, "name") != nil, hasFile = stringArg(a, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(message: "delete_metadata_policy takes exactly one of name or file — a broken entry has no name.")
        }
        do {
            if hasFile {
                let file = try MetadataPolicyLibrary.validatedFileName(requiredString(a, "file"))
                try MetadataPolicyLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            let policy = try MetadataPolicyLibrary.named(requiredString(a, "name"))
            let file = try MetadataPolicyLibrary.listing().entries.first { $0.item.name == policy.name }?.file
            try MetadataPolicyLibrary.delete(name: policy.name)
            var reply: [String: Any] = ["ok": true]
            if let file { reply["file"] = file }
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    // MARK: - Shared argument parsing

    private func itemsMode(_ a: [String: Any], default fallback: LayerExportItems)
        throws -> LayerExportItems {
        guard let name = stringArg(a, "items") else { return fallback }
        guard let mode = LayerExportItems(rawValue: name) else {
            throw ToolError(message: "items must be selected, visible or all.")
        }
        return mode
    }

    private func sheetLayout(_ a: [String: Any]) throws -> ContactSheetLayout {
        guard let object = a["layout"] else { return ContactSheetLayout.defaults }
        guard let object = object as? [String: Any] else {
            throw ToolError(message: "layout must be an object.")
        }
        do { return try ContactSheetLayout.decode(object) }
        catch { throw ToolError(message: error.localizedDescription) }
    }

    private func sheetOutput(_ a: [String: Any], document: ImageDocument?)
        throws -> ContactSheetOutput {
        guard let raw = a["output"] else { return .document }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: "output must be an object.")
        }
        switch object["kind"] as? String ?? "document" {
        case "document":
            return .document
        case "export":
            do {
                if let name = object["preset"] as? String, object["settings"] == nil {
                    let preset = try ExportPresetLibrary.named(name)
                    try AgentPathGuard.shared.check(
                        exportPreset: preset, argument: "export preset \"\(preset.name)\"")
                    var settings = try preset.resolvedSettings()
                    if let policy = object["metadata_policy"] as? String {
                        settings.metadata = try MetadataPolicyLibrary.named(policy).policy
                    }
                    return .export(settings: settings, destination: preset.destination,
                                   pattern: preset.pattern, preset: preset.name)
                }
                guard var fields = object["settings"] as? [String: Any] else {
                    throw ToolError(message: "An export output needs a preset name or settings.")
                }
                if fields["format"] == nil {
                    fields["format"] = document.map { ExportSettings.defaults(for: $0).format.requestName }
                        ?? "png"
                }
                var settings = try ExportSettings.decode(fields)
                if let policy = object["metadata_policy"] as? String {
                    settings.metadata = try MetadataPolicyLibrary.named(policy).policy
                }
                guard let destination = object["destination"] as? String,
                      let pattern = object["pattern"] as? String else {
                    throw ToolError(message: "An export output needs destination and pattern.")
                }
                return .export(settings: settings, destination: destination, pattern: pattern,
                               preset: object["preset"] as? String)
            } catch let error as ToolError {
                throw error
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
        default:
            throw ToolError(message: "output.kind must be document or export.")
        }
    }

    private func sheetResult(_ outcome: ContactSheetOutcome) -> [String: Any] {
        var fields: [String: Any] = [
            "ok": true,
            "cells": outcome.cells,
            "width": outcome.width,
            "height": outcome.height,
            "working_memory_plan_bytes": outcome.planBytes,
        ]
        if let document = outcome.document { fields["document"] = summary(document) }
        if let url = outcome.url { fields["path"] = url.path }
        if let result = outcome.result {
            fields["encoded_sha256"] = SHA256.hash(data: result.bytes)
                .map { String(format: "%02x", $0) }.joined()
            if let info = try? result.info(), let metadata = info["metadata"] {
                fields["metadata"] = metadata
            }
        }
        return fields
    }

    // MARK: - Catalog

    @inline(never) func productionOutputsCatalog() -> [[String: Any]] {
        let items: [String: Any] = [
            "type": "string", "enum": ["selected", "visible", "all"],
            "description": "Which TOP-LEVEL layers or groups to use. selected takes the top-level "
                + "ancestor of every selected entry.",
        ]
        let layout: [String: Any] = [
            "type": "object",
            "description":
                "Grid layout: columns 1–32, cell_width/cell_height 16–2048, gutter 0–256, "
                + "caption_height 0–128, captions (bool), background \"#rrggbb\"|\"#rrggbbaa\", "
                + "filter nearest|bilinear|catmull-rom|lanczos3. "
                + "width = gutter + columns*(cell_width+gutter); "
                + "height = gutter + rows*(cell_height+caption_height+gutter).",
        ]
        let output: [String: Any] = [
            "type": "object",
            "description":
                "{\"kind\":\"document\"} opens the sheet as a new document, or "
                + "{\"kind\":\"export\", preset | settings + destination + pattern, "
                + "metadata_policy?} writes it. A sheet is composed at 8 bits, so an export "
                + "output with bit_depth 16 is refused before any cell is built.",
        ]
        let policy: [String: Any] = [
            "type": "object",
            "description":
                "{exif|xmp|iptc|density: default|keep|drop, stamp:{author,copyright}|null}. "
                + "default defers to strip_metadata; density keep is identical to default. There "
                + "is no orientation rule.",
        ]
        return [
            // File > Export Layers…
            tool("export_layers",
                 "File > Export Layers…: deliver every selected (or visible, or every) top-level "
                 + "layer or group as its own file, each rendered in isolation against "
                 + "transparency. Give preset, or settings + destination + pattern; the pattern "
                 + "must contain {layer} or {index}. Nothing is overwritten and no folder is "
                 + "created. A partial run is NOT an error: ok is false and every item reports its "
                 + "path or its refusal. The document is unchanged.",
                 ["document_id": Self.docIDProperty, "items": items,
                  "trimmed": ["type": "boolean", "default": false,
                              "description": "Deliver the alpha bounding box instead of the "
                                  + "canvas. A fully transparent item is then refused."],
                  "preset": ["type": "string"], "settings": ["type": "object"],
                  "destination": ["type": "string"],
                  "pattern": ["type": "string",
                              "description": "Must contain {layer} or {index}. Tokens: "
                                  + LayerExport.tokenHelp],
                  "metadata_policy": ["type": "string"]]),
            // File > Contact Sheet…
            tool("contact_sheet",
                 "File > Contact Sheet…: compose this document's items into a grid, as a new "
                 + "document or straight to a file. Cells are 8-bit sRGB, never upscaled and "
                 + "centred; a transparent item shows the background. A 16-bit item is reduced "
                 + "to 8 bits and a 32-bit float item is tone-mapped by the document's tone map "
                 + "first. The document is unchanged.",
                 ["document_id": Self.docIDProperty, "items": items, "layout": layout,
                  "output": output]),
            tool("contact_sheet_from_sources",
                 "The same sheet built from FILES or other open documents. It has no single "
                 + "document to replay against, so it is not recordable as an Action step. The "
                 + "sources are opened read-only and never modified. An open 16-bit or 32-bit "
                 + "float document's cell is its 8-bit display rendering (a float one through its "
                 + "tone map). profile_refusals lists each "
                 + "file whose embedded colour profile the open converted from or ignored, and "
                 + "each untagged CMYK file, converted from the working CMYK profile.",
                 ["files": ["type": "array", "items": ["type": "string"]],
                  "documents": ["type": "array", "items": ["type": "integer"]],
                  "layout": layout, "output": output]),
            // File > Export… > Metadata policy > Manage…
            tool("list_metadata_policies",
                 "File > Export… > Metadata policy > Manage…: list the bounded versioned "
                 + "metadata-policy library. Policies live outside every document. A file that is "
                 + "not a readable policy is reported as a `broken` row — {file, code, reason, "
                 + "repairable} with code one of unreadable, oversize, not_json, unknown_version, "
                 + "invalid_schema, duplicate_name — instead of refusing the whole library; repair "
                 + "is RAW-only, so repairable is always false here.", [:]),
            tool("save_metadata_policy",
                 "File > Export… > Metadata policy > Manage… > Save As…: store a policy under a "
                 + "name. A library write is a preference: no Undo step, no Action step, and no "
                 + "document changes because a policy changed.",
                 ["name": ["type": "string"], "policy": policy,
                  "replace": ["type": "boolean", "default": false]],
                 required: ["name", "policy"]),
            tool("delete_metadata_policy",
                 "File > Export… > Metadata policy > Manage… > Delete: remove ONE library file. "
                 + "Pass exactly one of name (a listed policy) or file (any library file, including "
                 + "a broken one that has no name); passing both or neither is refused. Recorded "
                 + "steps keep their resolved policy.",
                 ["name": ["type": "string"],
                  "file": libraryFileProperty("list_metadata_policies")]),
        ]
    }
}
