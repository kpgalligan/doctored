import Foundation

/// The working CMYK profile (#547 phase 5): the profile an untagged CMYK TIFF or JPEG is
/// converted from as the core opens it, the destination of Image ▸ Mode ▸ CMYK Color, File ▸ New
/// in CMYK, the Color Picker's and the Info panel's CMYK numbers on an RGB document, and the
/// Working CMYK proof. Edit ▸ Color Settings… chooses it (`ColorSettings.workingCMYK`): Automatic
/// is U.S. Web Coated (SWOP) v2, which Adobe installs beside its apps, and
/// without it the system's Generic CMYK; a chosen file is used when it reads and the core accepts
/// it, else Automatic stands in. Installed at launch and again whenever Color Settings changes
/// (`ColorSettings.didChange`), handed to the core (`DoctoredCoreProcess.setWorkingCMYKProfile`);
/// with nothing usable nothing is set, and an untagged CMYK file reads without colour management
/// under a sentence saying so.
enum WorkingCMYK {
    /// The automatic choice's files, in order.
    static let candidates: [URL] = [
        URL(fileURLWithPath:
            "/Library/Application Support/Adobe/Color/Profiles/Recommended/USWebCoatedSWOP.icc"),
        URL(fileURLWithPath: "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"),
    ]

    /// A profile file larger than this is not read: a CMYK press profile is
    /// well under a megabyte, and the core's own document profile cap is the
    /// same 16 MiB.
    private static let maxBytes = 16 * 1024 * 1024

    /// The installed profile's name, for Image ▸ Mode ▸ CMYK Color's popup and
    /// `convert_mode`'s reply (#547); nil when none is set.
    private(set) static var installedName: String?

    /// The installed profile's bytes, verbatim (what a document converted into the working CMYK
    /// embeds); nil when none is set.
    private(set) static var installedData: Data?

    /// The name Automatic resolves to on this machine (Color Settings' "Automatic — <name>").
    static var automaticName: String? {
        for url in candidates {
            if let data = read(url), let name = RasterProfile.describe(data)?.name { return name }
        }
        return nil
    }

    private static var observer: NSObjectProtocol?

    private static func read(_ url: URL) -> Data? {
        guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
              size > 0, size <= maxBytes else { return nil }
        return try? Data(contentsOf: url)
    }

    /// Hands the chosen profile (else the first automatic candidate that reads and that the core
    /// accepts) to the core, returning its URL; nil, with nothing changed, when none qualifies.
    /// The first call also starts reinstalling on every Color Settings change.
    @discardableResult
    static func install() -> URL? {
        if observer == nil {
            observer = NotificationCenter.default.addObserver(
                forName: ColorSettings.didChange, object: nil, queue: .main) { _ in install() }
        }
        var urls = candidates
        if case .file(let path) = ColorSettings.workingCMYK {
            urls.insert(URL(fileURLWithPath: (path as NSString).expandingTildeInPath), at: 0)
        }
        for url in urls {
            guard let data = read(url) else { continue }
            if (try? DoctoredCoreProcess.setWorkingCMYKProfile(data)) != nil {
                installedName = RasterProfile.describe(data)?.name
                    ?? url.deletingPathExtension().lastPathComponent
                installedData = data
                return url
            }
        }
        return nil
    }
}
