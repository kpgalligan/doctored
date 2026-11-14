import AppKit

/// `combine_paths`, the agent mirror of Paths panel ▸ Combine with Path…
/// (EditorViewController+PathCommands.swift): a path of the list replaced by its fill region
/// united with, less, intersected with or excluding another's (PathCombine.swift). The result is
/// an ordinary path — `path` keeps its id, name, kind and clipping designation, `with` is left as
/// it is — committed through the Paths tools' one list edit (`editPaths`, AgentServer+Paths.swift),
/// so the core's caps and refusal sentences apply and it is one undo step.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let pathCombineHandlers: [String: ToolHandler] = [
        "combine_paths": { $0.combinePaths },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. A path is document state
    /// at every depth and in every colour mode that holds paths (`pathsToolFacts`).
    static let pathCombineToolFacts: [String: ToolFacts] = [
        "combine_paths": [.commonTool, .floatTool, .cmykTool],
    ]

    func combinePaths(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let name = try nameArgument(a, "path") else {
            throw ToolError(
                message: "combine_paths requires path — the path to change: a saved path's name, "
                    + "or \"Work Path\".")
        }
        guard let other = try nameArgument(a, "with") else {
            throw ToolError(
                message: "combine_paths requires with — the name of the path to combine it with.")
        }
        let names = PathCombineOperation.allCases.map(\.rawValue).joined(separator: ", ")
        guard Self.present(a["operation"]) else {
            throw ToolError(message: "combine_paths requires operation: one of \(names).")
        }
        guard let text = a["operation"] as? String,
              let operation = PathCombineOperation(rawValue: text) else {
            throw ToolError(message: "operation must be one of: \(names).")
        }
        var set = doc.paths
        guard let index = set.entries.firstIndex(where: { $0.name == name }) else {
            throw ToolError(message: Self.noSuchPath(name, set))
        }
        guard let second = set.entry(named: other) else {
            throw ToolError(message: Self.noSuchPath(other, set))
        }
        guard other != name else {
            throw ToolError(
                message: "with names the same path as path: combine \"\(name)\" with another "
                    + "path.")
        }
        let result = PathCombine.combine(set.entries[index].shape, second.shape, operation)
        guard !result.isEmpty else {
            throw ToolError(
                message: "Nothing is left: the \(operation.rawValue) of \"\(name)\" and "
                    + "\"\(other)\" fills no area, and a path cannot be emptied this way. Nothing "
                    + "changed.")
        }
        set.entries[index].shape = result
        let fields: [String: Any] = [
            "path": name, "with": other, "operation": operation.rawValue,
            "anchors": result.anchorCount, "subpaths": result.subpaths.count,
        ]
        guard try editPaths(document, "Combine Paths", set) else {
            return try pathNoOp(fields, why: "\"\(name)\" already is that outline.")
        }
        return try pathResult(document, fields)
    }
}
