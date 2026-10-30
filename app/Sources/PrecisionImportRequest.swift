import AppKit
import ImageIO

/// Scoped synchronous NSDocument import choice, like RAW and PSD requests.
/// An explicit choice is parsed before an already-open document is returned.
enum PrecisionImportRequest {
    enum Mode { case ask, preserve, rgba8 }
    static var mode: Mode = .ask

    static func requested(_ value: Any?) throws -> Mode {
        guard let value else { return .preserve }
        guard let text = value as? String else {
            throw DoctoredCoreError(message: "precision must be preserve or rgba8.")
        }
        switch text { case "preserve": return .preserve; case "rgba8": return .rgba8
        default: throw DoctoredCoreError(message: "precision must be preserve or rgba8.") }
    }

    /// Container inspection only. Actual decode/profile validation belongs to
    /// the typed core; a failed preserving decode never falls back to RGBA8.
    static func isHighDepthImage(_ url: URL) -> Bool {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
                  [kCGImageSourceShouldCache: false] as CFDictionary),
              let type = CGImageSourceGetType(source) as String?,
              ["public.png", "public.tiff"].contains(type),
              let info = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let bits = info[kCGImagePropertyDepth] as? NSNumber else { return false }
        return bits.intValue > 8
    }

    /// The three containers that open as a 32-bit float document.
    enum FloatContainer {
        case tiff, openEXR, radiance

        /// What a refusal calls the file.
        var article: String {
            switch self {
            case .tiff: return "a floating-point TIFF"
            case .openEXR: return "an OpenEXR file"
            case .radiance: return "a Radiance HDR file"
            }
        }
    }

    /// Which float container `url` is, or nil. OpenEXR and Radiance RGBE are
    /// float containers by their signature alone, read from the file's
    /// first bytes — the same magic the core routes on, so a mislabelled
    /// extension cannot send one to a platform decode that would clamp it. A
    /// TIFF is one only when `kCGImagePropertyIsFloat` says so: that is the
    /// platform's own answer and the only one asked, because a 32-bit INTEGER
    /// TIFF carries depth 32 too, so depth alone would offer a float open the
    /// core then refuses. The core's seams are the authority — this only
    /// decides which choice to offer.
    static func floatContainer(_ url: URL) -> FloatContainer? {
        if let file = try? FileHandle(forReadingFrom: url) {
            let head = (try? file.read(upToCount: 10)) ?? Data()
            try? file.close()
            if head.starts(with: [0x76, 0x2f, 0x31, 0x01]) { return .openEXR }
            if head.starts(with: Data("#?RADIANCE".utf8)) || head.starts(with: Data("#?RGBE".utf8)) {
                return .radiance
            }
        }
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
                  [kCGImageSourceShouldCache: false] as CFDictionary),
              let type = CGImageSourceGetType(source) as String?, type == "public.tiff",
              let info = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        else { return nil }
        return (info[kCGImagePropertyIsFloat] as? NSNumber)?.boolValue == true ? .tiff : nil
    }

    /// True for a file that opens as a 32-bit float document.
    static func isFloatImage(_ url: URL) -> Bool { floatContainer(url) != nil }

    /// `cmyk` is the caller's one `RasterPrecisionDocument.cmykSource(at:)`
    /// answer (TIFF or JPEG), asked once per open.
    static func resolve(for url: URL, cmyk: Bool) throws -> Mode {
        // Phase 11F: a float TIFF — and an OpenEXR or Radiance
        // file — opens as a 32-bit FLOAT document or not at all. There is no
        // 8-bit reading of scene-linear extended-range samples this build
        // would be willing to call the same picture.
        if let container = floatContainer(url) {
            if mode == .rgba8 {
                throw DoctoredCoreError(
                    message: "“\(url.lastPathComponent)” is \(container.article): it opens as a "
                        + "32-bit float document. Open it and convert explicitly to get 8-bit "
                        + "pixels.")
            }
            if mode == .preserve { return .preserve }
            let alert = NSAlert()
            alert.messageText = "Open “\(url.lastPathComponent)” as a 32-bit float document?"
            alert.informativeText =
                "This file stores linear light with an extended range — values above white and "
                + "below zero are kept exactly. " + FloatDocument.limitation
            alert.addButton(withTitle: "Open as 32-bit float")
            alert.addButton(withTitle: "Cancel")
            guard alert.runModal() == .alertFirstButtonReturn else { throw CocoaError(.userCancelled) }
            return .preserve
        }
        // #1: a CMYK TIFF or JPEG (`dr_path_cmyk_source`) opens as a CMYK
        // document at either depth with NO 16-bit prompt — its choice is not
        // about 256 values but about keeping the file's own ink — and only an
        // explicit rgba8 keeps the conversion to sRGB.
        if cmyk {
            return mode == .rgba8 ? .rgba8 : .preserve
        }
        guard isHighDepthImage(url) else { return mode }
        switch mode {
        case .preserve, .rgba8: return mode
        case .ask:
            let alert = NSAlert()
            alert.messageText = "Open “\(url.lastPathComponent)” in 16-bit mode?"
            alert.informativeText = "Preserve keeps the source's 16-bit samples and profile. The qualified tools are available; "
                + "styles, other blend modes and content-aware tools remain unavailable. Opening in 8-bit reduces each channel "
                + "to 256 values. The source file is unchanged."
            alert.addButton(withTitle: "Preserve 16-bit")
            alert.addButton(withTitle: "Open in 8-bit")
            alert.addButton(withTitle: "Cancel")
            switch alert.runModal() {
            case .alertFirstButtonReturn: return .preserve
            case .alertSecondButtonReturn: return .rgba8
            default: throw CocoaError(.userCancelled)
            }
        }
    }
}
