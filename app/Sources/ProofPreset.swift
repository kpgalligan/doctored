import AppKit

// A preset stores the profile's name, path and SHA-256 — never its bytes — and `resolvedSetup()`
// accepts the saved path's bytes, else `InstalledProfiles.named`'s, only when the digest matches,
// and otherwise refuses with the "has changed or moved" sentence; `matches` (digest + every
// simulation field, no I/O) is the menu checkmark and the sheet's "Custom" test. The default proof
// preset (`PreferenceLibraryDefault`) seeds LAZILY — ⌘Y/⇧⌘Y with no setup, the sheet opened with
// none, and `set_proof_setup`/`render_proof`'s last fallback (`baseProofSetup`: preset, profile,
// existing, default) — never at window open; a failure reaches the sheet as its note's first line.

/// A saved soft-proof condition — the profile a window proofs
/// through and how the simulation runs — in a bounded `PreferenceLibrary`
/// (`DR_PROOF_PRESETS_DIR`), reachable from the Proof Setup sheet, View ▸
/// Proof Setup ▸ <name> and MCP (`AgentServer+ProofPresets.swift`).
///
/// A preset stores the profile's NAME, PATH and SHA-256, never its bytes: a
/// press profile can be 640 KB and the library's per-file bound is 4 KiB.
/// `resolvedSetup()` reads the bytes back and accepts them only when their
/// digest is the one saved, so a profile replaced under the preset is refused
/// by name instead of silently proofing through something else.
///
/// Library writes are PREFERENCES, and applying a preset writes VIEW state:
/// no undo step, no dirty flag, never an Action step.
struct ProofPreset: Equatable {
    static let schemaVersion = 1

    let name: String
    let profileName: String
    let profilePath: String
    /// Lowercase hex SHA-256 of the profile's bytes when the preset was saved.
    let profileSHA256: String
    let intent: RenderingIntent
    let blackPointCompensation: Bool
    let simulatePaperWhite: Bool
    let simulateInkBlack: Bool
    /// `#RRGGBB`, `ProofSetup.hex`'s spelling.
    let gamutColor: String

    /// Captures `setup` under `name`, normalized the way the library lists
    /// it, so a padded or decomposed name never saves as an unlistable file.
    init(name: String, setup: ProofSetup) throws {
        let normalized = try PreferenceLibrary<ProofPresetCodec>.normalizedName(name)
        guard let path = setup.path, !path.isEmpty else {
            throw DoctoredCoreError(
                message: "A proof preset needs a profile that came from a file; choose an "
                    + "installed profile or load one from a file first.")
        }
        self.init(name: normalized, profileName: setup.profileName, profilePath: path,
                  profileSHA256: setup.profileSHA256, intent: setup.intent,
                  blackPointCompensation: setup.blackPointCompensation,
                  simulatePaperWhite: setup.simulatePaperWhite,
                  simulateInkBlack: setup.simulateInkBlack,
                  gamutColor: ProofSetup.hex(setup.gamutColor))
    }

    private init(name: String, profileName: String, profilePath: String, profileSHA256: String,
                 intent: RenderingIntent, blackPointCompensation: Bool,
                 simulatePaperWhite: Bool, simulateInkBlack: Bool, gamutColor: String) {
        self.name = name
        self.profileName = profileName
        self.profilePath = profilePath
        self.profileSHA256 = profileSHA256
        self.intent = intent
        self.blackPointCompensation = blackPointCompensation
        self.simulatePaperWhite = simulatePaperWhite
        self.simulateInkBlack = simulateInkBlack
        self.gamutColor = gamutColor
    }

    var json: [String: Any] {
        ["schema_version": Self.schemaVersion, "name": name,
         "profile_name": profileName, "profile_path": profilePath,
         "profile_sha256": profileSHA256, "intent": intent.rawValue,
         "black_point_compensation": blackPointCompensation,
         "simulate_paper_white": simulatePaperWhite,
         "simulate_ink_black": simulateInkBlack, "gamut_color": gamutColor]
    }

