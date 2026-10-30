import Foundation

// HDR Toning's presets: the built-in, Default (Local Adaptation at its
// defaults), and the user's own, saved from the dialog's Preset popup (`HdrToningPresetControl`)
// into a `PreferenceLibrary` (`DR_HDR_TONING_PRESETS_DIR`). `hdr_toning`'s `preset` names one; a
// recorded call keeps the preset's explicit values, never its name (`recordedArguments`).

/// One HDR Toning preset: a method and every value it takes. Library writes are PREFERENCES: no
/// undo step, no dirty flag, never an Action step.
struct HdrToningPreset: Equatable {
    static let schemaVersion = 1
    static let defaultName = "Default"
    let name: String
    let method: HdrToningMethod
    /// Exposure and Gamma's own two values; (0, 1) in every built-in.
    let exposure: Double, gamma: Double
    let local: HdrLocalAdaptation

    private typealias Library = PreferenceLibrary<HdrToningPresetCodec>

    /// The base `validated` and `recordedArguments` read values over: Default's values.
    private static let neutral = HdrToningSettings(method: .localAdaptation, exposure: 0, gamma: 1,
                                                   local: HdrLocalAdaptation(), bitDepth: 16)

    /// Default alone (Local Adaptation at its defaults).
    static let builtIns: [HdrToningPreset] = [
        HdrToningPreset(name: defaultName, method: .localAdaptation, exposure: 0, gamma: 1,
                        local: HdrLocalAdaptation()),
    ]

    /// `hdr_toning`'s keys of its method, every value explicit, no `bit_depth`.
    var arguments: [String: Any] {
        var arguments = applied(to: Self.neutral).stepArguments
        arguments.removeValue(forKey: "bit_depth")
        return arguments
    }

    var json: [String: Any] {
        ["schema_version": Self.schemaVersion, "name": name, "hdr_toning": arguments]
    }

    /// `settings` with this preset's method and every value; its `bitDepth` is kept.
    func applied(to settings: HdrToningSettings) -> HdrToningSettings {
        var out = settings
        out.method = method
        out.exposure = exposure
        out.gamma = gamma
        out.local = local
        return out
    }

    /// A user preset: the name normalized and not a built-in's, `arguments` holding `method` and
    /// only the keys a method takes (no `bit_depth`, no `preset`), and values the core's own
    /// parser accepts (`AdjustmentPreset.accepts`'s pattern, on a 1×1 grey).
    static func validated(name: String, arguments: [String: Any]) throws -> HdrToningPreset {
        let normalized = try Library.normalizedName(name)
        guard !builtIns.contains(where: { Library.key($0.name) == Library.key(normalized) }) else {
            throw DoctoredCoreError(message: "“\(normalized)” is a built-in HDR Toning preset; "
                + "choose another name.")
        }
        let allowed = Set(["method", "exposure", "gamma"] + HdrLocalAdaptation.ownKeys)
        for key in arguments.keys.sorted() where !allowed.contains(key) {
            throw DoctoredCoreError(message: "An HDR Toning preset holds method, exposure, gamma "
                + "and Local Adaptation's values only, not \(key).")
        }
        guard let method = arguments["method"] as? String, !method.isEmpty else {
            throw DoctoredCoreError(message: "Missing required argument: method")
        }
        let settings = try HdrToningSettings.values(arguments, over: neutral)
        guard let probe = RasterDocument.blank(width: 1, height: 1, rgba: (128, 128, 128, 255)) else {
            throw DoctoredCoreError(message: "The HDR Toning preset could not be checked.")
        }
        _ = try probe.hdrToningPlan(settings.coreRequest())
        return HdrToningPreset(name: normalized, method: settings.method, exposure: settings.exposure,
                               gamma: settings.gamma, local: settings.local)
    }

