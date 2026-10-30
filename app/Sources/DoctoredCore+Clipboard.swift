import AppKit

// MARK: - Typed clipboard payload (Phase 12G-1)

/// Owning wrapper around a `DrClipboardPayload *`: one typed clipboard
/// window plus its RZCB encoding (`precision_clipboard`). Immutable; `deinit`
/// frees the handle.
final class RasterClipboardPayload {
    let ptr: OpaquePointer

    init(owning pointer: OpaquePointer) { ptr = pointer }
    deinit { dr_clipboard_payload_free(ptr) }

    /// Decodes RZCB bytes read back off a pasteboard; refuses a malformed
    /// stream rather than repairing it.
    static func decode(_ data: Data, workingLimit: UInt64,
                       otherRetained: UInt64) throws -> RasterClipboardPayload {
        var error: UnsafeMutablePointer<CChar>?
        let result = data.withUnsafeBytes { raw in
            dr_clipboard_payload_decode(raw.bindMemory(to: UInt8.self).baseAddress, raw.count,
                                        workingLimit, otherRetained, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The clipboard's typed payload could not be read."))
        }
        return RasterClipboardPayload(owning: result)
    }

    /// The canonical RZCB bytes the core encoded once for this handle.
    var bytes: Data {
        var count = 0
        guard let pointer = dr_clipboard_payload_bytes(ptr, &count), count > 0 else { return Data() }
        return withExtendedLifetime(self) { Data(bytes: pointer, count: count) }
    }

    /// `{version, mode, storage, depth, origin_depth, width, height, offset,
    /// ppi, profile_description, has_coverage, tone_map?}`; a CMYK payload
    /// adds `cmyk_depth` (#1 phase 4c), a Gray payload `gray_depth` and `gray_profile` (#1
    /// phase 4), a Lab payload `lab_depth` (#1 Lab phase 4c, `"mode":"lab"`).
    func info() throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_clipboard_payload_info(ptr, &error)
        return try precisionJSON(result, error)
    }

    /// The payload's pixels as a typed image sharing its samples; nil for a
    /// CMYK payload, whose ink is no RGB image (#1 phase 4c).
    func image() -> RasterPrecisionImage? {
        dr_clipboard_payload_image(ptr).map { RasterPrecisionImage(owning: $0) }
    }

    /// #1 phase 4c: the profile bytes the samples are in
    /// (`dr_clipboard_payload_icc`) — the RGB ICC profile, or a CMYK
    /// payload's policy bytes, verbatim.
    var profileData: Data {
        var count = 0
        guard let pointer = dr_clipboard_payload_icc(ptr, &count), count > 0 else { return Data() }
        return withExtendedLifetime(self) { Data(bytes: pointer, count: count) }
    }

