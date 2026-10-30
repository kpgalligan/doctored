import AppKit

/// Explicit normalized SDR master encoding. The document's working profile is
/// independent: replacing a source converts only its evaluated layer fallback.
///
/// Where the profile lands. A 16-bit open: the document's profile is the
/// output space's. An 8-bit open, a place and a redevelop: the layer's retained
/// 16-bit master carries it, the document's profile is unchanged and the core
/// converts the fallback.
///
/// The four profiles are the core's own built-ins. Adobe RGB (1998) and
/// ProPhoto RGB are built there from the published primaries; no system or
/// Adobe profile file is read. Adobe RGB (1998) carries the reference
/// profile's 563/256 gamma. Core Image encodes it with 2.2, at most 1.65e-4
/// linear away (about 0.04 of an 8-bit level), and nothing converts pixels to
/// close that. ProPhoto RGB is ProPhoto RGB at gamma 1.8, a pure power, not the
/// ISO 22028-2 ROMM RGB encoding with its linear toe: the profile carries the
/// 461/256 gamma of Kodak's reference profile. Core Image is not handed that
/// profile to render into, because it encodes a toe into it that the profile
/// does not declare; `cgSpace` gives it the system's named ROMM RGB space, which
/// it encodes as a pure 1.8 power, at most 1.94e-4 linear from the profile's
/// 461/256 (about 0.05 of an 8-bit level; `icc_builtin.rs`).
///
/// Out-of-range colours. Core Image renders the decoder's image into unsigned
/// normalized RGBA16 in the chosen ICC space, so a component below 0 or above 1
/// in that space is clamped per channel to 0 or 65535. The store is u16:
/// nothing wraps and no NaN can be stored, and `RetainedRawDecoder.develop`
/// keeps refusing a rendering whose alpha is not 65535 everywhere. Neutral
/// highlights clip at the same exposure in every space (white maps to white);
/// a saturated colour that clips a channel in sRGB can stay below 65535 in
/// Adobe RGB or ProPhoto.
///
/// Persistence. `outputSpace` is an existing string field the core stores
/// opaquely and presets hold the raw value, so the two new spaces bump no
/// DRDC, recipe or preset version. New cases go after `.displayP3`: probes
/// select the Develop popup by index 0 and 1.
///
/// The 32-bit float (linear) encoding (`RawOutputEncoding.linearFloat`)
/// has its own policy strings per space (`policy(_:toneCurve:)`) and the same four
/// profiles: a float document holds linear light in its profile's primaries,
/// so the development is rendered into `linearCGSpace` and tagged with
/// `profileData` as it is; no linear ICC profile is written.
enum RawOutputSpace: String, CaseIterable, Codable {
    case sRGB = "srgb"
    case displayP3 = "display_p3"
    case adobeRGB = "adobe_rgb"
    case proPhotoRGB = "prophoto_rgb"

    var builtin: RasterBuiltinProfile {
        switch self {
        case .sRGB: return .sRGB
        case .displayP3: return .displayP3
        case .adobeRGB: return .adobeRGB
        case .proPhotoRGB: return .proPhotoRGB
        }
    }

    var displayName: String {
        switch self {
        case .sRGB: return "sRGB"
        case .displayP3: return "Display P3"
        case .adobeRGB: return "Adobe RGB (1998)"
        case .proPhotoRGB: return "ProPhoto RGB"
        }
    }

    /// The ICC bytes the core writes for the space: what the retained master
    /// carries and what a 16-bit open's document is tagged with.
    var profileData: Data { RasterProfile.builtin(builtin) }

    /// The space Core Image renders the development into: the profile's own,
    /// except for ProPhoto RGB, which renders into the system's named ROMM RGB
    /// space (same primaries and white, a pure 1.8 power in Core Image) and is
    /// then tagged with `profileData`.
    var cgSpace: CGColorSpace {
        if self == .proPhotoRGB, let named = CGColorSpace(name: CGColorSpace.rommrgb) {
            return named
        }
        return ColorProfile.space(for: profileData)
    }

    var policy: String {
        switch self {
        case .sRGB: return "srgb-rgba16-normalized-v1"
        case .displayP3: return "display-p3-rgba16-normalized-v1"
        case .adobeRGB: return "adobe-rgb-1998-rgba16-normalized-v1"
        case .proPhotoRGB: return "prophoto-rgb-rgba16-normalized-v1"
        }
    }

