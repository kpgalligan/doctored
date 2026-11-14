import Foundation

/// One user gradient: its stops (position, authored sRGB `#rrggbb`, opacity
/// and the two midpoints) and its Smoothness. A stop's CMYK `ink` is not
/// stored: a preset is authored colour, which a CMYK document converts as it
/// converts any authored colour.
struct GradientPreset: Equatable {
    static let schemaVersion = 1
    static let stopCount = 2...32

    let name: String
    let stops: [GradientStop]
    /// Smoothness as a fraction, 0…1 (1 is the Classic ramp).
    let smoothness: Double

    /// A preset from `stops` and `smoothness`: the name normalized, 2…32
    /// stops with positions 0…1 never decreasing, `#rrggbb` colours, opacity
    /// 0…1, midpoints 0.05…0.95 (0.5 stored as none) and smoothness 0…1.
    static func validated(name: String, stops: [GradientStop], smoothness: Double) throws
        -> GradientPreset {
        let normalized = try PreferenceLibrary<GradientPresetCodec>.normalizedName(name)
        guard stopCount.contains(stops.count) else {
            throw DoctoredCoreError(message: "A gradient holds 2 to 32 stops.")
        }
        var previous = 0.0
        let checked = try stops.enumerated().map { index, stop -> GradientStop in
            guard stop.position.isFinite, (0...1).contains(stop.position), stop.position >= previous
            else {
                throw DoctoredCoreError(message: "A gradient's stops[\(index)].position must be "
                    + "0…1 and not below the stop before it.")
            }
            previous = stop.position
            guard stop.opacity.isFinite, (0...1).contains(stop.opacity) else {
                throw DoctoredCoreError(message: "A gradient's stops[\(index)].opacity must be 0…1.")
            }
            let color = stop.color.lowercased()
            guard color.count == 7, color.hasPrefix("#"),
                  color.dropFirst().allSatisfy({ $0.isHexDigit }) else {
                throw DoctoredCoreError(message: "A gradient's stops[\(index)].color must be "
                    + "#rrggbb.")
            }
            for value in [stop.midpoint, stop.opacityMidpoint].compactMap({ $0 }) {
                guard value.isFinite, GradientStop.midpointRange.contains(value) else {
                    throw DoctoredCoreError(message: "A gradient's stops[\(index)] midpoints "
                        + "must be 0.05…0.95.")
                }
            }
            return GradientStop(
                position: stop.position, color: color, opacity: stop.opacity,
                midpoint: stop.midpoint == 0.5 ? nil : stop.midpoint,
                opacityMidpoint: stop.opacityMidpoint == 0.5 ? nil : stop.opacityMidpoint)
        }
        guard smoothness.isFinite, (0...1).contains(smoothness) else {
            throw DoctoredCoreError(message: "A gradient's smoothness must be 0…1.")
        }
        return GradientPreset(name: normalized, stops: checked, smoothness: smoothness)
    }

    /// The stops as `gradient`'s and a gradient fill layer's `stops` rows.
    var stopArguments: [[String: Any]] {
        stops.map { stop in
            var row: [String: Any] = [
                "position": stop.position, "color": stop.color, "opacity": stop.opacity,
            ]
            if let midpoint = stop.midpoint { row["midpoint"] = midpoint }
            if let midpoint = stop.opacityMidpoint { row["opacity_midpoint"] = midpoint }
            return row
        }
    }

    /// The MCP row: the name, the stops and the smoothness.
    var fields: [String: Any] {
        ["name": name, "stops": stopArguments, "smoothness": smoothness]
    }

    /// The preset's library file, and one entry of a gradient file.
    var json: [String: Any] {
        fields.merging(["schema_version": Self.schemaVersion]) { _, new in new }
    }

