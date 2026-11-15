import Foundation

// A RAW local's tone and colour controls: Exposure (a linear-light gain on the development,
// in stops), Contrast, Highlights, Shadows, Whites, Blacks, Saturation, Clarity, Texture, and the
// detail pair Sharpness and Noise Reduction. The core applies them to the development through
// its existing adjustment, Presence, Reduce Noise, Smart Sharpen and Gaussian Blur kernels and
// blends the result through the local's coverage (`core/src/raw_local_tone.rs` has the mapping,
// `core/src/local_detail.rs` the detail stage's), so a local with only these controls develops no
// camera variant. On a 32-bit float (linear) development each control runs in the domain its
// filter or adjustment uses at 32 bits (rule L, `core/src/raw_local_tone_float.rs`). Sharpness and Noise Reduction are stored only when not 0, so a
// recipe without them encodes byte-identically to one written before they existed (and a recipe
// that names them is one an older build's `sameKeys` check refuses, keeping its saved rendering).
// Camera exposure (`exposure_offset`) and white balance stay on the camera path: they re-develop
// the RAW, which recovers highlights a gain on the development cannot. This file is the recipe's
// model: the JSON an `edit_raw_development` operation carries, what `get_raw_locals` reports, and
// the settings the core receives.

struct RawLocalTone: Codable, Equatable {
    var exposure = 0.0
    var contrast = 0.0
    var highlights = 0.0
    var shadows = 0.0
    var whites = 0.0
    var blacks = 0.0
    var saturation = 0.0
    var clarity = 0.0
    var texture = 0.0
    /// nil ≡ 0: the subscript stores 0 as nil, so the recipe carries the key only when it is set.
    var sharpness: Double?
    var noiseReduction: Double?

    static let refusal = "A RAW local tone needs exposure −4…4 stops, highlights, shadows and noise reduction 0…100, and contrast, whites, blacks, saturation, clarity, texture and sharpness −100…100."

    /// The JSON keys in panel order, each with its range.
    static let fields: [(key: String, range: ClosedRange<Double>)] = [
        ("exposure", -4...4), ("contrast", -100...100), ("highlights", 0...100), ("shadows", 0...100),
        ("whites", -100...100), ("blacks", -100...100), ("saturation", -100...100),
        ("clarity", -100...100), ("texture", -100...100), ("sharpness", -100...100),
        ("noise_reduction", 0...100)]

    /// Every control at 0: the local's tone changes nothing, and the recipe stores none.
    var isNeutral: Bool { self == RawLocalTone() }

    /// Whether the local reads a neighbourhood (a Shadows/Highlights guide, the Presence plane or
    /// the detail stage): the single Swift copy of the core's `LocalTone::reads_plane`.
    var readsPlane: Bool {
        highlights != 0 || shadows != 0 || clarity != 0 || texture != 0
            || self["sharpness"] != 0 || self["noise_reduction"] != 0
    }

    /// The core's own bounds (`LocalTone::validate`), so a refusal names the tone here rather
    /// than inside the development.
    func validate() throws {
        for (key, range) in Self.fields {
            let value = self[key]
            guard value.isFinite, range.contains(value) else { throw DoctoredCoreError(message: Self.refusal) }
        }
    }

    subscript(key: String) -> Double {
        get {
            switch key {
            case "exposure": return exposure
            case "contrast": return contrast
            case "highlights": return highlights
            case "shadows": return shadows
            case "whites": return whites
            case "blacks": return blacks
            case "saturation": return saturation
            case "clarity": return clarity
            case "texture": return texture
            case "sharpness": return sharpness ?? 0
            case "noise_reduction": return noiseReduction ?? 0
            default: return 0
            }
        }
        set {
            switch key {
            case "exposure": exposure = newValue
            case "contrast": contrast = newValue
            case "highlights": highlights = newValue
            case "shadows": shadows = newValue
            case "whites": whites = newValue
            case "blacks": blacks = newValue
            case "saturation": saturation = newValue
            case "clarity": clarity = newValue
            case "texture": texture = newValue
            case "sharpness": sharpness = newValue == 0 ? nil : newValue
            case "noise_reduction": noiseReduction = newValue == 0 ? nil : newValue
            default: break
            }
        }
    }

    /// Every key, by its JSON name: what `get_raw_locals` reports and the core receives.
    var json: [String: Any] {
        Dictionary(uniqueKeysWithValues: Self.fields.map { ($0.key, self[$0.key] as Any) })
    }
}

extension RawLocalTone {
    /// The `tone` object of a `create` or `update` operation. Omitted keys are 0; unknown keys are
    /// refused.
    init(json object: [String: Any]) throws {
        self.init()
        try RawLocalJSON.keys(object, Set(Self.fields.map(\.key)))
        for (key, range) in Self.fields {
            guard object[key] != nil else { continue }
            guard let value = try? RawLocalJSON.number(object, key, range: range) else {
                throw DoctoredCoreError(message: Self.refusal)
            }
            self[key] = value
        }
        try validate()
    }
}