    /// #1 phase 4c: a CMYK TIFF or JPEG another app put on the pasteboard as
    /// a CMYK payload (`dr_clipboard_payload_from_cmyk_image`): its ink, depth
    /// and PPI at (0, 0), its own profile when it embeds one, else
    /// `fallbackProfile` (else the working CMYK) assigned — `assigned` is
    /// then the untagged sentence. nil when the bytes are not a CMYK image.
    static func fromCmykImage(_ data: Data, fallbackProfile: Data?, workingLimit: UInt64,
                              otherRetained: UInt64) throws
        -> (payload: RasterClipboardPayload, assigned: String?)? {
        var error: UnsafeMutablePointer<CChar>?
        var assigned: UnsafeMutablePointer<CChar>?
        let fallback = fallbackProfile ?? Data()
        let result: OpaquePointer? = data.withUnsafeBytes { raw in
            fallback.withUnsafeBytes { icc in
                dr_clipboard_payload_from_cmyk_image(
                    raw.bindMemory(to: UInt8.self).baseAddress, raw.count,
                    fallbackProfile == nil ? nil : icc.bindMemory(to: UInt8.self).baseAddress,
                    fallbackProfile == nil ? 0 : icc.count, workingLimit, otherRetained,
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
                    error, fallback: "The clipboard's CMYK image could not be read."))
            }
            return nil
        }
        return (RasterClipboardPayload(owning: result), sentence)
    }

    /// Phase 12G-2: a BYTE-ORIGIN payload from an 8-bit copy
    /// (`dr_clipboard_payload_from_rgba8`) — u16 `v · 257`, no coverage (the
    /// byte Copy already multiplied alpha), the exact profile bytes, the
    /// canvas offset and the PPI.
    static func fromRGBA8(_ image: RasterImage, profile: Data, offset: (x: Int, y: Int),
                          ppi: (x: Double, y: Double), workingLimit: UInt64,
                          otherRetained: UInt64) throws -> RasterClipboardPayload {
        guard let ox = Int32(exactly: offset.x), let oy = Int32(exactly: offset.y) else {
            throw DoctoredCoreError(message: "The clipboard offset is out of range.")
        }
        let width = image.width, height = image.height
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer? = withExtendedLifetime(image) {
            image.withPlateBytes { pixels in
                profile.withUnsafeBytes { raw in
                    dr_clipboard_payload_from_rgba8(
                        pixels, width * height * 4, UInt32(width), UInt32(height),
                        raw.bindMemory(to: UInt8.self).baseAddress, raw.count, ox, oy,
                        Float(ppi.x), Float(ppi.y), workingLimit, otherRetained, &error)
                }
            } ?? nil
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The 8-bit copy could not become a typed payload."))
        }
        return RasterClipboardPayload(owning: result)
    }

    /// The payload's pixels as 8-bit RGBA (`dr_clipboard_payload_rgba8`) in
    /// `profile` (nil = the payload's own): `.convert` keeps another
    /// profile's appearance, `.preserve` its numbers; a float payload maps
    /// through its own tone map.
    func rgba8(in profile: Data? = nil, profile mode: PasteProfile = .convert,
               workingLimit: UInt64, otherRetained: UInt64) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer? = (profile ?? Data()).withUnsafeBytes { raw in
            dr_clipboard_payload_rgba8(
                ptr, profile == nil ? nil : raw.bindMemory(to: UInt8.self).baseAddress,
                profile == nil ? 0 : raw.count, mode == .preserve, workingLimit, otherRetained,
                nil, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The clipboard payload has no 8-bit form."))
        }
        return RasterImage(owning: result)
    }

    /// Phase 12G-3: a new handle carrying `section` as its editable section
    /// (`dr_clipboard_payload_with_editable`); the core validates the section
    /// and admits the new handle before copying anything.
    func withEditable(_ section: String, workingLimit: UInt64,
                      otherRetained: UInt64) throws -> RasterClipboardPayload {
        try precisionCString(section)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_clipboard_payload_with_editable(
            ptr, section, workingLimit, otherRetained, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The editable record could not be attached to the copy."))
        }
        return RasterClipboardPayload(owning: result)
    }

    /// Phase 12G-3: the payload's editable section, or nil when it carries
    /// none (`dr_clipboard_payload_editable`).
    func editableSection() throws -> String? {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_clipboard_payload_editable(ptr, &error) else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(
                    error, fallback: "The clipboard's editable record could not be read."))
            }
            return nil
        }
        defer { dr_string_free(result) }
        return String(cString: result)
    }

    /// This payload trimmed to its content box (every pixel with alpha
    /// > 0; `dr_clipboard_payload_trim`), its offset moved by the box origin
    /// and every other field carried; nil when there is nothing to trim
    /// (fully transparent, or already tight).
    func trim(workingLimit: UInt64, otherRetained: UInt64) throws -> RasterClipboardPayload? {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_clipboard_payload_trim(ptr, workingLimit, otherRetained,
                                                     &error) else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(
                    error, fallback: "The clipboard payload could not be trimmed."))
            }
            return nil
        }
        return RasterClipboardPayload(owning: result)
    }
}

extension RasterPrecisionDocument {
    /// New from Clipboard at the payload's own depth, profile, size and PPI
    /// (`dr_precision_doc_from_clipboard`).
    static func fromClipboard(_ payload: RasterClipboardPayload, workingLimit: UInt64,
                              otherRetained: UInt64) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_from_clipboard(
            payload.ptr, workingLimit, otherRetained, nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "A document could not be made from the clipboard."))
        }
        return RasterPrecisionDocument(owning: result)
    }
}