    /// Strict: the exact key set, typed values, a real boolean for each flag,
    /// a 64-digit lowercase digest and a `#RRGGBB` colour. Anything else
    /// throws, and the library lists the file as one `invalid_schema` row.
    static func decode(_ object: [String: Any]) throws -> ProofPreset {
        func flag(_ key: String) -> Bool? {
            guard let value = object[key] as? NSNumber,
                  CFGetTypeID(value) == CFBooleanGetTypeID() else { return nil }
            return value.boolValue
        }
        guard Set(object.keys) == ["schema_version", "name", "profile_name", "profile_path",
                                   "profile_sha256", "intent", "black_point_compensation",
                                   "simulate_paper_white", "simulate_ink_black", "gamut_color"],
              let name = object["name"] as? String,
              let profileName = object["profile_name"] as? String, !profileName.isEmpty,
              let profilePath = object["profile_path"] as? String, profilePath.hasPrefix("/"),
              let digest = object["profile_sha256"] as? String, digest.count == 64,
              digest.allSatisfy({ "0123456789abcdef".contains($0) }),
              let intentName = object["intent"] as? String,
              let intent = RenderingIntent(rawValue: intentName),
              let bpc = flag("black_point_compensation"),
              let paper = flag("simulate_paper_white"),
              let ink = flag("simulate_ink_black"),
              let gamut = object["gamut_color"] as? String, gamut.count == 7, gamut.hasPrefix("#"),
              let color = ProofSetup.color(hex: gamut) else {
            throw DoctoredCoreError(message: "Invalid or unsupported proof preset.")
        }
        return ProofPreset(name: name, profileName: profileName, profilePath: profilePath,
                           profileSHA256: digest, intent: intent, blackPointCompensation: bpc,
                           simulatePaperWhite: paper, simulateInkBlack: ink,
                           gamutColor: ProofSetup.hex(color))
    }

    /// The sentence a preset whose profile can no longer be found answers.
    private var staleRefusal: String {
        "the profile “\(profileName)” has changed or moved since proof preset “\(name)” was "
            + "saved — choose it again in Proof Setup and re-save the preset"
    }

    /// The setup this preset describes, with bytes read NOW: the saved path
    /// when its bytes still carry the saved digest, else the installed
    /// profile of the same name when ITS bytes do. The resolved `path` is
    /// whichever source matched.
    func resolvedSetup() throws -> ProofSetup {
        var source: (bytes: Data, path: String)?
        if let bytes = Self.read(URL(fileURLWithPath: profilePath)),
           ProofSetup.sha256Hex(bytes) == profileSHA256 {
            source = (bytes, profilePath)
        } else if let installed = InstalledProfiles.named(profileName),
                  let bytes = Self.read(installed.url),
                  ProofSetup.sha256Hex(bytes) == profileSHA256 {
            source = (bytes, installed.path)
        }
        guard let source else { throw DoctoredCoreError(message: staleRefusal) }
        guard let description = RasterProfile.describe(source.bytes) else {
            throw DoctoredCoreError(message: staleRefusal)
        }
        guard description.proofable else {
            throw DoctoredCoreError(
                message: "“\(profileName)” cannot be proofed through — "
                    + (description.proofRefusal
                        ?? "it needs both a device-to-PCS and a PCS-to-device table."))
        }
        var setup = ProofSetup(profile: source.bytes, profileName: profileName, path: source.path)
        setup.intent = intent
        setup.blackPointCompensation = blackPointCompensation
        setup.simulatePaperWhite = simulatePaperWhite
        setup.simulateInkBlack = simulateInkBlack
        if let color = ProofSetup.color(hex: gamutColor) { setup.gamutColor = color }
        return setup
    }

