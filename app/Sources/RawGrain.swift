import Foundation
import CoreFoundation

// Develop grain is the last stage after optics, seeded from the camera file's SHA-256.

/// Develop grain: the `add_grain` kernel as the last development stage,
/// after the camera decode, locals/vignette and explicit source geometry.
/// Amount 0 is no grain and is stored as an absent key, never a neutral value.
struct RawGrainSettings: Codable, Equatable {
    var amount: Double
    /// Gaussian sigma in pixels, the `add_grain` unit.
    var size: Double = 1
    var roughness: Double = 50

    static let neutral = RawGrainSettings(amount: 0)
    static let amountRange = 0.0...100.0, sizeRange = 0.5...4.0, roughnessRange = 0.0...100.0

    var isNeutral: Bool { amount == 0 }

    func validate() throws {
        guard amount.isFinite, Self.amountRange.contains(amount) else {
            throw DoctoredCoreError(message: "RAW grain amount must be between 0 and 100.")
        }
        guard size.isFinite, Self.sizeRange.contains(size) else {
            throw DoctoredCoreError(message: "RAW grain size must be between 0.5 and 4 pixels.")
        }
        guard roughness.isFinite, Self.roughnessRange.contains(roughness) else {
            throw DoctoredCoreError(message: "RAW grain roughness must be between 0 and 100.")
        }
    }

    var json: [String: Any] { ["amount": amount, "size": size, "roughness": roughness] }

    /// Strict: unknown keys and out-of-range or boolean values are refused by
    /// name. Omitted size/roughness take the defaults, never a saved value.
    static func parse(_ object: [String: Any]) throws -> RawGrainSettings {
        let allowed: Set<String> = ["amount", "size", "roughness"]
        guard Set(object.keys).isSubset(of: allowed) else {
            throw DoctoredCoreError(message: "Unknown RAW grain field: \(Set(object.keys).subtracting(allowed).sorted().joined(separator: ", ")).")
        }
        func number(_ key: String, _ range: ClosedRange<Double>) throws -> Double? {
            guard let raw = object[key] else { return nil }
            guard let value = raw as? NSNumber, CFGetTypeID(value) != CFBooleanGetTypeID(),
                  value.doubleValue.isFinite, range.contains(value.doubleValue) else {
                throw DoctoredCoreError(message: "RAW grain \(key) must be a number in \(range.lowerBound)…\(range.upperBound).")
            }
            return value.doubleValue
        }
        guard let amount = try number("amount", amountRange) else {
            throw DoctoredCoreError(message: "RAW grain requires amount (0…100).")
        }
        var result = RawGrainSettings(amount: amount)
        if let size = try number("size", sizeRange) { result.size = size }
        if let roughness = try number("roughness", roughnessRange) { result.roughness = roughness }
        return result
    }

    /// The first 8 bytes (big-endian) of the embedded camera file's SHA-256:
    /// identical across re-render, redevelop, reopen, presets and sync.
    static func seed(contentSHA256: String) throws -> UInt64 {
        guard contentSHA256.utf8.count == 64,
              contentSHA256.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }),
              let seed = UInt64(contentSHA256.prefix(16), radix: 16) else {
            throw DoctoredCoreError(message: "RAW grain needs the camera file's SHA-256 digest.")
        }
        return seed
    }
}

extension RasterDocument {
    /// Returns self for nil/neutral grain or a core no-op.
    ///
    /// A 32-bit float (linear) development (#4) takes grain too, by
    /// Doctored's own rule (not measured against Adobe Camera Raw): the same
    /// request runs the one Add Grain kernel on the layer's display encoding
    /// E (the document profile's curve on 0…1, continued as a logarithm above
    /// white, `display_encoding`) and decodes the result back. Each pixel's
    /// encoded offset δ is the 16-bit grain's δ for its encoded luma clamped
    /// to 0…1, added to each channel's E(v) without the 16-bit 0…1 clamp
    /// (float colour is never clamped). So a channel whose encoded result stays
    /// in 0…1 gets exactly the 16-bit grain of its encoded value; one pushed
    /// past white continues above white and one pushed past black goes below
    /// zero, where 16 bits clips; a pixel whose encoded luma is at or above
    /// white keeps its value (white's offset is 0); and a channel above white
    /// in a darker pixel takes that pixel's δ in encoded units.
    ///
    /// Its charge is the core's own float selected-edit plan for `add_grain`
    /// (`float_filter_apply`), measured in Release (`scripts/raw-decode-probe`
    /// mode `grain`, GR32, `applyingRawGrain` alone on an open float
    /// development, plan = the request's `selectedEditPlan` `scratch_bytes`,
    /// MallocLargeCache=0, 3 runs + twin, every twin E 0): plan ÷ max E is
    /// 1.323 on the 6 MP NEF, 1.307 on D24 and 1.304 on the 45 MP D850, every
    /// case ≥ 1.25, so no constant was raised.
    func applyingRawGrain(_ grain: RawGrainSettings?, contentSHA256: String,
                          control: RasterJobControl? = nil) throws -> RasterDocument {
        guard let grain, !grain.isNeutral else { return self }
        try grain.validate()
        guard let precision else {
            throw DoctoredCoreError(message: "RAW grain requires genuine RGBA16 or 32-bit float storage.")
        }
        let seed = try RawGrainSettings.seed(contentSHA256: contentSHA256)
        let request = try RawLocalJSON.string(["layer": 0, "op": "add_grain", "params": [
            "amount": grain.amount, "size": grain.size, "roughness": grain.roughness,
            "seed": Int(bitPattern: UInt(seed))]])
        let budget = try PrecisionWorkContext(source: precision)
        let job = try control ?? RasterJobControl()
        let next = try precision.selectedEdit(request: request, coverage: [], workingLimit: budget.limit,
                                              otherRetained: budget.otherRetained, control: job)
        return try next.map { try RasterDocument(precision: $0) } ?? self
    }
}
