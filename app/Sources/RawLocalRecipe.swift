import Foundation
import CoreFoundation

// Recipes 1/2 retain their meanings. Native `RZLM` coverage is held in immutable core owners and
// generic resources, never JSON/display samples.
//
// Same-geometry migration preserves exact planes; unsupported versions retain cached rendering and
// refuse semantic edits.

/// Manual lens compensation in the developed source's own calibrated profile.
/// Coordinates are normalized only in the recipe/UI; the core receives actual
/// oriented source coordinates and evaluates at pixel centers.
struct RawVignetteSettings: Codable, Equatable {
    var edgeExposure: Double = 0
    var centerX: Double = 0.5
    var centerY: Double = 0.5
    var falloff: Double = 2
    static let neutral = RawVignetteSettings()

    func validate() throws {
        guard edgeExposure.isFinite, (-4...4).contains(edgeExposure),
              centerX.isFinite, (0...1).contains(centerX), centerY.isFinite, (0...1).contains(centerY),
              falloff.isFinite, (0.25...4).contains(falloff) else {
            throw DoctoredCoreError(message: "RAW vignette needs edge exposure −4…4, center 0…1 and falloff 0.25…4.")
        }
    }
    var json: [String: Any] {
        ["edge_exposure": edgeExposure, "center": [centerX, centerY], "falloff": falloff]
    }
    func coreJSON(width: Int, height: Int, reference: RawVignetteReference? = nil) throws -> String {
        try validate()
        if let reference {
            try reference.validate()
            let basisWidth = Double(reference.referenceSize[0])
            let basisHeight = Double(reference.referenceSize[1])
            return try RawLocalJSON.string(["stops": edgeExposure,
                "center": [centerX * basisWidth, centerY * basisHeight], "falloff": falloff,
                "reference_size": reference.referenceSize,
                "reference_from_source": reference.referenceFromSource])
        }
        return try RawLocalJSON.string(["stops": edgeExposure,
            "center": [centerX * Double(width), centerY * Double(height)], "falloff": falloff])
    }
}

enum RawLocalWhiteBalance: String, Codable, CaseIterable {
    case inherit
    case asShot = "as_shot"
    case custom
}

/// Every variable decoder control and exact coupled WB output for a local.
/// Static camera/decoder/orientation/profile controls are inherited from the
/// enclosing dense global recipe, never independently defaulted on reopen.
struct RawLocalEffective: Codable, Equatable {
    var values: RawDevelopValues
    var chromaticityX: Double
    var chromaticityY: Double
}

struct RawLocalNode: Codable, Equatable {
    var id: String
    var name: String
    var enabled = true
    var density: Double = 1
    var feather: Double = 0
    var exposureOffset: Double = 0
    var whiteBalance: RawLocalWhiteBalance = .inherit
    var temperature: Double?
    var tint: Double?
    var effective: RawLocalEffective?
    /// Luminance/colour range multiplying the mask's coverage (`RawLocalRange.swift`). Absent
    /// for a local with none, so a recipe without ranges encodes exactly as it did before them.
    var range: RawLocalRange?
    /// Set when the mask's coverage came from Vision (`RawLocalGenerated.swift`) and names which
    /// request made it, so Recompute can ask again. The coverage itself is an ordinary stored
    /// mask and never changes by itself. Absent otherwise, so a recipe with no generated local
    /// encodes exactly as it did before; a build that predates the key refuses the recipe
    /// (`RawSourceDescriptor.read`'s key check) and keeps the saved rendering.
    var generated: RawLocalGeneratedKind?
    /// The local's tone and colour controls (`RawLocalTone.swift`), applied to the development
    /// inside the coverage; nil when every control is 0, so a recipe without tone encodes exactly
    /// as it did before (version 3 marks one that has any).
    var tone: RawLocalTone?

    /// Whether the local develops its own camera variant: always below recipe version 3 (every
    /// local did, and an old recipe keeps that result), and in version 3 only for a white-balance
    /// or camera-exposure change. Any other local reads the development itself.
    func developsCameraVariant(version: Int) -> Bool {
        version < 3 || whiteBalance != .inherit || exposureOffset != 0
    }

