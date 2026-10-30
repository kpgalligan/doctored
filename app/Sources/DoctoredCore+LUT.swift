import AppKit

// MARK: - Cube LUTs

/// The `.cube` lookup-table parser — a free function, because parsing a file
/// owns no handle: it produces the params object a `color_lookup` adjustment
/// stores, which the caller then hands to `add_adjustment_layer` or to
/// `applyingAdjustment`.
enum RasterLUT {
    /// The params object parsed from the Adobe Cube LUT at `path`, or the
    /// core's message (`DoctoredCoreError.message`, since Swift's `Result`
    /// needs a failure type that conforms to `Error` and this file already
    /// has exactly one). A table larger than the core stores is resampled
    /// down and `source_size` reports the size the FILE declared, so a host
    /// can say "resampled from 64". The core never panics on a malformed
    /// file — every failure comes back as a message naming the path.
    static func parseCube(path: String) -> Result<[String: Any], DoctoredCoreError> {
        var err: UnsafeMutablePointer<CChar>? = nil
        guard let json = dr_lut_parse_cube(path, &err) else {
            return .failure(DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not read the LUT.")))
        }
        defer { dr_string_free(json) }
        let text = String(cString: json)
        guard let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data),
              let params = object as? [String: Any]
        else {
            return .failure(DoctoredCoreError(
                message: "The LUT parsed but its parameters could not be read."))
        }
        return .success(params)
    }

    /// The params object for the ICC LOOK at `path`: an RGB→RGB
    /// device link, or an abstract profile (Lab or XYZ on either side) baked
    /// through `documentProfile`, relative colorimetric — the same 3D shape
    /// `parseCube` returns. A link whose table is a plain grid of at most 33
    /// nodes keeps its own grid (`size` n); any other link and every
    /// abstract is sampled at 33³, and `source_size` reports the link's own
    /// grid when it is larger. An
    /// abstract's params add `abstract_profile` (its bytes, base64), which
    /// every later Assign, Convert or Don't-Convert paste re-bakes the table
    /// through. nil `documentProfile` means no document profile:
    /// links still load, abstracts refuse. Every other class or space comes
    /// back as the core's message naming the path.
    static func parseProfile(path: String, documentProfile: Data?) -> Result<[String: Any], DoctoredCoreError> {
        var err: UnsafeMutablePointer<CChar>? = nil
        let profile = documentProfile ?? Data()
        let json: UnsafeMutablePointer<CChar>? = profile.withUnsafeBytes { buffer in
            dr_lut_parse_profile(path, buffer.bindMemory(to: UInt8.self).baseAddress,
                                 buffer.count, &err)
        }
        guard let json else {
            return .failure(DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not read the ICC profile.")))
        }
        defer { dr_string_free(json) }
        guard let data = String(cString: json).data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data),
              let params = object as? [String: Any]
        else {
            return .failure(DoctoredCoreError(
                message: "The ICC profile parsed but its parameters could not be read."))
        }
        return .success(params)
    }

    /// `parseProfile`'s params from the look profile's BYTES: the
    /// carried-look re-bake `AdjustmentSchema.validate` runs for Action
    /// replay and agent edits. The core's message names no path.
    static func bakeProfile(_ look: Data, documentProfile: Data?) -> Result<[String: Any], DoctoredCoreError> {
        var err: UnsafeMutablePointer<CChar>? = nil
        let profile = documentProfile ?? Data()
        let json: UnsafeMutablePointer<CChar>? = look.withUnsafeBytes { lookBuffer in
            profile.withUnsafeBytes { buffer in
                dr_lut_bake_profile(
                    lookBuffer.bindMemory(to: UInt8.self).baseAddress, lookBuffer.count,
                    buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &err)
            }
        }
        guard let json else {
            return .failure(DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not bake the ICC look.")))
        }
        defer { dr_string_free(json) }
        guard let data = String(cString: json).data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data),
              let params = object as? [String: Any]
        else {
            return .failure(DoctoredCoreError(
                message: "The ICC look baked but its parameters could not be read."))
        }
        return .success(params)
    }

    /// The one entry a Color Lookup file goes through: `.icc`/`.icm` (any
    /// case) to `parseProfile`, everything else to `parseCube`.
    static func parse(path: String, documentProfile: Data?) -> Result<[String: Any], DoctoredCoreError> {
        let ext = (path as NSString).pathExtension.lowercased()
        return ext == "icc" || ext == "icm"
            ? parseProfile(path: path, documentProfile: documentProfile)
            : parseCube(path: path)
    }
}
