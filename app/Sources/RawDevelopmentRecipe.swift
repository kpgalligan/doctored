import AppKit
import CoreImage
import ImageIO

// Core Image renders directly to straight RGBA16 with a float working format and explicit
// normalized sRGB, Display P3, Adobe RGB (1998) or ProPhoto RGB output (`RawOutputSpace`, whose
// doc states the per-channel clamp of a colour outside the chosen space); previews never become
// masters. The 32-bit float (linear) development (`RawOutputEncoding.linearFloat`) is the
// same filter rendered as RGBAf into the space's extended linear twin, unclamped, with the local
// tone map off; its tone curve and Shadows are the decoder's own under the extended range,
// starting at tone curve 0: one decode seam, two encodings.
//
// Redevelopment preserves original bytes, source identity, affine and retouch layers through one
// central edit. Native v18/v19 save originals and usable renderings; reopening never invokes a RAW
// decoder. Unknown recipes or changed decoders refuse ordinary redevelopment. RAW capture/copy/hash
// and each camera render remain noncancellable inside their platform call; compound RAW work checks
// the shared cancellation owner before and after each camera stage and uses fresh native child
// tokens. RAW-only native v18/v19 remain compatible; RAW local attachments use the existing v21/v22
// shared binary pool. Qualification and bounds are recorded in
// `designs/phase-8g-raw-enhancements-status.md`.

/// Dense, versioned development state. Saved work never resolves missing fields
/// against newer decoder defaults. Unsupported recipes still retain their raster.
struct RawDevelopmentRecipe: Codable, Equatable {
    var version = 1
    var values: RawDevelopValues
    let capabilities: RawCapabilities
    let orientation: UInt32
    var baselineExposure: Float
    let shadowBias: Float
    let gamutMapping: Bool
    var moire: Float
    let moireSupported: Bool
    let localToneMap: Float
    let localToneMapSupported: Bool
    /// `CIRAWFilter.extendedDynamicRangeAmount`: 0 for a 16-bit SDR master,
    /// `RawOutputEncoding.linearFloat.extendedDynamicRange` for a 32-bit float
    /// (linear) development. A `var`: `updating` sets it with the
    /// policy, and the pair is what `hasSupportedOutputPolicy` accepts.
    var extendedDynamicRange: Float
    let asShotChromaticityX: Double
    let asShotChromaticityY: Double
    var chromaticityX: Double
    var chromaticityY: Double
    var outputSpace: String
    let scale: Float
    let draft: Bool
    var enhancements: RawEnhancementRecipe?
    var sourceGeometry: RawLensGeometry?
    var decoderFrame: RawDecoderFrame?
    var vignetteReference: RawVignetteReference?
    /// The bundled Lensfun profile matched at the first development when the decoder carries no
    /// lens profile (v1-lensfun); absent otherwise, so a recipe without one encodes exactly as
    /// before. Enable Profile Corrections applies it (`lensfunApplies`); the decoder's own profile,
    /// when there is one, always wins, and the two are never both applied.
    var lensProfile: RawLensfunProfile?

    /// Whether the Lensfun stage runs: no decoder lens profile, Profile Corrections on, and a
    /// matched profile.
    var lensfunApplies: Bool { !capabilities.lensCorrection && values.lensCorrection && lensProfile != nil }

    var outputColorSpace: RawOutputSpace? { RawOutputSpace(policy: outputSpace) }
    /// Which master the recipe develops: nil for a policy this build does not
    /// know (an older build reads a float policy that way and keeps the saved
    /// rendering).
    var outputEncoding: RawOutputEncoding? { RawOutputSpace.encoding(policy: outputSpace) }
    /// The 32-bit float (linear) development: f32 in linear light with
    /// the local tone map off, scene-linear at tone curve 0 (where it starts);
    /// a tone curve above 0 is the decoder's own and writes the `-v2`
    /// policy (`RawOutputSpace.policy(_:toneCurve:)`).
    var isLinearFloat: Bool { outputEncoding == .linearFloat }
    /// The version of this recipe with its enhancements, geometry and frame
    /// stripped (a local's camera-only variant): 1 for the normalized sRGB
    /// policy, which is all version 1 ever held, else 2.
    var cameraOnlyVersion: Int {
        outputSpace == RawOutputSpace.sRGB.policy(.normalized16) ? 1 : 2
    }
    /// No recipe version was added for the float development: it is a policy
    /// string beside its `extendedDynamicRange`, accepted as exactly that pair
    /// at versions 2 to 4. Version 1 is the normalized sRGB policy only.
    var hasSupportedOutputPolicy: Bool {
        guard let encoding = outputEncoding, outputColorSpace != nil, scale == 1, !draft,
              extendedDynamicRange == encoding.extendedDynamicRange else { return false }
        switch version {
        case 1: return cameraOnlyVersion == 1 && enhancements == nil
        case 2: return enhancements == nil
        case 3: return enhancements != nil
        // Version 5 (v1-lensfun) is version 4 with a Lensfun profile; an older build keeps
        // the saved rendering.
        case 4, 5: return true
        default: return false
        }
    }