extension RasterPrecisionDocument {
    /// Copy / Copy Merged of one canvas window (`dr_precision_doc_clipboard_copy`).
    /// An empty `coverage` means the window is fully covered.
    func clipboardCopy(request: String, coverage: [UInt16], workingLimit: UInt64,
                       otherRetained: UInt64,
                       control: RasterJobControl?) throws -> RasterClipboardPayload {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { input in
            dr_precision_doc_clipboard_copy(ptr, request, input.isEmpty ? nil : input.baseAddress,
                                            input.count, workingLimit, otherRetained,
                                            control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Copy failed."))
        }
        return RasterClipboardPayload(owning: result)
    }

    /// Cut: the payload, and the erased document — nil WITHOUT an error when
    /// the erase changed no byte (a transparency lock).
    func clipboardCut(request: String, coverage: [UInt16], workingLimit: UInt64,
                      otherRetained: UInt64, control: RasterJobControl?) throws
        -> (payload: RasterClipboardPayload, document: RasterPrecisionDocument?) {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        var payload: OpaquePointer?
        let result = coverage.withUnsafeBufferPointer { input in
            dr_precision_doc_clipboard_cut(ptr, request, input.isEmpty ? nil : input.baseAddress,
                                           input.count, workingLimit, otherRetained,
                                           control?.ptr, &payload, &error)
        }
        let document = result.map { RasterPrecisionDocument(owning: $0) }
        if let error {
            if let payload { dr_clipboard_payload_free(payload) }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Cut failed."))
        }
        guard let payload else { throw DoctoredCoreError(message: "Cut produced no payload.") }
        return (RasterClipboardPayload(owning: payload), document)
    }

    /// Paste one payload as a new raster record (`dr_precision_doc_clipboard_paste`).
    func clipboardPasting(_ payload: RasterClipboardPayload, request: String,
                          workingLimit: UInt64, otherRetained: UInt64,
                          control: RasterJobControl?) throws -> RasterPrecisionDocument {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_clipboard_paste(
            ptr, payload.ptr, request, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Paste failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }
}

extension RasterDocument {
    /// The report a layers paste that promoted nothing answers: the three
    /// lists of `dr_precision_doc_paste_layers`' `report_out`, empty.
    static let emptyLayerPasteReport: [String: Any] = [
        "rasterized": [[String: Any]](), "blend_reset": [[String: Any]](),
        "effect_blend_reset": [[String: Any]](),
    ]

    /// Every record of the layer clipboard `layers` spliced in at the
    /// slot `position` names relative to `anchor`, translated by `offset`
    /// first (the Move tool's rule) — ONE core call, so one undo step
    /// (`dr_doc_paste_layers` / `dr_precision_doc_paste_layers`). Both
    /// documents must be byte, or both typed. An 8- or 16-bit document
    /// refuses another depth by name, and another profile under `.convert`;
    /// under `.preserve` it relabels that clip after the
    /// Assign check, keeping every record's numbers and RAW layers bound.
    /// A 32-bit float document takes a typed clip of either
    /// depth and any profile and promotes it under `profile`. `report` holds `rasterized`,
    /// `blend_reset` and `effect_blend_reset` — what that promotion changed,
    /// empty arrays at 8 bits and when nothing was promoted.
    func pastingLayers(_ layers: RasterDocument, anchor: Int, position: LayerInsertPosition,
                       offset: (x: Int, y: Int), profile: PasteProfile = .convert) throws
        -> (document: RasterDocument, report: [String: Any]) {
        guard let dx = Int32(exactly: offset.x), let dy = Int32(exactly: offset.y) else {
            throw DoctoredCoreError(message: "The pasted layers' offset is out of range.")
        }
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            guard let clip = layers.precision else {
                throw DoctoredCoreError(message: "Pasted layers must be at the document's depth; "
                    + "convert them first.")
            }
            let budget = try PrecisionWorkContext(source: precision)
            var reportOut: UnsafeMutablePointer<CChar>?
            guard let result = dr_precision_doc_paste_layers(
                precision.ptr, clip.ptr, anchor, position.code, dx, dy, profile == .preserve,
                budget.limit, budget.otherRetained, &reportOut, &error) else {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Paste failed."))
            }
            var report = Self.emptyLayerPasteReport
            if let reportOut {
                let data = Data(String(cString: reportOut).utf8)
                dr_string_free(reportOut)
                if let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                    for key in report.keys { report[key] = parsed[key] ?? report[key] }
                }
            }
            return (try RasterDocument(precision: RasterPrecisionDocument(owning: result)), report)
        }
        guard let ptr, let clip = layers.ptr else {
            throw DoctoredCoreError(message: "Pasted layers must be at the document's depth; "
                + "convert them first.")
        }
        guard let result = dr_doc_paste_layers(ptr, clip, anchor, position.code, dx, dy,
                                               profile == .preserve, &error)
        else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Paste failed."))
        }
        return (RasterDocument(owning: result), Self.emptyLayerPasteReport)
    }
}
