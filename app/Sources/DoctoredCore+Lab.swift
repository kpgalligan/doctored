import AppKit

// MARK: - Lab colour values

/// The colour picker's value-only Lab pair: the core's D50 Lab
/// through the BUILT-IN sRGB profile — `dr_doc_lab`'s pipeline on an sRGB
/// document, so the picker and the Info panel read the same numbers. A
/// colour, not a pixel: no handle.
enum RasterLab {
    /// L, a, b of the encoded sRGB components `srgb` (3 values, each clamped
    /// into 0…1 by the core); nil for a wrong count, a non-finite value, or
    /// a core that cannot model its built-in sRGB profile.
    static func lab(srgb: [Double]) -> [Double]? {
        guard srgb.count == 3 else { return nil }
        let input = srgb.map { Float($0) }
        var out = [Float](repeating: 0, count: 3)
        let ok = input.withUnsafeBufferPointer { inBuffer in
            out.withUnsafeMutableBufferPointer { outBuffer in
                dr_srgb_to_lab(inBuffer.baseAddress, outBuffer.baseAddress)
            }
        }
        return ok ? out.map { Double($0) } : nil
    }

    /// The encoded sRGB components (each clamped into 0…1) of the D50 Lab
    /// colour `lab` (3 values), and whether it was inside the sRGB gamut
    /// before that clamp; nil for a wrong count or a non-finite value.
    static func srgb(lab: [Double]) -> (rgb: [Double], inGamut: Bool)? {
        guard lab.count == 3 else { return nil }
        let input = lab.map { Float($0) }
        var out = [Float](repeating: 0, count: 3)
        var inGamut = false
        let ok = input.withUnsafeBufferPointer { inBuffer in
            out.withUnsafeMutableBufferPointer { outBuffer in
                dr_lab_to_srgb(inBuffer.baseAddress, outBuffer.baseAddress, &inGamut)
            }
        }
        return ok ? (out.map { Double($0) }, inGamut) : nil
    }

    /// #547 Lab phase 2d: the D50 Lab colour `lab` (3 values) as the encoded RGB (each 0…1) of
    /// the space a Lab document's `op` runs its colour form in — ProPhoto RGB, or sRGB for
    /// color_lookup and gradient_map: what that kernel reads (`dr_lab_kernel_rgb`). nil for a
    /// wrong count, a non-finite value or an op without the colour form.
    static func kernelRGB(op: String, lab: [Double]) -> [Double]? {
        guard lab.count == 3 else { return nil }
        let input = lab.map { Float($0) }
        var out = [Float](repeating: 0, count: 3)
        let ok = input.withUnsafeBufferPointer { inBuffer in
            out.withUnsafeMutableBufferPointer { outBuffer in
                dr_lab_kernel_rgb(op, inBuffer.baseAddress, outBuffer.baseAddress)
            }
        }
        return ok ? out.map { Double($0) } : nil
    }
}

// MARK: - The Lab Color document

// The Lab Color document's core wrappers (#547 Lab phases 1, 1b and 3a): every `dr_*` call of the
// Lab area, the view's (`RasterPrecisionImage.labDisplay`), the fit-first thumbnail's, the Lab TIFF
// save's and the flat Lab TIFF open's included.
// The gate that decides what a Lab document admits is `LabDocument.swift`; Image ▸ Mode's job is
// `EditorViewController+CmykMode.swift`'s (`CmykModeConversion`'s lab arms,
// `EditorViewController+LabMode.swift`).

/// `dr_path_lab_source`'s answers (#547 Lab phase 3a).
enum LabSource: Int32 {
    case none = 0, tiff = 1
}

extension RasterPrecisionDocument {
    /// Whether `url` is a flat Lab TIFF the Lab open accepts, header work only
    /// (`dr_path_lab_source`: CIELab or ICCLab, chunky, 3 samples or 4 with an
    /// unassociated alpha, 8 or 16 bits).
    static func labSource(at url: URL) -> LabSource {
        LabSource(rawValue: dr_path_lab_source(url.path)) ?? .none
    }

    /// A flat Lab TIFF as a Lab Color document of its depth (`dr_precision_doc_open_lab`).
    static func openLab(url: URL, workingLimit: UInt64, otherRetained: UInt64,
                        control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_open_lab(url.path, workingLimit, otherRetained,
                                                     control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not open the Lab file."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    private static func withLabProfile<T>(_ profile: Data,
                                          _ body: (UnsafePointer<UInt8>?, Int) throws -> T) rethrows -> T {
        try profile.withUnsafeBytes { raw in
            try body(raw.bindMemory(to: UInt8.self).baseAddress, profile.count)
        }
    }

    private static func labJSON(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return "{}" }
        return text
    }

