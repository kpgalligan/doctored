import AppKit

extension RasterPrecisionDocument {
    // MARK: - PSD at 16 bits

    /// A 16-bit RGB PSD opens ONLY here, and so does a 32-bit one, as a
    /// 32-bit float document: the byte importer refuses both rather
    /// than converting a depth silently. Mode 0 imports the supported layer
    /// subset, 1 the file's stored composite. The fingerprint comes from the
    /// same compatibility report the 8-bit path uses.
    static func openPSD(at url: URL, mode: Int, fingerprint: String, workingLimit: UInt64,
                        otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_precision_doc_open_psd(url.path, Int32(mode), fingerprint,
            workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not import PSD."))
        }
        return RasterPrecisionDocument(owning: handle)
    }

    /// A Grayscale PSD, PSB or layered TIFF at 8 or 16 bits, or a PSD or PSB at 32
    /// (`import_precision` `"gray"`, #1 phases 3a and 3c), opens ONLY here, as a Grayscale
    /// document of its depth: mode 0 its layers, 1 its stored composite. `workingGray` is the
    /// working Gray's wire name, assigned to an untagged file.
    static func openGrayPSD(at url: URL, mode: Int, fingerprint: String, workingGray: String,
                            workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_precision_doc_open_gray_psd(url.path, Int32(mode), fingerprint,
            workingGray, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not import PSD."))
        }
        return RasterPrecisionDocument(owning: handle)
    }

    /// A Lab PSD, PSB or layered TIFF at 8 or 16 bits (`import_precision` `"lab"`, #1 Lab
    /// phase 3a) opens ONLY here, as a Lab Color document of its depth: mode 0 its layers, 1 its
    /// stored composite (a TIFF has none). An embedded profile is not used.
    static func openLabPSD(at url: URL, mode: Int, fingerprint: String, workingLimit: UInt64,
                           otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_precision_doc_open_lab_psd(url.path, Int32(mode), fingerprint,
            workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not import PSD."))
        }
        return RasterPrecisionDocument(owning: handle)
    }

    /// The export preflight for one container and mode: `depth` 16, or 32 for
    /// a float document, which writes a PSD or PSB at 32 bits and refuses a
    /// layered TIFF; a Grayscale document's declared 8, 16 or 32 with `color_mode`
    /// `"gray"` (#1 phases 3b and 3c; at 32 the float report). Reads no pixels.
    func psdExportReport(container: LayeredContainer, mode: Int) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_psd_export_report(ptr, container.coreValue, Int32(mode), &error)
        return try precisionJSON(result, error)
    }

    /// Writes a 16-bit RGB PSD, PSB or layered TIFF. `projection` is the
    /// authoritative rendering of THIS revision, as `saveProjection16`, and
    /// is required for an RGB document; the core validates its extent and ICC
    /// bytes. A CMYK document passes nil and is written at its declared depth,
    /// its composite rendered by the core (#1 phase 3). Atomic, so a refusal
    /// leaves an existing destination byte-identical. A float document writes
    /// a 32-bit RGB PSD or PSB from its RGBA32 float projection; its
    /// layered TIFF is refused. A Grayscale document writes a colour-mode-1 file at its declared
    /// 8 or 16 bits from its required RGBA16 projection (#1 phase 3b), and at 32 a PSD or PSB
    /// from its required RGBA32 float projection, its layered TIFF refused (#1 phase 3c).
    func savePSD(_ projection: RasterPrecisionImage?, to url: URL, container: LayeredContainer,
                 mode: Int, workingLimit: UInt64, otherRetained: UInt64,
                 control: RasterJobControl? = nil) throws {
        let path = url.path
        try precisionCString(path, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        guard dr_precision_doc_save_psd(ptr, projection?.ptr, path, container.coreValue, Int32(mode),
                                        workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not export \(url.lastPathComponent)."))
        }
    }

    static func resourceReport(documents: [RasterPrecisionDocument], images: [RasterPrecisionImage])
        throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let documents = documents.map { Optional($0.ptr) }
        let images = images.map { Optional($0.ptr) }
        let result = documents.withUnsafeBufferPointer { d in
            images.withUnsafeBufferPointer { i in
                dr_precision_resources_report(d.baseAddress, d.count, i.baseAddress, i.count, &error)
            }
        }
        return try precisionJSON(result, error)
    }
}

extension RasterDocument {
    static func psdImportReport(at url: URL) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        guard let json = dr_psd_import_report(url.path, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not inspect PSD."))
        }
        defer { dr_string_free(json) }
        let data = Data(String(cString: json).utf8)
        guard let report = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid PSD compatibility report.")
        }
        return report
    }

    static func openPSD(at url: URL, mode: Int, fingerprint: String,
                        control: RasterJobControl? = nil) throws -> RasterDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_doc_open_psd(url.path, Int32(mode), fingerprint, control?.ptr,
                                           &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not import PSD."))
        }
        return RasterDocument(owning: handle)
    }

    /// True when `url` is a TIFF whose Photoshop layer block the core reads as
    /// layers (a big-endian unsigned 8- or 16-bit RGB, CMYK, gray or Lab TIFF
    /// holding one, #1, or a 32-bit float RGB one of either byte order). Every
    /// other TIFF answers false and opens flat; `tiffHasLayerBlock` says whether
    /// its block holds layers all the same. Reads no pixel strip.
    static func tiffHasLayers(at url: URL) -> Bool {
        dr_tiff_has_layers(url.path)
    }

    /// True when `url` is a TIFF carrying Photoshop's layer block with layers in it,
    /// read or not by `tiffHasLayers` (either byte order, BigTIFF, any model, depth or
    /// sample format); a block whose entries cannot be walked answers true. Asked of a
    /// TIFF opened flat (`ImageDocument.sourceLayersUnopened`). Reads no pixel strip.
    static func tiffHasLayerBlock(at url: URL) -> Bool {
        dr_tiff_has_layer_block(url.path)
    }

    /// The export preflight for one container and mode: what the file can
    /// hold, what it cannot, and how large it could get. Reads no pixels. A
    /// 16-bit document answers for itself, at its own depth (the `saveNative`
    /// shape).
    func psdExportReport(container: LayeredContainer, mode: Int) throws -> [String: Any] {
        if let precision {
            return try precision.psdExportReport(container: container, mode: mode)
        }
        var error: UnsafeMutablePointer<CChar>?
        guard let json = dr_doc_psd_export_report(ptr, container.coreValue, Int32(mode), &error) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not inspect PSD export."))
        }
        defer { dr_string_free(json) }
        let data = Data(String(cString: json).utf8)
        guard let report = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid PSD export report.")
        }
        return report
    }

    /// Writes the PSD, PSB or layered TIFF. `flattened` is the
    /// already-rendered composite when the caller has one warm; nil lets the
    /// core composite. Atomic, so a refusal leaves an existing destination
    /// byte-identical.
    func savePSD(_ flattened: RasterImage?, to url: URL, container: LayeredContainer,
                 mode: Int, control: RasterJobControl? = nil) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_doc_save_psd(ptr, flattened?.ptr, url.path, container.coreValue, Int32(mode),
                              control?.ptr, &error) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not export \(url.lastPathComponent)."))
        }
    }
}