    /// True when `setup` proofs exactly what this preset describes — the
    /// profile's digest and every simulation field. No I/O: the menu's
    /// checkmark and the sheet's popup ask it on every validation.
    func matches(_ setup: ProofSetup?) -> Bool {
        guard let setup else { return false }
        return setup.profileSHA256 == profileSHA256
            && setup.intent == intent
            && setup.blackPointCompensation == blackPointCompensation
            && setup.simulatePaperWhite == simulatePaperWhite
            && setup.simulateInkBlack == simulateInkBlack
            && ProofSetup.hex(setup.gamutColor) == gamutColor
    }

    /// True when the profile lives where `InstalledProfiles` looks, so the
    /// installed fallback can find it again if the file is replaced.
    var profileIsInstalled: Bool { Self.isInstalled(path: profilePath) }

    static func isInstalled(path: String) -> Bool {
        let roots = InstalledProfiles.folders + [InstalledProfiles.printerRoot]
        return roots.contains { root in
            let prefix = root.standardizedFileURL.path
            return path.hasPrefix(prefix.hasSuffix("/") ? prefix : prefix + "/")
        }
    }

    /// The largest profile a preset reads back — `InstalledProfiles`' own bound.
    private static let maximumProfileBytes = 16 * 1024 * 1024

    private static func read(_ url: URL) -> Data? {
        guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
              size > 0, size <= maximumProfileBytes,
              let bytes = try? Data(contentsOf: url), !bytes.isEmpty else { return nil }
        return bytes
    }
}

/// The proof-preset library's codec on the shared `PreferenceLibrary`
/// contract: digest filenames, the advisory lock, atomic publication and
/// per-entry diagnosis.
enum ProofPresetCodec: PreferenceLibraryCodec {
    typealias Item = ProofPreset

    static let noun = "proof preset"
    static let directoryEnvironment = "DR_PROOF_PRESETS_DIR"
    static let directoryName = "Doctored/Proof Presets"
    static let maximumEntries = 64
    static let maximumFileBytes = 4096
    static let schemaVersions = ProofPreset.schemaVersion...ProofPreset.schemaVersion
    static let didChange = Notification.Name("ProofPresetLibraryDidChange")

    static func name(of item: ProofPreset) -> String { item.name }
    static func json(of item: ProofPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> ProofPreset {
        try ProofPreset.decode(object)
    }
}

/// The bounded versioned proof-preset library and its one default — the
/// preset a window with no setup proofs through on ⌘Y / ⇧⌘Y, in the sheet and
/// over MCP. Its writes are PREFERENCES: outside document Undo and never
/// Action steps.
enum ProofPresetLibrary {
    private typealias Library = PreferenceLibrary<ProofPresetCodec>
    private typealias Default = PreferenceLibraryDefault<ProofPresetCodec>

    static let maximumPresets = ProofPresetCodec.maximumEntries
    static let maximumFileBytes = ProofPresetCodec.maximumFileBytes
    static let didChange = ProofPresetCodec.didChange

    static func validatedFileName(_ file: String) throws -> String {
        try Library.validatedFileName(file)
    }

    static func listing() throws -> PreferenceLibraryListing<ProofPreset> {
        try Library.listing()
    }

    static func entries() throws -> [ProofPreset] {
        try Library.entries()
    }

    static func named(_ name: String) throws -> ProofPreset {
        try Library.named(name)
    }

    @discardableResult
    static func save(_ preset: ProofPreset, replace: Bool = false) throws -> ProofPreset {
        try Library.save(preset, replace: replace)
    }

    static func delete(name: String) throws {
        try Default.delete(name: name)
    }

    static func delete(file: String) throws {
        try Default.delete(file: file)
    }

    static func defaultPresetName() throws -> String? {
        try Default.name()
    }

    static func defaultPreset() throws -> ProofPreset? {
        guard let name = try defaultPresetName() else { return nil }
        return try named(name)
    }

    static func setDefault(name: String?) throws {
        try Default.set(name: name)
    }
}
