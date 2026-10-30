import AppKit

// The Bitmap document's core wrappers (#1 Bitmap phase 1a): every `dr_*` call of the Bitmap
// area. The gate that decides what a Bitmap document admits is `BitmapDocument.swift`; Image ▸
// Mode's job is `EditorViewController+CmykMode.swift`'s (`CmykModeConversion`'s bitmap arms,
// `EditorViewController+BitmapMode.swift`). Phase 1c adds the 1-bit files: which PNG, TIFF and
// BMP files open as a Bitmap document, their open, and a Bitmap PSD or PSB's stored composite.

/// `dr_path_bitmap_source`'s answers (#1 Bitmap phase 1c).
enum BitmapSource: Int32 {
    case none = 0, png = 1, tiff = 2, bmp = 3
}

extension RasterPrecisionDocument {
    /// Which 1-bit file `url` is, header work only (`dr_path_bitmap_source`).
    static func bitmapSource(at url: URL) -> BitmapSource {
        BitmapSource(rawValue: dr_path_bitmap_source(url.path)) ?? .none
    }

    /// A 1-bit PNG, TIFF or BMP as a Bitmap document; `workingGray` is the profile of a file with
    /// none (`dr_precision_doc_open_bitmap`).
    static func openBitmap(url: URL, workingGray: String, workingLimit: UInt64, otherRetained: UInt64,
                           control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_open_bitmap(url.path, workingGray, workingLimit, otherRetained,
                                                        control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not open the 1-bit file."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// A Bitmap (colour mode 0) PSD or PSB's stored composite as a Bitmap document
    /// (`import_precision` `"bitmap"`; the file holds no layers), still the file `fingerprint`
    /// names (`dr_precision_doc_open_bitmap_psd`).
    static func openBitmapPSD(at url: URL, fingerprint: String, workingGray: String,
                              workingLimit: UInt64, otherRetained: UInt64,
                              control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_precision_doc_open_bitmap_psd(url.path, fingerprint, workingGray,
            workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not import PSD."))
        }
        return RasterPrecisionDocument(owning: handle)
    }

    private static func bitmapJSON(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return "{}" }
        return text
    }

    /// Image ▸ Mode ▸ Bitmap's plan: `working_memory_plan_bytes`, `refusal`, `flatten_required`,
    /// `flattens`, `width`, `height` (`dr_precision_doc_convert_to_bitmap_plan`).
    func convertToBitmapPlan(options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.bitmapJSON(options)
        return try precisionJSON(dr_precision_doc_convert_to_bitmap_plan(ptr, text, &error), error)
    }

    /// Image ▸ Mode ▸ Bitmap: a new Bitmap document from an 8-bit Grayscale one
    /// (`dr_precision_doc_convert_to_bitmap`).
    func convertToBitmap(options: [String: Any], workingLimit: UInt64, otherRetained: UInt64,
                         control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.bitmapJSON(options)
        guard let result = dr_precision_doc_convert_to_bitmap(ptr, text, workingLimit, otherRetained,
                                                              control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to Bitmap."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ Grayscale on a Bitmap document, planned: `working_memory_plan_bytes`,
    /// `refusal`, `width`, `height` (`dr_precision_doc_convert_bitmap_to_gray_plan`).
    func convertBitmapToGrayPlan(options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.bitmapJSON(options)
        return try precisionJSON(dr_precision_doc_convert_bitmap_to_gray_plan(ptr, text, &error), error)
    }

    /// Image ▸ Mode ▸ Grayscale on a Bitmap document, with its Size Ratio
    /// (`dr_precision_doc_convert_bitmap_to_gray`).
    func convertBitmapToGray(options: [String: Any], workingLimit: UInt64, otherRetained: UInt64,
                             control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.bitmapJSON(options)
        guard let result = dr_precision_doc_convert_bitmap_to_gray(ptr, text, workingLimit, otherRetained,
                                                                   control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to Grayscale."))
        }
        return RasterPrecisionDocument(owning: result)
    }
}