    /// Strict: exactly `json`'s keys, `schema_version` 1, each stop exactly
    /// `position`, `color`, `opacity` and the optional midpoints, and values
    /// `validated` accepts under a normalized name.
    static func decode(_ object: [String: Any]) throws -> GradientPreset {
        guard Set(object.keys) == ["schema_version", "name", "stops", "smoothness"],
              NativeBrushStroke.jsonNumber(object["schema_version"]) == Double(schemaVersion),
              let name = object["name"] as? String, let rows = object["stops"] as? [Any],
              let smoothness = NativeBrushStroke.jsonNumber(object["smoothness"]) else {
            throw DoctoredCoreError(message: "A gradient holds schema_version 1, name, stops "
                + "and smoothness and nothing else.")
        }
        let allowed: Set<String> = ["position", "color", "opacity", "midpoint", "opacity_midpoint"]
        let stops = try rows.enumerated().map { index, raw -> GradientStop in
            guard let row = raw as? [String: Any], Set(row.keys).isSubset(of: allowed),
                  let position = NativeBrushStroke.jsonNumber(row["position"]),
                  let color = row["color"] as? String,
                  let opacity = NativeBrushStroke.jsonNumber(row["opacity"]) else {
                throw DoctoredCoreError(message: "A gradient's stops[\(index)] holds position, "
                    + "color, opacity and the optional midpoint and opacity_midpoint.")
            }
            let midpoint = row["midpoint"].flatMap(NativeBrushStroke.jsonNumber)
            let opacityMidpoint = row["opacity_midpoint"].flatMap(NativeBrushStroke.jsonNumber)
            guard (row["midpoint"] == nil) == (midpoint == nil),
                  (row["opacity_midpoint"] == nil) == (opacityMidpoint == nil) else {
                throw DoctoredCoreError(message: "A gradient's stops[\(index)] midpoints must be "
                    + "numbers.")
            }
            return GradientStop(position: position, color: color, opacity: opacity,
                                midpoint: midpoint, opacityMidpoint: opacityMidpoint)
        }
        let preset = try validated(name: name, stops: stops, smoothness: smoothness)
        guard preset.name == name, preset.stops == stops else {
            throw DoctoredCoreError(message: "A gradient's name and colours must be normalized.")
        }
        return preset
    }
}

/// The gradient library's codec on the shared `PreferenceLibrary` contract
/// (`PreferenceLibrary.swift`'s header): one `GradientPreset` per file.
enum GradientPresetCodec: PreferenceLibraryCodec {
    typealias Item = GradientPreset

    static let noun = "gradient"
    static let directoryEnvironment = "DR_GRADIENTS_DIR"
    static let directoryName = "Doctored/Gradients"
    static let maximumEntries = 256
    static let maximumFileBytes = 16_384
    static let schemaVersions = GradientPreset.schemaVersion...GradientPreset.schemaVersion
    static let didChange = Notification.Name("GradientPresetLibraryDidChange")

    static func name(of item: GradientPreset) -> String { item.name }
    static func json(of item: GradientPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> GradientPreset {
        try GradientPreset.decode(object)
    }
}

/// The user's gradient library (`DR_GRADIENTS_DIR`) and Doctored's own gradient
/// file, which exchanges it between machines: `{"doctored_gradients": 1,
/// "gradients": [<a library file's object>, …]}`, extension
/// `.doctoredgradients`. There are no built-ins: the Gradient tool's
/// Foreground → Background ramp stays its default. Writes are PREFERENCES:
/// outside document Undo and never Action steps. A call naming a library
/// gradient (`{"name": …}` in place of `stops`) is resolved before any handler
/// runs (`resolvedArguments`), so a recorded step carries the stops.
enum GradientPresetLibrary {
    typealias Library = PreferenceLibrary<GradientPresetCodec>

    static let didChange = GradientPresetCodec.didChange
    static let fileExtension = "doctoredgradients"
    static let fileFormatKey = "doctored_gradients"
    static let maximumFileBytes = 1_048_576
    static let maximumFileGradients = 256

    private static var cache: [GradientPreset]?

    /// The library's gradients, by name; empty when it cannot be read.
    static var presets: [GradientPreset] {
        if let cache { return cache }
        let read = (try? Library.entries()) ?? []
        cache = read
        return read
    }

    /// Drops the held list, so the next read sees the folder as it is now.
    static func reload() {
        cache = nil
    }

    static func listing() throws -> PreferenceLibraryListing<GradientPreset> {
        try Library.listing()
    }

    static func named(_ name: String) throws -> GradientPreset {
        try Library.named(name)
    }

    @discardableResult
    static func save(name: String, stops: [GradientStop], smoothness: Double, replace: Bool) throws
        -> GradientPreset {
        let preset = try GradientPreset.validated(name: name, stops: stops, smoothness: smoothness)
        cache = nil
        defer { cache = nil }
        return try Library.save(preset, replace: replace)
    }

    static func delete(name: String) throws {
        cache = nil
        defer { cache = nil }
        try Library.delete(name: name)
    }

    static func delete(file: String) throws {
        cache = nil
        defer { cache = nil }
        try Library.delete(file: try Library.validatedFileName(file))
    }

