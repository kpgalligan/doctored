import Foundation
import CoreFoundation

// A RAW local's range: a luminance band, a colour distance, or both, that multiplies the local's
// shape coverage. The core evaluates it on the BASE development (the camera-only global render,
// before any local) as CIE L*a*b* against D50 through the base's own profile, so a local's own
// exposure or white balance cannot move its selection and the same bounds select the same tones in
// every output space (`core/src/raw_local_range.rs` has the laws). This file is the recipe's model
// of it: the JSON an `edit_raw_development` operation carries, what `get_raw_locals` reports, and
// the settings the core receives.
//
// A colour is stored as `lab`. A request may name a source point instead (`sample`: the Sample
// Colour click, or an MCP caller that knows where but not what); `RawLocalEvaluation` resolves it
// on the base and a saved recipe never holds one.

/// `low`…`high` are L*/100. Each falloff is the width, in the same unit, of the smoothstep outside
/// its bound; 0 is a hard edge.
struct RawLuminanceRange: Codable, Equatable {
    var low: Double
    var high: Double
    var lowFalloff: Double = 0
    var highFalloff: Double = 0
    var invert = false
}

/// `amount` is the ΔE*ab radius of full coverage about the colour and `falloff` the width of the
/// smoothstep past it. Exactly one of `lab` ([L, a, b]) and `sample` ([x, y], oriented source
/// pixels) is set.
struct RawColorRange: Codable, Equatable {
    var lab: [Double]?
    var sample: [Double]?
    var amount: Double
    var falloff: Double = 0
    var invert = false
}

struct RawLocalRange: Codable, Equatable {
    var luminance: RawLuminanceRange?
    var color: RawColorRange?

    static let refusal = "A RAW local range needs a luminance band with 0 ≤ low ≤ high ≤ 1 and falloffs 0…1, or a colour with lab [L 0…100, a and b −256…256] or sample [x, y] and amount and falloff 0…200."

    init(luminance: RawLuminanceRange? = nil, color: RawColorRange? = nil) {
        self.luminance = luminance; self.color = color
    }

    /// The `range` object of a `create` or `update` operation. Omitted falloffs are 0 and an
    /// omitted `invert` is false; unknown keys are refused.
    init(json object: [String: Any]) throws {
        try RawLocalJSON.keys(object, ["luminance", "color"])
        if let raw = object["luminance"], !(raw is NSNull) {
            guard let fields = raw as? [String: Any] else { throw DoctoredCoreError(message: Self.refusal) }
            try RawLocalJSON.keys(fields, ["low", "high", "low_falloff", "high_falloff", "invert"])
            guard let low = try RawLocalJSON.number(fields, "low", range: 0...1),
                  let high = try RawLocalJSON.number(fields, "high", range: 0...1) else {
                throw DoctoredCoreError(message: Self.refusal)
            }
            luminance = RawLuminanceRange(low: low, high: high,
                lowFalloff: try RawLocalJSON.number(fields, "low_falloff", range: 0...1) ?? 0,
                highFalloff: try RawLocalJSON.number(fields, "high_falloff", range: 0...1) ?? 0,
                invert: try RawLocalJSON.flag(fields, "invert") ?? false)
        }
        if let raw = object["color"], !(raw is NSNull) {
            guard let fields = raw as? [String: Any] else { throw DoctoredCoreError(message: Self.refusal) }
            try RawLocalJSON.keys(fields, ["lab", "sample", "amount", "falloff", "invert"])
            guard let amount = try RawLocalJSON.number(fields, "amount", range: 0...200) else {
                throw DoctoredCoreError(message: Self.refusal)
            }
            color = RawColorRange(lab: try Self.numbers(fields, "lab", count: 3),
                sample: try Self.numbers(fields, "sample", count: 2), amount: amount,
                falloff: try RawLocalJSON.number(fields, "falloff", range: 0...200) ?? 0,
                invert: try RawLocalJSON.flag(fields, "invert") ?? false)
        }
        try validate(resolved: false)
    }

    private static func numbers(_ object: [String: Any], _ key: String, count: Int) throws -> [Double]? {
        guard let raw = object[key] else { return nil }
        guard let values = raw as? [Any], values.count == count else { throw DoctoredCoreError(message: refusal) }
        return try values.map {
            guard let number = $0 as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
                  number.doubleValue.isFinite else { throw DoctoredCoreError(message: refusal) }
            return number.doubleValue
        }
    }

    /// What `get_raw_locals` reports and an operation carries: every field, by its JSON name. A
    /// pending colour reports `sample`; a saved one always reports `lab`.
    var json: [String: Any] {
        var result: [String: Any] = [:]
        if let luminance {
            result["luminance"] = ["low": luminance.low, "high": luminance.high,
                "low_falloff": luminance.lowFalloff, "high_falloff": luminance.highFalloff,
                "invert": luminance.invert] as [String: Any]
        }
        if let color {
            var fields: [String: Any] = ["amount": color.amount, "falloff": color.falloff, "invert": color.invert]
            if let lab = color.lab { fields["lab"] = lab }
            if let sample = color.sample { fields["sample"] = sample }
            result["color"] = fields
        }
        return result
    }

    /// The core's own bounds (`LocalRange::validate`), so a refusal names the range here rather
    /// than inside the development. `resolved` also requires the colour to be a `lab`: a saved
    /// recipe and the core's settings never carry a sample point.
    func validate(resolved: Bool) throws {
        func within(_ value: Double, _ range: ClosedRange<Double>) -> Bool {
            value.isFinite && range.contains(value)
        }
        var valid = luminance != nil || color != nil
        if let luminance {
            valid = valid && within(luminance.low, 0...1) && within(luminance.high, 0...1)
                && luminance.low <= luminance.high
                && within(luminance.lowFalloff, 0...1) && within(luminance.highFalloff, 0...1)
        }
        if let color {
            valid = valid && within(color.amount, 0...200) && within(color.falloff, 0...200)
                && (color.lab == nil) != (color.sample == nil)
            if let lab = color.lab {
                valid = valid && lab.count == 3 && within(lab[0], 0...100)
                    && within(lab[1], -256...256) && within(lab[2], -256...256)
            }
            if let sample = color.sample {
                valid = valid && !resolved && sample.count == 2
                    && sample.allSatisfy { $0.isFinite && $0 >= 0 }
            }
        }
        guard valid else { throw DoctoredCoreError(message: Self.refusal) }
    }

    /// The `range` member of one local's settings for `dr_precision_doc_develop_locals`, and the
    /// range JSON of the mask overlay. Refuses a colour that is still a sample point.
    func coreJSON() throws -> [String: Any] {
        try validate(resolved: true)
        return json
    }
}
