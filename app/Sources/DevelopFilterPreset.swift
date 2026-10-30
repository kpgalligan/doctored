import Foundation

/// Filters ▸ Develop Filter…'s thirteen controls, in the core's
/// order (`sampling_develop_filter.rs`'s `DEVELOP_FILTER_KEYS`, which is the sheet's
/// order too), and the filter's own preset kind beside Develop's.
///
/// `params` is what the sheet records, `apply_filter` takes and Repeat Last
/// Filter replays: all thirteen keys, the ±100 controls as integers and
/// Exposure in stops rounded to 0.01.
struct DevelopFilterControls: Equatable {
    static let keys: [String] = [
        "temperature", "tint", "exposure", "contrast", "highlights", "shadows", "whites", "blacks",
        "texture", "clarity", "dehaze", "vibrance", "saturation",
    ]
    /// Exposure is Develop's own range (`RawDevelopRange.exposure`); every
    /// other control is −100…100. 0 is off for all of them.
    static let ranges: [String: ClosedRange<Double>] = Dictionary(
        uniqueKeysWithValues: keys.map { ($0, $0 == "exposure" ? RawDevelopRange.exposure : -100.0...100.0) })

    /// Absent is 0.
    var values: [String: Double] = [:]

    func value(_ key: String) -> Double { values[key] ?? 0 }

    /// The value as recorded: Exposure to 0.01 stop, the rest whole units.
    static func recorded(_ key: String, _ value: Double) -> Double {
        key == "exposure" ? (value * 100).rounded() / 100 : value.rounded()
    }

    var params: [String: Any] {
        Dictionary(uniqueKeysWithValues: Self.keys.map { key -> (String, Any) in
            let value = Self.recorded(key, self.value(key))
            return (key, key == "exposure" ? value as Any : Int(value) as Any)
        })
    }

    /// Every control at 0 (as recorded): the core's exact bypass.
    var isIdentity: Bool { Self.keys.allSatisfy { Self.recorded($0, value($0)) == 0 } }

    static func == (lhs: DevelopFilterControls, rhs: DevelopFilterControls) -> Bool {
        keys.allSatisfy { recorded($0, lhs.value($0)) == recorded($0, rhs.value($0)) }
    }

    /// A Develop preset applied to the filter, by name and meaning: only
    /// `exposure` (the same stops over the same range) is a filter control.
    /// Develop's `temperature`/`tint` are an absolute kelvin against the
    /// camera's as-shot white, `contrast` is CIRAWFilter's local contrast,
    /// `shadows` its tone-curve shadow boost, and every other key is
    /// decoder-only, so all of those are `ignored`, named for the panel's
    /// status line. Every control the preset does not take is 0.
    static func developTransfer(_ preset: RawPreset)
        -> (controls: DevelopFilterControls, taken: [String], ignored: [String])
    {
        let object = preset.settings.jsonObject()
        var controls = DevelopFilterControls()
        var taken: [String] = []
        if let exposure = (object["exposure"] as? NSNumber)?.doubleValue {
            let range = RawDevelopRange.exposure
            controls.values["exposure"] = min(range.upperBound, max(range.lowerBound, exposure))
            taken.append("exposure")
        }
        let ignored = object.keys.filter { $0 != "exposure" }.sorted()
        return (controls, taken, ignored)
    }
}

/// One saved Develop Filter look: `{"schema_version": 1, "name": …,
/// "develop_filter": {the non-zero controls only, at least one}}`. Library writes
/// are PREFERENCES: no undo step, no dirty flag, never an Action step.
struct DevelopFilterPreset {
    static let schemaVersion = 1
    let name: String
    let controls: DevelopFilterControls

    var json: [String: Any] {
        var stored: [String: Any] = [:]
        for key in DevelopFilterControls.keys {
            let value = DevelopFilterControls.recorded(key, controls.value(key))
            if value != 0 { stored[key] = key == "exposure" ? value as Any : Int(value) as Any }
        }
        return ["schema_version": Self.schemaVersion, "name": name, "develop_filter": stored]
    }

