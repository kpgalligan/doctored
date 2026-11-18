import AppKit

/// `combine_paths`, the agent mirror of Paths panel ▸ Combine with Path…
/// (EditorViewController+PathCommands.swift) and of the selection tools' Combine Selected Subpaths
/// (EditorViewController+PathSelect.swift): a path of the list replaced by its fill region
/// united with, less, intersected with or excluding another's — or, with `subpath_indices`, the
/// region of the listed subpaths against the region of its other subpaths (PathCombine.swift); on
/// a path that holds subpath operations, the listed subpaths take the operation and the fold
/// becomes the path, holding none.
/// The result is an ordinary path — `path` keeps its id, name, kind and clipping designation,
/// `with` is left as it is — committed through the Paths tools' one list edit (`editPaths`,
/// AgentServer+Paths.swift), so the core's caps and refusal sentences apply and it is one undo
/// step.
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
        let bySubpaths = Self.present(a["subpath_indices"])
        let other = try nameArgument(a, "with")
        if other != nil, bySubpaths {
            throw ToolError(
                message: "Pass with or subpath_indices, not both: with combines path with another "
                    + "path, subpath_indices combines some of path's own subpaths with the rest.")
        }
        guard other != nil || bySubpaths else {
            throw ToolError(
                message: "combine_paths requires with — the name of the path to combine it with — "
                    + "or subpath_indices — the subpaths of path to combine with its other "
                    + "subpaths.")
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
        let result: PathShape
        var fields: [String: Any] = ["path": name, "operation": operation.rawValue]
        if let other {
            guard let second = set.entry(named: other) else {
                throw ToolError(message: Self.noSuchPath(other, set))
            }
            guard other != name else {
                throw ToolError(
                    message: "with names the same path as path: combine \"\(name)\" with "
                        + "another path.")
            }
            guard let combined = PathCombine.combine(
                set.entries[index].shape, second.shape, operation)
            else { throw ToolError(message: PathCombine.unboundedRefusal) }
            result = combined
            guard !result.isEmpty else {
                throw ToolError(
                    message: "Nothing is left: the \(operation.rawValue) of \"\(name)\" and "
                        + "\"\(other)\" fills no area, and a path cannot be emptied this way. "
                        + "Nothing changed.")
            }
            fields["with"] = other
        } else {
            let shape = set.entries[index].shape
            let indices = try subpathIndices(a["subpath_indices"], path: name, shape: shape)
            guard let combined = PathCombine.combineSubpaths(
                shape, selected: Set(indices), operation)
            else { throw ToolError(message: PathCombine.unboundedRefusal) }
            result = combined
            guard !result.isEmpty else {
                throw ToolError(
                    message: "Nothing is left: the \(operation.rawValue) of the listed subpaths of "
                        + "\"\(name)\" and its other subpaths fills no area, and a path cannot be "
                        + "emptied this way. Nothing changed.")
            }
            fields["subpath_indices"] = indices
        }
        set.entries[index].shape = result
        fields["anchors"] = result.anchorCount
        fields["subpaths"] = result.subpaths.count
        guard try editPaths(document, "Combine Paths", set) else {
            return try pathNoOp(fields, why: "\"\(name)\" already is that outline.")
        }
        return try pathResult(document, fields)
    }

    /// `subpath_indices` as distinct indices into `shape`'s subpaths, ascending: a non-empty
    /// array of whole numbers (or their string spellings), each below the subpath count.
    private func subpathIndices(_ value: Any?, path: String, shape: PathShape) throws -> [Int] {
        let count = shape.subpaths.count
        guard count > 0 else {
            throw ToolError(message: "\"\(path)\" is empty: it has no subpath to combine.")
        }
        let range = count == 1 ? "0" : "0 to \(count - 1)"
        guard let list = value as? [Any] else {
            throw ToolError(
                message: "subpath_indices must be an array of subpath indices of \"\(path)\" "
                    + "(\(range)), in list_paths' order.")
        }
        guard !list.isEmpty else {
            throw ToolError(
                message: "subpath_indices must list at least one subpath of \"\(path)\" "
                    + "(\(range)).")
        }
        var indices: Set<Int> = []
        for entry in list {
            let parsed: Int?
            if let number = entry as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
               number.doubleValue.rounded() == number.doubleValue {
                parsed = number.intValue
            } else {
                parsed = (entry as? String).flatMap(Int.init)
            }
            guard let index = parsed else {
                throw ToolError(
                    message: "subpath_indices must hold whole numbers: \(entry) is not a "
                        + "subpath index.")
            }
            guard (0..<count).contains(index) else {
                throw ToolError(
                    message: "subpath_indices names subpath \(index), but \"\(path)\" has "
                        + "\(count) subpath\(count == 1 ? "" : "s") (\(range)).")
            }
            indices.insert(index)
        }
        return indices.sorted()
    }
}
