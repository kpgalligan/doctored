import AppKit

/// Mirrors Select ▸ Grow and Select ▸ Similar
/// (EditorViewController+SelectSimilar.swift): one tool, `contiguous` picks
/// which menu item it is.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let selectSimilarHandlers: [String: ToolHandler] = [
        "select_similar": { $0.selectSimilar },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let selectSimilarToolFacts: [String: ToolFacts] = [
        // Compares the R1 display copy in a float document.
        "select_similar": [.selectionSensitive, .commonTool, .floatTool, .cmykTool],
    ]

    var selectSimilarCatalog: [[String: Any]] {
        [
            tool(
                "select_similar",
                "Colour-based, NOT a pixel-radius expand (that is modify_selection grow): "
                    + "Select ▸ Grow / Select ▸ Similar. Every pixel at least 50% "
                    + "selected is a reference; their range on seven projections — R, G, B, A, "
                    + "R−G, G−B and R−B — widened by tolerance on both sides decides which "
                    + "pixels of the composite are admitted, so a hue shift counts about double "
                    + "against the tolerance. The admitted region is ADDED to the selection at "
                    + "full coverage, promoting partly selected pixels that fall in range. "
                    + "contiguous=true is Select ▸ Grow (four-connected from the selected "
                    + "pixels); false (default) is Select ▸ Similar (the whole image). At 16 "
                    + "bits the tolerance is scaled ×257. Repeating a call can grow further. "
                    + "Reports changed:false and adds no undo step when nothing new would be "
                    + "selected. Errors with no selection. In a 32-bit float document the "
                    + "projections are the display-encoded copy, so a value above white "
                    + "compares as white.",
                [
                    "tolerance": [
                        "type": "number", "minimum": 0, "maximum": 255,
                        "description": "0–255 (default 32), as the Magic Wand's.",
                    ],
                    "contiguous": [
                        "type": "boolean",
                        "description": "true = Select ▸ Grow, false (default) = Select ▸ Similar.",
                    ],
                    "document_id": Self.docIDProperty,
                ]),
        ]
    }

    func selectSimilar(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let tolerance = doubleArg(a, "tolerance") ?? 32
        guard tolerance.isFinite, (0...255).contains(tolerance) else {
            throw ToolError(message: "Tolerance must be between 0 and 255.")
        }
        let contiguous = boolArg(a, "contiguous") ?? false
        let capture = try SelectionCapture(document: document, needsSource: false)
        let changed = try capture.commitSimilar(
            tolerance: tolerance, contiguous: contiguous, in: document)
        var result = selectionHistorySummary(document)
        result["changed"] = changed
        if !changed {
            result["note"] = "Every admitted pixel was already selected; the selection is "
                + "unchanged and no undo step was added."
        }
        return try jsonResult(result)
    }
}
