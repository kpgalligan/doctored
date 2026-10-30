import Foundation
import CoreFoundation

// Presets are always WRITTEN at `RawPreset.schemaVersion` (4) with field-for-field identity —
// absent stays absent, no default is synthesized — while `Entry.schemaVersion` reports the ON-DISK
// version.
//
// `RawPreset.json` is the FILE form and never carries `file`; `Entry.json` is the MCP row.
//
// Preset v1 remains compatible; v2 adds transferable baseline compensation, supported moiré and
// manual vignette; v3 develop grain; v4 Texture, Clarity and Dehaze.

/// Transferable controls, deliberately independent of camera/decoder recipes.
/// White balance is one field: Custom copies temperature+tint, As Shot asks the
/// receiving camera to use its own captured as-shot chromaticity.
enum RawSettingsField: String, CaseIterable {
    case exposure, whiteBalance = "white_balance", toneCurve = "tone_curve", shadows
    case contrast, sharpness, detail, luminanceNoise = "luminance_noise", colorNoise = "color_noise"
    case lensCorrection = "lens_correction", highlightRecovery = "highlight_recovery"
    case outputSpace = "output_space"
    case baselineCompensation = "baseline_compensation", moire, vignette
    case grain
    case texture, clarity, dehaze

    var title: String {
        switch self {
        case .exposure: return "Exposure"
        case .whiteBalance: return "White balance"
        case .toneCurve: return "Tone curve"
        case .shadows: return "Shadows"
        case .contrast: return "Local contrast"
        case .sharpness: return "Sharpness"
        case .detail: return "Detail"
        case .luminanceNoise: return "Luminance noise"
        case .colorNoise: return "Colour noise"
        case .lensCorrection: return "Profile corrections"
        case .highlightRecovery: return "Highlight recovery"
        case .outputSpace: return "Developed source colour space"
        case .baselineCompensation: return "Camera-relative baseline compensation"
        case .moire: return "Moiré reduction"
        case .vignette: return "Manual vignette compensation"
        case .grain: return "Grain"
        case .texture: return "Texture"
        case .clarity: return "Clarity"
        case .dehaze: return "Dehaze"
        }
    }
    func supported(by capabilities: RawCapabilities, recipe: RawDevelopmentRecipe? = nil) -> Bool {
        switch self {
        case .contrast: return capabilities.contrast
        case .sharpness: return capabilities.sharpness
        case .detail: return capabilities.detail
        case .luminanceNoise: return capabilities.luminanceNoise
        case .colorNoise: return capabilities.colorNoise
        case .lensCorrection: return capabilities.lensCorrection
        case .highlightRecovery: return capabilities.highlightRecovery
        case .moire: return recipe?.moireSupported == true
        default: return true
        }
    }
    static func parse(_ value: Any?) throws -> [Self] {
        guard let strings = value as? [String], !strings.isEmpty, strings.count <= allCases.count else {
            throw DoctoredCoreError(message: "Choose one or more explicit RAW settings fields.")
        }
        let values = strings.compactMap(Self.init(rawValue:))
        guard values.count == strings.count, Set(values).count == values.count else {
            throw DoctoredCoreError(message: "RAW fields must be unique supported field names: \(allCases.map(\.rawValue).joined(separator: ", ")).")
        }
        return values
    }
}

enum RawSettingsPatch {
    static let keys: Set<String> = ["exposure", "temperature", "tint", "as_shot_white_balance",
        "tone_curve", "shadows", "contrast", "sharpness", "detail", "luminance_noise", "color_noise",
        "lens_correction", "highlight_recovery", "output_space", "baseline_compensation", "moire", "vignette", "grain",
        "texture", "clarity", "dehaze"]

    static func capture(values: RawDevelopValues, capabilities: RawCapabilities,
                        outputSpace: RawOutputSpace, fields: [RawSettingsField],
                        recipe: RawDevelopmentRecipe? = nil) throws -> RawDevelopSettings {
        guard !fields.isEmpty, Set(fields).count == fields.count else {
            throw DoctoredCoreError(message: "Choose unique RAW controls to copy.")
        }
        var result = RawDevelopSettings()
        for field in fields {
            guard field.supported(by: capabilities, recipe: recipe) else {
                throw DoctoredCoreError(message: "The source camera does not support \(field.title).")
            }
            switch field {
            case .exposure: result.exposure = values.exposure
            case .whiteBalance:
                if values.customWhiteBalance { result.temperature = values.temperature; result.tint = values.tint }
                else { result.asShotWhiteBalance = true }
            case .toneCurve: result.toneCurve = values.toneCurve
            case .shadows: result.shadows = values.shadows
            case .contrast: result.contrast = values.contrast
            case .sharpness: result.sharpness = values.sharpness
            case .detail: result.detail = values.detail
            case .luminanceNoise: result.luminanceNoise = values.luminanceNoise
            case .colorNoise: result.colorNoise = values.colorNoise
            case .lensCorrection: result.lensCorrection = values.lensCorrection
            case .highlightRecovery: result.highlightRecovery = values.highlightRecovery
            case .outputSpace: result.outputSpace = outputSpace
            case .baselineCompensation: result.baselineCompensation = recipe?.enhancements?.baselineCompensation ?? 0
            case .moire: result.moire = Double(recipe?.moire ?? 0)
            case .vignette: result.vignette = recipe?.enhancements?.vignette ?? .neutral
            case .grain: result.grain = recipe?.enhancements?.grain ?? .neutral
            case .texture: result.texture = recipe?.enhancements?.presence?.texture ?? 0
            case .clarity: result.clarity = recipe?.enhancements?.presence?.clarity ?? 0
            case .dehaze: result.dehaze = recipe?.enhancements?.presence?.dehaze ?? 0
            }
        }
        return try validated(result)
    }

