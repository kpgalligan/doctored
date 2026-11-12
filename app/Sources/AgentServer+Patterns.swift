import Foundation

// `define_pattern` reads the document (the shared selection, the composite) and writes the
// library; it and the four library tools are `nonRecordable` — a library write is a preference,
// never a step — and the four are `documentFree`. Every tool that takes a pattern object accepts
// `{"name": …}` for a library pattern (`patternTile(_:at:)`, `resolvedPatterns`), and the recorder
// stores the resolved tile (`PatternLibrary.recordedArguments`), so no step needs the library.

/// The user's pattern library over MCP.
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `definePattern` | Edit ▸ Define Pattern… (`EditorViewController+DefinePattern.swift`) |
/// | `listPatterns` | a pattern picker's rows (`PatternChoice.all`): the built-ins, then the library |
/// | `deletePattern` | Edit ▸ Patterns ▸ Delete Pattern… (a row's `name`, or a broken `file`) |
/// | `importPatterns` | Edit ▸ Patterns ▸ Import Patterns… |
/// | `exportPatterns` | Edit ▸ Patterns ▸ Export Patterns… (every pattern; `names` picks some) |
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let patternsHandlers: [String: ToolHandler] = [
        "define_pattern": { $0.definePattern },
        "list_patterns": { $0.listPatterns },
        "delete_pattern": { $0.deletePattern },
        "import_patterns": { $0.importPatterns },
        "export_patterns": { $0.exportPatterns },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. `define_pattern` reads
    /// what Copy Merged reads, at every depth and in CMYK.
    static let patternsToolFacts: [String: ToolFacts] = [
        "define_pattern": [.nonRecordable, .selectionSensitive, .typedTool, .floatTool, .cmykTool],
        "list_patterns": [.nonRecordable, .documentFree],
        "delete_pattern": [.nonRecordable, .documentFree],
        "import_patterns": [.nonRecordable, .documentFree],
        "export_patterns": [.nonRecordable, .documentFree],
    ]

    /// Mirrors Edit ▸ Define Pattern…: the composite inside the shared selection (a hard-edged
    /// rectangle) or the whole canvas, saved under `name`.
    func definePattern(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard !document.quickMaskActive else {
            throw ToolError(message: "Define Pattern reads the selection; exit Quick Mask "
                + "(set_quick_mask) first.")
        }
        let name = try requiredString(a, "name")
        return try patternLibrary {
            let tile = try DefinePattern.tile(of: doc, in: document,
                                              selection: document.canvasSelection)
            let saved = try PatternLibrary.save(
                name: name, width: tile.width, height: tile.height, rgba: tile.rgba,
                replace: boolArg(a, "replace") ?? false)
            return try jsonResult([
                "ok": true, "pattern": ["name": saved.name, "width": saved.width,
                                        "height": saved.height],
                "source": tile.source,
            ])
        }
    }

    /// Mirrors a pattern picker's rows: the four built-ins, then the library.
    func listPatterns(_ a: [String: Any]) throws -> String {
        try patternLibrary {
            let listing = try PatternLibrary.listing()
            let builtIns: [[String: Any]] = RasterDocument.patternPresets().map {
                ["name": $0.name, "preset": $0.id, "builtin": true, "width": $0.width,
                 "height": $0.height]
            }
            let saved: [[String: Any]] = listing.entries.map {
                ["name": $0.item.name, "builtin": false, "width": $0.item.width,
                 "height": $0.item.height, "file": $0.file]
            }
            return try jsonResult([
                "patterns": builtIns + saved,
                "broken": listing.broken.map(\.json),
                "maximum_patterns": PatternLibraryCodec.maximumEntries,
                "maximum_side": LibraryPattern.maximumSide,
            ])
        }
    }

    /// Removes ONE library file: `name` (a library pattern) or `file` (any file, a broken one
    /// included).
    func deletePattern(_ a: [String: Any]) throws -> String {
        let name = stringArg(a, "name"), file = stringArg(a, "file")
        guard (name == nil) != (file == nil) else {
            throw ToolError(message: "delete_pattern takes exactly one of name or file — "
                + "a broken entry has no name.")
        }
        return try patternLibrary {
            if let file {
                try PatternLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            try PatternLibrary.delete(name: name ?? "")
            return try jsonResult(["ok": true, "name": name ?? ""])
        }
    }

    /// Reads one Doctored pattern file into the library.
    func importPatterns(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else {
            throw ToolError(message: "No file at \(url.path)")
        }
        return try patternLibrary {
            let result = try PatternLibrary.importFile(url)
            return try jsonResult(["ok": true, "imported": result.imported,
                                   "skipped": result.skipped, "file": url.path])
        }
    }

    /// Writes every library pattern, or `names`'s alone, as one Doctored pattern file.
    func exportPatterns(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var names: [String]?
        if let raw = a["names"] {
            guard let list = raw as? [Any], !list.isEmpty, list.allSatisfy({ $0 is String }) else {
                throw ToolError(message: "names must be a non-empty array of pattern names.")
            }
            names = list.compactMap { $0 as? String }
        }
        return try patternLibrary {
            let written = try PatternLibrary.exportFile(to: url, names: names)
            return try jsonResult(["ok": true, "exported": written, "file": url.path])
        }
    }

    /// The library pattern `name`, as an in-band `ToolError` when there is none.
    func libraryPattern(_ name: String) throws -> LibraryPattern {
        try patternLibrary { try PatternLibrary.named(name) }
    }

    /// `value` with every `{"name": …}` pattern object resolved to its library tile
    /// (`PatternLibrary.resolvingReferences`); refusals in-band.
    func resolvedPatterns(_ value: Any) throws -> Any {
        try patternLibrary { try PatternLibrary.resolvingReferences(in: value) }
    }

    /// A library refusal becomes an in-band `ToolError` with its own sentence.
    private func patternLibrary<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
