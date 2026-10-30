import Foundation
import CoreFoundation
import CryptoKit
import Darwin

// A preference library is a folder of digest-named `.json` files under an env-overridable
// Application Support path, an advisory cross-instance `flock`, atomic publication, a per-file
// bound, and **strictness PER ENTRY** — one bad file is a diagnosed `broken` row (`{file, code,
// reason, repairable}`), never a refusal of the whole library. A library supplies a
// `PreferenceLibraryCodec` (its `noun`, from which EVERY sentence is built so two libraries cannot
// word one refusal two ways; its bounds, its schema range, and how one item becomes JSON and back)
// and gets the lock, the bounded reader, the fault table, the duplicate-name post-pass, the
// occupied-slot refusal, `delete(name:)` and `delete(file:)` for free. The twelve codecs are
// `RawPresetCodec` (128 × 8 KiB, schemas 1…2),
// `ExportPresetCodec` (64 × 4 KiB, 1…2), `MetadataPolicyCodec` (64 × 4 KiB, 1…1),
// `NewDocumentPresetCodec` (64 × 4 KiB, 1…1, `DR_DOCUMENT_PRESETS_DIR`), `ShortcutCodec` (4 × 32
// KiB, 1…1, `DR_SHORTCUTS_DIR`), `WorkspaceCodec` (32 × 4 KiB, 1…1, `DR_WORKSPACES_DIR`),
// `ProofPresetCodec` (64 × 4 KiB, 1…1, `DR_PROOF_PRESETS_DIR`), `AdjustmentPresetCodec` (256 ×
// 16 KiB, 1…1, `DR_ADJUSTMENT_PRESETS_DIR`), `DevelopFilterPresetCodec` (128 × 4 KiB, 1…1,
// `DR_DEVELOP_FILTER_PRESETS_DIR`), `UserSkyCodec` (64 × 4 KiB, 1…1, `DR_SKIES_DIR`),
// `StylePresetCodec` (256 × 2.25 MiB, 1…1, `DR_STYLE_PRESETS_DIR`) and `HdrToningPresetCodec`
// (128 × 4 KiB, 1…1, `DR_HDR_TONING_PRESETS_DIR`).
// `PreferenceLibraryDefault<Codec>`
// is a library's ONE default entry: its name in a hidden `.default` file (512-byte bound, never a
// listed or broken row), `name()` answering nil for a name that no longer lists, and
// `delete(name:)` / `delete(file:)` clearing a default that names the deleted entry; Export's Quick
// Export default and the proof-preset default use it. REPAIR stays RAW-only — only RAW has a
// field-by-field recovery to run — so `readListing(repairable:)` takes the owning library's dry run
// as that flag and every other library reports `false`; `RawPresetLibrary` keeps
// `Fault`/`Broken`/`Entry`/`Listing`/`repair(file:)` as wrappers so no caller's signature moved.

/// THE bounded preference-library contract, lifted out of `RawPresetLibrary`
/// in 12C so every library speaks it: a folder of digest-named `.json` files
/// under an env-overridable Application Support path, an advisory
/// cross-instance `flock`, atomic publication, a per-file bound, and
/// **strictness PER ENTRY** — one bad file is a diagnosed row, never a refusal
/// of the whole library.
///
/// A library supplies a `PreferenceLibraryCodec`: its noun (every sentence is
/// built from it, so two libraries cannot word one refusal two ways), its
/// bounds, its schema range and how one item becomes JSON and back. Everything
/// else — the lock, the reader, the fault table, the duplicate-name post-pass,
/// the occupied-slot refusal, delete by name and delete by FILE — lives here
/// once.
///
/// Library writes are PREFERENCES: outside document Undo and never Action
/// steps. Repair stays RAW-only (`RawPresetLibrary.repair(file:)`), because
/// only RAW has a field-by-field recovery to run; the shared listing takes a
/// caller's dry run as the `repairable` flag and reports `false` everywhere
/// else.
protocol PreferenceLibraryCodec {
    associatedtype Item

