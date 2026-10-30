import Foundation

/// Edit ▸ Sky Replacement: the core's plan, the dialog's image preview and the edit at 8
/// and 16 bits (`dr_sky_replace_scratch_bytes`, `dr_image_sky_replace`, `dr_doc_sky_replace`,
/// `dr_precision_doc_sky_replace`). The only callers of those four exports; the run is
/// `SkyReplacementRun` (SkyReplacement.swift).
extension RasterImage {
    /// This foreground with its sky replaced by `sky`: the `coverageWidth × coverageHeight`
    /// coverage (any size, the canvas's in the app) read at each pixel centre, the edge widths
    /// taken from the coverage's size, so a reduced proxy previews the commit's widths. Admitted
    /// against the main-thread working limit. Throws the core's sentence.
    func skyReplaced(sky: RasterImage, coverage: [UInt16], coverageWidth: Int, coverageHeight: Int,
                     params: [String: Any], control: RasterJobControl?) throws -> RasterImage {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: params), as: UTF8.self)
        guard coverage.count == coverageWidth * coverageHeight, coverageWidth > 0, coverageHeight > 0 else {
            throw DoctoredCoreError(message: "The sky coverage does not match its size.")
        }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_image_sky_replace(
            ptr, sky.ptr, coverage, coverage.count, UInt32(coverageWidth), UInt32(coverageHeight), json,
            context.limit, context.otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Sky Replacement was refused."))
        }
        return RasterImage(owning: result)
    }
}

extension RasterDocument {
    /// The core's working-set plan for one replacement on a `width × height` canvas with a
    /// `skyWidth × skyHeight` sky, writing `bits` (8 or 16) as `output`, New Layers with the
    /// "Foreground Edge" layer when `decontaminate`; 0 for a zero size.
    static func skyReplaceScratchBytes(width: Int, height: Int, skyWidth: Int, skyHeight: Int,
                                       bits: Int, output: SkyReplacementOutput,
                                       decontaminate: Bool) -> UInt64 {
        dr_sky_replace_scratch_bytes(UInt32(width), UInt32(height), UInt32(skyWidth), UInt32(skyHeight),
                                     UInt32(bits), output.code, decontaminate)
    }

    /// The document with the output `params["output"]` names above `layer` (one new canvas-sized
    /// raster layer, or the Sky Replacement Group): its sky replaced by `sky` through the
    /// canvas-sized `coverage`. `budget` is the byte working-memory
    /// context; nil reads `legacyPrecisionWorkContext()`, which is main-only, so an off-main byte
    /// caller MUST pass it. Ignored for a typed document. A cancel throws `operation cancelled`.
    func skyReplacing(layer: Int, sky: RasterImage, coverage: [UInt16], params: [String: Any],
                      budget: (limit: UInt64, other: UInt64)?, control: RasterJobControl?) throws
        -> RasterDocument {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: params), as: UTF8.self)
        let refused = "Sky Replacement was refused."
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            guard let result = dr_precision_doc_sky_replace(
                precision.ptr, layer, sky.ptr, coverage, coverage.count, json, context.limit,
                context.otherRetained, control?.ptr, &error) else {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: refused))
            }
            return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for Sky Replacement.") }
        let budget = try budget ?? legacyPrecisionWorkContext()
        guard let document = wrap(dr_doc_sky_replace(
            ptr, layer, sky.ptr, coverage, coverage.count, json, budget.limit, budget.other, control?.ptr,
            &error)) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: refused))
        }
        return document
    }
}
