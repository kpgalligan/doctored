import AppKit

// The user's pattern library: Edit ▸ Define Pattern… writes it (`DefinePattern.swift`,
// `EditorViewController+DefinePattern.swift`), Edit ▸ Patterns deletes, imports and exports it
// (`EditorViewController+PatternLibrary.swift`), every pattern picker lists it after the four
// built-ins (`PatternFillRows`, the Fill sheet, Mode ▸ Bitmap's Custom Pattern) and every MCP
// argument that takes a pattern object accepts `{"name": …}` for one of its patterns
// (`resolvingReferences`). A pattern a document uses is EMBEDDED there as a custom tile under its
// name, so no document and no recorded step depends on the library. Over MCP:
// `AgentServer+Patterns.swift`.

/// One library pattern: a name and its tile, straight RGBA8, each side 1…512 — the custom tile a
/// Pattern Overlay, a pattern fill layer and Fill… already read at every depth and in every mode.
struct LibraryPattern {
    static let schemaVersion = 1
    static let maximumSide = 512

    let name: String
    let width: Int
    let height: Int
    let rgba: Data

    /// The library file's object (also an entry of the pattern file).
    var json: [String: Any] {
        ["schema_version": Self.schemaVersion, "name": name, "width": width, "height": height,
         "rgba": rgba.base64EncodedString()]
    }

    /// The tile as the custom `PatternRef` a style, a fill layer or Fill… embeds.
    var ref: PatternRef {
        PatternRef(name: name, width: width, height: height, rgba: rgba.base64EncodedString())
    }

    /// The tile as the core's pattern object (`{name, width, height, rgba}`).
    var patternObject: [String: Any] {
        ["name": name, "width": width, "height": height, "rgba": rgba.base64EncodedString()]
    }

    /// A strict decode: exactly the keys `json` writes, sides 1…512 and `width × height × 4`
    /// bytes of base64. Any other shape throws.
    static func decode(_ object: [String: Any]) throws -> LibraryPattern {
        let keys: Set<String> = ["schema_version", "name", "width", "height", "rgba"]
        guard Set(object.keys).isSubset(of: keys), object.count == keys.count,
              let name = object["name"] as? String,
              let width = NativeBrushStroke.jsonNumber(object["width"]).flatMap({ Int(exactly: $0) }),
              let height = NativeBrushStroke.jsonNumber(object["height"]).flatMap({ Int(exactly: $0) }),
              let text = object["rgba"] as? String
        else {
            throw DoctoredCoreError(message: "A pattern holds schema_version, name, width, height "
                + "and rgba, and nothing else.")
        }
        return try validated(name: name, width: width, height: height,
                             rgba: Data(base64Encoded: text) ?? Data())
    }

    /// A pattern after the library's checks: a 1–120 character name and a tile of sides 1…512
    /// with exactly `width × height × 4` bytes.
    static func validated(name: String, width: Int, height: Int, rgba: Data) throws
        -> LibraryPattern {
        let normalized = try PatternLibrary.Library.normalizedName(name)
        guard (1...maximumSide).contains(width), (1...maximumSide).contains(height) else {
            throw DoctoredCoreError(message: "A pattern tile is 1 to \(maximumSide) pixels a side.")
        }
        guard rgba.count == width * height * 4 else {
            throw DoctoredCoreError(message: "A pattern's rgba must be base64 of width × height × 4 "
                + "straight RGBA8 bytes.")
        }
        return LibraryPattern(name: normalized, width: width, height: height, rgba: rgba)
    }

    /// The tile as a 16-point swatch for a popup row.
    var swatch: NSImage {
        let image = NSImage(size: NSSize(width: 16, height: 16))
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
            samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
            bitmapFormat: [.alphaNonpremultiplied], bytesPerRow: width * 4, bitsPerPixel: 32),
            let pixels = rep.bitmapData
        else { return image }
        rgba.copyBytes(to: pixels, count: rgba.count)
        image.addRepresentation(rep)
        return image
    }
}

/// One row of a pattern picker: a built-in (`preset:<id>`) or a library pattern (`library:<name>`),
/// its swatch, and the pattern a choice embeds — a built-in's `{preset}`, a library pattern's
/// custom tile.
struct PatternChoice {
    let key: String
    let name: String
    let image: NSImage
    let ref: PatternRef