    init(filter: CIRAWFilter) {
        capabilities = RawCapabilities(filter)
        // A new development starts with Profile Corrections on when the file
        // carries a lens profile; `capabilities.defaults` stays as shot.
        values = capabilities.developDefaults
        orientation = filter.orientation.rawValue
        baselineExposure = filter.baselineExposure
        shadowBias = filter.shadowBias
        gamutMapping = filter.isGamutMappingEnabled
        moire = filter.moireReductionAmount
        moireSupported = filter.isMoireReductionSupported
        localToneMap = filter.localToneMapAmount
        localToneMapSupported = filter.isLocalToneMapSupported
        // A new recipe is the normalized SDR u16 policy. The 32-bit float
        // (linear) development is the other explicit policy (`updating`,
        // `RawOutputEncoding`); nothing is clipped implicitly.
        extendedDynamicRange = 0
        asShotChromaticityX = filter.neutralChromaticity.x
        asShotChromaticityY = filter.neutralChromaticity.y
        chromaticityX = filter.neutralChromaticity.x
        chromaticityY = filter.neutralChromaticity.y
        outputSpace = "srgb-rgba16-normalized-v1"
        scale = 1
        draft = false
        sourceGeometry = nil
        decoderFrame = nil
        vignetteReference = nil
        // v1-lensfun: with no decoder lens profile, the bundled Lensfun database's automatic
        // match for the file's camera and lens starts with Profile Corrections on, as a decoder
        // profile does (no plane size: a RAW development is the camera frame).
        if !capabilities.lensCorrection,
           let match = try? LensProfileMatch.resolve(LensIdentity(imageProperties: filter.properties), .auto),
           let selected = match.selected, let profile = match.profileJSON {
            lensProfile = RawLensfunProfile(id: selected.id, lens: selected.model, profile: profile)
            values.lensCorrection = true
            version = 5
        }
    }

