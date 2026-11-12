import AppKit

extension RasterDocument {
    // MARK: - Colour management and metadata

    /// A variable-length blob read STRAIGHT into a Data: ask the core its
    /// length, allocate exactly that, fill it. The `len` the core takes back
    /// is a capacity check, never the authority — the ONE shape every blob
    /// getter below uses.
    private static func readBlob(
        _ length: () -> Int, _ fill: (UnsafeMutablePointer<UInt8>, Int) -> Bool
    ) -> Data? {
        let len = length()
        guard len > 0 else { return nil }
        var bytes = [UInt8](repeating: 0, count: len)
        let ok = bytes.withUnsafeMutableBufferPointer { buffer -> Bool in
            guard let base = buffer.baseAddress else { return false }
            return fill(base, len)
        }
        return ok ? Data(bytes) : nil
    }

    /// The document's ICC profile bytes — what a CGColorSpace is built from
    /// and what an export embeds. A document ALWAYS has a profile, so nil
    /// means only that the read failed.
    ///
    /// Read once and kept: a handle's pixels and profile never change (every
    /// core op returns a NEW handle), while `colorSpace` is asked on every
    /// redraw and once per layer and channel row, and each ask was copying
    /// the whole profile out of the core and hashing it. Cheap for the usual
    /// half-kilobyte profile, but a document can legitimately carry
    /// megabytes of one. Safe unlocked on the same terms as `flattened()` —
    /// one queue at a time touches a handle.
    var iccProfile: Data? {
        if let precisionMetadata { return precisionMetadata.profile }
        RasterDocument.iccLock.lock()
        defer { RasterDocument.iccLock.unlock() }
        if let cached = cachedICCProfile { return cached }
        let read = RasterDocument.readBlob({ dr_doc_icc_profile_len(ptr) }) { base, len in
            dr_doc_icc_profile(ptr, base, len)
        }
        cachedICCProfile = .some(read)
        return read
    }

    /// The profile's display name, for the status bar and the sheets.
    var profileName: String {
        if let precisionMetadata { return precisionMetadata.raw["profile"] as? String ?? "" }
        guard let name = dr_doc_profile_name(ptr) else { return "" }
        defer { dr_string_free(name) }
        return String(cString: name)
    }

    /// True when the profile is an RGB matrix/TRC model — the calibrated
    /// adjustments' precondition. False for a LUT-based profile, which is kept
    /// and re-embedded all the same. At 16 and 32 bits the typed document
    /// answers, adding the high-depth precision checks.
    var profileIsConvertible: Bool { precision?.profileIsConvertible ?? dr_doc_profile_is_convertible(ptr) }