    static func validated(_ settings: RawDevelopSettings) throws -> RawDevelopSettings {
        try decode(settings.jsonObject())
    }

    /// Stored presets use canonical JSON scalar types. MCP string coercion is
    /// handled by the existing rawOpenSettings parser before this boundary.
    static func decode(_ object: [String: Any]) throws -> RawDevelopSettings {
        guard !object.isEmpty, Set(object.keys).isSubset(of: keys) else {
            throw DoctoredCoreError(message: "A RAW preset requires known user controls; camera recipes and decoder migration cannot be stored in presets.")
        }
        func number(_ key: String, _ range: ClosedRange<Double>) throws -> Double? {
            guard let raw = object[key] else { return nil }
            guard let value = raw as? NSNumber, CFGetTypeID(value) != CFBooleanGetTypeID(),
                  value.doubleValue.isFinite, range.contains(value.doubleValue) else {
                throw DoctoredCoreError(message: "Preset \(key) must be a finite number in \(range.lowerBound)...\(range.upperBound).")
            }
            return value.doubleValue
        }
        func flag(_ key: String) throws -> Bool? {
            guard let raw = object[key] else { return nil }
            guard let value = raw as? NSNumber, CFGetTypeID(value) == CFBooleanGetTypeID() else {
                throw DoctoredCoreError(message: "Preset \(key) must be a boolean.")
            }
            return value.boolValue
        }
        var result = RawDevelopSettings()
        result.baselineCompensation = try number("baseline_compensation", -4...4)
        result.moire = try number("moire", 0...1)
        if let raw = object["vignette"] {
            guard let vignette = raw as? [String: Any] else {
                throw DoctoredCoreError(message: "Preset vignette requires an explicit object of supported controls.")
            }
            result.vignette = try RawLocalJSON.vignette(vignette)
        }
        if let raw = object["grain"] {
            guard let grain = raw as? [String: Any] else {
                throw DoctoredCoreError(message: "Preset grain requires an object with amount, size and roughness.")
            }
            result.grain = try RawGrainSettings.parse(grain)
        }
        result.texture = try number("texture", RawPresenceSettings.range)
        result.clarity = try number("clarity", RawPresenceSettings.range)
        result.dehaze = try number("dehaze", RawPresenceSettings.range)
        result.exposure = try number("exposure", RawDevelopRange.exposure)
        result.temperature = try number("temperature", RawDevelopRange.temperature)
        result.tint = try number("tint", RawDevelopRange.tint)
        result.asShotWhiteBalance = try flag("as_shot_white_balance")
        if result.asShotWhiteBalance == false { result.asShotWhiteBalance = nil }
        guard result.asShotWhiteBalance != true || (result.temperature == nil && result.tint == nil) else {
            throw DoctoredCoreError(message: "As-shot white balance cannot be combined with temperature or tint.")
        }
        result.toneCurve = try number("tone_curve", RawDevelopRange.toneCurve)
        result.shadows = try number("shadows", RawDevelopRange.shadows)
        result.contrast = try number("contrast", RawDevelopRange.contrast)
        result.sharpness = try number("sharpness", RawDevelopRange.sharpness)
        result.detail = try number("detail", RawDevelopRange.detail)
        result.luminanceNoise = try number("luminance_noise", RawDevelopRange.luminanceNoise)
        result.colorNoise = try number("color_noise", RawDevelopRange.colorNoise)
        result.lensCorrection = try flag("lens_correction")
        result.highlightRecovery = try flag("highlight_recovery")
        if let value = object["output_space"] {
            guard let text = value as? String, let space = RawOutputSpace(rawValue: text) else {
                throw DoctoredCoreError(message: "Preset output_space must be \(RawOutputSpace.acceptedNames).")
            }
            result.outputSpace = space
        }
        guard !result.jsonObject().isEmpty else { throw DoctoredCoreError(message: "A RAW preset must change at least one control.") }
        return result
    }

    /// Per-key recovery for a repairable preset file: each key of `object` is
    /// decoded ALONE, and kept only when that lone decode succeeds. A key the
    /// whole-object decode would refuse — an unknown name, an out-of-range
    /// number, a flag whose only value contributes no control — is therefore
    /// dropped by NAME instead of taking the rest of the preset with it.
    ///
    /// `dropped` is sorted: `keys` is a Set, so set iteration order is never
    /// the reported order.
    static func recover(_ object: [String: Any]) throws -> (settings: RawDevelopSettings, dropped: [String]) {
        var kept: [String: Any] = [:]
        for (name, value) in object where (try? decode([name: value])) != nil {
            kept[name] = value
        }
        // The one pair that survives its lone decode and still contradicts:
        // as-shot white balance cannot stand beside an explicit temperature or
        // tint, and the explicit pair is the more specific statement.
        if let flag = kept["as_shot_white_balance"] as? NSNumber, flag.boolValue,
           kept["temperature"] != nil || kept["tint"] != nil {
            kept.removeValue(forKey: "as_shot_white_balance")
        }
        guard !kept.isEmpty, let settings = try? decode(kept) else {
            throw DoctoredCoreError(message: "The file has no recoverable controls.")
        }
        return (settings, object.keys.filter { kept[$0] == nil }.sorted())
    }
}

struct RawPreset {
    /// The version every preset is WRITTEN at. Version 1 files are still read;
    /// a save or a repair rewrites them here, field for field, because the two
    /// schemas differ only in which controls may appear.
    static let schemaVersion = 4
    let name: String
    let settings: RawDevelopSettings
    var json: [String: Any] {
        ["schema_version": Self.schemaVersion, "name": name, "raw": settings.jsonObject()]
    }
}