    /// The space of a normalized 16-bit policy or of a 32-bit float (linear)
    /// one, `-v1` or `-v2`: the primaries are the same four either way.
    init?(policy: String) {
        guard let space = RawOutputSpace.allCases.first(where: { $0.names(policy) }) else {
            return nil
        }
        self = space
    }

    /// The recipe's `outputSpace` string for this space at `encoding`: the
    /// normalized policy above, or `<space>-rgba32f-linear-v1`
    /// (`srgb-rgba32f-linear-v1`, `display-p3-rgba32f-linear-v1`,
    /// `adobe-rgb-1998-rgba32f-linear-v1`, `prophoto-rgb-rgba32f-linear-v1`).
    func policy(_ encoding: RawOutputEncoding) -> String {
        switch encoding {
        case .normalized16:
            return policy
        case .linearFloat:
            let suffix = "-rgba16-normalized-v1"
            return String(policy.dropLast(suffix.count)) + "-rgba32f-linear-v1"
        }
    }

    /// The recipe's `outputSpace` string for this space at `encoding` with the
    /// recipe's tone curve: `policy(_:)`, except that a float development with
    /// a tone curve above 0 writes `<space>-rgba32f-linear-v2`. The
    /// tone curve is the decoder's own (`boostAmount`, measurement M2 in
    /// `RawOutputEncoding`), which an earlier build pinned to 0 at float:
    /// that build reads `-v2` as a policy it does not know and keeps the saved
    /// rendering instead of re-rendering it without the curve. Tone curve 0
    /// keeps `-v1` byte for byte, so every earlier float development is
    /// unchanged. The policy is the only marker: no DRDC word, no recipe or
    /// preset version.
    func policy(_ encoding: RawOutputEncoding, toneCurve: Double) -> String {
        let named = policy(encoding)
        guard encoding == .linearFloat, toneCurve > 0 else { return named }
        return String(named.dropLast("-v1".count)) + "-v2"
    }

    /// Whether `policy` is one of this space's strings: either encoding, and
    /// the float encoding's `-v1` or `-v2`.
    private func names(_ policy: String) -> Bool {
        RawOutputEncoding.allCases.contains { encoding in
            self.policy(encoding) == policy || self.policy(encoding, toneCurve: 1) == policy
        }
    }

    /// Which encoding a recipe's `outputSpace` string names (the float
    /// encoding's `-v1` or `-v2`); nil for a string no space and encoding of
    /// this build produces.
    static func encoding(policy: String) -> RawOutputEncoding? {
        RawOutputEncoding.allCases.first { encoding in
            allCases.contains { $0.policy(encoding) == policy || $0.policy(encoding, toneCurve: 1) == policy }
        }
    }

    /// The space Core Image renders a 32-bit float (linear) development into:
    /// `cgSpace`'s primaries and white with a linear transfer and no clamp
    /// (`CGColorSpaceCreateExtendedLinearized`), so a value above the SDR
    /// white stays above 1 and a colour outside the gamut keeps a component
    /// below 0. The samples are tagged with `profileData`. nil when the
    /// platform cannot linearize the space: the float development is then
    /// refused, never rendered gamma-encoded and stored as linear light.
    var linearCGSpace: CGColorSpace? {
        CGColorSpaceCreateExtendedLinearized(cgSpace)
    }

    /// The raw values a refusal lists: "srgb, display_p3, adobe_rgb or prophoto_rgb".
    static var acceptedNames: String {
        let names = allCases.map(\.rawValue)
        guard let last = names.last, names.count > 1 else { return names.joined() }
        return names.dropLast().joined(separator: ", ") + " or " + last
    }

    /// The Develop window's note under the output row: non-nil only for
    /// ProPhoto RGB heading into an 8-bit document, where 256 codes per channel
    /// spread over so wide a gamut band visibly.
    func depthNote(eightBit: Bool) -> String? {
        guard self == .proPhotoRGB, eightBit else { return nil }
        return "ProPhoto RGB is meant for 16-bit work: a 16-bit document is recommended."
    }
}