    func updating(_ settings: RawDevelopSettings) throws -> Self {
        guard hasSupportedOutputPolicy else {
            throw DoctoredCoreError(message: "This saved RAW recipe cannot be edited by this build. Its saved rendering remains intact.")
        }
        var updated = self
        if let geometry = settings.sourceGeometry {
            try geometry.validate()
            updated.sourceGeometry = geometry.isNeutral ? nil : geometry
            updated.version = max(updated.version, 4)
        }
        // The space and the encoding are one policy string. Either request
        // alone keeps the other's saved choice.
        let currentEncoding = outputEncoding ?? .normalized16
        let encoding = settings.outputEncoding ?? currentEncoding
        // Compared as space and encoding, not as the string: a float `-v2`
        // policy is the same space and encoding as its `-v1`.
        if let space = settings.outputSpace ?? outputColorSpace,
           space != outputColorSpace || encoding != currentEncoding {
            updated.outputSpace = space.policy(encoding)
            updated.extendedDynamicRange = encoding.extendedDynamicRange
            updated.version = max(updated.version, 2)
        }
        if encoding == .linearFloat {
            // The tone curve and Shadows apply, and so does grain, on
            // the display encoding (#680, `RawGrain.swift`). Presence is
            // defined on display-referred values and is refused: the
            // request's amounts over the saved ones must all be 0. Every
            // float path (Develop, Redevelop RAW, presets, Sync Settings, the
            // MCP raw tools) reaches this one check.
            let presence = (enhancements?.presence ?? .neutral).patched(
                texture: settings.texture, clarity: settings.clarity, dehaze: settings.dehaze)
            if !presence.isNeutral {
                throw DoctoredCoreError(message: RawPresenceSettings.floatRefusal)
            }
            // A saved local tone is display-referred in the same way.
            try RawLocalTone.refuseAtFloat(nodes: enhancements?.locals ?? [])
        }
        if let compensation = settings.baselineCompensation {
            guard compensation.isFinite, (-4...4).contains(compensation) else {
                throw DoctoredCoreError(message: "RAW baseline compensation must be between −4 and 4 stops.")
            }
            if compensation != (enhancements?.baselineCompensation ?? 0) {
                updated.enableEnhancements()
                updated.enhancements?.baselineCompensation = compensation
                if let reference = updated.enhancements?.baselineReference {
                    updated.baselineExposure = Float(Double(reference) + compensation)
                }
            }
        }
        if let amount = settings.moire {
            guard moireSupported else { throw DoctoredCoreError(message: "This RAW decoder does not support moiré reduction.") }
            guard amount.isFinite, (0...1).contains(amount) else {
                throw DoctoredCoreError(message: "RAW moiré reduction must be between 0 and 1.")
            }
            if Float(amount) != moire {
                updated.enableEnhancements()
                updated.moire = Float(amount)
                updated.enhancements?.moire = Double(Float(amount))
            }
        }
        if let vignette = settings.vignette {
            try vignette.validate()
            if vignette != (enhancements?.vignette ?? .neutral) {
                updated.enableEnhancements()
                updated.enhancements?.vignette = vignette
            }
        }
        if let grain = settings.grain {
            try grain.validate()
            let normalized: RawGrainSettings? = grain.isNeutral ? nil : grain
            if normalized != enhancements?.grain {
                updated.enableEnhancements()
                updated.enhancements?.grain = normalized
            }
        }
        if settings.texture != nil || settings.clarity != nil || settings.dehaze != nil {
            let merged = (enhancements?.presence ?? .neutral).patched(
                texture: settings.texture, clarity: settings.clarity, dehaze: settings.dehaze)
            try merged.validate()
            let normalized: RawPresenceSettings? = merged.isNeutral ? nil : merged
            if normalized != enhancements?.presence {
                updated.enableEnhancements()
                updated.enhancements?.presence = normalized
            }
        }
        // Sparse redevelopment patches the saved effective values. It never
        // resets unspecified controls to today's as-shot defaults.
        let fields: [(Bool, Bool, String)] = [
            (settings.contrast != nil, capabilities.contrast, "contrast"),
            (settings.sharpness != nil, capabilities.sharpness, "sharpness"),
            (settings.detail != nil, capabilities.detail, "detail"),
            (settings.luminanceNoise != nil, capabilities.luminanceNoise, "luminance_noise"),
            (settings.colorNoise != nil, capabilities.colorNoise, "color_noise"),
            (settings.lensCorrection != nil, capabilities.lensCorrection, "lens_correction"),
            (settings.highlightRecovery != nil, capabilities.highlightRecovery, "highlight_recovery")]
        for (requested, supported, name) in fields where requested && !supported {
            // v1-lensfun: Profile Corrections also switch a matched Lensfun profile.
            if name == "lens_correction" {
                if lensProfile != nil { continue }
                throw DoctoredCoreError(message: "This RAW decoder does not support lens_correction, "
                    + "and no Lensfun profile matched this camera and lens at its first development.")
            }
            throw DoctoredCoreError(message: "This RAW decoder does not support \(name).")
        }
        if settings.asShotWhiteBalance == true {
            guard settings.temperature == nil, settings.tint == nil else {
                throw DoctoredCoreError(message: "As-shot white balance cannot be combined with temperature or tint.")
            }
            updated.values.customWhiteBalance = false
            updated.values.temperature = capabilities.defaults.temperature
            updated.values.tint = capabilities.defaults.tint
            updated.chromaticityX = asShotChromaticityX
            updated.chromaticityY = asShotChromaticityY
        }
        if let v = settings.exposure { updated.values.exposure = v }
        if let v = settings.temperature { updated.values.temperature = v }
        if let v = settings.tint { updated.values.tint = v }
        if settings.temperature != nil || settings.tint != nil { updated.values.customWhiteBalance = true }
        if let v = settings.toneCurve { updated.values.toneCurve = v }
        if encoding == .linearFloat, currentEncoding != .linearFloat, settings.toneCurve == nil {
            // Into a float development with no curve named: it starts
            // scene-linear, at tone curve 0.
            updated.values.toneCurve = 0
        } else if encoding != .linearFloat, currentEncoding == .linearFloat {
            // Back to a 16-bit SDR master: the request's curve, or the file's own.
            updated.values.toneCurve = settings.toneCurve ?? capabilities.defaults.toneCurve
        }
        if let v = settings.shadows { updated.values.shadows = v }
        if let v = settings.contrast { updated.values.contrast = v }
        if let v = settings.sharpness { updated.values.sharpness = v }
        if let v = settings.detail { updated.values.detail = v }
        if let v = settings.luminanceNoise { updated.values.luminanceNoise = v }
        if let v = settings.colorNoise { updated.values.colorNoise = v }
        if let v = settings.lensCorrection { updated.values.lensCorrection = v }
        if let v = settings.highlightRecovery { updated.values.highlightRecovery = v }
        // A float development's policy follows its resolved tone curve: `-v2`
        // above 0, `-v1` at 0, so a build that pinned the curve to 0 keeps the
        // saved rendering instead of re-rendering it without the curve.
        if encoding == .linearFloat, let space = updated.outputColorSpace {
            let policy = space.policy(.linearFloat, toneCurve: updated.values.toneCurve)
            if policy != updated.outputSpace {
                updated.outputSpace = policy
                updated.version = max(updated.version, 2)
            }
        }
        if updated.lensfunApplies { updated.version = 5 }
        try updated.validate()
        return updated
    }

