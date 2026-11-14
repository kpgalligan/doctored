import Foundation

// The four library tools and `save_gradient` are `nonRecordable` — a library write is a
// preference, never a step — and `documentFree`. `gradient`'s `stops` and a gradient fill layer's
// `gradient.stops` accept `{"name": …}` for a library gradient, resolved by the dispatcher before
// any handler (`gradientPresetArguments`), so a recorded step carries the stops and replays
// without the library.

/// The user's gradient library over MCP.
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `listGradients` | the stop editor's Preset popup (`GradientStopsPane`) |
/// | `saveGradient` | the Preset popup's Save Preset… |
/// | `deleteGradient` | Edit ▸ Gradients ▸ Delete Gradient… (a row's `name`, or a broken `file`) |
/// | `importGradients` | Edit ▸ Gradients ▸ Import Gradients… |
/// | `exportGradients` | Edit ▸ Gradients ▸ Export Gradients… (every gradient; `names` picks some) |
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let gradientsHandlers: [String: ToolHandler] = [
        "list_gradients": { $0.listGradients },
        "save_gradient": { $0.saveGradient },
        "delete_gradient": { $0.deleteGradient },
        "import_gradients": { $0.importGradients },
        "export_gradients": { $0.exportGradients },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift.
    static let gradientsToolFacts: [String: ToolFacts] = [
        "list_gradients": [.nonRecordable, .documentFree],
        "save_gradient": [.nonRecordable, .documentFree],
        "delete_gradient": [.nonRecordable, .documentFree],
        "import_gradients": [.nonRecordable, .documentFree],
        "export_gradients": [.nonRecordable, .documentFree],
    ]

    /// Mirrors the Preset popup's rows: the library, by name.
    func listGradients(_ a: [String: Any]) throws -> String {
        try gradientLibrary {
            let listing = try GradientPresetLibrary.listing()
            let rows = listing.entries.map { entry in
                entry.item.fields.merging(["file": entry.file]) { _, new in new }
            }
            return try jsonResult([
                "gradients": rows,
                "broken": listing.broken.map(\.json),
                "maximum_gradients": GradientPresetCodec.maximumEntries,
            ])
        }
    }

    /// Mirrors Save Preset…: `stops` (`gradient`'s rows) and `smoothness` under `name`.
    func saveGradient(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        guard let rows = a["stops"] as? [[String: Any]],
              GradientPreset.stopCount.contains(rows.count) else {
            throw ToolError(message: "stops must be an array of 2 to 32 {position, color, "
                + "opacity?, midpoint?, opacity_midpoint?} objects")
        }
        let stops = try rows.enumerated().map { index, row -> GradientStop in
            try refuseNullOrBooleanNumbers(row, ["position", "opacity"], in: "stops[\(index)]")
            guard let position = doubleArg(row, "position") else {
                throw ToolError(message: "stops[\(index)].position must be 0…1")
            }
            var opacity = 1.0
            if row["opacity"] != nil {
                guard let value = doubleArg(row, "opacity") else {
                    throw ToolError(message: "stops[\(index)].opacity must be 0…1")
                }
                opacity = value
            }
            guard row["color"] is String else {
                throw ToolError(message: "stops[\(index)] requires color, a hex like #RRGGBB")
            }
            // A hex's own alpha scales the stop's opacity, as a fill layer's stops do.
            let color = try parseColor(row, "color", fallback: .black)
            let midpoint = try stopMidpoint(row, at: "stops[\(index)]")
            return GradientStop(
                position: position, color: LayerStyleColor.hex(color),
                opacity: opacity * Double(color.alphaComponent),
                midpoint: midpoint.color, opacityMidpoint: midpoint.opacity)
        }
        let smoothness = try gradientSmoothnessArgument(a)
        return try gradientLibrary {
            let saved = try GradientPresetLibrary.save(
                name: name, stops: stops, smoothness: smoothness,
                replace: boolArg(a, "replace") ?? false)
            return try jsonResult(["ok": true, "gradient": saved.fields])
        }
    }

    /// Removes ONE library file: `name` (a library gradient) or `file` (any file, a broken one
    /// included).
    func deleteGradient(_ a: [String: Any]) throws -> String {
        let name = stringArg(a, "name"), file = stringArg(a, "file")
        guard (name == nil) != (file == nil) else {
            throw ToolError(message: "delete_gradient takes exactly one of name or file — "
                + "a broken entry has no name.")
        }
        return try gradientLibrary {
            if let file {
                try GradientPresetLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            try GradientPresetLibrary.delete(name: name ?? "")
            return try jsonResult(["ok": true, "name": name ?? ""])
        }
    }

    /// Reads one Doctored gradient file into the library.
    func importGradients(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else {
            throw ToolError(message: "No file at \(url.path)")
        }
        return try gradientLibrary {
            let result = try GradientPresetLibrary.importFile(url)
            return try jsonResult(["ok": true, "imported": result.imported,
                                   "skipped": result.skipped, "file": url.path])
        }
    }

    /// Writes every library gradient, or `names`'s alone, as one Doctored gradient file.
    func exportGradients(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var names: [String]?
        if let raw = a["names"] {
            guard let list = raw as? [Any], !list.isEmpty, list.allSatisfy({ $0 is String }) else {
                throw ToolError(message: "names must be a non-empty array of gradient names.")
            }
            names = list.compactMap { $0 as? String }
        }
        return try gradientLibrary {
            let written = try GradientPresetLibrary.exportFile(to: url, names: names)
            return try jsonResult(["ok": true, "exported": written, "file": url.path])
        }
    }

    /// The dispatcher's one argument pass for library gradients
    /// (`GradientPresetLibrary.resolvedArguments`); refusals in-band.
    func gradientPresetArguments(tool: String, arguments: [String: Any]) throws -> [String: Any] {
        try gradientLibrary {
            try GradientPresetLibrary.resolvedArguments(tool: tool, arguments: arguments)
        }
    }

    /// A library refusal becomes an in-band `ToolError` with its own sentence.
    private func gradientLibrary<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