    /// The library's noun in the middle of a sentence — "RAW preset",
    /// "export preset", "metadata policy". Every message is built from it.
    static var noun: String { get }
    /// Its plural, for the two sentences that count things.
    static var plural: String { get }
    /// The environment variable a test instance sets to isolate the folder.
    static var directoryEnvironment: String { get }
    /// The folder under Application Support, e.g. `Doctored/RAW Presets`.
    static var directoryName: String { get }
    static var maximumEntries: Int { get }
    static var maximumFileBytes: Int { get }
    /// Every on-disk `schema_version` this build reads. A file outside it is
    /// REPORTED, never rewritten: this build cannot know what it would drop.
    static var schemaVersions: ClosedRange<Int> { get }
    static var didChange: Notification.Name { get }

    static func name(of item: Item) -> String
    static func json(of item: Item) -> [String: Any]
    /// Strict: every unexpected key, missing field or out-of-range value
    /// THROWS, and the throw becomes one `invalid_schema` row.
    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> Item
}

extension PreferenceLibraryCodec {
    static var plural: String { noun + "s" }
}

/// Why ONE file is not a listed entry. The raw values are the MCP `code`
/// field: a caller decides what to offer from the code, not from prose.
enum PreferenceLibraryFault: String {
    case unreadable, oversize, notJSON = "not_json", unknownVersion = "unknown_version",
         invalidSchema = "invalid_schema", duplicateName = "duplicate_name"
}

/// One bad file. `repairable` is filled in by a dry run of the owning
/// library's repair plan once the whole listing is known, because a repair can
/// be refused by something no single file can see (another entry's name,
/// another file holding the canonical slot). A library with no repair reports
/// `false`.
struct PreferenceLibraryBroken: Error {
    let file: String
    let fault: PreferenceLibraryFault
    let reason: String
    let repairable: Bool
    var json: [String: Any] {
        ["file": file, "code": fault.rawValue, "reason": reason, "repairable": repairable]
    }
}

/// One listed item AND the file it came from. `schemaVersion` is the ON-DISK
/// version, so a v1 entry reads 1 until it is saved or repaired.
struct PreferenceLibraryEntry<Item> {
    let file: String
    let schemaVersion: Int
    let item: Item
}

struct PreferenceLibraryListing<Item> {
    let entries: [PreferenceLibraryEntry<Item>]
    let broken: [PreferenceLibraryBroken]
}

enum PreferenceLibrary<Codec: PreferenceLibraryCodec> {
    typealias Item = Codec.Item
    typealias Entry = PreferenceLibraryEntry<Item>
    typealias Broken = PreferenceLibraryBroken
    typealias Fault = PreferenceLibraryFault
    typealias Listing = PreferenceLibraryListing<Item>

    /// "RAW preset" at the start of a sentence.
    private static var capitalized: String {
        Codec.noun.prefix(1).uppercased() + Codec.noun.dropFirst()
    }
    /// "A RAW preset" / "An export preset" — the article the noun asks for.
    private static var article: String {
        "aeiou".contains(Codec.noun.lowercased().prefix(1)) ? "An" : "A"
    }

    static var directory: URL {
        if let path = ProcessInfo.processInfo.environment[Codec.directoryEnvironment], !path.isEmpty {
            return URL(fileURLWithPath: path, isDirectory: true)
        }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support")
        return support.appendingPathComponent(Codec.directoryName, isDirectory: true)
    }

    static func normalizedName(_ name: String) throws -> String {
        let result = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !result.isEmpty, result.count <= 120, result.utf8.count <= 512,
              result.rangeOfCharacter(from: .controlCharacters) == nil else {
            throw DoctoredCoreError(
                message: "Use a \(Codec.noun) name of 1–120 characters without control characters.")
        }
        return result.precomposedStringWithCanonicalMapping
    }

    static func key(_ name: String) -> String {
        name.folding(options: [.caseInsensitive], locale: Locale(identifier: "en_US_POSIX"))
    }

