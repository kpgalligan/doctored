import Foundation
import CoreFoundation

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let persistedWarpHandlers: [String: ToolHandler] = [
        "get_warp": { $0.getWarp },
        "warp_layer": { $0.warpLayer },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let persistedWarpToolFacts: [String: ToolFacts] = [
        // Recording sweep: `get_warp` only reads. The five assisted-annotation
        // tools drive their own gated study window over an external
        // workspace — they read it, open a case, draw in the session or
        // write its replay file — and edit no document an action runs on.
        // Layer ▸ Warp takes the same extended-range resampler (D3).
        // A float warp keeps no re-editable field, so `get_warp` answers
        // null and each `warp_layer` set warps the current pixels again.
        "get_warp": [.nonRecordable, .typedTool, .floatTool, .cmykTool],
        "warp_layer": [.typedTool, .floatTool, .cmykTool],
    ]

    static func isUnchangedWarpResult(_ result: String) -> Bool {
        guard let data = result.data(using: .utf8),
              let call = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              call["isError"] as? Bool == false,
              let content = call["content"] as? [[String: Any]] else { return false }
        return content.contains { item in
            guard item["type"] as? String == "text", let text = item["text"] as? String,
                  let data = text.data(using: .utf8),
                  let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else { return false }
            return payload["ok"] as? Bool == true && payload["changed"] as? Bool == false
        }
    }

    /// Mirrors Layer ▸ Warp… and uses its captured native work/commit seam.
    func getWarp(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        let layer = try warpLayer(arguments, document: document)
        let info = try source.warpInfo(layer)
        return try jsonResult(["ok": true, "layer": layer, "field": info?["field"] ?? NSNull()])
    }

    /// Mirrors the Apply, Remove Warp and Bake Warp sheet buttons.
    func warpLayer(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        let layer = try warpLayer(arguments, document: document)
        guard let op = stringArg(arguments, "operation") else {
            throw ToolError(message: "warp_layer needs operation:set, clear or bake.")
        }
        let setOnly = ["target", "columns", "rows", "displacements", "filter"]
        let request: PersistedWarpRequest
        switch op {
        case "set":
            guard setOnly.allSatisfy({ arguments[$0] != nil }),
                  let targetName = arguments["target"] as? String,
                  let target = PersistedWarpTarget(rawValue: targetName),
                  let filterName = arguments["filter"] as? String,
                  let filter = PersistedWarpFilter(rawValue: filterName),
                  let columns = warpInteger(arguments["columns"]),
                  let rows = warpInteger(arguments["rows"]),
                  let raw = arguments["displacements"] as? [Any] else {
                throw ToolError(message: "warp_layer set needs target, columns, rows, displacements and filter.")
            }
            let values = try raw.map { row -> [Double] in
                guard let pair = row as? [Any], pair.count == 2 else {
                    throw ToolError(message: "Warp displacement nodes need [dx,dy] pairs.")
                }
                return try pair.map { value in
                    guard let number = value as? NSNumber,
                          CFGetTypeID(number) != CFBooleanGetTypeID(),
                          number.doubleValue.isFinite else {
                        throw ToolError(message: "Warp node values must be finite numbers.")
                    }
                    return number.doubleValue
                }
            }
            request = .set(layer: layer, target: target,
                field: PersistedWarpField(columns: columns, rows: rows,
                                          displacements: values, filter: filter))
        case "clear", "bake":
            guard setOnly.allSatisfy({ arguments[$0] == nil }) else {
                throw ToolError(message: "warp_layer clear/bake accept no field members.")
            }
            request = op == "clear" ? .clear(layer: layer) : .bake(layer: layer)
        default:
            throw ToolError(message: "warp_layer operation must be set, clear or bake.")
        }
        let work = PersistedWarpWork(source: source, request: request)
        guard let prepared = try work.wait(in: document, title: "Warp Layer") else {
            return try jsonResult(["ok": true, "changed": false, "layer": layer,
                                   "field": (try source.warpInfo(layer))?["field"] ?? NSNull(),
                                   "document": summary(document)])
        }
        document.preparedEdit = prepared
        try performGroupedEdit(document, "Warp Layer", allowWarpChange: true) { _ in
            prepared.document
        }
        return try jsonResult(["ok": true, "changed": true, "layer": layer,
                               "field": (try document.doc?.warpInfo(layer))?["field"] ?? NSNull(),
                               "document": summary(document)])
    }

    private func warpLayer(_ arguments: [String: Any], document: ImageDocument) throws -> Int {
        let index = arguments["layer"] == nil ? document.activeLayerIndex :
            (warpInteger(arguments["layer"]) ?? -1)
        guard index >= 0, index < (document.doc?.layerCount ?? 0) else {
            throw ToolError(message: "Invalid warp layer index.")
        }
        return index
    }

    private func warpInteger(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber,
              CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite,
              number.doubleValue.rounded() == number.doubleValue else { return nil }
        return number.intValue
    }
}
