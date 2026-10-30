import AppKit

// The Duotone document's core wrappers (#547, `designs/duotone-mode.md`): every `dr_*` call of the
// Duotone area — the options normalized and read, Image ▸ Mode ▸ Duotone…'s plan and edit, and the
// view; the render through the inks, Duotone → RGB and the `.ado` file; the thumbnail through the
// inks; Duotone → Grayscale and Duotone → Bitmap.
// The gate is `DuotoneDocument.swift`; the edit `DuotoneOptions.swift`'s `DuotoneEdit`.

extension RasterPrecisionDocument {
    private static func duotoneJSON(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return "{}" }
        return text
    }

    /// `options` checked and normalized by the core (`dr_duotone_options_normalize`): the type,
    /// every ink, every overprint key (NSNull where default) and `overprint_display`.
    static func normalizeDuotone(_ options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_duotone_options_normalize(duotoneJSON(options), &error), error)
    }

    /// A Duotone document's normalized options; nil for every other document
    /// (`dr_precision_doc_duotone_options`).
    func duotoneOptions() throws -> [String: Any]? {
        var error: UnsafeMutablePointer<CChar>?
        guard let text = dr_precision_doc_duotone_options(ptr, &error) else {
            if error == nil { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not read the Duotone options."))
        }
        return try precisionJSON(text, nil)
    }

    /// Image ▸ Mode ▸ Duotone…'s plan: `working_memory_plan_bytes`, `refusal`, `changes`
    /// (`dr_precision_doc_set_duotone_plan`).
    func setDuotonePlan(_ options: DuotoneOptions) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.duotoneJSON(options.json)
        return try precisionJSON(dr_precision_doc_set_duotone_plan(ptr, text, &error), error)
    }

    /// Image ▸ Mode ▸ Duotone…: a new document with `options` as its description, every sample
    /// shared; nil when nothing changes (`dr_precision_doc_set_duotone`).
    func settingDuotone(_ options: DuotoneOptions) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_set_duotone(ptr, Self.duotoneJSON(options.json), &error)
        guard let result else {
            if error == nil { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not set the Duotone options."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The Duotone view of `image` (a projection or layer source of this document) as RGBA16 in the
    /// document's profile, alpha kept (`dr_precision_doc_duotone_view`).
    func duotoneView(of image: RasterPrecisionImage, workingLimit: UInt64, otherRetained: UInt64,
                     control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_duotone_view(ptr, image.ptr, workingLimit, otherRetained,
                                                         control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not make the Duotone view."))
        }
        return RasterPrecisionImage(owning: result)
    }

    // #547 Duotone phase 1b: the render through the inks, Duotone → RGB and the `.ado` file.

    private static func withDuotoneProfile<T>(_ profile: Data,
                                              _ body: (UnsafePointer<UInt8>?, Int) throws -> T) rethrows -> T {
        try profile.withUnsafeBytes { raw in
            try body(raw.bindMemory(to: UInt8.self).baseAddress, profile.count)
        }
    }

    /// The render's plan: `working_memory_plan_bytes`, `refusal`, `flatten_required`, `flattens`
    /// (`dr_precision_doc_duotone_render_plan`).
    func duotoneRenderPlan(flatten: Bool) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_precision_doc_duotone_render_plan(ptr, flatten, &error), error)
    }

    /// The render through the inks: an RGB document in the twin, each raster through the view
    /// (`dr_precision_doc_duotone_render`).
    func duotoneRender(flatten: Bool, workingLimit: UInt64, otherRetained: UInt64,
                       control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_duotone_render(ptr, flatten, workingLimit, otherRetained,
                                                           control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the inks."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Duotone → RGB Color's plan under `{"intent"?,"black_point_compensation"?,"flatten"?}`
    /// (`dr_precision_doc_convert_duotone_to_rgb_plan`).
    func convertDuotoneToRGBPlan(profile: Data, options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.duotoneJSON(options)
        let result = Self.withDuotoneProfile(profile) { bytes, count in
            dr_precision_doc_convert_duotone_to_rgb_plan(ptr, bytes, count, text, &error)
        }
        return try precisionJSON(result, error)
    }

    /// Duotone → RGB Color: the render, then Convert to Profile from the twin
    /// (`dr_precision_doc_convert_duotone_to_rgb`).
    func convertDuotoneToRGB(profile: Data, options: [String: Any], workingLimit: UInt64,
                             otherRetained: UInt64, control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.duotoneJSON(options)
        let result = Self.withDuotoneProfile(profile) { bytes, count in
            dr_precision_doc_convert_duotone_to_rgb(ptr, bytes, count, text, workingLimit, otherRetained,
                                                    control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to RGB."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Duotone → Grayscale's plan: `working_memory_plan_bytes`, `refusal`, `flatten_required`,
    /// `flattens` (`dr_precision_doc_convert_duotone_to_gray_plan`).
    func convertDuotoneToGrayPlan(flatten: Bool) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_precision_doc_convert_duotone_to_gray_plan(ptr, flatten, &error), error)
    }

    /// Duotone → Grayscale: the same Gray document with each raster through the inks' luminance,
    /// the inks dropped (`dr_precision_doc_convert_duotone_to_gray`).
    func convertDuotoneToGray(flatten: Bool, workingLimit: UInt64, otherRetained: UInt64,
                              control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_convert_duotone_to_gray(ptr, flatten, workingLimit, otherRetained,
                                                                    control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to Grayscale."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Duotone → Bitmap's plan under Bitmap's options, `width` and `height` included
    /// (`dr_precision_doc_convert_duotone_to_bitmap_plan`).
    func convertDuotoneToBitmapPlan(options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.duotoneJSON(options)
        return try precisionJSON(dr_precision_doc_convert_duotone_to_bitmap_plan(ptr, text, &error), error)
    }

    /// Duotone → Bitmap: Duotone → Grayscale, then Gray → Bitmap
    /// (`dr_precision_doc_convert_duotone_to_bitmap`).
    func convertDuotoneToBitmap(options: [String: Any], workingLimit: UInt64, otherRetained: UInt64,
                                control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_convert_duotone_to_bitmap(
            ptr, Self.duotoneJSON(options), workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to Bitmap."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// A Duotone layer's (or composite's) thumbnail, fitted at u16 BEFORE the view (#547
    /// Duotone phase 2a, `dr_precision_doc_duotone_thumbnail_rgba8`): RGBA8 in the document's
    /// profile, never upscaled.
    func duotoneThumbnailRGBA8(of image: RasterPrecisionImage, maxSide: Int, workingLimit: UInt64,
                               otherRetained: UInt64) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_duotone_thumbnail_rgba8(
            ptr, image.ptr, UInt32(clamping: maxSide), workingLimit, otherRetained, nil, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "Could not make the Duotone thumbnail."))
        }
        return RasterImage(owning: result)
    }

    /// The Duotone Options (`.ado`) file at `url` as normalized options (`dr_duotone_read_ado`).
    static func readDuotoneADO(at url: URL) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_duotone_read_ado(url.path, &error), error)
    }

    /// `options` written to `url` as a Duotone Options (`.ado`) file (`dr_duotone_write_ado`).
    static func writeDuotoneADO(_ options: DuotoneOptions, to url: URL) throws {
        var error: UnsafeMutablePointer<CChar>?
        guard dr_duotone_write_ado(url.path, duotoneJSON(options.json), &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not write the Duotone options."))
        }
    }
}
