import Foundation

/// The brush preset library's codec on the shared `PreferenceLibrary` contract
/// (`PreferenceLibrary.swift`'s header): one `BrushPreset` per file.
enum BrushPresetCodec: PreferenceLibraryCodec {
    typealias Item = BrushPreset

    static let noun = "brush preset"
    static let directoryEnvironment = "DR_BRUSH_PRESETS_DIR"
    static let directoryName = "Doctored/Brush Presets"
    static let maximumEntries = 256
    static let maximumFileBytes = 4096
    static let schemaVersions = BrushPreset.schemaVersion...BrushPreset.schemaVersion
    static let didChange = Notification.Name("BrushPresetLibraryDidChange")

    static func name(of item: BrushPreset) -> String { item.name }
    static func json(of item: BrushPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> BrushPreset {
        try BrushPreset.decode(object)
    }
}

/// The user's brush presets (`DR_BRUSH_PRESETS_DIR`) and Doctored's own preset
/// file, which exchanges them between machines: `{"doctored_brush_presets": 1,
/// "presets": [<a library file's object>, …]}`, extension `.doctoredbrushes`.
/// Its writes are PREFERENCES: outside document Undo and never Action steps.
///
/// The paint options bar reads `presets` on every refresh, so the decoded
/// list is held here and read from disk only when it is first asked for,
/// after this instance writes, and when a bar is presented (`reload`), which
/// is how another instance's writes arrive.
enum BrushPresetLibrary {
    typealias Library = PreferenceLibrary<BrushPresetCodec>

    static let didChange = BrushPresetCodec.didChange
    static let fileExtension = "doctoredbrushes"
    static let fileFormatKey = "doctored_brush_presets"
    static let maximumFileBytes = 1_048_576
    static let maximumFilePresets = 256

    private static var cache: [BrushPreset]?

    /// The library's presets, by name; empty when it cannot be read.
    static var presets: [BrushPreset] {
        if let cache { return cache }
        let read = (try? Library.entries()) ?? []
        cache = read
        return read
    }

    /// Drops the held list, so the next read sees the folder as it is now.
    static func reload() {
        cache = nil
    }

    static func listing() throws -> PreferenceLibraryListing<BrushPreset> {
        try Library.listing()
    }

    /// Saves the six values under `name`. Refuses a built-in's name, a name in
    /// use (unless `replace`), and values another preset already holds: the
    /// popup finds a preset by its values, so a repeat could never be chosen.
    @discardableResult
    static func save(name: String, values: [String: Any], replace: Bool) throws -> BrushPreset {
        let preset = try BrushPreset.validated(name: name, values: values)
        try refuseRepeat(of: preset, in: (try? Library.entries()) ?? [])
        changed()
        defer { changed() }
        return try Library.save(preset, replace: replace)
    }

    static func delete(name: String) throws {
        changed()
        defer { changed() }
        try Library.delete(name: name)
    }

    static func delete(file: String) throws {
        changed()
        defer { changed() }
        try Library.delete(file: try Library.validatedFileName(file))
    }

    /// Writes the user's presets — all, or those `names` names — as one preset
    /// file at `url`. The built-ins are every copy's already, so they are not
    /// written. Returns the names written.
    static func exportFile(to url: URL, names: [String]?) throws -> [String] {
        let entries = try Library.entries()
        var chosen = entries
        if let names {
            chosen = try names.map { name in
                let normalized = try Library.normalizedName(name)
                guard let preset = entries.first(where: { Library.key($0.name) == Library.key(normalized) }) else {
                    throw DoctoredCoreError(message: BrushPreset.builtIns.contains {
                        Library.key($0.name) == Library.key(normalized) }
                        ? "“\(normalized)” is a built-in brush preset; every copy of Doctored has it."
                        : "No brush preset named “\(normalized)”.")
                }
                return preset
            }
        }
        guard !chosen.isEmpty else {
            throw DoctoredCoreError(message: "The library holds no brush presets to export.")
        }
        let object: [String: Any] = [fileFormatKey: 1, "presets": chosen.map(\.json)]
        let data = try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
        do {
            try data.write(to: url, options: .atomic)
        } catch {
            throw DoctoredCoreError(message: "Could not write the brush preset file at \(url.path): "
                + error.localizedDescription)
        }
        return chosen.map(\.name)
    }

    /// What an import added and what it passed over, each with its reason.
    struct ImportResult {
        var imported: [String] = []
        var skipped: [[String: Any]] = []
    }

    /// Reads one preset file into the library. Each entry goes through the
    /// library file's own strict decode; one that fails, or that repeats the
    /// values of a preset already listed (or imported before it), is skipped
    /// by name and reason, never a refusal of the whole file. A name already
    /// taken becomes “Name 2”, “Name 3”…
    static func importFile(_ url: URL) throws -> ImportResult {
        let object = try readFile(url)
        guard let entries = object["presets"] as? [Any] else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) holds no presets list.")
        }
        guard entries.count <= maximumFilePresets else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) holds more than "
                + "\(maximumFilePresets) brush presets.")
        }
        var result = ImportResult()
        var listed = (try? Library.entries()) ?? []
        changed()
        defer { changed() }
        for (index, raw) in entries.enumerated() {
            let label = (raw as? [String: Any])?["name"] as? String ?? "entry \(index + 1)"
            do {
                guard let entry = raw as? [String: Any] else {
                    throw DoctoredCoreError(message: "The entry is not a JSON object.")
                }
                let decoded = try BrushPreset.decode(entry)
                try refuseRepeat(of: decoded, in: listed)
                let preset = BrushPreset(name: freeName(decoded.name, among: listed),
                    hardness: decoded.hardness, flow: decoded.flow, spacing: decoded.spacing,
                    angle: decoded.angle, roundness: decoded.roundness, airbrush: decoded.airbrush)
                try Library.save(preset)
                listed.append(preset)
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
                + "\(maximumFileBytes / 1_048_576) MiB brush preset file limit.")
        }
        guard let data = FileManager.default.contents(atPath: url.path),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              NativeBrushStroke.jsonNumber(object[fileFormatKey]) == 1 else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) is not a Doctored brush preset file "
                + "(\(fileFormatKey) 1).")
        }
        return object
    }

    /// Refuses `preset` when a built-in or another listed preset (not one of
    /// the same name, which a replace overwrites) holds its six values.
    private static func refuseRepeat(of preset: BrushPreset, in listed: [BrushPreset]) throws {
        let others = BrushPreset.builtIns + listed.filter { Library.key($0.name) != Library.key(preset.name) }
        if let twin = others.first(where: { $0.repeats(preset) }) {
            throw DoctoredCoreError(message: "The brush preset “\(twin.name)” already holds these values.")
        }
    }

    /// `name`, or the first of “name 2”, “name 3”… that neither a built-in nor
    /// a listed preset holds.
    private static func freeName(_ name: String, among listed: [BrushPreset]) -> String {
        let taken = Set((BrushPreset.builtIns + listed).map { Library.key($0.name) })
        guard taken.contains(Library.key(name)) else { return name }
        var number = 2
        while taken.contains(Library.key("\(name) \(number)")) { number += 1 }
        return "\(name) \(number)"
    }

    /// Drops the held list after a write (or a refused one: the folder may
    /// have moved under it). Each write that lands posts `didChange` itself
    /// (`PreferenceLibrary.save` / `delete`).
    private static func changed() {
        cache = nil
    }
}