    /// The canonical file one name owns: a SHA-256 of the case-folded name, so
    /// the same name always lands on the same file and a rename never leaves
    /// two.
    static func url(_ name: String) -> URL {
        let digest = SHA256.hash(data: Data(key(name).utf8)).map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent(digest + ".json")
    }

    static func fileURL(_ file: String) -> URL {
        directory.appendingPathComponent(file)
    }

    /// A file name from a listing row: a bare `*.json` basename in the library
    /// folder and nothing else.
    static func validatedFileName(_ file: String) throws -> String {
        guard !file.isEmpty, file.count <= 255, file != ".", file != "..",
              !file.contains("/"), file.hasSuffix(".json"), file.count > 5,
              file.rangeOfCharacter(from: .controlCharacters) == nil,
              file == (file as NSString).lastPathComponent else {
            throw DoctoredCoreError(
                message: "Use a \(Codec.noun) file name from the listing: a “.json” basename with no “/”.")
        }
        return file
    }

    /// Advisory per-library lock coordinates multiple app instances. Refuse a
    /// busy library immediately instead of blocking the main thread.
    static func locked<T>(create: Bool, _ body: () throws -> T) throws -> T {
        if create {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                                                    attributes: [.posixPermissions: 0o700])
        }
        let lockURL = directory.appendingPathComponent(".library.lock")
        let descriptor = Darwin.open(lockURL.path, O_CREAT | O_RDWR | O_NOFOLLOW | O_NONBLOCK, 0o600)
        guard descriptor >= 0 else {
            throw DoctoredCoreError(message: "Could not open the \(Codec.noun) library lock.")
        }
        defer { Darwin.close(descriptor) }
        var attributes = stat()
        guard fstat(descriptor, &attributes) == 0, attributes.st_mode & S_IFMT == S_IFREG,
              flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
            throw DoctoredCoreError(
                message: "The \(Codec.noun) library is busy or its lock is invalid. Try again.")
        }
        defer { flock(descriptor, LOCK_UN) }
        return try body()
    }

    /// The ONE bounded reader: `O_NOFOLLOW`, a regular file, and never a byte
    /// past the library's limit — a broken entry is read under exactly the
    /// same bounds as a good one, including a repair plan's re-read.
    static func readBytes(_ url: URL) -> Result<Data, Broken> {
        let file = url.lastPathComponent
        func broken(_ fault: Fault, _ reason: String) -> Result<Data, Broken> {
            .failure(Broken(file: file, fault: fault, reason: reason, repairable: false))
        }
        let descriptor = Darwin.open(url.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
        guard descriptor >= 0 else { return broken(.unreadable, "\(file) could not be opened.") }
        let stream = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        defer { try? stream.close() }
        var attributes = stat()
        guard fstat(descriptor, &attributes) == 0, attributes.st_mode & S_IFMT == S_IFREG,
              attributes.st_size > 0 else {
            return broken(.unreadable, "\(file) is not a readable non-empty regular file.")
        }
        guard attributes.st_size <= Codec.maximumFileBytes else {
            return broken(.oversize, "\(file) is larger than the \(Codec.maximumFileBytes)-byte preset limit.")
        }
        guard let data = try? stream.read(upToCount: Codec.maximumFileBytes + 1), !data.isEmpty else {
            return broken(.unreadable, "\(file) could not be read.")
        }
        guard data.count <= Codec.maximumFileBytes else {
            return broken(.oversize, "\(file) is larger than the \(Codec.maximumFileBytes)-byte preset limit.")
        }
        return .success(data)
    }

    /// The PER-FILE fault, and nothing else. `repairable` is false here by
    /// construction: whether a repair would land depends on the rest of the
    /// library, which `readListing` decides afterwards.
    static func diagnose(_ url: URL) -> Result<Entry, Broken> {
        let file = url.lastPathComponent
        func broken(_ fault: Fault, _ reason: String) -> Result<Entry, Broken> {
            .failure(Broken(file: file, fault: fault, reason: reason, repairable: false))
        }
        let data: Data
        switch readBytes(url) {
        case .success(let bytes): data = bytes
        case .failure(let bad): return .failure(bad)
        }
        guard let parsed = try? JSONSerialization.jsonObject(with: data),
              let object = parsed as? [String: Any] else {
            return broken(.notJSON, "\(file) does not contain a JSON object.")
        }
        // A FUTURE version is reported, never rewritten: this build cannot
        // know what it would be discarding.
        guard let version = object["schema_version"] as? NSNumber,
              CFGetTypeID(version) != CFBooleanGetTypeID(),
              version.doubleValue == Double(version.intValue),
              Codec.schemaVersions.contains(version.intValue) else {
            let range = Codec.schemaVersions.lowerBound == Codec.schemaVersions.upperBound
                ? "\(Codec.schemaVersions.lowerBound)"
                : "\(Codec.schemaVersions.lowerBound) to \(Codec.schemaVersions.upperBound)"
            return broken(.unknownVersion, "\(file) carries no supported schema_version (\(range)).")
        }
        guard let item = try? Codec.decode(object, schemaVersion: version.intValue) else {
            return broken(.invalidSchema, "\(file) is not a \(Codec.noun) this build can read.")
        }
        guard (try? normalizedName(Codec.name(of: item))) == Codec.name(of: item) else {
            return broken(.invalidSchema, "\(file) carries no valid normalized \(Codec.noun) name.")
        }
        return .success(Entry(file: file, schemaVersion: version.intValue, item: item))
    }

    /// The whole folder: every listed entry, every bad file, and the set of
    /// `.json` basenames already taken. Library-LEVEL faults still throw — an
    /// unreadable folder, too many entries, a busy lock — because none of them
    /// names a file a caller could remove.
    ///
    /// `repairable` is the owning library's dry run of the very plan its
    /// `repair(file:)` uses, computed in a post-pass once the entries and the
    /// occupied file names are known; `nil` means the library has no repair.
    static func readListing(
        repairable: ((URL, [Entry], Set<String>) -> Bool)? = nil
    ) throws -> (listing: Listing, occupied: Set<String>) {
        guard let files = FileManager.default.enumerator(at: directory,
            includingPropertiesForKeys: [.isRegularFileKey],
            options: [.skipsHiddenFiles, .skipsSubdirectoryDescendants]) else {
            throw DoctoredCoreError(message: "Could not list \(Codec.plural).")
        }
        var urls: [URL] = []
        var visited = 0
        for case let file as URL in files {
            visited += 1
            guard visited <= 256 else {
                throw DoctoredCoreError(message: "The \(Codec.noun) folder contains too many entries.")
            }
            guard file.pathExtension == "json" else { continue }
            urls.append(file)
        }
        guard urls.count <= Codec.maximumEntries else {
            throw DoctoredCoreError(
                message: "The \(Codec.noun) library exceeds \(Codec.maximumEntries) \(Codec.plural).")
        }
        let occupied = Set(urls.map(\.lastPathComponent))
        var valid: [Entry] = []
        var broken: [Broken] = []
        for url in urls.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
            switch diagnose(url) {
            case .success(let entry): valid.append(entry)
            case .failure(let bad): broken.append(bad)
            }
        }
        // Two files claiming one name: the one at the canonical digest name
        // wins, else the lexically first, and the losers become bad files with
        // a name of their own to remove or repair.
        var groups: [String: [Entry]] = [:]
        for entry in valid { groups[key(Codec.name(of: entry.item)), default: []].append(entry) }
        var listed: [Entry] = []
        for group in groups.values {
            let winner = group.first { $0.file == url(Codec.name(of: $0.item)).lastPathComponent }
                ?? group[0]
            listed.append(winner)
            let winnerName = Codec.name(of: winner.item)
            for entry in group where entry.file != winner.file {
                broken.append(Broken(
                    file: entry.file, fault: .duplicateName,
                    reason: "\(entry.file) repeats the name “\(winnerName)”, already listed from \(winner.file).",
                    repairable: false))
            }
        }
        let entries = listed.sorted {
            Codec.name(of: $0.item).localizedStandardCompare(Codec.name(of: $1.item)) == .orderedAscending
        }
        let diagnosed = broken.map { bad in
            Broken(file: bad.file, fault: bad.fault, reason: bad.reason,
                   repairable: repairable?(fileURL(bad.file), entries, occupied) ?? false)
        }.sorted { $0.file < $1.file }
        return (Listing(entries: entries, broken: diagnosed), occupied)
    }

    static func listing(
        repairable: ((URL, [Entry], Set<String>) -> Bool)? = nil
    ) throws -> Listing {
        guard FileManager.default.fileExists(atPath: directory.path) else {
            return Listing(entries: [], broken: [])
        }
        return try locked(create: false) { try readListing(repairable: repairable).listing }
    }

    static func entries() throws -> [Item] {
        try listing().entries.map(\.item)
    }

    static func named(_ name: String) throws -> Item {
        let normalized = try normalizedName(name)
        guard let item = try entries().first(where: { key(Codec.name(of: $0)) == key(normalized) }) else {
            throw DoctoredCoreError(message: "No \(Codec.noun) named “\(normalized)”.")
        }
        return item
    }

    /// Writes one item. Replacing writes to the matched entry's OWN file, so
    /// nothing else is touched; a new name writes to its canonical digest file
    /// — and refuses when some other file already holds that slot, valid or
    /// broken, rather than silently overwriting it. The library's bound counts
    /// `.json` FILES, so a folder full of broken files is full.
    @discardableResult
    static func save(_ item: Item, replace: Bool = false) throws -> Item {
        let name = try normalizedName(Codec.name(of: item))
        let data = try JSONSerialization.data(withJSONObject: Codec.json(of: item), options: [.sortedKeys])
        guard data.count <= Codec.maximumFileBytes else {
            throw DoctoredCoreError(
                message: "\(capitalized) exceeds \(Codec.maximumFileBytes / 1024) KiB.")
        }
        try locked(create: true) {
            let (existing, occupied) = try readListing()
            let match = existing.entries.first { key(Codec.name(of: $0.item)) == key(name) }
            guard match == nil || replace else {
                throw DoctoredCoreError(message: "\(article) \(Codec.noun) named “\(name)” already exists.")
            }
            guard match != nil || occupied.count < Codec.maximumEntries else {
                throw DoctoredCoreError(
                    message: "The \(Codec.noun) library folder holds \(Codec.maximumEntries) files; "
                        + "delete a preset or a broken file first.")
            }
            guard let match else {
                let target = url(name)
                guard !occupied.contains(target.lastPathComponent) else {
                    throw DoctoredCoreError(
                        message: "\(capitalized) file \(target.lastPathComponent) is already in use; "
                            + "repair or delete it first.")
                }
                try DurableFile.write(data, to: target)
                return
            }
            try DurableFile.write(data, to: fileURL(match.file))
        }
        NotificationCenter.default.post(name: Codec.didChange, object: nil)
        return item
    }

    static func delete(name: String) throws {
        let normalized = try normalizedName(name)
        guard FileManager.default.fileExists(atPath: directory.path) else {
            throw DoctoredCoreError(message: "No \(Codec.noun) named “\(normalized)”.")
        }
        try locked(create: false) {
            guard let entry = try readListing().listing.entries.first(where: {
                key(Codec.name(of: $0.item)) == key(normalized)
            }) else {
                throw DoctoredCoreError(message: "No \(Codec.noun) named “\(normalized)”.")
            }
            try FileManager.default.removeItem(at: fileURL(entry.file))
        }
        NotificationCenter.default.post(name: Codec.didChange, object: nil)
    }

    /// Removes ONE file by name, valid or broken — the only way to clear a
    /// file that has no entry name to ask for.
    static func delete(file: String) throws {
        let name = try validatedFileName(file)
        guard FileManager.default.fileExists(atPath: directory.path) else {
            throw DoctoredCoreError(message: "No \(Codec.noun) file \(name).")
        }
        try locked(create: false) {
            let target = fileURL(name)
            let descriptor = Darwin.open(target.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
            guard descriptor >= 0 else {
                throw DoctoredCoreError(message: "No \(Codec.noun) file \(name).")
            }
            var attributes = stat()
            let regular = fstat(descriptor, &attributes) == 0 && attributes.st_mode & S_IFMT == S_IFREG
            Darwin.close(descriptor)
            guard regular else {
                throw DoctoredCoreError(message: "\(capitalized) file \(name) is not a regular file.")
            }
            try FileManager.default.removeItem(at: target)
        }
        NotificationCenter.default.post(name: Codec.didChange, object: nil)
    }
}