    /// The four built-ins, then the library's patterns (re-read when `reload` is true).
    static func all(reload: Bool = true) -> [PatternChoice] {
        if reload { PatternLibrary.reload() }
        let builtIns = RasterDocument.patternPresets().map {
            PatternChoice(key: "preset:" + $0.id, name: $0.name, image: $0.image,
                          ref: PatternRef(preset: $0.id))
        }
        let library = PatternLibrary.patterns.map {
            PatternChoice(key: "library:" + $0.name, name: $0.name, image: $0.swatch, ref: $0.ref)
        }
        return builtIns + library
    }
}

extension PatternRef {
    /// The tile's straight RGBA8 bytes: a custom tile's own, a built-in's from the core's preset
    /// list; nil for a malformed tile or an unknown preset.
    func tileBytes() -> (width: Int, height: Int, rgba: [UInt8])? {
        if let preset {
            guard let found = RasterDocument.patternPresets().first(where: { $0.id == preset }),
                  let bytes = Data(base64Encoded: found.rgba) else { return nil }
            return (found.width, found.height, [UInt8](bytes))
        }
        guard let width, let height, let rgba, let bytes = Data(base64Encoded: rgba),
              (1...LibraryPattern.maximumSide).contains(width),
              (1...LibraryPattern.maximumSide).contains(height),
              bytes.count == width * height * 4 else { return nil }
        return (width, height, [UInt8](bytes))
    }

    /// The core's pattern object: `{preset}` or `{name, width, height, rgba}`.
    var patternObject: [String: Any] {
        if let preset { return ["preset": preset] }
        var object: [String: Any] = ["name": name ?? "Custom"]
        if let width { object["width"] = width }
        if let height { object["height"] = height }
        if let rgba { object["rgba"] = rgba }
        return object
    }

    /// What a reply echoes: `{preset}`, or the tile's name and size without its pixels.
    var echo: [String: Any] {
        if let preset { return ["preset": preset] }
        return ["name": name ?? "Custom", "width": width ?? 0, "height": height ?? 0]
    }
}

/// The pattern library's codec on the shared `PreferenceLibrary` contract
/// (`PreferenceLibrary.swift`'s header): one `LibraryPattern` per file.
enum PatternLibraryCodec: PreferenceLibraryCodec {
    typealias Item = LibraryPattern

    static let noun = "pattern"
    static let directoryEnvironment = "DR_PATTERNS_DIR"
    static let directoryName = "Doctored/Patterns"
    static let maximumEntries = 128
    /// A 512² tile is 1,398,104 bytes of base64 beside a few keys, written with unescaped
    /// slashes (`PreferenceLibrary.save`): an escaped `\/` would double an opaque white tile.
    static let maximumFileBytes = 1_572_864
    static let schemaVersions = LibraryPattern.schemaVersion...LibraryPattern.schemaVersion
    static let didChange = Notification.Name("PatternLibraryDidChange")

    static func name(of item: LibraryPattern) -> String { item.name }
    static func json(of item: LibraryPattern) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> LibraryPattern {
        try LibraryPattern.decode(object)
    }
}

/// The user's patterns (`DR_PATTERNS_DIR`) and Doctored's own pattern file, which exchanges them
/// between machines: `{"doctored_patterns": 1, "patterns": [<a library file's object>, …]}`,
/// extension `.doctoredpatterns`. Its writes are PREFERENCES: outside document Undo and never
/// Action steps.
///
/// The pickers read `patterns` when they are built, so the decoded list is held here and read
/// from disk only when first asked for, after this instance writes, and when a picker is built
/// (`reload`), which is how another instance's writes arrive.
enum PatternLibrary {
    typealias Library = PreferenceLibrary<PatternLibraryCodec>

    static let didChange = PatternLibraryCodec.didChange
    static let fileExtension = "doctoredpatterns"
    static let fileFormatKey = "doctored_patterns"
    static let maximumFilePatterns = 64
    /// `maximumFilePatterns` library files at their codec's bound.
    static let maximumFileBytes = 96 * 1_048_576

    /// The MCP tools whose arguments may name a library pattern (`resolvingReferences`); the
    /// recorder resolves their steps (`recordedArguments`).
    static let patternTools: Set<String> = [
        "fill_selection", "add_fill_layer", "edit_fill_layer", "set_layer_style", "convert_mode",
    ]

    private static var cache: [LibraryPattern]?

    /// The library's patterns, by name; empty when it cannot be read.
    static var patterns: [LibraryPattern] {
        if let cache { return cache }
        let read = (try? Library.entries()) ?? []
        cache = read
        return read
    }