/// How a RAW development's master is encoded (MCP and Action key
/// `raw.output_encoding`).
///
/// `normalized16` is the 16-bit SDR master every development had before:
/// encoded by the output profile's curve, clamped to 0…65535. `linearFloat`
/// is the 32-bit float (linear) development: scene-linear f32 in the output
/// space's primaries, with the highlights the sensor recorded above the SDR
/// white kept above 1.0. It exists only in a 32-bit float document.
///
/// What the float encoding changes in the decoder, and nothing else: the
/// render is `.RGBAf` into `RawOutputSpace.linearCGSpace`,
/// `extendedDynamicRangeAmount` is `extendedDynamicRange` and the local tone
/// map is 0. Exposure, white balance, the tone curve (`boostAmount`) and
/// Shadows (`boostShadowAmount`), noise reduction, sharpening, detail,
/// local contrast, moiré, profile corrections and highlight recovery act in
/// the decoder as they do at 16 bits. Grain, a stage after the decoder,
/// applies on the display encoding (#4, `RawGrain.swift`).
///
/// A float development starts at tone curve 0: scene-linear, linear in
/// exposure and with no highlight shoulder, so it looks brighter in the
/// highlights than the 16-bit development of the same recipe until it is
/// tone-mapped; where a channel exceeds white the 16-bit development also
/// moves the colour toward white, so the clipped view differs from it in hue
/// as well. A tone curve above 0 is the 16-bit curve under the extended
/// range, and it keeps the values above white (measurement M2 below).
enum RawOutputEncoding: String, CaseIterable, Codable {
    case normalized16 = "sdr16"
    case linearFloat = "linear_float"

    /// `CIRAWFilter.extendedDynamicRangeAmount` for the encoding. 0 for the
    /// 16-bit master. For the float development, 1: measurement M1
    /// (`scripts/raw-probe/float_edr.py`, the Nikon D70s `camera.NEF`,
    /// macOS 26.5.1, decoder 8) took the first of 0, 1, 2 that passes its
    /// five statements. At 0 the float render is the 16-bit one in float
    /// (largest value 1.04 at +2 EV, exposure not linear: 86 % of points
    /// within 3.6…4.4 of ×4). At 1 the largest lattice value at +2 EV is
    /// 2.245 with 316 of 640 points above 1.0, +1 EV multiplies unclipped
    /// values by 2.0024 (median; 100 % within 1.9…2.1) and +2 EV by 4.0064,
    /// and below the 16-bit development's shoulder (0.25 linear) the clipped,
    /// encoded float agrees with it within 0.00043 (median, 0 EV; 100 % within
    /// 0.01). 2 passes too and is not needed.
    ///
    /// Measurement M2 (`scripts/raw-probe/float_tone.py`, the same file,
    /// macOS 26.5.1, decoder 8) applies the decoder's tone curve and
    /// Shadows under amount 1. Tone curve 0.5 and 1 at +2 EV keep 429 and
    /// 453 of 640 lattice points above 1.0 (largest 2.356 and 2.467). Tone
    /// curve 0.5 and 1 against 0, and Shadows 1 against 0 at tone curve 1,
    /// move the clipped, encoded float the way they move the 16-bit
    /// development in 1838 of 1838, 1878 of 1878 and 427 of 427 channels;
    /// below the shoulder the two agree within 0.00012 (95th percentile).
    var extendedDynamicRange: Float {
        switch self {
        case .normalized16: return 0
        case .linearFloat: return 1
        }
    }

    var displayName: String {
        switch self {
        case .normalized16: return "16-bit SDR master"
        case .linearFloat: return "32-bit float (linear)"
        }
    }

    /// The raw values a refusal lists.
    static let acceptedNames = "sdr16 or linear_float"

    /// The Develop window's note at the float encoding (Rulings 1 and 2 of
    /// the batch): what a linear, unbounded development looks like beside
    /// the 16-bit one, and which controls do not apply.
    static let linearFloatNote = "32-bit float (linear): at tone curve 0 linear in exposure with "
        + "no highlight shoulder, so highlights look brighter than the 16-bit development until "
        + "tone-mapped; values above white are kept, and a tone curve keeps them too. Where a "
        + "channel exceeds white the 16-bit development moves the colour toward white, so the "
        + "clipped view differs in hue as well; tone-map it rather than clip it."
}