/// A library's ONE default entry (Export's Quick Export preset, the proof
/// preset new windows proof through): the NAME lives in a hidden `.default`
/// file beside the entries rather than in an entry, so marking a different
/// entry the default rewrites nothing a recorded step depends on. The listing
/// skips hidden files, so the file is never a broken row. It is bounded
/// (`maximumBytes`) and read without the lock; a name that no longer lists is
/// "no default", and deleting the entry it names clears it.
enum PreferenceLibraryDefault<Codec: PreferenceLibraryCodec> {
    private typealias Library = PreferenceLibrary<Codec>

    static var fileName: String { ".default" }
    static var maximumBytes: Int { 512 }

    private static var url: URL { Library.directory.appendingPathComponent(fileName) }

    /// The stored text, normalized; nil when unset, oversize or unreadable.
    private static func stored() -> String? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
              let size = attributes[.size] as? NSNumber, size.intValue <= maximumBytes,
              let data = FileManager.default.contents(atPath: url.path),
              data.count <= maximumBytes, let text = String(data: data, encoding: .utf8)
        else { return nil }
        return try? Library.normalizedName(text)
    }

    /// The listed entry's own spelling of the default, or nil when none is
    /// set or the stored name names no listed entry.
    static func name() throws -> String? {
        guard FileManager.default.fileExists(atPath: Library.directory.path) else { return nil }
        guard let name = stored() else { return nil }
        guard let item = try Library.entries().first(where: {
            Library.key(Codec.name(of: $0)) == Library.key(name)
        }) else { return nil }
        return Codec.name(of: item)
    }

    /// nil clears the default; otherwise `name` must name a listed entry.
    static func set(name: String?) throws {
        guard let name else {
            try Library.locked(create: true) {
                if FileManager.default.fileExists(atPath: url.path) {
                    try FileManager.default.removeItem(at: url)
                }
            }
            NotificationCenter.default.post(name: Codec.didChange, object: nil)
            return
        }
        let resolved = Codec.name(of: try Library.named(name))
        try Library.locked(create: true) {
            try DurableFile.write(Data(resolved.utf8), to: url)
        }
        NotificationCenter.default.post(name: Codec.didChange, object: nil)
    }

    /// `PreferenceLibrary.delete(name:)`, then clears a default naming it: a
    /// stale name would make every reader refuse with the wrong sentence.
    static func delete(name: String) throws {
        let normalized = try Library.normalizedName(name)
        try Library.delete(name: normalized)
        clear(naming: normalized)
    }

    /// `PreferenceLibrary.delete(file:)`, then clears a default naming the
    /// entry that file held — read from the listing BEFORE the delete.
    static func delete(file: String) throws {
        let validated = try Library.validatedFileName(file)
        let name = (try? Library.listing())?.entries
            .first { $0.file == validated }
            .map { Codec.name(of: $0.item) }
        try Library.delete(file: validated)
        if let name { clear(naming: name) }
    }

    private static func clear(naming name: String) {
        guard let current = stored(), Library.key(current) == Library.key(name) else { return }
        try? Library.locked(create: false) {
            try? FileManager.default.removeItem(at: url)
        }
    }
}

extension AgentServer {
    /// The `file` argument every preference library's delete tool takes, in
    /// one place so the three families cannot word it three ways. A broken
    /// entry has no name, which is the whole reason the argument exists.
    func libraryFileProperty(_ listTool: String) -> [String: Any] {
        ["type": "string",
         "description": "A library file basename from \(listTool) (a row's `file`). "
             + "Pass exactly one of name or file; a broken entry has no name."]
    }
}
