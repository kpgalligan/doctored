import Foundation

// Image ▸ Adjustments ▸ Equalize's byte call (#1; `core/src/ffi_equalize.rs`). The typed
// documents run the core's selected edit `{"op":"equalize"}` through `SelectedAdjustment`.

extension RasterImage {
    /// Equalized: one table from the summed R, G and B histogram, applied to every pixel. `mask`
    /// is image-sized histogram weight only (nil: every pixel weighs alike); the caller writes the
    /// result through its selection. nil when nothing would change.
    func equalized(mask: [UInt8]?) throws -> RasterImage? {
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer?
        if let mask {
            result = mask.withUnsafeBufferPointer { dr_image_equalize(ptr, $0.baseAddress, $0.count, &error) }
        } else {
            result = dr_image_equalize(ptr, nil, 0, &error)
        }
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not equalize.")) }
        return result.map { RasterImage(owning: $0) }
    }
}