    mutating func enableEnhancements() {
        if enhancements == nil {
            enhancements = RawEnhancementRecipe(baselineReference: baselineExposure, moire: Double(moire))
        }
        version = max(version, 3)
    }

    func validate() throws {
        let pairs: [(Double, ClosedRange<Double>)] = [
            (values.exposure, -32...32), (values.temperature, 1000...65536), (values.tint, -1000...1000),
            (values.toneCurve, 0...1), (values.shadows, 0...2), (values.contrast, 0...1),
            (values.sharpness, 0...1), (values.detail, 0...3), (values.luminanceNoise, 0...1),
            (values.colorNoise, 0...1), (Double(baselineExposure), -128...128),
            (Double(shadowBias), -128...128), (Double(moire), 0...1), (Double(localToneMap), 0...1),
            (chromaticityX, 0...1), (chromaticityY, 0...1),
            (asShotChromaticityX, 0...1), (asShotChromaticityY, 0...1)]
        guard pairs.allSatisfy({ $0.0.isFinite && $0.1.contains($0.0) }) else {
            throw DoctoredCoreError(message: "The saved RAW recipe contains unsupported numerical values. Its rendering remains intact.")
        }
        if let enhancements {
            try enhancements.validate()
            guard (3...5).contains(version), enhancements.moire == Double(moire),
                  baselineExposure == Float(Double(enhancements.baselineReference) + enhancements.baselineCompensation) else {
                throw DoctoredCoreError(message: "The saved RAW enhancement controls disagree with their effective camera settings.")
            }
            for local in enhancements.locals {
                if let effective = local.effective {
                    var variant = self
                    variant.enhancements = nil
                    variant.sourceGeometry = nil
                    variant.decoderFrame = nil
                    variant.vignetteReference = nil
                    variant.lensProfile = nil
                    variant.version = cameraOnlyVersion
                    variant.values = effective.values
                    variant.chromaticityX = effective.chromaticityX
                    variant.chromaticityY = effective.chromaticityY
                    try variant.validate()
                }
            }
        }
        if version <= 3 {
            guard sourceGeometry == nil, decoderFrame == nil, vignetteReference == nil else {
                throw DoctoredCoreError(message: "Older RAW recipes cannot contain registered geometry.")
            }
        }
        if let sourceGeometry { try sourceGeometry.validate() }
        if let decoderFrame { try decoderFrame.validate() }
        if let lensProfile {
            guard version == 5, !capabilities.lensCorrection, !lensProfile.id.isEmpty,
                  (try? JSONSerialization.jsonObject(with: Data(lensProfile.profile.utf8))) is [String: Any] else {
                throw DoctoredCoreError(message: "The saved Lensfun profile does not fit its RAW recipe.")
            }
        }
        if let vignetteReference {
            guard enhancements != nil else {
                throw DoctoredCoreError(message: "A registered RAW vignette requires enhancements.")
            }
            try vignetteReference.validate()
        }
    }

