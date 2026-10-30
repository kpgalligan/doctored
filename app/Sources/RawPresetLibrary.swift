import Foundation
import CoreFoundation
import Darwin

// The bounded JSON library uses atomic publication and a cross-instance lock; editing a preset
// cannot reinterpret a saved document or recorded Action. Strictness is PER ENTRY: `listing()`
// answers `{entries, broken}` where each bad file carries its own `code`/`reason`/`repairable` and
// only LIBRARY-level faults (unreadable folder, >256 entries, >128 files, busy lock) still throw.
// `repairable` is a dry run of the very `repairPlan` `repair(file:)` uses, computed in a post-pass
// once the entries and the occupied file names are known — never inside the per-file `diagnose`,
// which cannot see a name collision.
//
// A save under a new name, and a repair, both REFUSE a canonical digest file another file already
// holds rather than overwriting it.

/// The RAW preset library's own codec on the shared `PreferenceLibrary`
/// contract. Everything generic — the folder, the lock, the bounded reader,
/// the fault table, the duplicate-name post-pass, delete by name and by file —
/// lives there; this file keeps only what is RAW's: the schema-1/2/3/4 rules and
/// the field-by-field `repair(file:)` no other library has.
enum RawPresetCodec: PreferenceLibraryCodec {
    typealias Item = RawPreset

    static let noun = "RAW preset"
    static let directoryEnvironment = "DR_RAW_PRESETS_DIR"
    static let directoryName = "Doctored/RAW Presets"
    static let maximumEntries = 128
    static let maximumFileBytes = 8192
    static let schemaVersions = 1...4
    static let didChange = Notification.Name("RawPresetLibraryDidChange")

    static func name(of item: RawPreset) -> String { item.name }
    static func json(of item: RawPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> RawPreset {
        guard Set(object.keys) == ["schema_version", "name", "raw"] else {
            throw DoctoredCoreError(message: "A RAW preset holds exactly schema_version, name and raw.")
        }
        guard let name = object["name"] as? String else {
            throw DoctoredCoreError(message: "A RAW preset needs a name.")
        }
        guard let settings = object["raw"] as? [String: Any] else {
            throw DoctoredCoreError(message: "A RAW preset needs a raw settings object.")
        }
        // An older file may not carry a control a later schema introduced:
        // the older meaning is not a subset of the newer one.
        guard schemaVersion >= 2
            || Set(settings.keys).isDisjoint(with: ["baseline_compensation", "moire", "vignette"]) else {
            throw DoctoredCoreError(message: "This RAW preset uses schema 1 with controls that require schema 2.")
        }
        guard schemaVersion >= 3 || settings["grain"] == nil else {
            throw DoctoredCoreError(message: "This RAW preset uses schema \(schemaVersion) with grain, which requires schema 3.")
        }
        guard schemaVersion >= 4
            || Set(settings.keys).isDisjoint(with: ["texture", "clarity", "dehaze"]) else {
            throw DoctoredCoreError(message: "This RAW preset uses schema \(schemaVersion) with texture, clarity or dehaze, which requires schema 4.")
        }
        return RawPreset(name: name, settings: try RawSettingsPatch.decode(settings))
    }
}

/// Small separate preference library. Native documents retain effective recipes,
/// so editing/deleting a preset cannot reinterpret saved pixels or Actions.
enum RawPresetLibrary {
    private typealias Library = PreferenceLibrary<RawPresetCodec>

    static let maximumPresets = RawPresetCodec.maximumEntries
    static let maximumFileBytes = RawPresetCodec.maximumFileBytes
    static let didChange = RawPresetCodec.didChange

    static var directory: URL { Library.directory }

    static func normalizedName(_ name: String) throws -> String {
        try Library.normalizedName(name)
    }

    /// Why ONE file is not a listed preset. The raw values are the MCP `code`
    /// field: a caller decides what to offer from the code, not from prose.
    typealias Fault = PreferenceLibraryFault
    /// One bad file, with its own `code`/`reason`/`repairable`.
    typealias Broken = PreferenceLibraryBroken

    /// One listed preset AND the file it came from. `schemaVersion` is the
    /// ON-DISK version, so a v1 entry reads 1 until it is saved or repaired.
    struct Entry {
        let file: String
        let schemaVersion: Int
        let preset: RawPreset
        var json: [String: Any] {
            var result = preset.json
            result["schema_version"] = schemaVersion
            result["file"] = file
            return result
        }
        init(file: String, schemaVersion: Int, preset: RawPreset) {
            self.file = file
            self.schemaVersion = schemaVersion
            self.preset = preset
        }
        init(_ entry: PreferenceLibraryEntry<RawPreset>) {
            self.init(file: entry.file, schemaVersion: entry.schemaVersion, preset: entry.item)
        }
    }

    struct Listing {
        let entries: [Entry]
        let broken: [Broken]
    }

    private struct RepairPlan {
        let name: String
        let settings: RawDevelopSettings
        let dropped: [String]
        let target: URL
    }

    /// A file name from a listing row: a bare `*.json` basename in the library
    /// folder and nothing else.
    static func validatedFileName(_ file: String) throws -> String {
        try Library.validatedFileName(file)
    }

    /// The dry run the shared listing hangs `repairable` on — the very plan
    /// `repair(file:)` uses, so the flag and the tool can never disagree.
    private static func repairable(
        _ file: URL, _ entries: [PreferenceLibraryEntry<RawPreset>], _ occupied: Set<String>
    ) -> Bool {
        (try? repairPlan(file: file, entries: entries.map(Entry.init), occupied: occupied)) != nil
    }

