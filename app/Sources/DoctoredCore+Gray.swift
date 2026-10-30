import AppKit

// The Grayscale document's core wrappers (#1 phase 1a): every `dr_*` call of the Gray area. The
// gate that decides what a Gray document admits is `GrayDocument.swift`; Image ▸ Mode's job is
// `EditorViewController+CmykMode.swift`'s (`CmykModeConversion`'s gray arms).

/// `DR_GRAY_SOURCE_*`: which container a file opens as a Grayscale document from.
enum GraySource: Int32 {
    case none = 0, png = 1, tiff = 2, jpeg = 3
}

extension RasterPrecisionDocument {
    /// Which gray container `url` is, header work only (`dr_path_gray_source`).
    static func graySource(at url: URL) -> GraySource {
        GraySource(rawValue: dr_path_gray_source(url.path)) ?? .none
    }

    private static func withGrayProfile<T>(_ profile: Data?,
                                           _ body: (UnsafePointer<UInt8>?, Int) throws -> T) rethrows -> T {
        guard let profile, !profile.isEmpty else { return try body(nil, 0) }
        return try profile.withUnsafeBytes { raw in
            try body(raw.bindMemory(to: UInt8.self).baseAddress, profile.count)
        }
    }

    private static func grayJSON(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return "{}" }
        return text
    }

    /// A gray PNG, TIFF or JPEG as a Grayscale document; `workingGray` is the working Gray's wire
    /// name, assigned to a file with no profile (`dr_precision_doc_open_gray`).
    static func openGray(url: URL, workingGray: String, workingLimit: UInt64, otherRetained: UInt64,
                         control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_open_gray(url.path, workingGray, workingLimit, otherRetained,
                                                      control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not open the gray file."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// File ▸ New ▸ Grayscale: request `{"depth","space"?,"fill_srgb","layer_name","ppi"}`;
    /// `profile` (a Gray ICC) wins over `space` (`dr_precision_doc_new_gray`).
    static func newGray(width: Int, height: Int, profile: Data?, request: [String: Any],
                        workingLimit: UInt64, otherRetained: UInt64) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = grayJSON(request)
        let result = withGrayProfile(profile) { bytes, count in
            dr_precision_doc_new_gray(UInt32(clamping: width), UInt32(clamping: height), bytes, count,
                                      text, workingLimit, otherRetained, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not create the Grayscale document."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ Grayscale's plan: `working_memory_plan_bytes`, `refusal`,
    /// `flatten_required`, `flattens`. Options `{"space"?,"flatten"?,"depth"?}`.
    func convertToGrayPlan(profile: Data?, options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.grayJSON(options)
        let result = Self.withGrayProfile(profile) { bytes, count in
            dr_precision_doc_convert_to_gray_plan(ptr, bytes, count, text, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Image ▸ Mode ▸ Grayscale: a new Grayscale document.
    func convertToGray(profile: Data?, options: [String: Any], workingLimit: UInt64,
                       otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.grayJSON(options)
        let result = Self.withGrayProfile(profile) { bytes, count in
            dr_precision_doc_convert_to_gray(ptr, bytes, count, text, workingLimit, otherRetained,
                                             control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to Grayscale."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ RGB Color's plan on a Grayscale document; `profile` the destination RGB.
    func convertGrayToRGBPlan(profile: Data, options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.grayJSON(options)
        let result = Self.withGrayProfile(profile) { bytes, count in
            dr_precision_doc_convert_gray_to_rgb_plan(ptr, bytes, count, text, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Image ▸ Mode ▸ RGB Color on a Grayscale document: Convert to Profile from its twin.
    func convertGrayToRGB(profile: Data, options: [String: Any], workingLimit: UInt64,
                          otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.grayJSON(options)
        let result = Self.withGrayProfile(profile) { bytes, count in
            dr_precision_doc_convert_gray_to_rgb(ptr, bytes, count, text, workingLimit, otherRetained,
                                                 control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to RGB."))
        }
        return RasterPrecisionDocument(owning: result)
    }
}

// #1 phase 5a: Assign / Convert to Profile and 8/16/32 Bits/Channel on a Grayscale document
// (`GrayProfileEdit.swift`).
extension RasterPrecisionDocument {
    /// The Gray profile edit's plan: `working_memory_plan_bytes`, `refusal`, `changes`. Request
    /// `{"op","space"?,"intent"?,"black_point_compensation"?,"depth"?}`; `profile` (a Gray ICC)
    /// wins over `space` (`dr_precision_doc_gray_profile_plan`).
    func grayProfilePlan(profile: Data?, request: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.grayJSON(request)
        let result = Self.withGrayProfile(profile) { bytes, count in
            dr_precision_doc_gray_profile_plan(ptr, bytes, count, text, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Assign, Convert or Depth on a Grayscale document; nil when it changes nothing
    /// (`dr_precision_doc_gray_profile_edit`).
    func grayProfileEdit(profile: Data?, request: [String: Any], workingLimit: UInt64,
                         otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.grayJSON(request)
        let result = Self.withGrayProfile(profile) { bytes, count in
            dr_precision_doc_gray_profile_edit(ptr, bytes, count, text, workingLimit, otherRetained,
                                               control?.ptr, &error)
        }
        guard let result else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The Gray profile edit was refused."))
            }
            return nil
        }
        return RasterPrecisionDocument(owning: result)
    }
}

// #1 phase 6a-i: a type, shape or fill layer's colours as their gray (`GrayArtwork.swift`).
extension RasterPrecisionDocument {
    /// A description's colours as their gray (`dr_precision_doc_gray_artwork_colors`): request
    /// `{"srgb"?,"linear"?,"pattern"?}` (each list at most 1024 `[r, g, b]`), answered under the
    /// same keys. Throws for a document that is not Grayscale.
    func grayArtworkColors(_ request: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.grayJSON(request)
        return try precisionJSON(dr_precision_doc_gray_artwork_colors(ptr, text, &error), error)
    }
}

// #1 phase 4: the Gray profile and the Grayscale clipboard's pasteboard TIFF.
extension RasterPrecisionDocument {
    /// A Grayscale document's Gray ICC bytes, verbatim (`dr_precision_doc_gray_profile`); empty for
    /// every other document.
    var grayProfileData: Data {
        var count = 0
        guard let pointer = dr_precision_doc_gray_profile(ptr, &count), count > 0 else { return Data() }
        return withExtendedLifetime(self) { Data(bytes: pointer, count: count) }
    }
}

extension RasterClipboardPayload {
    /// A Gray payload's one-channel TIFF for the pasteboard (`dr_clipboard_payload_gray_tiff`): its
    /// gray, plus alpha only when some pixel is not opaque, at 8 or 16 bits, the Gray ICC embedded.
    /// Throws for a payload that is not Gray.
    func grayTIFF(workingLimit: UInt64, otherRetained: UInt64) throws -> Data {
        var count = 0
        var error: UnsafeMutablePointer<CChar>?
        guard let pointer = dr_clipboard_payload_gray_tiff(ptr, workingLimit, otherRetained, &count,
                                                           &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The Grayscale clipboard image could not be written."))
        }
        return withExtendedLifetime(self) { Data(bytes: pointer, count: count) }
    }

    /// A gray TIFF another app put on the pasteboard as a Gray payload
    /// (`dr_clipboard_payload_from_gray_image`): its gray (and alpha) at its depth at (0, 0), its
    /// own Gray profile when it embeds one, else `fallbackProfile` (else the working Gray
    /// `workingGray`) assigned — `assigned` is then the untagged sentence. nil when the bytes are
    /// not a gray TIFF; an embedded profile the core cannot use throws, as a gray open does.
    static func fromGrayImage(_ tiff: Data, fallbackProfile: Data?, workingGray: String,
                              workingLimit: UInt64, otherRetained: UInt64) throws
        -> (payload: RasterClipboardPayload, assigned: String?)? {
        var error: UnsafeMutablePointer<CChar>?
        var assigned: UnsafeMutablePointer<CChar>?
        let fallback = fallbackProfile ?? Data()
        let result: OpaquePointer? = tiff.withUnsafeBytes { raw in
            fallback.withUnsafeBytes { icc in
                dr_clipboard_payload_from_gray_image(
                    raw.bindMemory(to: UInt8.self).baseAddress, raw.count,
                    fallbackProfile == nil ? nil : icc.bindMemory(to: UInt8.self).baseAddress,
                    fallbackProfile == nil ? 0 : icc.count, workingGray, workingLimit, otherRetained,
                    &assigned, &error)
            }
        }
        let sentence = assigned.map { pointer -> String in
            defer { dr_string_free(pointer) }
            return String(cString: pointer)
        }
        guard let result else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(
                    error, fallback: "The clipboard's gray image could not be read."))
            }
            return nil
        }
        return (RasterClipboardPayload(owning: result), sentence)
    }
}
