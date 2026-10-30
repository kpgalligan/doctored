import Foundation

/// Lensfun lens profiles (v1-lensfun): the only `dr_lens_*` calls. The core
/// reads the lens identity out of an EXIF packet and answers the bundled
/// Lensfun database's match (`LensProfile.swift` is the Swift model over both).
enum RasterLensCore {
    /// `dr_lens_exif_identity`: the identity JSON an EXIF packet (a TIFF
    /// stream, no `Exif\0\0` prefix) names, every field null when unreadable;
    /// nil for an empty packet.
    static func exifIdentity(_ exif: Data) -> [String: Any]? {
        guard !exif.isEmpty else { return nil }
        let raw = exif.withUnsafeBytes { bytes in
            dr_lens_exif_identity(bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
        }
        guard let raw else { return nil }
        defer { dr_string_free(raw) }
        return (try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))) as? [String: Any]
    }

    /// `dr_lens_match`: the database's answer to `request`. Throws the core's
    /// refusal (an unknown lens id, a malformed request); no match is an
    /// answer with a reason, not an error.
    static func match(_ request: [String: Any]) throws -> [String: Any] {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: request), as: UTF8.self)
        var error: UnsafeMutablePointer<CChar>?
        guard let raw = dr_lens_match(json, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not match a lens profile."))
        }
        defer { dr_string_free(raw) }
        guard let answer = try JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))
                as? [String: Any] else {
            throw DoctoredCoreError(message: "The lens database answered no object.")
        }
        return answer
    }
}
