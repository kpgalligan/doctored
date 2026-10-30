import Foundation

// The two v1.x HDR exports of the core: the HDR still's float intake and the
// extended-range (EDR) view. `float_host_intake` and `float_view` own every rule; these wrappers
// only marshal.

extension RasterDocument {
    /// A host-decoded HDR still as a 32-bit float document: `transfer`
    /// `"linear"` with an `rgba32f_*` layout and the matrix/TRC `profile` the
    /// samples are in, or `"pq"` / `"hlg"` with an integer 16-bit or
    /// `rgb10_packed_le` layout and `primaries` (`bt709`, `display_p3`,
    /// `bt2020`), no profile. 203 cd/m² opens as 1.0 (the core's
    /// `hdr_transfer`).
    static func fromHostHDRPixels(bytes: Data, width: Int, height: Int, rowBytes: Int,
                                  layout: String, orientation: Int, transfer: String,
                                  primaries: String?, profile: Data?,
                                  control: RasterJobControl? = nil) throws -> RasterDocument {
        guard let w = UInt32(exactly: width), let h = UInt32(exactly: height), w > 0, h > 0,
              rowBytes > 0, (1...8).contains(orientation) else {
            throw DoctoredCoreError(message: "Unsupported HDR host sample extent or orientation.")
        }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = bytes.withUnsafeBytes { samples -> OpaquePointer? in
            let base = samples.bindMemory(to: UInt8.self).baseAddress
            guard let profile else {
                return dr_precision_doc_from_host_hdr_pixels(base, samples.count, w, h, rowBytes,
                    layout, UInt32(orientation), transfer, primaries, nil, 0,
                    context.limit, context.otherRetained, control?.ptr, &error)
            }
            return profile.withUnsafeBytes { icc in
                dr_precision_doc_from_host_hdr_pixels(base, samples.count, w, h, rowBytes,
                    layout, UInt32(orientation), transfer, primaries,
                    icc.bindMemory(to: UInt8.self).baseAddress, icc.count,
                    context.limit, context.otherRetained, control?.ptr, &error)
            }
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "Could not capture the HDR photo's samples."))
        }
        return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
    }
}

extension RasterPrecisionDocument {
    /// The float projection `image` as the extended-range view (the core's
    /// rule X): RGBA binary16, premultiplied, linear in the document
    /// primaries, `exposureStops` of view exposure, no tone map. `peak` is
    /// the largest RGB component written. The memory plan is the core's
    /// `float_view::extended_display_scratch` (`EXTENDED_DISPLAY_BYTES_PER_PIXEL`,
    /// 46 B/px), admitted by the core before it writes; the 8 B/px `Data` is
    /// the output buffer, allocated here before that check.
    func displayExtended(of image: RasterPrecisionImage, exposureStops: Double,
                         workingLimit: UInt64, otherRetained: UInt64,
                         control: RasterJobControl? = nil) throws -> (pixels: Data, peak: Float) {
        let (count, overflow) = image.width.multipliedReportingOverflow(by: image.height)
        let (length, lengthOverflow) = count.multipliedReportingOverflow(by: 8)
        guard !overflow, !lengthOverflow, length > 0 else {
            throw DoctoredCoreError(message: "The extended-range view is too large.")
        }
        var pixels = Data(count: length)
        var peak: Float = 0
        var error: UnsafeMutablePointer<CChar>?
        let ok = pixels.withUnsafeMutableBytes { out in
            dr_precision_doc_display_extended(ptr, image.ptr, exposureStops,
                out.bindMemory(to: UInt8.self).baseAddress, out.count, &peak,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "The extended-range view could not be produced."))
        }
        return (pixels, peak)
    }
}
