import AppKit

// Image ▸ Mode ▸ Color Table over MCP (#1 Indexed phase 1c): `get_color_table` reads an Indexed
// Color document's palette and transparent index (and with `path` writes them as an Adobe Color
// Table `.act`); `set_color_table` replaces them from `colors` or an `.act` at `path`, ONE undo
// step, every pixel keeping its index — the UI path is `EditorViewController+ColorTable.swift`'s
// `showColorTable:` → `ColorTableSheetController` → `IndexedColorTableEdit`. Both `path`
// arguments are declared in `AgentPathGuard.toolPaths`.

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let indexedColorTableHandlers: [String: ToolHandler] = [
        "get_color_table": { $0.getColorTable },
        "set_color_table": { $0.setColorTable },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in
    /// AgentServer+Registry.swift.
    static let indexedColorTableToolFacts: [String: ToolFacts] = [
        "get_color_table": [.nonRecordable, .commonTool],
        "set_color_table": [.commonTool],
    ]

    /// The refusal on any document but an Indexed Color one.
    private func colorTableRefusal(_ tool: String) -> ToolError {
        ToolError(message: "\(tool) applies to an Indexed Color document, as Image ▸ Mode ▸ Color "
            + "Table does.")
    }

    /// The file `path` names, `~` expanded.
    private func colorTableURL(_ path: String) -> URL {
        URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
    }

    /// Mirrors Image ▸ Mode ▸ Color Table…'s table and its Save… (`showColorTable:`).
    func getColorTable(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let typed = document.doc?.precision, document.doc?.isIndexed == true else {
            throw colorTableRefusal("get_color_table")
        }
        do {
            let table = try typed.indexedColorTable()
            var reply = table.json
            reply["count"] = table.colors.count
            if let path = stringArg(a, "path") {
                let url = colorTableURL(path)
                try table.writeAct(to: url)
                reply["path"] = url.path
            }
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors Image ▸ Mode ▸ Color Table…'s OK and its Load… (`showColorTable:`): one undo step.
    func setColorTable(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard document.doc?.isIndexed == true else { throw colorTableRefusal("set_color_table") }
        let colors = a["colors"], path = stringArg(a, "path")
        guard (colors == nil) != (path == nil) else {
            throw ToolError(message: "Pass exactly one of colors and path.")
        }
        if path != nil, a["transparent_index"] != nil {
            throw ToolError(message: "transparent_index applies with colors; an .act file carries its own.")
        }
        do {
            let table: IndexedColorTable
            if let path {
                table = try IndexedColorTable.readAct(url: colorTableURL(path))
            } else {
                var json: [String: Any] = ["colors": colors ?? []]
                if let value = a["transparent_index"], !(value is NSNull) {
                    guard let index = intArg(a, "transparent_index") else {
                        throw ToolError(message: "transparent_index must be an index or null.")
                    }
                    json["transparent_index"] = index
                }
                table = try IndexedColorTable(json: json)
            }
            let edit = try IndexedColorTableEdit(document, table: table)
            if let refusal = edit.refusal { throw ToolError(message: refusal) }
            var reply: [String: Any] = [
                "ok": true, "changed": edit.changes, "colors": table.colors.count,
                "transparent_index": table.transparentIndex.map { $0 as Any } ?? NSNull(),
                "working_memory_plan_bytes": edit.scratch,
            ]
            if edit.changes {
                let prepared = try edit.compute()
                try edit.publish(prepared, to: document, record: .notACommand)
            }
            reply["document"] = summary(document)
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