    /// Strict: exactly `{schema_version, name, develop_filter}`, or the same with the key a
    /// preset saved before the rename carries (`LegacyNames.developFilterPresetKey`; a file
    /// holding both keys is refused), a normalized name and at least one known control, each
    /// a number inside its range and not 0. Anything else throws, and the library lists the
    /// file as one `invalid_schema` row. A re-save writes `develop_filter`.
    static func decode(_ object: [String: Any]) throws -> DevelopFilterPreset {
        let invalid = DoctoredCoreError(message: "Invalid or unsupported Develop Filter preset.")
        let legacy = LegacyNames.developFilterPresetKey
        let key = object[legacy] != nil ? legacy : "develop_filter"
        guard Set(object.keys) == ["schema_version", "name", key],
              let name = object["name"] as? String,
              let stored = object[key] as? [String: Any], !stored.isEmpty,
              Set(stored.keys).isSubset(of: DevelopFilterControls.keys),
              let normalized = try? PreferenceLibrary<DevelopFilterPresetCodec>.normalizedName(name),
              normalized == name
        else { throw invalid }
        var controls = DevelopFilterControls()
        for (key, raw) in stored {
            guard let number = raw as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
                  let range = DevelopFilterControls.ranges[key], range.contains(number.doubleValue),
                  number.doubleValue != 0
            else { throw invalid }
            controls.values[key] = number.doubleValue
        }
        return DevelopFilterPreset(name: name, controls: controls)
    }
}

/// The Develop Filter preset library's codec on the shared
/// `PreferenceLibrary` contract (`PreferenceLibrary.swift`'s header).
enum DevelopFilterPresetCodec: PreferenceLibraryCodec {
    typealias Item = DevelopFilterPreset

    static let noun = "Develop Filter preset"
    static let directoryEnvironment = "DR_DEVELOP_FILTER_PRESETS_DIR"
    static let directoryName = "Doctored/Develop Filter Presets"
    static let maximumEntries = 128
    static let maximumFileBytes = 4096
    static let schemaVersions = DevelopFilterPreset.schemaVersion...DevelopFilterPreset.schemaVersion
    static let didChange = Notification.Name("DevelopFilterPresetLibraryDidChange")

    static func name(of item: DevelopFilterPreset) -> String { item.name }
    static func json(of item: DevelopFilterPreset) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> DevelopFilterPreset {
        try DevelopFilterPreset.decode(object)
    }
}

/// The bounded Develop Filter preset library
/// (`DR_DEVELOP_FILTER_PRESETS_DIR`). Its writes are PREFERENCES:
/// outside document Undo and never Action steps. Develop's own presets are
/// only read beside it (`DevelopFilterControls.developTransfer`).
enum DevelopFilterPresetLibrary {
    private typealias Library = PreferenceLibrary<DevelopFilterPresetCodec>

    static let didChange = DevelopFilterPresetCodec.didChange

    static func entries() throws -> [DevelopFilterPreset] {
        try Library.entries()
    }

    @discardableResult
    static func save(name: String, controls: DevelopFilterControls, replace: Bool) throws
        -> DevelopFilterPreset
    {
        let normalized = try Library.normalizedName(name)
        guard !controls.isIdentity else {
            throw DoctoredCoreError(message: "A Develop Filter preset must set at least one control.")
        }
        for key in DevelopFilterControls.keys {
            guard let range = DevelopFilterControls.ranges[key], range.contains(controls.value(key)) else {
                throw DoctoredCoreError(message: "Develop Filter \(key) is out of range.")
            }
        }
        return try Library.save(DevelopFilterPreset(name: normalized, controls: controls), replace: replace)
    }

    static func delete(name: String) throws {
        try Library.delete(name: name)
    }
}