    func apply(to filter: CIRAWFilter) throws {
        try validate()
        guard hasSupportedOutputPolicy,
              let direction = CGImagePropertyOrientation(rawValue: orientation) else {
            throw DoctoredCoreError(message: "This saved RAW recipe is unavailable in this build. Its saved rendering is intact.")
        }
        let current = RawCapabilities(filter)
        guard current.lensCorrection == capabilities.lensCorrection,
              current.luminanceNoise == capabilities.luminanceNoise,
              current.colorNoise == capabilities.colorNoise,
              current.sharpness == capabilities.sharpness, current.contrast == capabilities.contrast,
              current.detail == capabilities.detail, current.highlightRecovery == capabilities.highlightRecovery,
              filter.isMoireReductionSupported == moireSupported,
              filter.isLocalToneMapSupported == localToneMapSupported else {
            throw DoctoredCoreError(message: "The available RAW decoder has different capabilities. Its saved rendering is intact.")
        }
        filter.orientation = direction
        filter.isDraftModeEnabled = draft
        filter.scaleFactor = scale
        filter.exposure = Float(values.exposure)
        filter.baselineExposure = baselineExposure
        filter.shadowBias = shadowBias
        filter.boostAmount = Float(values.toneCurve)
        filter.boostShadowAmount = Float(values.shadows)
        filter.isGamutMappingEnabled = gamutMapping
        filter.extendedDynamicRangeAmount = extendedDynamicRange
        if capabilities.lensCorrection { filter.isLensCorrectionEnabled = values.lensCorrection }
        if capabilities.luminanceNoise { filter.luminanceNoiseReductionAmount = Float(values.luminanceNoise) }
        if capabilities.colorNoise { filter.colorNoiseReductionAmount = Float(values.colorNoise) }
        if capabilities.sharpness { filter.sharpnessAmount = Float(values.sharpness) }
        if capabilities.contrast { filter.contrastAmount = Float(values.contrast) }
        if capabilities.detail { filter.detailAmount = Float(values.detail) }
        if moireSupported { filter.moireReductionAmount = moire }
        // The float development's local tone map is off (its tone curve applies).
        if localToneMapSupported { filter.localToneMapAmount = isLinearFloat ? 0 : localToneMap }
        if #available(macOS 26.0, *), capabilities.highlightRecovery {
            filter.isHighlightRecoveryEnabled = values.highlightRecovery
        }
        // Saved white balance is the captured chromaticity. Replaying the
        // derived temperature/tint on each exposure edit would round-trip it
        // again and allow small cumulative changes in untouched white balance.
        filter.neutralChromaticity = CGPoint(x: chromaticityX, y: chromaticityY)
    }
}

/// A RAW recipe's Lensfun profile (v1-lensfun): the database lens id, its model name for
/// display, and the resolved `lens_correction` `profile` object as sorted-key JSON, fixed at the
/// first development so a later database or EXIF reading never changes a saved development.
struct RawLensfunProfile: Codable, Equatable {
    var id: String
    var lens: String
    var profile: String
}

struct RawSourceDescriptor: Codable, Equatable {
    struct Decoder: Codable, Equatable {
        let identifier: String
        let version: String
        let implementation: String
    }
    let schema_version: Int
    let original_filename: String
    let content_sha256: String
    let decoder: Decoder
    var recipe: RawDevelopmentRecipe