    /// What a repair WOULD write, or a refusal that names the fault. Shared by
    /// the listing's dry run and `repair(file:)`.
    private static func repairPlan(file: URL, entries: [Entry], occupied: Set<String>) throws -> RepairPlan {
        let basename = file.lastPathComponent
        let data: Data
        switch Library.readBytes(file) {
        case .success(let bytes): data = bytes
        case .failure(let bad):
            throw DoctoredCoreError(message: "Cannot repair \(basename) (\(bad.fault.rawValue)): \(bad.reason)")
        }
        guard let parsed = try? JSONSerialization.jsonObject(with: data), let object = parsed as? [String: Any] else {
            throw DoctoredCoreError(message: "Cannot repair \(basename) (\(Fault.notJSON.rawValue)): it does not contain a JSON object.")
        }
        guard let version = object["schema_version"] as? NSNumber, CFGetTypeID(version) != CFBooleanGetTypeID(),
              version.doubleValue == Double(version.intValue), (1...4).contains(version.intValue) else {
            throw DoctoredCoreError(message: "Cannot repair \(basename) (\(Fault.unknownVersion.rawValue)): only schema versions 1, 2, 3 and 4 can be rewritten.")
        }
        guard let stored = object["name"] as? String, let name = try? normalizedName(stored) else {
            throw DoctoredCoreError(message: "Cannot repair \(basename) (\(Fault.invalidSchema.rawValue)): it carries no usable preset name.")
        }
        guard let settings = object["raw"] as? [String: Any] else {
            throw DoctoredCoreError(message: "Cannot repair \(basename) (\(Fault.invalidSchema.rawValue)): its raw settings are not an object.")
        }
        let recovered: (settings: RawDevelopSettings, dropped: [String])
        do { recovered = try RawSettingsPatch.recover(settings) } catch {
            throw DoctoredCoreError(message: "Cannot repair \(basename) (\(Fault.invalidSchema.rawValue)): no recoverable controls.")
        }
        guard !entries.contains(where: {
            Library.key($0.preset.name) == Library.key(name) && $0.file != basename
        }) else {
            throw DoctoredCoreError(message: "Cannot repair \(basename) (\(Fault.duplicateName.rawValue)): a RAW preset named “\(name)” is already listed.")
        }
        let target = Library.url(name)
        guard target.lastPathComponent == basename || !occupied.contains(target.lastPathComponent) else {
            throw DoctoredCoreError(message: "RAW preset file \(target.lastPathComponent) is already in use; repair or delete it first.")
        }
        return RepairPlan(name: name, settings: recovered.settings, dropped: recovered.dropped, target: target)
    }

    static func listing() throws -> Listing {
        let listed = try Library.listing(repairable: repairable)
        return Listing(entries: listed.entries.map(Entry.init), broken: listed.broken)
    }

    static func entries() throws -> [RawPreset] {
        try listing().entries.map(\.preset)
    }

    static func named(_ name: String) throws -> RawPreset {
        let normalized = try normalizedName(name)
        guard let preset = try entries().first(where: {
            Library.key($0.name) == Library.key(normalized)
        }) else {
            throw DoctoredCoreError(message: "No RAW preset named “\(normalized)”.")
        }
        return preset
    }

    @discardableResult
    static func save(name: String, settings: RawDevelopSettings, replace: Bool = false) throws -> RawPreset {
        let preset = RawPreset(name: try normalizedName(name), settings: try RawSettingsPatch.validated(settings))
        return try Library.save(preset, replace: replace)
    }

    static func delete(name: String) throws {
        try Library.delete(name: name)
    }

    /// Removes ONE file by name, valid or broken — the only way to clear a
    /// file that has no preset name to ask for.
    static func delete(file: String) throws {
        try Library.delete(file: file)
    }

    /// Rewrites ONE bad file as a canonical schema-4 preset, keeping every
    /// control that survives its own decode and naming the rest in `dropped`.
    /// The one library-specific operation: only RAW has a field-by-field
    /// recovery to run, so the shared contract carries the `repairable` flag
    /// and nothing else.
    @discardableResult
    static func repair(file: String) throws -> (entry: Entry, dropped: [String]) {
        let name = try validatedFileName(file)
        guard FileManager.default.fileExists(atPath: directory.path) else {
            throw DoctoredCoreError(message: "No RAW preset file \(name).")
        }
        let result: (entry: Entry, dropped: [String]) = try Library.locked(create: false) {
            let (listed, occupied) = try Library.readListing(repairable: repairable)
            let entries = listed.entries.map(Entry.init)
            guard !entries.contains(where: { $0.file == name }) else {
                throw DoctoredCoreError(message: "RAW preset file \(name) is already a readable preset; there is nothing to repair.")
            }
            let source = Library.fileURL(name)
            let plan = try repairPlan(file: source, entries: entries, occupied: occupied)
            let preset = RawPreset(name: plan.name, settings: plan.settings)
            let data = try JSONSerialization.data(withJSONObject: preset.json, options: [.sortedKeys])
            guard data.count <= maximumFileBytes else { throw DoctoredCoreError(message: "RAW preset exceeds 8 KiB.") }
            try DurableFile.write(data, to: plan.target)
            if plan.target.lastPathComponent != name { try FileManager.default.removeItem(at: source) }
            return (Entry(file: plan.target.lastPathComponent, schemaVersion: RawPreset.schemaVersion,
                          preset: preset), plan.dropped)
        }
        NotificationCenter.default.post(name: didChange, object: nil)
        return result
    }
}
