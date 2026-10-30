import AppKit

// The `duplicate_to_new_document` handler refuses by name while `BatchRun.active` is set.

/// `duplicate_to_new_document` — mirrors Layer ▸ Duplicate to New
/// Document… (`EditorViewController+DuplicateToNewDocument.swift`): the
/// given entries, with their subtrees, copied into a NEW untitled document at
/// the source's depth, canvas, profile and resolution through ONE core call.
/// The source gets no undo step and no dirty change. Recorded by the generic
/// recorder: a recorded step replays its literal `index`/`layers`.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let duplicateToNewDocumentHandlers: [String: ToolHandler] = [
        "duplicate_to_new_document": { $0.duplicateToNewDocument },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let duplicateToNewDocumentToolFacts: [String: ToolFacts] = [
        // Layer ▸ Duplicate to New Document, one typed
        // `duplicate_to_new_document` structure request.
        // Duplicate to New Document keeps the float document's depth.
        // #1 phase 4c: a CMYK document's clip is a CMYK document.
        "duplicate_to_new_document": [.typedTool, .floatTool, .cmykTool],
    ]

    func duplicateToNewDocument(_ a: [String: Any]) throws -> String {
        // A batch replays each file's steps; a duplicate there would leave one
        // untitled document open per file.
        guard BatchRun.active == nil else {
            throw ToolError(
                message: "Duplicate to New Document cannot run inside a Batch: each file would "
                    + "leave an untitled document open.")
        }
        let source = try target(a)
        guard let doc = source.doc else { throw ToolError(message: "Document has no image") }
        let indices = try structuralIndices(a, source)
        let roots = doc.layerTree.independentRoots(indices)
        let made: ImageDocument
        do {
            made = try ImageDocument.openDuplicate(
                of: source, layers: indices, name: stringArg(a, "name"))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(
                message: (error as? DoctoredCoreError)?.message ?? error.localizedDescription)
        }
        guard let duplicate = made.doc else { throw ToolError(message: "Document has no image") }
        let resolution = duplicate.resolution
        let depth = duplicate.isFloat ? 32 : duplicate.isHighDepth ? 16 : 8
        return try jsonResult([
            "ok": true,
            "document": summary(made),
            "source_document_id": summary(source)["id"] ?? NSNull(),
            "source_layers": roots,
            "depth": depth,
            "profile_description": duplicate.profileName,
            "width": duplicate.width,
            "height": duplicate.height,
            "ppi": [resolution.x, resolution.y],
        ])
    }
}