    /// Why a NEW or EDITED `document_v2` `op` ("exposure" | "white_balance";
    /// create, edit, destructive apply, editable paste) cannot evaluate in
    /// this document's profile: the core's sentence at whichever depth it
    /// stores, nil when admitted. A profile failing the precision check on a
    /// falling boundary admits Exposure and refuses White Balance (#2). An
    /// existing 8-bit layer keeps rendering.
    func calibratedRefusal(op: String) -> String? {
        if let precision { return precision.calibratedRefusal(op: op) }
        guard let raw = dr_doc_calibrated_refusal(ptr, op) else { return nil }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// The CIE L*a*b* of one straight RGB triple of THIS document's pixels,
    /// read through the document's own profile against the D50 PCS white.
    /// The same bytes therefore read differently in an sRGB and a Display P3
    /// document, which is the point — a sampled colour is already in the
    /// document's space and converts nowhere (ColorProfile.swift's rule).
    /// nil for a profile the core cannot model (a LUT profile), where the
    /// readout must say so rather than assume sRGB.
    func lab(r: UInt8, g: UInt8, b: UInt8) -> (l: Double, a: Double, b: Double)? {
        var out = [Float](repeating: 0, count: 3)
        let ok = out.withUnsafeMutableBufferPointer { lab in
            dr_doc_lab(ptr, r, g, b, lab.baseAddress)
        }
        return ok ? (Double(out[0]), Double(out[1]), Double(out[2])) : nil
    }

    /// The print resolution in pixels per inch, per axis. Pixels never
    /// change with it — only the print size does.
    var resolution: (x: Double, y: Double) {
        if let precisionMetadata { return precisionMetadata.resolution }
        return (Double(dr_doc_resolution_x(ptr)), Double(dr_doc_resolution_y(ptr)))
    }

    /// One preserved metadata packet, verbatim, or nil when the document
    /// carries none of that kind.
    func metadata(_ kind: DrMetadataKind) -> Data? {
        if let precision { return precision.metadata(kind) }
        return RasterDocument.readBlob({ dr_doc_metadata_len(ptr, kind) }) { base, len in
            dr_doc_metadata(ptr, kind, base, len)
        }
    }

    /// REINTERPRETS the document in `bytes`: pixels unchanged, profile
    /// replaced. nil only for the profile the document already carries
    /// (nothing would change). Throws the core's sentence for bytes that are
    /// not an RGB ICC profile, a payload over 16 MiB, a retained source
    /// (`assignRefusal`) and whenever `assignProfileRefusal` is non-nil.
    func assigningProfile(_ bytes: Data) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionAdjustment(.assignProfile(bytes)) }
        var err: UnsafeMutablePointer<CChar>?
        let handle = bytes.withUnsafeBytes { raw in
            dr_doc_assign_profile(ptr, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &err)
        }
        return try fallible(handle, err, "Could not assign the profile.")
    }

    /// TRANSFORMS every layer's pixels into `bytes` and replaces the
    /// profile, so the picture looks the same and its numbers change. nil
    /// only when the two describe the same space. Throws the core's sentence
    /// for bytes that are not an RGB ICC profile, when the default-options
    /// preflight refuses (among its refusals a `document_v2` Exposure/White
    /// Balance layer or linear-light evaluation carried into a profile with
    /// no matrix/TRC model) and when the source cannot
    /// decode or the destination cannot encode.
    func convertingToProfile(_ bytes: Data) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionAdjustment(.convertProfile(bytes)) }
        var err: UnsafeMutablePointer<CChar>?
        let handle = bytes.withUnsafeBytes { raw in
            dr_doc_convert_to_profile(
                ptr, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &err)
        }
        return try fallible(handle, err, "Could not convert to the profile.")
    }

    /// Brings a freshly opened document into the working space, ONCE. The
    /// outcome is reported even when no document comes back, so a host can
    /// tell "already in the working space" from "kept, could not be
    /// converted from". Skip this for a `.rz`: it carries its own profile.
    func adoptingWorkingSpace(
        _ bytes: Data
    ) -> (document: RasterDocument?, outcome: RasterAdoptOutcome) {
        if isHighDepth { return (nil, .unchanged) }
        var raw: Int32 = 0
        let next = bytes.withUnsafeBytes { buffer in
            wrap(
                dr_doc_adopt_working_space(
                    ptr, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count, &raw))
        }
        return (next, RasterAdoptOutcome(rawValue: raw) ?? .unchanged)
    }

    /// Stores one metadata packet verbatim, or CLEARS it with nil. nil means
    /// no change: the document already carries this value. At either depth a
    /// refusal (a kind outside DrMetadataKind, a payload over 16 MiB) throws
    /// the core's sentence.
    func settingMetadata(_ kind: DrMetadataKind, _ bytes: Data?) throws -> RasterDocument? {
        if let precision {
            return try precision.settingMetadata(kind, bytes).map { try RasterDocument(precision: $0) }
        }
        var err: UnsafeMutablePointer<CChar>? = nil
        guard let bytes = bytes else {
            return try fallible(
                dr_doc_set_metadata(ptr, kind, nil, 0, &err), err, "Could not set metadata.")
        }
        let handle = bytes.withUnsafeBytes { raw in
            dr_doc_set_metadata(
                ptr, kind, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &err)
        }
        return try fallible(handle, err, "Could not set metadata.")
    }

    /// Attaches the platform reader's serialized packets without decoding its
    /// small carrier image or changing this photograph's pixels, ICC or PPI.
    func importingMetadata(from carrier: Data) throws
        -> (document: RasterDocument?, warnings: [String]) {
        if let precision {
            let (document, warnings) = try precision.importingMetadata(from: carrier)
            return (try document.map { try RasterDocument(precision: $0) }, warnings)
        }
        var err: UnsafeMutablePointer<CChar>?
        var warningJSON: UnsafeMutablePointer<CChar>?
        defer { if let warningJSON = warningJSON { dr_string_free(warningJSON) } }
        let result = carrier.withUnsafeBytes { raw in
            wrap(dr_doc_import_metadata(
                ptr, raw.bindMemory(to: UInt8.self).baseAddress, raw.count, &warningJSON, &err))
        }
        if err != nil {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not import metadata."))
        }
        let warnings: [String]
        if let warningJSON = warningJSON {
            warnings = (try? JSONSerialization.jsonObject(
                with: Data(String(cString: warningJSON).utf8))) as? [String] ?? []
        } else { warnings = [] }
        return (result, warnings)
    }

    /// The core scanner's per-container limits and verified packet warnings,
    /// plus `profile_refusal`: the sentence naming an embedded colour profile
    /// the flat open converted from or ignored (also the last warning).
    static func metadataImportReport(at url: URL) -> [String: Any]? {
        var err: UnsafeMutablePointer<CChar>?
        guard let raw = dr_path_metadata_import_report(url.path, &err) else {
            if let err = err { dr_string_free(err) }
            return nil
        }
        defer { dr_string_free(raw) }
        return (try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8)))
            as? [String: Any]
    }

    /// The flat open's `profile_refusal` for the file at `url` — the sentence
    /// naming an embedded profile the core open converted from or ignored —
    /// from the header alone (`metadataImportReport(at:)`); nil for a kept or
    /// absent profile, a file the flat open cannot read, and a CMYK TIFF or
    /// JPEG its gate refuses — the core open fails on those, so
    /// there is no core open to describe.
    static func flatOpenProfileRefusal(at url: URL) -> String? {
        metadataImportReport(at: url)?["profile_refusal"] as? String
    }

    /// The import-report sentence for a Gray, CMYK or Lab image a PLATFORM
    /// decoder converted to sRGB as it read it: the core
    /// open's converted sentence naming the embedded profile when the core
    /// would convert such samples from it itself, else a plain sentence
    /// naming the colour model. `encoded` is the image ImageIO decoded — a
    /// file's contents, a pasteboard's own TIFF or PNG; empty means no
    /// profile. Header work only (`dr_platform_conversion_sentence`).
    static func platformConversionSentence(
        encoded: Data, source: RasterPlatformSource
    ) -> String? {
        var err: UnsafeMutablePointer<CChar>?
        let raw = encoded.withUnsafeBytes { bytes in
            dr_platform_conversion_sentence(
                bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, source.rawValue, &err)
        }
        guard let raw = raw else {
            if let err = err { dr_string_free(err) }
            return nil
        }
        defer { dr_string_free(raw) }
        return String(cString: raw)
    }

    /// Sets the print resolution. nil on a non-finite component, or when
    /// nothing changes after sanitizing (clamped to [1, 30000] and quantized
    /// to four decimals like the global light, so a reported value echoed
    /// back registers no edit). At either depth a non-positive component
    /// throws the core's sentence.
    func settingResolution(x: Double, y: Double) throws -> RasterDocument? {
        guard x.isFinite, y.isFinite else { return nil }
        if let precision {
            return try precision.settingResolution(x: Float(x), y: Float(y))
                .map { try RasterDocument(precision: $0) }
        }
        var err: UnsafeMutablePointer<CChar>? = nil
        return try fallible(
            dr_doc_set_resolution(ptr, Float(x), Float(y), &err), err,
            "Could not set the print resolution.")
    }

    /// Writes the document as a flat image with its colour profile and its
    /// metadata packets, format permitting, and reports what was actually
    /// carried.
    ///
    /// `flattened` is THIS document's warm composite (the host's
    /// `projection`); nil makes the core flatten, which re-composites the
    /// whole layer stack. Passing an unrelated image is a caller bug — the
    /// canvas dimensions are not re-checked.
    func saveImage(
        _ flattened: RasterImage?, to url: URL, format: DrFormat, jpegQuality: Int,
        embedProfile: Bool, stripMetadata: Bool
    ) throws -> RasterSaveReport {
        var err: UnsafeMutablePointer<CChar>? = nil
        var carried: UInt32 = 0
        let quality = UInt8(min(max(jpegQuality, 1), 100))
        guard
            dr_doc_save_image(
                ptr, flattened?.ptr, url.path, format, quality, embedProfile, stripMetadata,
                &carried, &err)
        else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not save \(url.lastPathComponent)."))
        }
        return RasterSaveReport(carried: carried)
    }
}

