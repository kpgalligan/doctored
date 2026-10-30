import AppKit

/// `set_layer_label` (Layer ▸ Label) and `find_layers` (the layers panel's
/// filter bar). Both ask `LayerFilter`/`LayerLabel` the same questions the UI
/// does, so a row the panel shows and a row the tool reports cannot disagree.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let layerLabelsHandlers: [String: ToolHandler] = [
        "set_layer_label": { $0.setLayerLabel },
        "find_layers": { $0.findLayers },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let layerLabelsToolFacts: [String: ToolFacts] = [
        // Phase 12C: `new_document` CREATES a document (open_document's
        // reason — a replay would pop a second window beside the run's
        // target), `find_layers` only reads, and the three document-preset
        // tools write a preference library. `set_layer_label` IS a step.
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
        "set_layer_label": [.commonTool, .floatTool, .cmykTool],
        "find_layers": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// Mirrors Layer ▸ Label ▸ … over ONE layer or a `layers` SET, which is
    /// what the menu item does over a multi-selection.
    ///
    /// The reply is this tool's own shape rather than `structureNoOp`'s,
    /// because a partially-labelled set is the ordinary case: entries that
    /// were WRITTEN come back in `layers`, entries that already carried that
    /// label come back in `skipped`, and `changed` says whether an undo step
    /// exists at all. When every entry is skipped nothing is pushed — no
    /// document handle, no undo step — which is the core's purity rule
    /// reported honestly instead of as a refusal.
    func setLayerLabel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let indices = try structuralIndices(a, document)
        let name = try requiredString(a, "label")
        guard let label = LayerLabel(name: name) else {
            throw ToolError(
                message: "Unknown label \"\(name)\" — use none, "
                    + LayerLabel.allCases.dropFirst().map(\.name).joined(separator: ", ") + ".")
        }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image.") }
        let written = indices.filter { doc.layerLabel($0) != label }
        let skipped = indices.filter { doc.layerLabel($0) == label }
        guard !written.isEmpty else {
            return try jsonResult([
                "ok": true, "changed": false, "layers": [Int](), "skipped": skipped,
                "label": label.name, "document": summary(document),
                "note": indices.count == 1
                    ? "that layer already carries that label, so no undo step was added."
                    : "every one of those layers already carries that label, so no undo step was added.",
            ])
        }
        let actionName = label == .none ? "Clear Label" : "Label \(label.displayName)"
        guard try editStructure(document, actionName, renderNeutral: true, { start in
            var current = start
            var changed = false
            for index in written {
                guard let next = try current.withLayerLabel(index, label) else { continue }
                current = next
                changed = true
            }
            return changed ? current : nil
        }) else {
            throw ToolError(message: "The label could not be written.")
        }
        return try jsonResult([
            "ok": true, "changed": true, "layers": written, "skipped": skipped,
            "label": label.name, "document": summary(document),
        ])
    }

    /// Mirrors the layers panel's filter bar. A READ: it changes no document,
    /// and it walks EVERY entry regardless of group open state, so a match
    /// inside a closed group is reported exactly as the filtered panel draws
    /// it.
    func findLayers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image.") }
        var filter = LayerFilter()
        filter.query = stringArg(a, "name") ?? ""
        if let kindName = stringArg(a, "kind") {
            guard let kind = LayerKindFilter(rawValue: kindName) else {
                throw ToolError(
                    message: "Unknown kind \"\(kindName)\" — use "
                        + LayerKindFilter.allCases.map(\.rawValue).joined(separator: ", ") + ".")
            }
            filter.kind = kind
        }
        if let labelName = stringArg(a, "label") {
            guard let label = LayerLabel(name: labelName) else {
                throw ToolError(
                    message: "Unknown label \"\(labelName)\" — use none, "
                        + LayerLabel.allCases.dropFirst().map(\.name).joined(separator: ", ") + ".")
            }
            filter.label = label
        }
        let rows: [[String: Any]] = doc.layers(matching: filter).compactMap { index in
            guard let info = doc.layerInfo(index) else { return nil }
            return [
                "index": index, "id": info.id, "name": info.name,
                "kind": LayerKindFilter.kind(of: doc, index).rawValue,
                "label": doc.layerLabel(index).name, "depth": info.depth,
            ]
        }
        return try jsonResult(["layers": rows, "count": rows.count])
    }

    @inline(never) func layerLabelCatalog() -> [[String: Any]] {
        let labels = LayerLabel.allCases.map(\.name)
        let index: [String: Any] = [
            "type": "integer",
            "description": "Layer index (0 = bottom); omit for the active layer. An index may "
                + "name a layer GROUP, which carries a label like any other entry.",
        ]
        let indices: [String: Any] = [
            "type": "array", "items": ["type": "integer"],
            "description": "Several layers, exclusive with `layer`. In a recorded Action this is "
                + "{\"$layers\": \"selected\"}.",
        ]
        let label: [String: Any] = [
            "type": "string", "enum": labels,
            "description": "A colour label. Photoshop's palette and its `lclr` codes, so a label "
                + "survives export to PSD and back. \"none\" clears it.",
        ]
        return [
            tool("set_layer_label",
                 "Layer > Label: mark one layer or a `layers` SET with a colour label, in ONE undo "
                 + "step. A label is panel bookkeeping — it changes no pixel and is exempt from the "
                 + "layer locks, like a rename — and it persists in the .doctored at both depths and in a "
                 + "layered PSD's `lclr`. The reply reports `layers` (written), `skipped` (already "
                 + "at that label) and `changed`; when every entry is skipped no undo step is added.",
                 ["label": label, "layer": index, "layers": indices,
                  "document_id": Self.docIDProperty],
                 required: ["label"]),
            tool("find_layers",
                 "The layers panel's filter bar: find entries by name substring (case- and "
                 + "diacritic-insensitive), by kind and by colour label, ANDed. A READ — no "
                 + "document changes — and it walks EVERY entry whatever a group's disclosure "
                 + "state is, so a match inside a closed group is reported. Each row carries "
                 + "index, id, name, kind, label and depth.",
                 ["name": ["type": "string", "description": "Name substring."],
                  "kind": ["type": "string", "enum": LayerKindFilter.allCases.map(\.rawValue),
                           "description": "raster, adjustment, text, shape, group, or embedded "
                               + "(a camera RAW layer)."],
                  "label": label, "document_id": Self.docIDProperty]),
        ]
    }
}
