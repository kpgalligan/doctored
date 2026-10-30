import Foundation

/// Model masks (Select ▸ Sky, Select ▸ Person Parts…, the Object Finder): the only
/// `dr_model_mask_coverage` call. The core turns a segmentation model's low-resolution output
/// plane (`ModelPlane`, channel-major) into canvas u16 coverage over the canvas rectangle the
/// model saw. No admission of its own: the caller admits the core's figure
/// (`ModelMaskRun.coverageScratch`) and passes the working limit it measured.
enum RasterModelMaskCore {
    /// Canvas-sized u16 coverage (65535 = covered), row-major, and the count of samples at or
    /// above half coverage. `request` is `{"frame": [x, y, w, h], "values": "logit" |
    /// "probability", "channels": [c…]}`; `clip` is nil or canvas-sized bytes multiplied in.
    /// Throws the core's refusal: a malformed request, a frame outside the canvas, a non-finite
    /// sample, a plane past its caps, the working limit, a cancel.
    static func coverage(_ plane: ModelPlane, request: [String: Any], clip: [UInt8]?, width: Int,
                         height: Int, limit: UInt64, other: UInt64,
                         control: RasterJobControl?) throws -> (coverage: [UInt16], covered: UInt64) {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "Invalid model mask dimensions.")
        }
        guard clip == nil || clip?.count == width * height else {
            throw DoctoredCoreError(message: "The model mask clip does not match the canvas.")
        }
        let json = String(decoding: try JSONSerialization.data(withJSONObject: request), as: UTF8.self)
        var output = [UInt16](repeating: 0, count: width * height)
        var covered: UInt64 = 0
        var error: UnsafeMutablePointer<CChar>?
        let success = plane.samples.withUnsafeBufferPointer { samples in
            output.withUnsafeMutableBufferPointer { out in
                let (target, count) = (out.baseAddress, out.count)
                let call = { (clipPointer: UnsafePointer<UInt8>?, clipCount: Int) in
                    dr_model_mask_coverage(
                        samples.baseAddress, samples.count, UInt32(plane.width), UInt32(plane.height),
                        UInt32(plane.channels), json, clipPointer, clipCount, UInt32(width),
                        UInt32(height), target, count, &covered, limit, other,
                        control?.ptr, &error)
                }
                guard let clip else { return call(nil, 0) }
                return clip.withUnsafeBufferPointer { call($0.baseAddress, $0.count) }
            }
        }
        guard success else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not compute the model's selection."))
        }
        return (output, covered)
    }
}