    static let decoderIdentifier = "com.apple.CoreImage.CIRAWFilter"
    static var implementation: String {
        let bundle = Bundle(for: CIRAWFilter.self)
        let version = bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "unknown"
        return "\(ProcessInfo.processInfo.operatingSystemVersionString); CoreImage \(version)"
    }
    var compatible: Bool {
        schema_version == 1 && decoder.identifier == Self.decoderIdentifier
            && decoder.implementation == Self.implementation && recipe.hasSupportedOutputPolicy
    }
    func json() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return String(decoding: try encoder.encode(self), as: UTF8.self)
    }
    static func read(_ info: [String: Any]) throws -> Self {
        guard let raw = info["raw"] as? [String: Any] else {
            throw DoctoredCoreError(message: "Choose a retained RAW layer.")
        }
        do {
            let result = try JSONDecoder().decode(Self.self, from: JSONSerialization.data(withJSONObject: raw))
            var stored = raw
            stored.removeValue(forKey: "encoded_bytes")
            let known = try JSONSerialization.jsonObject(with: Data(result.json().utf8))
            guard sameKeys(stored, known) else {
                throw DoctoredCoreError(message: "The saved RAW recipe contains unknown controls.")
            }
            try result.recipe.validate()
            try result.recipe.validateLocalEffectiveState()
            return result
        } catch {
            throw DoctoredCoreError(message: "The saved RAW recipe is unavailable in this build. Its saved rendering is intact.")
        }
    }

    private static func sameKeys(_ actual: Any, _ known: Any) -> Bool {
        if let values = actual as? [Any], let expected = known as? [Any] {
            return values.count == expected.count && zip(values, expected).allSatisfy { sameKeys($0.0, $0.1) }
        }
        if let object = actual as? [String: Any], let expected = known as? [String: Any] {
            guard Set(object.keys) == Set(expected.keys) else { return false }
            return object.keys.allSatisfy { key in
                guard let value = object[key], let expectedValue = expected[key] else { return false }
                return sameKeys(value, expectedValue)
            }
        }
        return !(actual is [String: Any]) && !(known is [String: Any])

    }
}

/// The platform decode seam emits real UInt16 samples directly. A separate
/// RGBA8 render serves only the Develop window; it never enters source storage.
enum RetainedRawDecoder {
    static let contextOptions: [CIContextOption: Any] = [.workingFormat: CIFormat.RGBAf,
                                                         .outputPremultiplied: false]
    static let context = CIContext(options: contextOptions)
    static func filter(url: URL, descriptor: RawSourceDescriptor) throws -> CIRAWFilter {
        guard descriptor.compatible else {
            throw DoctoredCoreError(message: "The saved RAW decoder implementation is unavailable. The saved rendering remains usable; redevelopment requires the captured decoder.")
        }
        guard let filter = CIRAWFilter(imageURL: url),
              let version = filter.supportedDecoderVersions.first(where: { $0.rawValue == descriptor.decoder.version && $0 != .none }) else {
            throw DoctoredCoreError(message: "The captured RAW decoder version is unavailable. The saved rendering remains usable.")
        }
        filter.decoderVersion = version
        try descriptor.recipe.apply(to: filter)
        return filter
    }

    static func capture(url: URL, filename: String, sha256: String) throws -> RawSourceDescriptor {
        guard let filter = CIRAWFilter(imageURL: url),
              filter.supportedDecoderVersions.contains(where: { $0 != .none }),
              filter.outputImage != nil else { throw RawImportError(url: url, reason: "no camera RAW decoder is available") }
        return RawSourceDescriptor(schema_version: 1, original_filename: filename, content_sha256: sha256,
            decoder: .init(identifier: RawSourceDescriptor.decoderIdentifier, version: filter.decoderVersion.rawValue,
                           implementation: RawSourceDescriptor.implementation), recipe: .init(filter: filter))
    }

    private static func preparedFilter(url: URL, descriptor: RawSourceDescriptor,
                                       whiteBalanceChanged: Bool) throws -> CIRAWFilter {
        let filter = try filter(url: url, descriptor: descriptor)
        if whiteBalanceChanged {
            filter.neutralTemperature = Float(descriptor.recipe.values.temperature)
            filter.neutralTint = Float(descriptor.recipe.values.tint)
            // Canonicalize the first custom render through the same setter
            // used to restore it. CI's temperature/tint→XY getter alone differs
            // by a u16 code from replaying that XY; the explicit round-trip
            // makes exposure-only redevelopment reproduce every source code.
            filter.neutralChromaticity = filter.neutralChromaticity
        }
        return filter
    }

