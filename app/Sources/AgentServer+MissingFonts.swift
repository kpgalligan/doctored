import AppKit

/// `list_missing_fonts` and `replace_missing_fonts`: the agent mirrors of Layer ▸ Replace Missing
/// Fonts… (ReplaceFontsSheetController.swift) — its list of families and its Apply — over the
/// same ops (MissingFonts.swift), so a replacement made either way is the same layers, one undo
/// step. Neither is recorded: a step naming a family one Mac lacks means nothing on the next
/// document (designs/replace-missing-fonts.md).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let missingFontsHandlers: [String: ToolHandler] = [
        "list_missing_fonts": { $0.listMissingFonts },
        "replace_missing_fonts": { $0.replaceMissingFonts },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift. Text layers exist at every depth and in ink, as
    /// `edit_text_layer`'s facts say.
    static let missingFontsToolFacts: [String: ToolFacts] = [
        "list_missing_fonts": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "replace_missing_fonts": [.nonRecordable, .typedTool, .floatTool, .cmykTool],
    ]

    /// Mirrors the sheet's rows: each missing family with the text layers that name it.
    func listMissingFonts(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let rows = doc.missingFontLayers()
        let families = Set(rows.map(\.family)).sorted()
        return try jsonResult([
            "missing": families.map { family -> [String: Any] in
                ["font": family,
                 "layers": rows.filter { $0.family == family }
                    .map { ["layer": $0.layer, "name": $0.name] }]
            },
            "count": families.count,
        ])
    }

    /// Mirrors the sheet's Apply: every pair's missing family replaced on every text layer that
    /// names it, re-rendered as one undo step.
    func replaceMissingFonts(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let usage = "replacements must be a non-empty array of {\"font\": <a missing family>, "
            + "\"with\": <an installed family>}; list_missing_fonts names the missing families."
        guard let pairs = a["replacements"] as? [[String: Any]], !pairs.isEmpty else {
            throw ToolError(message: usage)
        }
        let missing = doc.missingFontLayers()
        let missingFamilies = Set(missing.map(\.family))
        let installed = TextStyle.installedFamilies()
        var replacements: [String: String] = [:]
        for pair in pairs {
            guard Set(pair.keys) == ["font", "with"], let font = pair["font"] as? String,
                  let replacement = pair["with"] as? String else { throw ToolError(message: usage) }
            guard missingFamilies.contains(font) else {
                throw ToolError(message: "No text layer names \"\(font)\" as a missing font "
                    + "family; list_missing_fonts names the families this document's text layers "
                    + "miss.")
            }
            // The test that makes a family missing (`TextStyle.isInstalled`), so a name the font
            // manager resolves loosely ("helvetica neue") is refused here, by name.
            guard installed.contains(replacement) else {
                throw ToolError(message: "with names no installed font family: "
                    + "\"\(replacement)\". Pass a FAMILY exactly as the system lists it "
                    + "(\"Helvetica Neue\", \"Times New Roman\"), not a PostScript face name.")
            }
            guard replacements[font] == nil else {
                throw ToolError(message: "\"\(font)\" appears twice in replacements.")
            }
            replacements[font] = replacement
        }
        let affected = missing.filter { replacements[$0.family] != nil }
        // Re-rendering rewrites each layer's pixels, so it answers to their locks.
        for row in affected { try rejectLockedEdit(document, row.layer, DR_EDIT_PIXELS) }
        do {
            // false: no layer named a pair's family by the time the edit ran.
            guard try performArtworkEdit(document, "Replace Missing Fonts", {
                try $0.replacingMissingFonts(replacements)
            }) else { throw ToolError(message: "Nothing was replaced; list_missing_fonts again.") }
        } catch let failure as MissingFontFailure {
            // A transparency lock refuses a new layout that moves the raster.
            try rejectLockedRerender(document, failure.layer)
            throw ToolError(message: failure.message)
        }
        let after = document.doc
        return try jsonResult([
            "ok": true,
            "replaced": affected.map { row -> [String: Any] in
                let info = after?.layerInfo(row.layer)
                return [
                    "layer": row.layer, "name": info?.name ?? row.name, "font": row.family,
                    "with": replacements[row.family] ?? "",
                    "bounds": ["x": info?.offsetX ?? 0, "y": info?.offsetY ?? 0,
                               "width": info?.width ?? 0, "height": info?.height ?? 0],
                ]
            },
        ])
    }
}