extension RasterDocument {
    /// The core's document-versus-format check, including packet refusals.
    func flatSaveLosses(
        format: DrFormat, embedProfile: Bool, stripMetadata: Bool
    ) throws -> [String] {
        var err: UnsafeMutablePointer<CChar>?
        guard let value = dr_doc_flat_save_losses(ptr, format, embedProfile, stripMetadata, &err)
        else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not check save preservation."))
        }
        defer { dr_string_free(value) }
        let data = Data(String(cString: value).utf8)
        return try JSONDecoder().decode([String].self, from: data)
    }
}

extension RasterPrecisionDocument {
    /// The editable state a flattened file of this typed document has no place for, in the
    /// byte list's words (`dr_precision_doc_flat_state_losses`); `alphaKept` says whether the
    /// file carries an alpha sample. The mode's own lines (profile, metadata, depth, spot
    /// channels) are its `saveLosses`'.
    func flatStateLosses(alphaKept: Bool) throws -> [String] {
        var err: UnsafeMutablePointer<CChar>?
        guard let value = dr_precision_doc_flat_state_losses(ptr, alphaKept, &err) else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not check save preservation."))
        }
        defer { dr_string_free(value) }
        return try JSONDecoder().decode([String].self, from: Data(String(cString: value).utf8))
    }
}

extension DoctoredCoreProcess {
    /// Sets the process-wide working CMYK profile an untagged CMYK
    /// TIFF or JPEG the core opens is converted from, and which its
    /// `profile_refusal` names; nil or empty clears it. Throws the core's
    /// sentence for bytes that are not a CMYK profile with a
    /// relative-colorimetric device-to-PCS table, keeping the one already set
    /// (`dr_set_working_cmyk_profile`).
    static func setWorkingCMYKProfile(_ icc: Data?) throws {
        var err: UnsafeMutablePointer<CChar>?
        let ok: Bool
        if let icc, !icc.isEmpty {
            ok = icc.withUnsafeBytes { bytes in
                dr_set_working_cmyk_profile(
                    bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, &err)
            }
        } else {
            ok = dr_set_working_cmyk_profile(nil, 0, &err)
        }
        guard ok else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not set the working CMYK profile."))
        }
    }
}
