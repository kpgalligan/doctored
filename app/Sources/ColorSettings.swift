import AppKit

/// The app-wide COLOUR WORKING SPACE: the profile a newly opened flat file
/// is converted to.
///
/// It is a preference, not document state — a document's own profile travels
/// in the file and in the `.rz`, and Assign/Convert Profile change that
/// document alone. Changing the working space affects FUTURE opens only.
///
/// One consequence is worth stating plainly, because it is the price of
/// having exactly one rule: a file with NO profile is assumed sRGB (that is
/// what every reader does), and once its numbers are sRGB numbers, honouring
/// the working space means converting them. So under Display P3 an untagged
/// file is converted on open, and opening one and immediately saving it back
/// does not reproduce the input bytes. Under the default sRGB working space
/// that branch is a no-op and never fires.
enum WorkingSpace: String, CaseIterable {
    case sRGB = "srgb"
    case displayP3 = "displayP3"

    var displayName: String {
        switch self {
        case .sRGB: return "sRGB"
        case .displayP3: return "Display P3"
        }
    }

    var builtin: RasterBuiltinProfile {
        switch self {
        case .sRGB: return .sRGB
        case .displayP3: return .displayP3
        }
    }

    /// The ICC bytes this build writes for the space — the same blob an
    /// export embeds, so "the working space" and "what lands in the file"
    /// are the same object.
    var profileData: Data { RasterProfile.builtin(builtin) }

    /// The space to DECODE into when the platform is doing the decoding and
    /// there is no source profile to preserve (a RAW development).
    var cgSpace: CGColorSpace { ColorProfile.space(for: profileData) }
}

/// Where the working space is stored and read. A `ColorSettingsStore` key
/// (`UserDefaults` unless `DR_COLOR_SETTINGS_DIR` names a private store), not
/// `ToolOptionsStore`: this is one app-wide enum, not a per-tool value, and it
/// is read on the open path where no tool is involved.
enum ColorSettings {
    static let workingSpaceKey = "ColorWorkingSpace"

    /// The current working space; `.sRGB` when unset or when the stored
    /// string is not one this build knows (a downgrade must not brick the
    /// open path).
    static var workingSpace: WorkingSpace {
        get {
            let raw = ColorSettingsStore.string(forKey: workingSpaceKey) ?? ""
            return WorkingSpace(rawValue: raw) ?? .sRGB
        }
        set { ColorSettingsStore.set(newValue.rawValue, forKey: workingSpaceKey) }
    }

    static let askWhenPastingKey = "ColorAskWhenPasting"

    /// Color Settings ▸ Profile Mismatches ▸ "Ask When
    /// Pasting". Off (unset → false, the default) every Paste
    /// converts into the destination's profile silently; on, a Paste whose
    /// pixels or layers are in another profile asks Convert / Don't Convert /
    /// Cancel first (`EditorViewController.askPasteProfile`). App-wide, like
    /// the working space: no MCP setter — an MCP `paste` passes the choice as
    /// its `profile` argument.
    static var askWhenPasting: Bool {
        get { ColorSettingsStore.bool(forKey: askWhenPastingKey) ?? false }
        set { ColorSettingsStore.set(newValue, forKey: askWhenPastingKey) }
    }

    // MARK: - Edit ▸ Color Settings (#547 phase 5)

    static let workingCMYKKey = "ColorWorkingCMYK"
    static let rgbPolicyKey = "ColorRGBPolicy"
    static let cmykPolicyKey = "ColorCMYKPolicy"
    static let intentKey = "ColorIntent"
    static let blackPointCompensationKey = "ColorBPC"

    /// Posted after Color Settings… stores a change; `WorkingCMYK` reinstalls the working CMYK
    /// profile into the core on it.
    static let didChange = Notification.Name("DoctoredColorSettingsDidChange")

    /// The working CMYK: `automatic` (U.S. Web Coated (SWOP) v2 when installed, else Generic
    /// CMYK — the default, and today's behaviour) or a CMYK profile file. Stored as "automatic"
    /// or the file's path.
    static var workingCMYK: WorkingCMYKChoice {
        get {
            let raw = ColorSettingsStore.string(forKey: workingCMYKKey) ?? ""
            return raw.isEmpty || raw == "automatic" ? .automatic : .file(path: raw)
        }
        set {
            switch newValue {
            case .automatic: ColorSettingsStore.set("automatic", forKey: workingCMYKKey)
            case .file(let path): ColorSettingsStore.set(path, forKey: workingCMYKKey)
            }
        }
    }

