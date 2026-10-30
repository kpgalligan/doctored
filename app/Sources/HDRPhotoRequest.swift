import Foundation

// The open choice for an HDR photo (v1.x): a gain-map HEIC/AVIF or a PQ/HLG still opens as a
// 32-bit float document (`HDRStillOpen.swift`) unless the SDR rendition is asked for. The choice is
// an app-wide preference (File ▸ Open HDR Photos as HDR, default on), a scoped per-open override
// (`mode`, which MCP `open_document hdr_photo` and Revert set), and nothing persisted in the
// document.

/// Which rendition of an HDR photo an open makes.
enum HDRPhotoRendition: String {
    /// A 32-bit float document at full headroom.
    case hdr
    /// A gain-map photo's SDR base, as before v1.x; a PQ/HLG still has none.
    case sdr
}

/// Scoped synchronous open choice, like `PrecisionImportRequest` and
/// `RawImportRequest`: a caller sets `mode` around one open and restores it
/// with `defer`. nil defers to the preference.
enum HDRPhotoRequest {
    static var mode: HDRPhotoRendition?

    /// The UserDefaults key of File ▸ Open HDR Photos as HDR.
    static let preferenceKey = "OpenHDRPhotosAsHDR"

    /// The app-wide choice; HDR until the user turns it off.
    static var preference: HDRPhotoRendition {
        get {
            UserDefaults.standard.object(forKey: preferenceKey) == nil
                || UserDefaults.standard.bool(forKey: preferenceKey) ? .hdr : .sdr
        }
        set { UserDefaults.standard.set(newValue == .hdr, forKey: preferenceKey) }
    }

    /// What this open uses: the scoped mode, else the preference.
    static var effective: HDRPhotoRendition { mode ?? preference }

    /// MCP `hdr_photo`: nil when absent (the preference applies).
    static func requested(_ value: Any?) throws -> HDRPhotoRendition? {
        guard let value else { return nil }
        guard let text = value as? String, let rendition = HDRPhotoRendition(rawValue: text) else {
            throw DoctoredCoreError(message: "hdr_photo must be hdr or sdr.")
        }
        return rendition
    }

    /// A PQ or HLG still asked for as SDR: it HAS no SDR rendition — its
    /// codes are absolute light — so the open refuses rather than inventing
    /// a tone map.
    static func noSDRRendition(_ kind: HDRStillKind) -> String {
        let name = kind == .pq ? "PQ" : "HLG"
        return "This image is an HDR photo coded in \(name), which has no SDR rendition to open. "
            + "Open it as HDR (File ▸ Open HDR Photos as HDR, or hdr_photo \"hdr\") and convert "
            + "it to 16 bits with Image ▸ Mode ▸ Convert Precision…."
    }

    /// The disclosure on a gain-map photo opened as its SDR base.
    static let sdrBaseDisclosure = "This HDR gain-map photo was opened as its SDR base: the gain "
        + "map was not applied. Turn on File ▸ Open HDR Photos as HDR (or pass hdr_photo \"hdr\") "
        + "to open it as a 32-bit float document at full headroom."

    /// `hdr_open`'s `reference_white`: what 1.0 means in the document.
    static func referenceWhite(_ kind: HDRStillKind) -> String {
        kind == .gainMap ? "sdr_base" : "203 cd/m2"
    }

    /// The `hdr_open` record an open reports.
    static func openRecord(_ kind: HDRStillKind, rendition: HDRPhotoRendition) -> [String: Any] {
        ["kind": kind.rawValue, "rendition": rendition.rawValue,
         "reference_white": referenceWhite(kind)]
    }
}
