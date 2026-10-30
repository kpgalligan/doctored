import AppKit

// MARK: - Colour profiles as values

/// One of the four profiles this build writes for itself. Adobe RGB (1998)
/// and ProPhoto RGB are built by the core from the published primaries, with
/// the reference 563/256 and 461/256 gammas (ProPhoto RGB at gamma 1.8, not
/// the ROMM RGB encoding with its linear toe); no system or Adobe profile
/// file is read.
enum RasterBuiltinProfile: Int32 {
    case sRGB = 0
    case displayP3 = 1
    case adobeRGB = 2
    case proPhotoRGB = 3

    var dr: DrBuiltinProfile {
        switch self {
        case .sRGB: return DR_PROFILE_SRGB
        case .displayP3: return DR_PROFILE_DISPLAY_P3
        case .adobeRGB: return DR_PROFILE_ADOBE_RGB
        case .proPhotoRGB: return DR_PROFILE_PROPHOTO_RGB
        }
    }
}

/// The colour model of an image a platform decoder converted to sRGB as it
/// read it — `DR_PLATFORM_SOURCE_*`, for
/// `RasterDocument.platformConversionSentence(encoded:source:)`.
enum RasterPlatformSource: Int32 {
    case gray = 0
    case cmyk = 1
    case lab = 2
}

/// What raw bytes turn out to be. Each case drives different copy and a
/// different set of enabled commands, which is why the core answers five
/// ways rather than "a profile or an error".
enum RasterProfileKind: Int32 {
    /// Not an ICC profile at all — refused, never stored.
    case notICC = 0
    /// An ICC profile whose space is not RGB (Gray, CMYK, Lab) — refused.
    case notRGB = 1
    /// An RGB profile the core cannot convert with (a LUT-based one). It IS
    /// stored: pixels are untouched, display is correct and an export
    /// re-embeds it; only Convert to Profile refuses.
    case rgbUnconvertible = 2
    /// An RGB matrix/TRC profile: full function.
    case rgbMatrix = 3
    /// RGB numbers in a profile whose CLASS is a device link, an abstract
    /// transform or a named-colour list (macOS's own `WebSafeColors.icc`) —
    /// refused. A link or an abstract profile describes a transform and a
    /// named-colour profile is a list of colours; neither is the space a
    /// picture's numbers live in, and CoreGraphics cannot convert OUT of one,
    /// so a document tagged with it drew as an empty image everywhere.
    case notImageProfile = 4
    /// An RGB LUT profile whose A2B/B2A tables this build CAN read (Phase
    /// 11C). Stored like `.rgbUnconvertible`, and `profileIsConvertible`
    /// stays false for it — that bit is the matrix/TRC question the
    /// calibrated adjustments ask — while Convert to Profile and soft
    /// proofing work through it.
    case rgbLut = 5

    /// True for the three the core will store on a document.
    var isStorable: Bool {
        self == .rgbUnconvertible || self == .rgbMatrix || self == .rgbLut
    }

    /// True when the core can convert INTO this profile — the Convert
    /// destination and export gate.
    var canBeDestination: Bool { self == .rgbMatrix || self == .rgbLut }
}

/// What an open did to a document's colour. Describes THE OPEN and is not
/// updated by a later Assign or Convert.
enum RasterAdoptOutcome: Int32 {
    case unchanged = 0
    case converted = 1
    case keptUnconvertible = 2
}

/// What raw profile bytes are, and what to call them.
struct RasterProfileInfo {
    let kind: RasterProfileKind
    /// Empty only for `notICC`, which has no profile to name.
    let name: String
}

/// Which of the profile, the packets and the resolution a save actually
/// wrote — the core's answer, not a guess.
struct RasterSaveReport {
    let carried: UInt32

    func carries(_ bit: UInt32) -> Bool { carried & bit != 0 }
}

/// The profile surface that needs no document: the built-in blobs, the
/// five-way inspection a refusal is explained with, and the per-format
/// capability table.
enum RasterProfile {
    /// The bytes of one of the four profiles this build writes. Identical on
    /// every call and every run.
    static func builtin(_ which: RasterBuiltinProfile) -> Data {
        let len = dr_builtin_profile_len(which.dr)
        guard len > 0 else { return Data() }
        var bytes = [UInt8](repeating: 0, count: len)
        let ok = bytes.withUnsafeMutableBufferPointer { buffer -> Bool in
            guard let base = buffer.baseAddress else { return false }
            return dr_builtin_profile(which.dr, base, len)
        }
        return ok ? Data(bytes) : Data()
    }

    /// Classifies raw bytes and names them. Ask this BEFORE assigning, so a
    /// refusal can say why instead of just beeping.
    static func inspect(_ bytes: Data) -> RasterProfileInfo {
        var name: UnsafeMutablePointer<CChar>? = nil
        let raw = bytes.withUnsafeBytes { buffer in
            dr_icc_inspect(buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &name)
        }
        defer { if let name = name { dr_string_free(name) } }
        return RasterProfileInfo(
            kind: RasterProfileKind(rawValue: raw) ?? .notICC,
            name: name.map { String(cString: $0) } ?? "")
    }

    /// True when two profiles describe the SAME colour space — the question
    /// Convert refuses on, asked so a sheet can disable Apply instead of
    /// letting the user discover the refusal as a beep.
    ///
    /// NOT byte equality, which is a different and much narrower question:
    /// the "sRGB IEC61966-2.1" blob most cameras and Photoshop embed is
    /// 3144 bytes against the built-in's 2568 and describes one space. False
    /// when either side is not an RGB profile or is a LUT-based one —
    /// separate refusals, which `inspect` names.
    static func describesSameSpace(_ a: Data, _ b: Data) -> Bool {
        a.withUnsafeBytes { first in
            b.withUnsafeBytes { second in
                dr_icc_describes_same_space(
                    first.bindMemory(to: UInt8.self).baseAddress, first.count,
                    second.bindMemory(to: UInt8.self).baseAddress, second.count)
            }
        }
    }

    /// Which `DR_CARRIES_*` bits `format` is able to write, so the UI can
    /// disable a checkbox for a reason rather than silently dropping data.
    static func carried(by format: DrFormat) -> UInt32 { dr_format_carries(format) }
}
