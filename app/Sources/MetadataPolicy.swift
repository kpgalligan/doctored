import AppKit
import CoreFoundation

// `MetadataPolicy.json` IS the request's `metadata` object and the CORE owns every rule (the
// spellings, the stamp's bounds, the two cross-field refusals), so this side only carries, stores
// and shows it; `MetadataPolicyLibrary` is `ExportPresetLibrary`'s contract at 64 × 4 KiB under
// `DR_METADATA_POLICIES_DIR`, and its writes are PREFERENCES — outside document Undo and never
// Action steps.

/// The Phase 11B metadata policy: what an export keeps, packet by packet, plus
/// an optional author-and-copyright stamp — and the bounded versioned library
/// that stores named ones.
///
/// The CORE owns every rule (which spellings exist, the stamp's bounds, the
/// two cross-field refusals). This side carries the value, stores it and shows
/// it; `MetadataPolicy.json` IS the request's `metadata` object, so a saved
/// policy and what the core parses can never drift.
///
/// Library writes are PREFERENCES: they never enter document Undo and are
/// never Action steps, exactly like `ExportPresetLibrary`.

// MARK: - The policy

/// What one packet's rule says. `default` is 11A's behaviour: defer to the
/// request's `strip_metadata`.
enum PacketRule: String, CaseIterable {
    case `default`
    case keep
    case drop

    var displayName: String {
        switch self {
        case .default: return "Default"
        case .keep: return "Keep"
        case .drop: return "Drop"
        }
    }
}

/// The credit a policy writes into the XMP packet as `dc:creator` /
/// `dc:rights`. At least one field must be present; the core bounds both.
struct MetadataStamp: Equatable {
    var author: String?
    var copyright: String?

    var isEmpty: Bool { (author ?? "").isEmpty && (copyright ?? "").isEmpty }

    var json: [String: Any] {
        [
            "author": author.map { $0 as Any } ?? NSNull(),
            "copyright": copyright.map { $0 as Any } ?? NSNull(),
        ]
    }

    static func decode(_ object: [String: Any]) throws -> MetadataStamp {
        guard Set(object.keys).isSubset(of: ["author", "copyright"]) else {
            throw DoctoredCoreError(message: "A metadata stamp carries an unknown key.")
        }
        func field(_ key: String) throws -> String? {
            guard let value = object[key], !(value is NSNull) else { return nil }
            guard let text = value as? String else {
                throw DoctoredCoreError(message: "A metadata stamp \(key) must be a string.")
            }
            return text
        }
        return MetadataStamp(author: try field("author"), copyright: try field("copyright"))
    }
}

struct MetadataPolicy: Equatable {
    var exif: PacketRule
    var xmp: PacketRule
    var iptc: PacketRule
    var density: PacketRule
    var stamp: MetadataStamp?

    static let defaults = MetadataPolicy(exif: .default, xmp: .default, iptc: .default,
                                         density: .default, stamp: nil)

    /// True when this policy asks for nothing the request does not already
    /// say, so an export can omit the key entirely and write 11A's bytes.
    var isDefault: Bool { self == Self.defaults }

    var json: [String: Any] {
        [
            "exif": exif.rawValue,
            "xmp": xmp.rawValue,
            "iptc": iptc.rawValue,
            "density": density.rawValue,
            "stamp": stamp.map { $0.json as Any } ?? NSNull(),
        ]
    }

    static func decode(_ object: [String: Any]) throws -> MetadataPolicy {
        guard Set(object.keys).isSubset(of: ["exif", "xmp", "iptc", "density", "stamp"]) else {
            throw DoctoredCoreError(message: "A metadata policy carries an unknown key.")
        }
        func rule(_ key: String) throws -> PacketRule {
            guard let value = object[key], !(value is NSNull) else { return .default }
            guard let name = value as? String, let rule = PacketRule(rawValue: name) else {
                throw DoctoredCoreError(
                    message: "A metadata policy \(key) must be default, keep or drop.")
            }
            return rule
        }
        var policy = MetadataPolicy(exif: try rule("exif"), xmp: try rule("xmp"),
                                    iptc: try rule("iptc"), density: try rule("density"),
                                    stamp: nil)
        if let stamp = object["stamp"], !(stamp is NSNull) {
            guard let stamp = stamp as? [String: Any] else {
                throw DoctoredCoreError(message: "A metadata stamp must be an object or null.")
            }
            policy.stamp = try MetadataStamp.decode(stamp)
        }
        return policy
    }

    /// A one-line summary for a notice or a popup's tooltip.
    var summary: String {
        var parts = ["EXIF \(exif.rawValue)", "XMP \(xmp.rawValue)", "IPTC \(iptc.rawValue)",
                     "density \(density.rawValue)"]
        if stamp?.isEmpty == false { parts.append("stamped") }
        return parts.joined(separator: ", ")
    }
}

// MARK: - Named policies

/// One saved policy. Like an export preset it lives OUTSIDE every document, so
/// editing or deleting one can never reinterpret saved pixels or a recorded
/// Action — a recorded step carries the resolved policy, never its name.
struct NamedMetadataPolicy: Equatable {
    static let schemaVersion = 1

    let name: String
    let policy: MetadataPolicy

