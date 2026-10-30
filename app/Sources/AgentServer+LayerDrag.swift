import AppKit

/// `copy_layers_to_document` — the MCP twin of dragging layers from one
/// document onto another (LayerDrag.swift, the drop in
/// EditorViewController+LayerDrag.swift): `layers` of `document_id` are
/// copied, subtrees included, into `to_document_id` above its active layer,
/// as ONE undo step there. The source document and the clipboard are not
/// touched. The paste is the layer clipboard's own
/// (`AgentServer.pasteLayers`), so depth and profile conversion and every
/// refusal are `paste`'s; the reply is its `LayerPasteOutcome.json`. The
/// entry is `LayerDrag.entry`'s, so a camera RAW layer lands as
/// pixels, as the drop's does.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let layerDragHandlers: [String: ToolHandler] = [
        "copy_layers_to_document": { $0.copyLayersToDocument },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let layerDragToolFacts: [String: ToolFacts] = [
        // Not recordable: an Action replays in ONE document, and this names
        // two. The source asks `layer_duplicate` at 16 bits and float
        // (`DocumentPrecision.layerCopyFamily`); the destination is asked as
        // a `paste` below.
        // #547 phase 4c: between CMYK documents, and across modes converted.
        "copy_layers_to_document": [.nonRecordable, .typedTool, .floatTool, .cmykTool],
    ]

    func copyLayersToDocument(_ a: [String: Any]) throws -> String {
        let source = try target(a)
        guard source.doc != nil else { throw ToolError(message: "Document has no image") }
        guard let destinationID = intArg(a, "to_document_id") else {
            throw ToolError(message: "copy_layers_to_document requires to_document_id, the "
                + "document the layers are copied into (list_documents).")
        }
        let destination = try target(["document_id": destinationID])
        guard destination !== source else {
            throw ToolError(message: "to_document_id names the source document. Dragging "
                + "layers within one document reorders them; use duplicate_layer to copy.")
        }
        guard destination.doc != nil else {
            throw ToolError(message: "The destination document has no image")
        }
        let indices = a["layers"] != nil
            ? try layerIndices(a, source) : source.selectedLayerIndices
        let placement = stringArg(a, "placement") ?? "auto"
        guard placement == "auto" || placement == "in_place" else {
            throw ToolError(message: "placement must be auto or in_place.")
        }
        // The destination's own depth gate, as a layers paste into it.
        if let reason = DocumentPrecision.refusal(
            tool: "paste", arguments: ["mode": "layers"], document: destination) {
            throw ToolError(message: reason)
        }
        let entry: LayerClipboard.Entry
        do {
            entry = try LayerDrag.entry(from: source, indices: indices)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try pasteLayers(
            ["document_id": destinationID, "placement": placement], entry: entry,
            recording: false)
    }
}