    /// File ▸ New ▸ Lab Color: request `{"depth":8|16,"fill_srgb","layer_name","ppi"}`
    /// (`dr_precision_doc_new_lab`).
    static func newLab(width: Int, height: Int, request: [String: Any], workingLimit: UInt64,
                       otherRetained: UInt64) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_new_lab(UInt32(clamping: width), UInt32(clamping: height),
                                                    labJSON(request), workingLimit, otherRetained,
                                                    &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "Could not create the Lab Color document."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ Lab Color's plan: `working_memory_plan_bytes`, `refusal`,
    /// `flatten_required`, `flattens`. Options `{"flatten"?,"depth"?}`.
    func convertToLabPlan(options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_convert_to_lab_plan(ptr, Self.labJSON(options), &error)
        return try precisionJSON(result, error)
    }

    /// Image ▸ Mode ▸ Lab Color: a new Lab document.
    func convertToLab(options: [String: Any], workingLimit: UInt64, otherRetained: UInt64,
                      control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_convert_to_lab(ptr, Self.labJSON(options), workingLimit,
                                                           otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to Lab Color."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ RGB Color's plan on a Lab document; `profile` the destination RGB.
    func convertLabToRGBPlan(profile: Data, options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.labJSON(options)
        let result = Self.withLabProfile(profile) { bytes, count in
            dr_precision_doc_convert_lab_to_rgb_plan(ptr, bytes, count, text, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Image ▸ Mode ▸ RGB Color on a Lab document: the twin into `profile`, a 16-bit RGB result.
    func convertLabToRGB(profile: Data, options: [String: Any], workingLimit: UInt64,
                         otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.labJSON(options)
        let result = Self.withLabProfile(profile) { bytes, count in
            dr_precision_doc_convert_lab_to_rgb(ptr, bytes, count, text, workingLimit, otherRetained,
                                                control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to RGB."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ 8/16 Bits/Channel's plan on a Lab document (#547 Lab phase 5a,
    /// `dr_precision_doc_lab_depth_plan`): `working_memory_plan_bytes`, `refusal`, `changes`.
    func labDepthPlan(bits: Int) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_lab_depth_plan(ptr, UInt32(clamping: bits), &error)
        return try precisionJSON(result, error)
    }

    /// Image ▸ Mode ▸ 8/16 Bits/Channel on a Lab document; nil when it changes nothing
    /// (`dr_precision_doc_lab_depth_edit`).
    func labDepthEdit(bits: Int, workingLimit: UInt64, otherRetained: UInt64,
                      control: RasterJobControl?) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_lab_depth_edit(ptr, UInt32(clamping: bits), workingLimit,
                                                           otherRetained, control?.ptr, &error) else {
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(error,
                    fallback: "The Lab Color depth change was refused."))
            }
            return nil
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The Lab TIFF save's plan (`dr_precision_doc_save_lab_plan`): `working_memory_plan_bytes`,
    /// `refusal`. `projection` nil makes the save render the composite itself.
    func saveLabPlan(projection: RasterPrecisionImage?, stripMetadata: Bool) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_save_lab_plan(ptr, projection?.ptr,
                                                    Self.labSaveRequest(stripMetadata), &error)
        return try precisionJSON(result, error)
    }

    /// Writes the composite (`projection`, else a fresh render) as a TIFF 6.0 CIELab file at the
    /// document's depth (#547 Lab phase 1b, `dr_precision_doc_save_lab`).
    func saveLab(to url: URL, projection: RasterPrecisionImage?, stripMetadata: Bool,
                 workingLimit: UInt64, otherRetained: UInt64) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_precision_doc_save_lab(ptr, projection?.ptr, url.path,
                                        Self.labSaveRequest(stripMetadata), workingLimit,
                                        otherRetained, nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not save the Lab TIFF."))
        }
    }

    private static func labSaveRequest(_ strip: Bool) -> String {
        labJSON(["format": "tiff", "strip_metadata": strip])
    }

    /// What a neural run on a Lab document reserves beside its model's own plan (#547 Lab phase
    /// 6b-iii, `dr_lab_neural_scratch_bytes`): the Display P3 view of the input and the Lab image
    /// of the output. `UInt64.max` on error, which refuses the run's admission.
    static func labNeuralScratchBytes(inputWidth: Int, inputHeight: Int, outputWidth: Int,
                                      outputHeight: Int) -> UInt64 {
        dr_lab_neural_scratch_bytes(UInt32(clamping: inputWidth), UInt32(clamping: inputHeight),
                                    UInt32(clamping: outputWidth), UInt32(clamping: outputHeight))
    }
}

extension RasterPrecisionImage {
    /// The Lab document's view: this Lab projection or layer source as Display P3 RGBA16, tagged
    /// Display P3 (`dr_precision_image_lab_display`), for `previewRGBA8`/`thumbnailRGBA8` to narrow.
    func labDisplay(workingLimit: UInt64, otherRetained: UInt64,
                    control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_lab_display(ptr, workingLimit, otherRetained,
                                                          control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "Could not make the Lab Color view."))
        }
        return RasterPrecisionImage(owning: result)
    }

    /// A Lab layer's Layers-panel thumbnail, fitted at u16 BEFORE it converts
    /// (`dr_precision_image_lab_thumbnail_rgba8`): Display P3 RGBA8, never upscaled.
    func labThumbnailRGBA8(maxSide: Int, workingLimit: UInt64,
                           otherRetained: UInt64) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_lab_thumbnail_rgba8(
            ptr, UInt32(clamping: maxSide), workingLimit, otherRetained, nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "Could not make the Lab Color thumbnail."))
        }
        return RasterImage(owning: result)
    }
}