    var json: [String: Any] {
        ["schema_version": Self.schemaVersion, "name": name, "policy": policy.json]
    }

    static func decode(_ object: [String: Any]) throws -> NamedMetadataPolicy {
        guard Set(object.keys) == ["schema_version", "name", "policy"],
              let version = object["schema_version"] as? NSNumber,
              CFGetTypeID(version) != CFBooleanGetTypeID(),
              version.intValue == schemaVersion,
              version.doubleValue == Double(version.intValue),
              let name = object["name"] as? String,
              let policy = object["policy"] as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid or unsupported metadata policy.")
        }
        return NamedMetadataPolicy(name: try MetadataPolicyLibrary.normalizedName(name),
                                   policy: try MetadataPolicy.decode(policy))
    }
}

/// The metadata-policy library's codec on the shared `PreferenceLibrary`
/// contract: `DR_METADATA_POLICIES_DIR`, digest filenames, an advisory
/// cross-instance `flock`, atomic publication and — since 12C — per-entry
/// diagnosis rather than whole-library strictness.
enum MetadataPolicyCodec: PreferenceLibraryCodec {
    typealias Item = NamedMetadataPolicy

    static let noun = "metadata policy"
    static let plural = "metadata policies"
    static let directoryEnvironment = "DR_METADATA_POLICIES_DIR"
    static let directoryName = "Doctored/Metadata Policies"
    static let maximumEntries = 64
    static let maximumFileBytes = 4096
    static let schemaVersions = NamedMetadataPolicy.schemaVersion...NamedMetadataPolicy.schemaVersion
    static let didChange = Notification.Name("MetadataPolicyLibraryDidChange")

    static func name(of item: NamedMetadataPolicy) -> String { item.name }
    static func json(of item: NamedMetadataPolicy) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> NamedMetadataPolicy {
        try NamedMetadataPolicy.decode(object)
    }
}

/// The bounded versioned metadata-policy library. Its writes are PREFERENCES:
/// outside document Undo and never Action steps.
enum MetadataPolicyLibrary {
    private typealias Library = PreferenceLibrary<MetadataPolicyCodec>

    static let maximumPolicies = MetadataPolicyCodec.maximumEntries
    static let maximumFileBytes = MetadataPolicyCodec.maximumFileBytes
    static let didChange = MetadataPolicyCodec.didChange

    static var directory: URL { Library.directory }

    static func normalizedName(_ name: String) throws -> String {
        try Library.normalizedName(name)
    }

    static func validatedFileName(_ file: String) throws -> String {
        try Library.validatedFileName(file)
    }

    /// Every listed policy and every bad file, each with its own code and
    /// reason; `repairable` is always false (repair stays RAW-only).
    static func listing() throws -> PreferenceLibraryListing<NamedMetadataPolicy> {
        try Library.listing()
    }

    static func entries() throws -> [NamedMetadataPolicy] {
        try Library.entries()
    }

    static func named(_ name: String) throws -> NamedMetadataPolicy {
        try Library.named(name)
    }

    @discardableResult
    static func save(_ entry: NamedMetadataPolicy, replace: Bool = false) throws -> NamedMetadataPolicy {
        let entry = NamedMetadataPolicy(name: try normalizedName(entry.name), policy: entry.policy)
        // The CORE owns the grammar and the bounds; ask it before a bad policy
        // reaches the library, and store the canonical echo it returns.
        let canonical = try RasterExport.validateMetadataPolicy(entry.policy.json)
        let resolved = NamedMetadataPolicy(name: entry.name,
                                           policy: try MetadataPolicy.decode(canonical))
        return try Library.save(resolved, replace: replace)
    }

    static func delete(name: String) throws {
        try Library.delete(name: name)
    }

    static func delete(file: String) throws {
        try Library.delete(file: file)
    }
}

// MARK: - The outcome

/// What an export actually wrote, read out of the result's `metadata` object.
/// Every field is named in the notice and in the MCP result, so the report a
/// user sees and the bytes a decoder finds can be compared directly.
struct MetadataOutcome {
    let profile: Bool
    let exif: Bool
    let xmp: Bool
    let iptc: Bool
    let density: Bool
    let stamped: Bool

    init?(_ info: [String: Any]) {
        guard let object = info["metadata"] as? [String: Any] else { return nil }
        func flag(_ key: String) -> Bool { (object[key] as? NSNumber)?.boolValue ?? false }
        profile = flag("profile")
        exif = flag("exif")
        xmp = flag("xmp")
        iptc = flag("iptc")
        density = flag("density")
        stamped = flag("stamped")
    }

    /// One sentence naming EVERY field, written or not — a report that only
    /// listed what travelled would leave the reader guessing about the rest.
    var sentence: String {
        let fields = [("Profile", profile), ("EXIF", exif), ("XMP", xmp), ("IPTC", iptc),
                      ("Resolution", density), ("Stamp", stamped)]
        let written = fields.filter(\.1).map(\.0)
        let missing = fields.filter { !$0.1 }.map(\.0)
        var parts: [String] = []
        if !written.isEmpty { parts.append("Written: " + written.joined(separator: ", ") + ".") }
        if !missing.isEmpty { parts.append("Not written: " + missing.joined(separator: ", ") + ".") }
        return parts.joined(separator: " ")
    }
}