    /// What an open does with an RGB file's profile; the default is Convert to Working.
    static var rgbPolicy: ColorPolicy {
        get { ColorPolicy(rawValue: ColorSettingsStore.string(forKey: rgbPolicyKey) ?? "") ?? .convert }
        set { ColorSettingsStore.set(newValue.rawValue, forKey: rgbPolicyKey) }
    }

    /// What an open does with a CMYK file's profile; the default is Preserve Embedded
    /// Profiles.
    static var cmykPolicy: ColorPolicy {
        get { ColorPolicy(rawValue: ColorSettingsStore.string(forKey: cmykPolicyKey) ?? "") ?? .preserve }
        set { ColorSettingsStore.set(newValue.rawValue, forKey: cmykPolicyKey) }
    }

    /// The Conversion Options' intent: every conversion's default (Mode, Convert to Profile,
    /// the picker, exports, proofs). The default is Relative Colorimetric.
    static var intent: RenderingIntent {
        get { RenderingIntent(rawValue: ColorSettingsStore.string(forKey: intentKey) ?? "") ?? .relative }
        set { ColorSettingsStore.set(newValue.rawValue, forKey: intentKey) }
    }

    /// The Conversion Options' Use Black Point Compensation; on by default.
    static var blackPointCompensation: Bool {
        // `ColorSettingsStore.bool` reads an argv string such as `-ColorBPC NO` too, which
        // `as? Bool` would take for absent.
        get { ColorSettingsStore.bool(forKey: blackPointCompensationKey) ?? true }
        set { ColorSettingsStore.set(newValue, forKey: blackPointCompensationKey) }
    }

    static let workingGrayKey = "ColorWorkingGray"

    /// The working Gray: the curve Load Selection's luminosity takes at 8 and 16 bits on an RGB
    /// document (`core/src/gray_working.rs`). Dot Gain 20 % when unset or unknown, the
    /// default. Stored as the core name.
    static var workingGray: GrayWorkingSpace {
        get {
            let stored = ColorSettingsStore.string(forKey: workingGrayKey) ?? ""
            return GrayWorkingSpace(rawValue: stored) ?? .dotGain20
        }
        set { ColorSettingsStore.set(newValue.rawValue, forKey: workingGrayKey) }
    }
}

/// Color Settings' eight Gray working spaces (`ColorSettings.workingGray`); the raw
/// value is the core's name (`dr_doc_composite_gray_plane`, the native `luminance` kind's `gray`).
enum GrayWorkingSpace: String, CaseIterable {
    case dotGain10 = "dot_gain_10", dotGain15 = "dot_gain_15", dotGain20 = "dot_gain_20",
         dotGain25 = "dot_gain_25", dotGain30 = "dot_gain_30",
         grayGamma18 = "gray_gamma_18", grayGamma22 = "gray_gamma_22", sGray = "sgray"

    /// The name the menus show.
    var displayName: String {
        switch self {
        case .dotGain10: return "Dot Gain 10%"
        case .dotGain15: return "Dot Gain 15%"
        case .dotGain20: return "Dot Gain 20%"
        case .dotGain25: return "Dot Gain 25%"
        case .dotGain30: return "Dot Gain 30%"
        case .grayGamma18: return "Gray Gamma 1.8"
        case .grayGamma22: return "Gray Gamma 2.2"
        case .sGray: return "sGray"
        }
    }
}

/// The Color Management Policies for one colour model: Off (assign the working space,
/// discarding the file's profile), Preserve Embedded Profiles, Convert to Working.
enum ColorPolicy: String, CaseIterable {
    case off, preserve, convert

    var displayName: String {
        switch self {
        case .off: return "Off"
        case .preserve: return "Preserve Embedded Profiles"
        case .convert: return "Convert to Working"
        }
    }
}

/// The working CMYK choice (`ColorSettings.workingCMYK`).
enum WorkingCMYKChoice: Equatable {
    /// U.S. Web Coated (SWOP) v2 when installed, else macOS's Generic CMYK Profile.
    case automatic
    /// A CMYK profile file the user chose.
    case file(path: String)
}