    /// Drops the held list, so the next read sees the folder as it is now.
    static func reload() {
        cache = nil
    }

    static func listing() throws -> PreferenceLibraryListing<LibraryPattern> {
        try Library.listing()
    }

    /// The library pattern named `name` (case-insensitive), read from disk.
    static func named(_ name: String) throws -> LibraryPattern {
        let normalized = try Library.normalizedName(name)
        guard let pattern = try Library.entries().first(where: {
            Library.key($0.name) == Library.key(normalized)
        }) else {
            throw DoctoredCoreError(message: "No pattern named “\(normalized)”. list_patterns lists "
                + "the library; the four built-ins are {\"preset\": id}.")
        }
        return pattern
    }

    /// Saves a tile under `name`. Refuses a built-in's name or id (a built-in is
    /// `{"preset": id}`, so a library pattern of that name could never be told apart on a
    /// picker) and a name in use unless `replace`.
    @discardableResult
    static func save(name: String, width: Int, height: Int, rgba: Data, replace: Bool) throws
        -> LibraryPattern {
        let pattern = try LibraryPattern.validated(
            name: name, width: width, height: height, rgba: rgba)
        try refuseBuiltInName(pattern.name)
        changed()
        defer { changed() }
        return try Library.save(pattern, replace: replace)
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

    /// Writes the user's patterns — all, or those `names` names — as one pattern file at `url`.
    /// The built-ins are every copy's already, so they are not written. Returns the names written.
    static func exportFile(to url: URL, names: [String]?) throws -> [String] {
        let entries = try Library.entries()
        var chosen = entries
        if let names {
            chosen = try names.map { name in
                let normalized = try Library.normalizedName(name)
                guard let pattern = entries.first(where: {
                    Library.key($0.name) == Library.key(normalized)
                }) else {
                    throw DoctoredCoreError(message: isBuiltInName(normalized)
                        ? "“\(normalized)” is a built-in pattern; every copy of Doctored has it."
                        : "No pattern named “\(normalized)”.")
                }
                return pattern
            }
        }
        guard !chosen.isEmpty else {
            throw DoctoredCoreError(message: "The library holds no patterns to export.")
        }
        guard chosen.count <= maximumFilePatterns else {
            throw DoctoredCoreError(message: "A pattern file holds at most \(maximumFilePatterns) "
                + "patterns; pass names to export fewer.")
        }
        // Each entry is written as its library file is (unescaped slashes), so a file of
        // `maximumFilePatterns` entries stays inside the `maximumFileBytes` an import reads.
        let object: [String: Any] = [fileFormatKey: 1, "patterns": chosen.map(\.json)]
        let data = try JSONSerialization.data(withJSONObject: object,
                                              options: [.sortedKeys, .withoutEscapingSlashes])
        do {
            try data.write(to: url, options: .atomic)
        } catch {
            throw DoctoredCoreError(message: "Could not write the pattern file at \(url.path): "
                + error.localizedDescription)
        }
        return chosen.map(\.name)
    }

    /// What an import added and what it passed over, each with its reason.
    struct ImportResult {
        var imported: [String] = []
        var skipped: [[String: Any]] = []
    }

    /// Reads one pattern file into the library. Each entry goes through the library file's own
    /// strict decode; one that fails is skipped by name and reason, never a refusal of the whole
    /// file. A name already taken (or a built-in's) becomes “Name 2”, “Name 3”…
    static func importFile(_ url: URL) throws -> ImportResult {
        let object = try readFile(url)
        guard let entries = object["patterns"] as? [Any] else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) holds no patterns list.")
        }
        guard entries.count <= maximumFilePatterns else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) holds more than "
                + "\(maximumFilePatterns) patterns.")
        }
        var result = ImportResult()
        var taken = Set(((try? Library.entries()) ?? []).map { Library.key($0.name) })
        changed()
        defer { changed() }
        for (index, raw) in entries.enumerated() {
            let label = (raw as? [String: Any])?["name"] as? String ?? "entry \(index + 1)"
            do {
                guard let entry = raw as? [String: Any] else {
                    throw DoctoredCoreError(message: "The entry is not a JSON object.")
                }
                let decoded = try LibraryPattern.decode(entry)
                let pattern = LibraryPattern(
                    name: freeName(decoded.name, taken: taken), width: decoded.width,
                    height: decoded.height, rgba: decoded.rgba)
                try Library.save(pattern)
                taken.insert(Library.key(pattern.name))
                result.imported.append(pattern.name)
            } catch {
                result.skipped.append(["name": label, "reason": error.localizedDescription])
            }
        }
        return result
    }

    /// `value` with every pattern object that names a library pattern — an object under a
    /// `pattern` key whose one key is `name` — replaced by that pattern's tile object, at any
    /// depth of dictionaries and arrays. `{"preset": …}` and a full custom tile keep their
    /// meaning. Throws for a name the library does not hold.
    static func resolvingReferences(in value: Any) throws -> Any {
        switch value {
        case let object as [String: Any]:
            var out = object
            for (key, child) in object {
                if key == "pattern", let name = referencedName(child) {
                    out[key] = try named(name).patternObject
                } else {
                    out[key] = try resolvingReferences(in: child)
                }
            }
            return out
        case let list as [Any]:
            return try list.map { try resolvingReferences(in: $0) }
        default:
            return value
        }
    }

    /// Whether `value` holds a library reference anywhere (`resolvingReferences`' rule).
    static func containsReference(_ value: Any) -> Bool {
        switch value {
        case let object as [String: Any]:
            return object.contains { key, child in
                (key == "pattern" && referencedName(child) != nil) || containsReference(child)
            }
        case let list as [Any]:
            return list.contains(where: containsReference)
        default:
            return false
        }
    }

    /// One pattern argument (`{preset}`, a custom tile or `{name}`) with a library reference
    /// replaced by its tile object; any other value unchanged.
    static func resolvedPatternArgument(_ value: Any) throws -> Any {
        guard let name = referencedName(value) else { return value }
        return try named(name).patternObject
    }

    /// A recorded step's arguments with every library reference resolved, so a replay never
    /// depends on the library; arguments of any other tool, or that fail to resolve, unchanged.
    /// `set_layer_style` also takes its style as a JSON string, which its handler parses and
    /// resolves (`AgentServer.styleArgument`); such a step records the resolved object.
    static func recordedArguments(_ tool: String, _ arguments: [String: Any]) -> [String: Any] {
        guard patternTools.contains(tool) else { return arguments }
        var parsed = arguments
        if tool == "set_layer_style", let text = arguments["style"] as? String,
           let object = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] {
            parsed["style"] = object
        }
        guard containsReference(parsed),
              let resolved = try? resolvingReferences(in: parsed) as? [String: Any]
        else { return arguments }
        return resolved
    }

    /// The library name a pattern object `{"name": …}` (that one key alone) names.
    private static func referencedName(_ value: Any) -> String? {
        guard let object = value as? [String: Any], object.count == 1,
              let name = object["name"] as? String else { return nil }
        return name
    }

    /// Whether `name` is a built-in's display name or id.
    static func isBuiltInName(_ name: String) -> Bool {
        RasterDocument.patternPresets().contains {
            Library.key($0.name) == Library.key(name) || Library.key($0.id) == Library.key(name)
        }
    }

    private static func refuseBuiltInName(_ name: String) throws {
        guard isBuiltInName(name) else { return }
        throw DoctoredCoreError(message: "“\(name)” is a built-in pattern's name; choose another.")
    }

    /// `name`, or the first of “name 2”, “name 3”… that neither a built-in nor `taken` holds.
    private static func freeName(_ name: String, taken: Set<String>) -> String {
        func free(_ candidate: String) -> Bool {
            !taken.contains(Library.key(candidate)) && !isBuiltInName(candidate)
        }
        guard !free(name) else { return name }
        var number = 2
        while !free("\(name) \(number)") { number += 1 }
        return "\(name) \(number)"
    }

    /// The file's top-level object, bounded and checked for the format key.
    private static func readFile(_ url: URL) throws -> [String: Any] {
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        guard let size = (attributes?[.size] as? NSNumber)?.intValue else {
            throw DoctoredCoreError(message: "No readable file at \(url.path).")
        }
        guard size <= maximumFileBytes else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) is larger than the "
                + "\(maximumFileBytes / 1_048_576) MiB pattern file limit.")
        }
        guard let data = FileManager.default.contents(atPath: url.path),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              NativeBrushStroke.jsonNumber(object[fileFormatKey]) == 1 else {
            throw DoctoredCoreError(message: "\(url.lastPathComponent) is not a Doctored pattern file "
                + "(\(fileFormatKey) 1).")
        }
        return object
    }

    /// Drops the held list after a write (or a refused one). Each write that lands posts
    /// `didChange` itself (`PreferenceLibrary.save` / `delete`).
    private static func changed() {
        cache = nil
    }
}