    private static func effectiveDescriptor(_ descriptor: RawSourceDescriptor, filter: CIRAWFilter) -> RawSourceDescriptor {
        var effective = descriptor
        let capabilities = RawCapabilities(filter)
        var values = capabilities.defaults
        values.customWhiteBalance = descriptor.recipe.values.customWhiteBalance
        // v1-lensfun: the decoder reads no Lensfun toggle back; keep the recipe's.
        if !capabilities.lensCorrection, descriptor.recipe.lensProfile != nil {
            values.lensCorrection = descriptor.recipe.values.lensCorrection
        }
        effective.recipe.values = values
        effective.recipe.chromaticityX = filter.neutralChromaticity.x
        effective.recipe.chromaticityY = filter.neutralChromaticity.y
        return effective
    }

    static func canonicalDescriptor(url: URL, descriptor: RawSourceDescriptor,
                                    whiteBalanceChanged: Bool) throws -> RawSourceDescriptor {
        effectiveDescriptor(descriptor, filter: try preparedFilter(url: url, descriptor: descriptor,
                                                                  whiteBalanceChanged: whiteBalanceChanged))
    }

    static func decoderFrame(url: URL, descriptor: RawSourceDescriptor) throws -> RawDecoderFrame {
        let filter = try preparedFilter(url: url, descriptor: descriptor, whiteBalanceChanged: false)
        guard let rect = filter.outputImage?.extent.integral,
              rect.width.isFinite, rect.height.isFinite,
              rect.width >= 1, rect.height >= 1 else {
            throw DoctoredCoreError(message: "The RAW decoder has no usable proposed frame.")
        }
        let swapped = filter.orientation.rawValue >= 5
        let frame = RawDecoderFrame(orientation: filter.orientation.rawValue,
            extent: [Double(rect.minX), Double(rect.minY),
                Double(swapped ? rect.height : rect.width),
                Double(swapped ? rect.width : rect.height)])
        try frame.validate()
        return frame
    }