    func validate(requireEffective: Bool = false) throws {
        guard RawLocalJSON.validID(id), !name.isEmpty, name.utf8.count <= 128,
              !name.utf8.contains(0), density.isFinite, (0...1).contains(density),
              feather.isFinite, (0...256).contains(feather), exposureOffset.isFinite,
              (-4...4).contains(exposureOffset) else {
            throw DoctoredCoreError(message: "The RAW local has an invalid identity, name, density, feather or exposure offset.")
        }
        if whiteBalance == .custom {
            guard let temperature, let tint, temperature.isFinite, tint.isFinite,
                  RawDevelopRange.temperature.contains(temperature), RawDevelopRange.tint.contains(tint) else {
                throw DoctoredCoreError(message: "Custom local white balance requires a supported temperature and tint.")
            }
        } else if temperature != nil || tint != nil {
            throw DoctoredCoreError(message: "Only custom local white balance stores temperature and tint.")
        }
        if requireEffective, effective == nil {
            throw DoctoredCoreError(message: "The saved local RAW effective recipe is missing.")
        }
        // A saved range holds a colour, never the point it was sampled at.
        try range?.validate(resolved: requireEffective)
        if let tone {
            try tone.validate()
            guard !tone.isNeutral else { throw DoctoredCoreError(message: "A saved RAW local tone must change something.") }
        }
        if let effective {
            guard effective.chromaticityX.isFinite, effective.chromaticityY.isFinite,
                  (0...1).contains(effective.chromaticityX), (0...1).contains(effective.chromaticityY) else {
                throw DoctoredCoreError(message: "The saved local RAW chromaticity is invalid.")
            }
        }
    }
    var controlsJSON: [String: Any] {
        var result: [String: Any] = ["name": name, "enabled": enabled, "density": density,
            "feather": feather, "exposure_offset": exposureOffset, "white_balance": whiteBalance.rawValue]
        if let temperature { result["temperature"] = temperature }
        if let tint { result["tint"] = tint }
        if let range { result["range"] = range.json }
        if let generated { result["generated"] = generated.rawValue }
        if let tone { result["tone"] = tone.json }
        return result
    }
}

struct RawEnhancementRecipe: Codable, Equatable {
    /// 1, or 2 when any local carries a range: a build that predates ranges refuses version 2
    /// and keeps the saved rendering instead of developing those locals without their ranges.
    /// 3 when any local carries a tone or there are more than four: a build that predates the
    /// tone path refuses it, and in version 3 only a camera local develops a variant
    /// (`RawLocalNode.developsCameraVariant`).
    var version = 1
    var evaluation = "oriented-encoded-raw-variants-v1"
    var baselineReference: Float
    var baselineCompensation: Double = 0
    var moire: Double
    var vignette = RawVignetteSettings.neutral
    // Every assignment site (Develop, presets, Sync Settings, Redevelop, decoder migration)
    // gets the right version from here; decoding does not run it, and `validate` checks it.
    var locals: [RawLocalNode] = [] {
        didSet { version = Self.version(for: locals) }
    }
    var grain: RawGrainSettings?
    /// Texture, Clarity and Dehaze (`RawPresence.swift`). Absent unless some amount is nonzero,
    /// so a recipe without Presence encodes exactly as it did before it.
    var presence: RawPresenceSettings?

    /// Locals one RAW layer holds, and how many of them may develop a camera variant
    /// (`core/src/raw_local_limits.rs`). Above about 32 MP the 1 GiB resource cap holds fewer
    /// (`RawLocalWork.maximumLocals`).
    static let maximumLocals = 16
    static let maximumCameraLocals = 4
    static let cameraLocalRefusal = "A RAW layer has at most four locals that change white balance or camera exposure."

    static func version(for locals: [RawLocalNode]) -> Int {
        if locals.contains(where: { $0.tone != nil }) || locals.count > maximumCameraLocals { return 3 }
        return locals.contains { $0.range != nil } ? 2 : 1
    }

    func validate() throws {
        guard version == Self.version(for: locals), evaluation == "oriented-encoded-raw-variants-v1",
              baselineReference.isFinite, (-128...128).contains(baselineReference),
              baselineCompensation.isFinite, (-4...4).contains(baselineCompensation),
              moire.isFinite, (0...1).contains(moire), locals.count <= Self.maximumLocals,
              Set(locals.map(\.id)).count == locals.count else {
            throw DoctoredCoreError(message: "The saved RAW enhancement policy is unavailable or exceeds its limits.")
        }
        guard locals.filter({ $0.developsCameraVariant(version: version) }).count <= Self.maximumCameraLocals else {
            throw DoctoredCoreError(message: Self.cameraLocalRefusal)
        }
        try vignette.validate()
        if let grain {
            try grain.validate()
            guard !grain.isNeutral else {
                throw DoctoredCoreError(message: "A saved RAW grain must have an amount above 0.")
            }
        }
        if let presence {
            try presence.validate()
            guard !presence.isNeutral else {
                throw DoctoredCoreError(message: "A saved RAW Presence must have a nonzero texture, clarity or dehaze.")
            }
        }
        for node in locals { try node.validate() }
    }
}

