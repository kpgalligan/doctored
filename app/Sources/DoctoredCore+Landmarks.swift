import Foundation

/// Landmark regions (Select ▸ Face Parts…): the only `dr_landmark_coverage`
/// call. The request JSON is `FaceAnalysis.regionsJSON`'s; the core turns each
/// point loop into a closed centripetal Catmull–Rom spline and answers its u16
/// coverage. No admission of its own: the caller admits the core's figure
/// (`FaceParts.coverageScratch`) and passes the working limit it measured.
enum RasterLandmarkCore {
    /// Canvas-sized u16 coverage (65535 = covered), row-major. Throws the
    /// core's refusal: a malformed request or cap, the working limit, the
    /// rasterizer's caps, a cancel.
    static func coverage(
        _ regionsJSON: String, width: Int, height: Int, limit: UInt64, other: UInt64,
        control: RasterJobControl?
    ) throws -> [UInt16] {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "Invalid landmark coverage dimensions.")
        }
        var output = [UInt16](repeating: 0, count: width * height)
        var error: UnsafeMutablePointer<CChar>?
        let success = output.withUnsafeMutableBufferPointer { samples in
            dr_landmark_coverage(
                regionsJSON, UInt32(width), UInt32(height), samples.baseAddress, samples.count,
                limit, other, control?.ptr, &error)
        }
        guard success else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not rasterize the face parts."))
        }
        return output
    }
}