    /// Strict: exactly `{schema_version, name, hdr_toning}`, a normalized name, and stored keys
    /// equal to the decoded preset's `arguments` keys. Anything else throws, and the library lists
    /// the file as one `invalid_schema` row.
    static func decode(_ object: [String: Any]) throws -> HdrToningPreset {
        let invalid = DoctoredCoreError(message: "Invalid or unsupported HDR Toning preset.")
        guard Set(object.keys) == ["schema_version", "name", "hdr_toning"],
              let name = object["name"] as? String,
              let stored = object["hdr_toning"] as? [String: Any]
        else { throw invalid }
        let preset = try validated(name: name, arguments: stored)
        guard preset.name == name, Set(stored.keys) == Set(preset.arguments.keys) else { throw invalid }
        return preset
    }

    /// A built-in by library key, else the user's library.
    static func named(_ name: String) throws -> HdrToningPreset {
        let normalized = try? Library.normalizedName(name)
        if let normalized {
            if let builtIn = builtIns.first(where: { Library.key($0.name) == Library.key(normalized) }) {
                return builtIn
            }
            if let saved = try HdrToningPresetLibrary.entries().first(where: {
                Library.key($0.name) == Library.key(normalized)
            }) {
                return saved
            }
        }
        throw DoctoredCoreError(message: "No HDR Toning preset named “\(normalized ?? name)”. The "
            + "built-in preset is \(defaultName); a preset saved from the dialog is named as saved.")
    }

    /// The recorder's step for an `hdr_toning` call (a step records what the UI records):
    /// a call naming a preset becomes the preset's explicit values with the call's own keys over
    /// them, so a later change to the library never changes the step's replay. A call with no
    /// `preset`, a name that does not resolve, or values that refuse is returned unchanged.
    static func recordedArguments(_ a: [String: Any]) -> [String: Any] {
        guard let name = a["preset"] as? String, let preset = try? named(name) else { return a }
        var own = a
        for key in ["preset", "bit_depth", "document_id"] { own.removeValue(forKey: key) }
        guard let settings = try? HdrToningSettings.values(own, over: preset.applied(to: neutral))
        else { return a }
        var out = settings.stepArguments
        out.removeValue(forKey: "bit_depth")
        if let depth = a["bit_depth"] { out["bit_depth"] = depth }
        if let id = a["document_id"] { out["document_id"] = id }
        return out
    }
}

/// The HDR Toning preset library's codec on the shared `PreferenceLibrary` contract
/// (`PreferenceLibrary.swift`'s header).
enum HdrToningPresetCodec: PreferenceLibraryCodec {
    typealias Item = HdrToningPreset

    static let noun = "HDR Toning preset"
    static let directoryEnvironment = "DR_HDR_TONING_PRESETS_DIR"
    static let directoryName = "Doctored/HDR Toning Presets"
    static let maximumEntries = 128
    static let maximumFileBytes = 4096
    static let schemaVersions = HdrToningPreset.schemaVersion...HdrToningPreset.schemaVersion
    static let didChange = Notification.Name("HdrToningPresetLibraryDidChange")

    static func name(of item: HdrToningPreset) -> String { item.name }
    static func json(of item: HdrToningPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> HdrToningPreset {
        try HdrToningPreset.decode(object)
    }
}

/// The bounded HDR Toning preset library (`DR_HDR_TONING_PRESETS_DIR`). Its writes are
/// PREFERENCES: outside document Undo and never Action steps.
enum HdrToningPresetLibrary {
    private typealias Library = PreferenceLibrary<HdrToningPresetCodec>

    static let didChange = HdrToningPresetCodec.didChange

    static func entries() throws -> [HdrToningPreset] {
        try Library.entries()
    }

    /// Saves `settings`' method and values (never its depth) under `name`.
    @discardableResult
    static func save(name: String, settings: HdrToningSettings, replace: Bool) throws -> HdrToningPreset {
        var arguments = settings.stepArguments
        arguments.removeValue(forKey: "bit_depth")
        return try Library.save(HdrToningPreset.validated(name: name, arguments: arguments),
                                replace: replace)
    }

    static func delete(name: String) throws {
        try Library.delete(name: name)
    }
}