enum RawLocalJSON {
    static func validID(_ value: String) -> Bool {
        !value.isEmpty && value.utf8.count <= 64 && value.utf8.allSatisfy {
            (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95
        }
    }
    static func string(_ value: Any) throws -> String {
        String(decoding: try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), as: UTF8.self)
    }
    static func keys(_ object: [String: Any], _ allowed: Set<String>) throws {
        guard Set(object.keys).isSubset(of: allowed) else {
            throw DoctoredCoreError(message: "Unknown RAW local field: \(Set(object.keys).subtracting(allowed).sorted().joined(separator: ", ")).")
        }
    }
    static func number(_ object: [String: Any], _ key: String, range: ClosedRange<Double>) throws -> Double? {
        guard let raw = object[key] else { return nil }
        guard let number = raw as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite, range.contains(number.doubleValue) else {
            throw DoctoredCoreError(message: "RAW local \(key) must be a finite number in \(range).")
        }
        return number.doubleValue
    }
    static func flag(_ object: [String: Any], _ key: String) throws -> Bool? {
        guard let raw = object[key] else { return nil }
        guard let value = raw as? NSNumber, CFGetTypeID(value) == CFBooleanGetTypeID() else {
            throw DoctoredCoreError(message: "RAW local \(key) must be true or false.")
        }
        return value.boolValue
    }
    static func vignette(_ object: [String: Any]) throws -> RawVignetteSettings {
        try keys(object, ["edge_exposure", "center", "falloff"])
        guard let edge = try number(object, "edge_exposure", range: -4...4),
              let falloff = try number(object, "falloff", range: 0.25...4),
              let center = object["center"] as? [Any], center.count == 2,
              let x = try number(["x": center[0]], "x", range: 0...1),
              let y = try number(["y": center[1]], "y", range: 0...1) else {
            throw DoctoredCoreError(message: "RAW vignette needs edge_exposure, center:[x,y] and falloff.")
        }
        return RawVignetteSettings(edgeExposure: edge, centerX: x, centerY: y, falloff: falloff)
    }
}

extension RawDevelopmentRecipe {
    /// Saved local recipes may vary only their declared exposure/WB controls.
    /// Otherwise a later mask edit would silently discard a forged or future
    /// per-node tone/lens control. Pending global edits validate after evaluation.
    func validateLocalEffectiveState() throws {
        for node in enhancements?.locals ?? [] {
            try node.validate(requireEffective: true)
            guard let local = node.effective else { continue }
            var expected = values
            expected.exposure = Double(Float(values.exposure + node.exposureOffset))
            // Read-only temperature/tint can have decoder-specific display
            // round-off. Saved chromaticity, not those labels, drives evaluation.
            expected.temperature = local.values.temperature
            expected.tint = local.values.tint
            // A local's camera development never carries the Lensfun toggle (v1-lensfun,
            // `RawLocalEvaluation.cameraDescriptor`): it is the decoder's own default there.
            if lensProfile != nil, !capabilities.lensCorrection {
                expected.lensCorrection = capabilities.defaults.lensCorrection
            }
            switch node.whiteBalance {
            case .inherit:
                guard local.chromaticityX == chromaticityX, local.chromaticityY == chromaticityY else {
                    throw DoctoredCoreError(message: "A saved inheriting local has a different white-balance chromaticity.")
                }
            case .asShot:
                expected.customWhiteBalance = false
                guard local.chromaticityX == asShotChromaticityX, local.chromaticityY == asShotChromaticityY else {
                    throw DoctoredCoreError(message: "A saved as-shot local has a different camera white balance.")
                }
            case .custom: expected.customWhiteBalance = true
            }
            guard local.values == expected else {
                throw DoctoredCoreError(message: "A saved local contains undeclared camera controls. Keep its saved rendering.")
            }
        }
    }
}
