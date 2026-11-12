import AppKit

/// The six LAYER COMP tools — the MCP mirror of Window ▸ Layer Comps…
/// (`LayerCompsWindowController`).
///
/// Comps are addressed by NAME on this side everywhere, for `selectLayer`'s
/// reason: an index means nothing on another document while "the comp called
/// Hero" is exactly what the user pointed at, and the core refuses a duplicate
/// name so the handle is unique. The NAME is resolved to the core's comp index
/// here, immediately before the edit.
///
/// Every mutating handler goes through `editStructure` — never through
/// `ImageDocument`'s own entry points, which are the UI's: an agent edit has
/// to open its own undo group (`AgentServer.performGroupedEdit`), and the two
/// share `RasterDocument.editingLayerComps(_:)`, the one writer, instead.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let layerCompsHandlers: [String: ToolHandler] = [
        "list_layer_comps": { $0.listLayerComps },
        "save_layer_comp": { $0.saveLayerComp },
        "apply_layer_comp": { $0.applyLayerComp },
        "update_layer_comp": { $0.updateLayerComp },
        "delete_layer_comp": { $0.deleteLayerComp },
        "reorder_layer_comp": { $0.reorderLayerComp },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let layerCompsToolFacts: [String: ToolFacts] = [
        // Phase 12D-1: `list_layer_comps` only READS, beside the other
        // `get_*`/`list_*` tools above. The other five ARE steps — a comp is
        // per-document, undoable and `.rz`-persisted — and they address a
        // comp by NAME, so a recording replays on another document.
        // Phase 12D-1: a layer COMP records per-layer visibility, position
        // and appearance by identity — no pixel storage is touched — and the
        // typed depth has an implementation of its own (the core's
        // `layer_comp` edit), so both depths answer all six.
        // Phase 12D-1: a layer comp records per-layer state, never pixels —
        // a float document admits `layer_name` and `layer_visible` for the
        // same reason. A recorded STYLE is skipped here, because a float
        // document carries none (`precision_comps`).
        // #1: a CMYK document answers all six too (the core's `layer_comp`
        // family on ink, `cmyk_comps`), position included.
        "list_layer_comps": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "save_layer_comp": [.commonTool, .floatTool, .cmykTool],
        "apply_layer_comp": [.commonTool, .floatTool, .cmykTool],
        "update_layer_comp": [.commonTool, .floatTool, .cmykTool],
        "delete_layer_comp": [.commonTool, .floatTool, .cmykTool],
        "reorder_layer_comp": [.commonTool, .floatTool, .cmykTool],
    ]

    /// Mirrors the Layer Comps window's list. A READ: it changes no document.
    func listLayerComps(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let comps = try compList(document)
        return try jsonResult(["comps": comps.map(\.json), "count": comps.count])
    }

    /// Mirrors the window's New… button.
    func saveLayerComp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let name = try requiredString(a, "name")
        var request: [String: Any] = [
            "action": "save", "name": name, "comment": stringArg(a, "comment") ?? "",
        ]
        request.merge(captures(a).arguments) { _, new in new }
        guard try editComps(document, "Save Layer Comp", request) else {
            throw ToolError(message: "The layer comp could not be saved.")
        }
        // The core TRIMS a comp name before it stores it, so read the new comp
        // back under the trimmed form rather than the caller's raw string.
        let savedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let saved = try compList(document).first(where: { $0.name == savedName }) else {
            throw ToolError(message: "The layer comp could not be saved.")
        }
        return try jsonResult([
            "ok": true, "changed": true, "comp": saved.json, "document": summary(document),
        ])
    }

    /// Mirrors the window's Apply button (and its ◀ ▶, which step the same
    /// list). The report is built from the comp read BEFORE the edit, through
    /// the same `applyReport(for:changed:)` the UI path uses.
    func applyLayerComp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let comp = try compNamed(a, document)
        let request: [String: Any] = ["action": "apply", "comp": comp.index]
        let changed = try editComps(document, "Apply Layer Comp", request)
        // The document now holds this comp's state whether or not an undo step
        // was opened — the same bookkeeping the UI path does
        // (`ImageDocument.applyLayerComp(named:)`), so the window's ◀ ▶ step
        // from the document's actual arrangement after an agent apply.
        document.lastAppliedLayerComp = comp.name
        let report = LayerCompApplyReport.applyReport(for: comp, changed: changed)
        var fields = report.fields
        fields["ok"] = true
        fields["name"] = comp.name
        fields["document"] = summary(document)
        if !changed {
            fields["note"] = "Nothing changed: the document already held that comp's state. "
                + "No undo step was added."
        }
        return try jsonResult(fields)
    }

    /// Mirrors the window's Update and Options… buttons.
    func updateLayerComp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let comp = try compNamed(a, document)
        var request: [String: Any] = [
            "action": "update", "comp": comp.index, "recapture": boolArg(a, "recapture") ?? true,
        ]
        if let newName = stringArg(a, "new_name") { request["name"] = newName }
        if let comment = stringArg(a, "comment") { request["comment"] = comment }
        if a["visibility"] != nil || a["position"] != nil || a["appearance"] != nil {
            request.merge(captures(a).arguments) { _, new in new }
        }
        guard try editComps(document, "Update Layer Comp", request) else {
            return try structureNoOp(
                ["name": comp.name, "document": summary(document)],
                why: "that layer comp already holds those values.")
        }
        // As on save: the core trims a new name before it stores it.
        let name = (stringArg(a, "new_name") ?? comp.name)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let updated = try compList(document).first(where: { $0.name == name }) else {
            throw ToolError(message: "The layer comp could not be updated.")
        }
        return try jsonResult([
            "ok": true, "changed": true, "comp": updated.json, "document": summary(document),
        ])
    }

    /// Mirrors the window's Delete button.
    func deleteLayerComp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let comp = try compNamed(a, document)
        let request: [String: Any] = ["action": "delete", "comp": comp.index]
        guard try editComps(document, "Delete Layer Comp", request) else {
            throw ToolError(message: "The layer comp could not be deleted.")
        }
        return try jsonResult([
            "ok": true, "changed": true, "name": comp.name, "document": summary(document),
        ])
    }

    /// Mirrors the window's drag-reorder (a row dragged to a new place in the
    /// list). `to` is the FINAL index, counted with the comp taken out.
    func reorderLayerComp(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let comp = try compNamed(a, document)
        guard let to = intArg(a, "to") else {
            throw ToolError(message: "Missing required argument: to")
        }
        // The one host guard: a negative `to` would reach the core as serde's
        // `usize` error, so it earns the core's own destination sentence here.
        guard to >= 0 else {
            let count = try compList(document).count
            throw ToolError(
                message: "layer comp destination index \(to) is out of range (0..\(count - 1))")
        }
        let request: [String: Any] = ["action": "reorder", "comp": comp.index, "to": to]
        let changed = try editComps(document, "Reorder Layer Comp", request)
        let order = try compList(document).map(\.name)
        let fields: [String: Any] = [
            "name": comp.name, "index": to, "order": order, "document": summary(document),
        ]
        guard changed else {
            return try structureNoOp(fields, why: "that layer comp is already at index \(to).")
        }
        return try jsonResult(fields.merging(["ok": true, "changed": true]) { _, new in new })
    }

    /// One comp edit through `editStructure` — the `setLayerLabel` precedent
    /// — with the CORE's refusal sentence carried out of the closure and
    /// thrown in band, rather than collapsed into `editStructure`'s false.
    /// False means the op was an exact no-op.
    private func editComps(
        _ document: ImageDocument, _ actionName: String, _ request: [String: Any]
    ) throws -> Bool {
        var refusal: String?
        let changed = try editStructure(document, actionName) { current in
            do {
                return try current.editingLayerComps(request)
            } catch {
                refusal = error.localizedDescription
                return nil
            }
        }
        if let refusal { throw ToolError(message: refusal) }
        return changed
    }

    /// The document's comps, with the core's own refusal handed back in band.
    private func compList(_ document: ImageDocument) throws -> [LayerComp] {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image.") }
        do {
            return try doc.layerComps()
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// The comp a call names, or a refusal that lists what there is.
    private func compNamed(_ a: [String: Any], _ document: ImageDocument) throws -> LayerComp {
        // Trimmed like `save_layer_comp` stores it, so a padded name finds
        // the comp the list shows instead of being refused.
        let name = try requiredString(a, "name")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let comps = try compList(document)
        guard let comp = comps.first(where: { $0.name == name }) else {
            let known = comps.map(\.name)
            throw ToolError(
                message: "No layer comp is called \"\(name)\" — "
                    + (known.isEmpty
                        ? "this document has none. Call save_layer_comp first."
                        : "this document has \(known.joined(separator: ", "))."))
        }
        return comp
    }

    /// The three capture flags, each defaulting to true — the core's own
    /// default, restated here so the reply and the request agree.
    private func captures(_ a: [String: Any]) -> LayerCompCaptures {
        LayerCompCaptures(
            visibility: boolArg(a, "visibility") ?? true,
            position: boolArg(a, "position") ?? true,
            appearance: boolArg(a, "appearance") ?? true)
    }

    @inline(never) func layerCompCatalog() -> [[String: Any]] {
        let name: [String: Any] = [
            "type": "string",
            "description": "The comp's name. Comp names are unique within a document and are "
                + "the handle every one of these tools takes.",
        ]
        let flag = { (what: String) -> [String: Any] in
            [
                "type": "boolean",
                "description": "Record \(what). Default true. Naming ANY of visibility, "
                    + "position or appearance states the WHOLE set: an unnamed flag becomes true.",
            ]
        }
        let visibility = flag("each layer's visibility")
        let position = flag("each RASTER layer's offset (a group and an adjustment have none)")
        let appearance = flag("each layer's opacity, blend mode and layer style")
        return [
            tool("list_layer_comps",
                 "Window > Layer Comps: every layer comp of a document, in list order. A READ — "
                 + "no document changes. Each row carries index, name, comment, the three "
                 + "`captures` flags, `stale_count`, `locked_count` and one `entries` row per "
                 + "layer with its CURRENT `layer` index (null when the layer is gone), the "
                 + "core's stable `id`, the `name` it was recorded under, `stale` and `locked`. "
                 + "An entry binds by that PERSISTED id at every depth, so a comp survives a "
                 + "save, a reopen, a reorder AND a rename; `locked` is a real answer at every "
                 + "depth too (a position-locked layer keeps its recorded offset and reports it). "
                 + "Each entry also carries the state the comp RECORDED — exactly what "
                 + "apply_layer_comp writes back — in get_document's vocabulary, each key present "
                 + "only when the comp captured it: `visible` (visibility), `offset` [x, y] "
                 + "(position; RASTER layers only), and `opacity`, `blend_mode` and `style` "
                 + "(appearance). `style` is the object set_layer_style takes, or null when the "
                 + "layer had no style when the comp was recorded. Each row also carries "
                 + "`unrecorded`: the layers ({layer, id, name}) the comp has no entry for and "
                 + "that are not groups — the ones apply_layer_comp would hide.",
                 ["document_id": Self.docIDProperty]),
            tool("save_layer_comp",
                 "Window > Layer Comps > New…: record the document's CURRENT per-layer "
                 + "visibility, position and appearance under a name, in ONE undo step. A comp "
                 + "holds no pixels, no selection and no history — that is what create_snapshot "
                 + "is for — and it travels in the .doctored at both depths. Up to 64 per document; a "
                 + "duplicate name is refused.",
                 ["name": name,
                  "comment": ["type": "string", "description": "Free text shown beside the comp."],
                  "visibility": visibility, "position": position, "appearance": appearance,
                  "document_id": Self.docIDProperty],
                 required: ["name"]),
            tool("apply_layer_comp",
                 "Window > Layer Comps > Apply: put the document back into the state a comp "
                 + "recorded, in ONE undo step. Only the attributes the comp captures are "
                 + "written. The reply reports `applied` (the layer indices that were written), "
                 + "`stale` (entries whose layer is gone, by the name they were recorded under), "
                 + "`locked` (bound entries whose recorded POSITION a layer lock forbids — their "
                 + "visibility and appearance still apply) and `changed`; when the document "
                 + "already held that state no undo step is added. Apply also "
                 + "HIDES every layer the comp has no record of (one added, placed, pasted or "
                 + "duplicated after the comp was saved or last updated), at any depth, whatever "
                 + "the comp captured and whatever its locks, except groups, which keep their "
                 + "visibility; the reply names those layers in `hidden_unrecorded`.",
                 ["name": name, "document_id": Self.docIDProperty],
                 required: ["name"]),
            tool("update_layer_comp",
                 "Window > Layer Comps > Update / Options…: re-record a comp from the document's "
                 + "current state, and/or rename it, re-comment it and change which attributes "
                 + "it captures. `recapture` defaults to true; with it false only the name, "
                 + "comment and flags move, and the comp's stale entries are dropped while its "
                 + "bound entries take their layers' current names. A recapture records every "
                 + "layer, so a layer the comp had no record of is no longer hidden by apply; "
                 + "with `recapture` false it stays unrecorded and apply still hides it.",
                 ["name": name,
                  "new_name": ["type": "string", "description": "A new name for the comp."],
                  "comment": ["type": "string", "description": "Replaces the comp's comment."],
                  "recapture": ["type": "boolean",
                                "description": "Re-record from the current state. Default true."],
                  "visibility": visibility, "position": position, "appearance": appearance,
                  "document_id": Self.docIDProperty],
                 required: ["name"]),
            tool("delete_layer_comp",
                 "Window > Layer Comps > Delete: remove one comp, in ONE undo step. The "
                 + "document's layers are untouched — deleting a comp forgets a recording, it "
                 + "does not undo one.",
                 ["name": name, "document_id": Self.docIDProperty],
                 required: ["name"]),
            tool("reorder_layer_comp",
                 "Window > Layer Comps (drag a row): move one comp to FINAL index `to` (counted "
                 + "with it taken out, as reorder_layer counts). The list order is what ◀ ▶ step "
                 + "through and what list_layer_comps returns, and it travels in the .doctored. ONE "
                 + "undo step; the comp's own index adds none. Layer locks do not apply.",
                 ["name": name,
                  "to": ["type": "integer", "minimum": 0,
                         "description": "The comp's FINAL index in the list, 0 first."],
                  "document_id": Self.docIDProperty],
                 required: ["name", "to"]),
        ]
    }
}