    /// Writes the library's gradients — all, or those `names` names — as one
    /// gradient file at `url`; returns the names written.
    static func exportFile(to url: URL, names: [String]?) throws -> [String] {
        let entries = try Library.entries()
        let chosen = try names.map { list in
            try list.map { name -> GradientPreset in
                let normalized = try Library.normalizedName(name)
                guard let preset = entries.first(where: { Library.key($0.name) == Library.key(normalized) })
                else { throw DoctoredCoreError(message: "No gradient named “\(normalized)”.") }
                return preset
            }
        } ?? entries
        guard !chosen.isEmpty else {
            throw DoctoredCoreError(message: "The library holds no gradients to export.")
        }
        guard chosen.count <= maximumFileGradients else {
            throw DoctoredCoreError(message: "One gradient file holds at most "
                + "\(maximumFileGradients) gradients.")
        }
        let object: [String: Any] = [fileFormatKey: 1, "gradients": chosen.map(\.json)]
        let data = try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
        do {
            try data.write(to: url, options: .atomic)
        } catch {
            throw DoctoredCoreError(message: "Could not write the gradient file at \(url.path): "
                + error.localizedDescription)
        }
        return chosen.map(\.name)
    }

    /// What an import added and what it passed over, each with its reason.
    struct ImportResult {
        var imported: [String] = []
        var skipped: [[String: Any]] = []
    }

    /// Reads one gradient file into the library. Each entry goes through the
    /// library file's own strict decode; one that fails is skipped by name and
    /// reason, never a refusal of the whole file. A name already taken
    /// becomes “Name 2”, “Name 3”…
    static func importFile(_ url: URL) throws -> ImportResult {
        let object = try readFile(url)
        guard let entries = object["gradients"] as? [Any] else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) holds no gradients list.")
        }
        guard entries.count <= maximumFileGradients else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) holds more than "
                + "\(maximumFileGradients) gradients.")
        }
        var result = ImportResult()
        var taken = Set(((try? Library.entries()) ?? []).map { Library.key($0.name) })
        cache = nil
        defer { cache = nil }
        for (index, raw) in entries.enumerated() {
            let label = (raw as? [String: Any])?["name"] as? String ?? "entry \(index + 1)"
            do {
                guard let entry = raw as? [String: Any] else {
                    throw DoctoredCoreError(message: "The entry is not a JSON object.")
                }
                let decoded = try GradientPreset.decode(entry)
                var name = decoded.name
                var number = 2
                while taken.contains(Library.key(name)) {
                    name = "\(decoded.name) \(number)"
                    number += 1
                }
                let preset = try GradientPreset.validated(
                    name: name, stops: decoded.stops, smoothness: decoded.smoothness)
                try Library.save(preset)
                taken.insert(Library.key(preset.name))
                result.imported.append(preset.name)
            } catch {
                result.skipped.append(["name": label, "reason": error.localizedDescription])
            }
        }
        return result
    }

    /// The file's top-level object, bounded and checked for the format key.
    private static func readFile(_ url: URL) throws -> [String: Any] {
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        guard let size = (attributes?[.size] as? NSNumber)?.intValue else {
            throw DoctoredCoreError(message: "No readable file at \(url.path).")
        }
        guard size <= maximumFileBytes else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) is larger than the "
                + "\(maximumFileBytes / 1_048_576) MiB gradient file limit.")
        }
        guard let data = FileManager.default.contents(atPath: url.path),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              NativeBrushStroke.jsonNumber(object[fileFormatKey]) == 1 else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) is not a Doctored gradient "
                + "file (\(fileFormatKey) 1).")
        }
        return object
    }

    // MARK: - `{"name": …}` in place of stops

    /// The library gradient a `stops` value names: `{"name": …}`, that one key alone.
    private static func referencedName(_ value: Any?) -> String? {
        guard let object = value as? [String: Any], object.count == 1,
              let name = object["name"] as? String else { return nil }
        return name
    }

    /// `arguments` with a library gradient named in place of a stops array
    /// resolved to the gradient's stops — `gradient`'s `stops`, and a fill
    /// layer's `gradient.stops` — and its Smoothness where the call gives none.
    /// Asked once by the MCP dispatcher before any handler, so the handlers at
    /// every depth parse plain stops and the recorded step carries them.
    static func resolvedArguments(tool: String, arguments: [String: Any]) throws -> [String: Any] {
        var out = arguments
        switch tool {
        case "gradient":
            guard let name = referencedName(arguments["stops"]) else { return arguments }
            let preset = try named(name)
            out["stops"] = preset.stopArguments
            if arguments["smoothness"] == nil || arguments["smoothness"] is NSNull {
                out["smoothness"] = preset.smoothness
            }
        case "add_fill_layer", "edit_fill_layer":
            guard var gradient = arguments["gradient"] as? [String: Any],
                  let name = referencedName(gradient["stops"]) else { return arguments }
            let preset = try named(name)
            gradient["stops"] = preset.stopArguments
            if gradient["smoothness"] == nil || gradient["smoothness"] is NSNull {
                gradient["smoothness"] = preset.smoothness
            }
            out["gradient"] = gradient
        default:
            return arguments
        }
        return out
    }
}