    static func develop(url: URL, descriptor: RawSourceDescriptor, maximumScratch: UInt64 = UInt64.max,
                        whiteBalanceChanged: Bool = false, cameraOnly: Bool = false) throws -> (RasterDocument, RawSourceDescriptor) {
        guard cameraOnly || descriptor.recipe.enhancements?.locals.isEmpty != false else {
            throw DoctoredCoreError(message: "Local RAW development requires its retained native mask resources.")
        }
        let filter = try preparedFilter(url: url, descriptor: descriptor, whiteBalanceChanged: whiteBalanceChanged)
        guard let output = filter.outputImage else { throw RawImportError(url: url, reason: "the captured decoder returned no image") }
        let rect = output.extent.integral
        guard rect.width.isFinite, rect.height.isFinite, rect.width >= 1, rect.height >= 1,
              rect.width * rect.height <= Double(RasterImage.maxResizePixels) else {
            throw RawImportError(url: url, reason: "the developed image exceeds the supported size")
        }
        let width = Int(rect.width), height = Int(rect.height)
        let linearFloat = descriptor.recipe.isLinearFloat
        let actualScratch = try RawSourceWork.scratch(width: width, height: height,
            encoded: RawSourceSnapshot.size(url), float: linearFloat)
        guard actualScratch <= maximumScratch else {
            throw RawImportError(url: url, reason: "its decoded extent exceeds the captured development budget")
        }
        _ = try RasterResources.shared.admit(scratch: actualScratch)
        guard let outputSpace = descriptor.recipe.outputColorSpace else {
            throw RawImportError(url: url, reason: "the saved output color policy is unsupported")
        }
        let document: RasterDocument
        if linearFloat {
            // The 32-bit float (linear) development: the same filter
            // rendered as f32 into the output space's primaries with a linear
            // transfer and no clamp. Nothing is repaired: an alpha that is not
            // 1 is refused here, and a non-finite sample or an RGB magnitude
            // above 65504 by the core's float intake.
            guard let space = outputSpace.linearCGSpace else {
                throw RawImportError(url: url, reason: "the platform has no linear \(outputSpace.displayName) "
                    + "space for a 32-bit float (linear) development")
            }
            var samples = [Float](repeating: 0, count: width * height * 4)
            do {
                let renderer = CIContext(options: contextOptions)
                samples.withUnsafeMutableBytes { bytes in
                    guard let base = bytes.baseAddress else { return }
                    renderer.render(output, toBitmap: base, rowBytes: width * 16, bounds: rect,
                                    format: .RGBAf, colorSpace: space)
                }
                renderer.clearCaches()
            }
            guard stride(from: 3, to: samples.count, by: 4).allSatisfy({ samples[$0] == 1 }) else {
                throw RawImportError(url: url, reason: "the platform did not produce an opaque 32-bit float camera rendering")
            }
            document = try RasterDocument.fromRawRGBA32(samples, width: width, height: height,
                                                        profile: outputSpace.profileData)
        } else {
            var pixels = [UInt16](repeating: 0, count: width * height * 4)
            let space = outputSpace.cgSpace
            do {
                let renderer = CIContext(options: contextOptions)
                pixels.withUnsafeMutableBytes { bytes in
                    guard let base = bytes.baseAddress else { return }
                    renderer.render(output, toBitmap: base, rowBytes: width * 8, bounds: rect,
                                    format: .RGBA16, colorSpace: space)
                }
                renderer.clearCaches()
            }
            guard stride(from: 3, to: pixels.count, by: 4).allSatisfy({ pixels[$0] == UInt16.max }) else {
                throw RawImportError(url: url, reason: "the platform did not produce an opaque 16-bit camera rendering")
            }
            document = try RasterDocument.fromRawRGBA16(pixels, width: width, height: height,
                                                        profile: outputSpace.profileData)
        }
        // Record Float-valued controls as the decoder reads them back. This
        // also records the coupled white balance after both setters ran.
        var effective = effectiveDescriptor(descriptor, filter: filter)
        if effective.recipe.version >= 4, effective.recipe.decoderFrame == nil {
            let swapped = filter.orientation.rawValue >= 5
            effective.recipe.decoderFrame = RawDecoderFrame(orientation: filter.orientation.rawValue,
                extent: [Double(rect.origin.x), Double(rect.origin.y),
                    Double(swapped ? height : width), Double(swapped ? width : height)])
        }
        if !cameraOnly {
            var developed = document
            if descriptor.recipe.lensfunApplies {
                developed = try developed.applyingRawLensfun(descriptor.recipe.lensProfile)
            }
            if let vignette = descriptor.recipe.enhancements?.vignette, vignette.edgeExposure != 0 {
                developed = try developed.developLocals(variants: [], masks: [], settings: "[]",
                    vignette: vignette.coreJSON(width: width, height: height,
                        reference: descriptor.recipe.vignetteReference))
            }
            if let geometry = descriptor.recipe.sourceGeometry {
                developed = try developed.applyingRawSourceGeometry(geometry.map().json())
            }
            developed = try developed.applyingRawPresence(descriptor.recipe.enhancements?.presence)
            developed = try developed.applyingRawGrain(descriptor.recipe.enhancements?.grain,
                contentSHA256: descriptor.content_sha256)
            return (developed, effective)
        }
        return (document, effective)
    }

    static func preview(url: URL, descriptor: RawSourceDescriptor, values: RawDevelopValues,
                        space _: CGColorSpace) -> CGImage? {
        var request = descriptor
        request.recipe.values = values
        if !values.customWhiteBalance {
            request.recipe.chromaticityX = request.recipe.asShotChromaticityX
            request.recipe.chromaticityY = request.recipe.asShotChromaticityY
        }
        guard let filter = try? filter(url: url, descriptor: request) else { return nil }
        if values.customWhiteBalance,
           values.temperature != descriptor.recipe.values.temperature || values.tint != descriptor.recipe.values.tint {
            filter.neutralTemperature = Float(values.temperature)
            filter.neutralTint = Float(values.tint)
            filter.neutralChromaticity = filter.neutralChromaticity
        }
        filter.scaleFactor = Float(min(1, 1600 / max(filter.nativeSize.width, filter.nativeSize.height, 1)))
        filter.isDraftModeEnabled = true
        guard let output = filter.outputImage else { return nil }
        guard let outputSpace = request.recipe.outputColorSpace else { return nil }
        // The preview follows the chosen normalized master encoding.
        return context.createCGImage(output, from: output.extent.integral, format: .RGBA8,
                                     colorSpace: outputSpace.cgSpace)
    }
}
