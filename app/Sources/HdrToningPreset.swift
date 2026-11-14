import Foundation

// HDR Toning's presets: the built-ins, Default (Local Adaptation at its defaults) and the looks
// after it, and the user's own, saved from the dialog's Preset popup or imported there from a
// `.hdt` file (`HdrToningPresetControl`, `HdrToningPresetImport.swift`) into a `PreferenceLibrary`
// (`DR_HDR_TONING_PRESETS_DIR`). `hdr_toning`'s `preset` names one; a recorded call keeps the
// preset's explicit values, never its name (`recordedArguments`).

/// One HDR Toning preset: a method and every value it takes. Library writes are PREFERENCES: no
/// undo step, no dirty flag, never an Action step.
struct HdrToningPreset: Equatable {
    static let schemaVersion = 1
    static let defaultName = "Default"
    let name: String
    /// A built-in look's one line on what it is for (the popup's caption and tooltip, and the
    /// catalog's `preset` text); nil for Default and every user preset. Never stored in a file.
    let purpose: String?
    let method: HdrToningMethod
    /// Exposure and Gamma's own two values; (0, 1) in every built-in.
    let exposure: Double, gamma: Double
    let local: HdrLocalAdaptation

    private typealias Library = PreferenceLibrary<HdrToningPresetCodec>

    /// The base `validated` and `recordedArguments` read values over: Default's values.
    private static let neutral = HdrToningSettings(method: .localAdaptation, exposure: 0, gamma: 1,
                                                   local: HdrLocalAdaptation(), bitDepth: 16)

    /// Default (Local Adaptation at its defaults), then the looks, in the popup's order. Every
    /// look is Local Adaptation, its values chosen from the kernel's map
    /// (`hdr_local_adaptation.rs`: the base is compressed into at most `8/gamma` stops below
    /// white, so a smaller gamma gives more contrast; Detail scales the rest by
    /// `1 + detail/100`; Exposure shifts both) and checked on renders of openly licensed
    /// fixtures (clipped and crushed fractions, mean level, distance from every other look);
    /// no other application's presets.
    static let builtIns: [HdrToningPreset] = [
        HdrToningPreset(name: defaultName, purpose: nil, method: .localAdaptation, exposure: 0,
                        gamma: 1, local: HdrLocalAdaptation()),
        look("Natural", "Close to the scene: its own colour and local contrast, nothing added.",
             HdrLocalAdaptation(radius: 32, strength: 0.4, gamma: 0.85, exposure: 0.4, detail: 0,
                                saturation: 0)),
        look("Crisp Detail", "Brings out fine texture in stone, foliage and fabric.",
             HdrLocalAdaptation(radius: 12, strength: 0.8, detail: 120)),
        look("Soft", "Gentle contrast with smoothed texture, for portraits and mist.",
             HdrLocalAdaptation(radius: 40, smoothEdges: true, gamma: 1.3, detail: -30,
                                highlight: -10, saturation: 10)),
        look("Vivid", "Stronger contrast and richer colour, for landscapes and sunsets.",
             HdrLocalAdaptation(gamma: 0.85, exposure: 0.5, detail: 50, vibrance: 50,
                                saturation: 35)),
        look("Black and White", "A neutral monochrome with firm contrast and texture.",
             HdrLocalAdaptation(gamma: 0.9, exposure: 0.4, detail: 60, saturation: -100)),
        look("Open Shadows", "Lifts dark areas, for backlit subjects and interiors.",
             HdrLocalAdaptation(gamma: 1.3, exposure: 0.2, shadow: 35)),
        look("Hold Highlights", "Keeps bright skies and windows from washing out.",
             HdrLocalAdaptation(gamma: 1.2, exposure: -0.15, detail: 20, highlight: -50,
                                toningCurve: [CurvePoint(x: 0, y: 0), CurvePoint(x: 190, y: 170),
                                              CurvePoint(x: 255, y: 255)])),
        look("Grading Base", "Low contrast and plain colour, a starting point for further edits.",
             HdrLocalAdaptation(gamma: 1.6, detail: 0, saturation: 0)),
    ]

    /// A built-in look: Local Adaptation at `local`.
    private static func look(_ name: String, _ purpose: String,
                             _ local: HdrLocalAdaptation) -> HdrToningPreset {
        HdrToningPreset(name: name, purpose: purpose, method: .localAdaptation, exposure: 0,
                        gamma: 1, local: local)
    }

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
        return HdrToningPreset(name: normalized, purpose: nil, method: settings.method,
                               exposure: settings.exposure, gamma: settings.gamma,
                               local: settings.local)
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
            + "built-in presets are \(builtInList); a preset saved from the dialog is named as saved.")
    }

    /// Every built-in's name, quoted, in order: “Default”, “Natural”, … and “Grading Base”.
    private static var builtInList: String {
        let quoted = builtIns.map { "“\($0.name)”" }
        guard let last = quoted.last else { return "" }
        return quoted.count == 1 ? last : quoted.dropLast().joined(separator: ", ") + " and " + last
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
        return try save(name: name, arguments: arguments, replace: replace)
    }

    /// Saves `arguments` (`hdr_toning`'s keys of one method, no `bit_depth`) under `name`, through
    /// `HdrToningPreset.validated`. The import (`HdrToningPresetImport.swift`) saves this way.
    @discardableResult
    static func save(name: String, arguments: [String: Any], replace: Bool) throws -> HdrToningPreset {
        try Library.save(HdrToningPreset.validated(name: name, arguments: arguments),
                         replace: replace)
    }

    static func delete(name: String) throws {
        try Library.delete(name: name)
    }
}
