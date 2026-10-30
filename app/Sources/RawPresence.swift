import Foundation

// Develop Presence runs the Filter ▸ Presence kernels on the finished 16-bit development, after
// locals, vignette and explicit optics and before grain.

/// Develop Texture, Clarity and Dehaze: the core `presence` kernel (Dehaze, then Texture
/// and Clarity of the dehazed picture) applied to the developed, display-encoded u16 output, so
/// its midtone weight and airlight read the developed tones. It runs after locals, vignette and
/// explicit source geometry, so the bands and airlight are measured on the frame as published,
/// and before grain, so it never amplifies synthetic grain. Its radii scale with the long side
/// (`presence_radii`), which keeps the look the same at any resolution. All three at 0 is off and
/// is stored as an absent key, never a neutral value.
struct RawPresenceSettings: Codable, Equatable {
    var texture: Double = 0
    var clarity: Double = 0
    var dehaze: Double = 0

    static let neutral = RawPresenceSettings()
    static let range = -100.0...100.0
    /// The sentence a 32-bit float (linear) RAW development throws for a nonzero amount.
    static let floatRefusal = "Texture, Clarity and Dehaze are defined on display-referred values and are not available in a 32-bit float RAW development."

    var isNeutral: Bool { texture == 0 && clarity == 0 && dehaze == 0 }

    func validate() throws {
        for (name, value) in [("texture", texture), ("clarity", clarity), ("dehaze", dehaze)] {
            guard value.isFinite, Self.range.contains(value) else {
                throw DoctoredCoreError(message: "RAW \(name) must be between −100 and 100.")
            }
        }
    }

    func patched(texture: Double?, clarity: Double?, dehaze: Double?) -> RawPresenceSettings {
        var result = self
        if let texture { result.texture = texture }
        if let clarity { result.clarity = clarity }
        if let dehaze { result.dehaze = dehaze }
        return result
    }
}

extension RasterDocument {
    /// Returns self for nil/neutral Presence or a core no-op; throws without RGBA16 storage.
    /// With no `control` it runs under a fresh one: an uncontrolled core call is capped at
    /// `MAX_DESTRUCTIVE_WORK`, which refuses a 45 MP development, while a controlled job is
    /// admitted by memory (`destructive_work_cap`).
    func applyingRawPresence(_ presence: RawPresenceSettings?,
                             control: RasterJobControl? = nil) throws -> RasterDocument {
        guard let presence, !presence.isNeutral else { return self }
        try presence.validate()
        guard let precision else {
            throw DoctoredCoreError(message: "RAW Presence requires genuine RGBA16 storage.")
        }
        let request = try RawLocalJSON.string(["layer": 0, "op": "presence", "params": [
            "texture": presence.texture, "clarity": presence.clarity, "dehaze": presence.dehaze]])
        let budget = try PrecisionWorkContext(source: precision)
        let job = try control ?? RasterJobControl()
        let next = try precision.selectedEdit(request: request, coverage: [], workingLimit: budget.limit,
                                              otherRetained: budget.otherRetained, control: job)
        return try next.map { try RasterDocument(precision: $0) } ?? self
    }
}
