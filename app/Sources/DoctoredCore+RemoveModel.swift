import Foundation

/// The Remove tool's Model mode: the core's plan, the model's native input and the
/// composite at 8 and 16 bits (`dr_doc_remove_model_plan` / `_input` / `_apply` and their
/// `dr_precision_doc_` twins). The only callers of those six exports; the run is
/// `RemoveModelRun` (RemoveModel.swift).
extension RasterDocument {
    /// The core's working set for one Remove on `layer` sampling `sample` with a model of native
    /// `side`, output to a new layer or not. Throws the core's refusal.
    func removeModelPlan(layer: Int, sample: RetouchSampling, side: Int, newLayer: Bool) throws
        -> UInt64 {
        var error: UnsafeMutablePointer<CChar>?
        let bytes: UInt64
        if let precision {
            bytes = dr_precision_doc_remove_model_plan(
                precision.ptr, layer, sample.rawValue, UInt32(side), newLayer, &error)
        } else {
            bytes = dr_doc_remove_model_plan(ptr, layer, sample.rawValue, UInt32(side), newLayer, &error)
        }
        guard bytes > 0 else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Remove was refused."))
        }
        return bytes
    }

    /// The model's native input for the canvas-sized `coverage`: the crop `[x, y, w, h]`, the
    /// image (`3·side²`, channel-major RGB 0…1) and the mask (`side²`, 1 = remove). `limit` and
    /// `other` are the working-memory context measured on main before the job.
    func removeModelInput(layer: Int, choice: RetouchSamplingChoice, coverage: [UInt16], side: Int,
                          limit: UInt64, other: UInt64, control: RasterJobControl?) throws
        -> (crop: [Int], image: [Float], mask: [Float]) {
        let cells = side * side
        var crop = [UInt32](repeating: 0, count: 4)
        var image = [Float](repeating: 0, count: 3 * cells)
        var mask = [Float](repeating: 0, count: cells)
        var error: UnsafeMutablePointer<CChar>?
        let sample = choice.mode.rawValue
        let ok = crop.withUnsafeMutableBufferPointer { cropOut in
            image.withUnsafeMutableBufferPointer { imageOut in
                mask.withUnsafeMutableBufferPointer { maskOut in
                    if let precision {
                        return dr_precision_doc_remove_model_input(
                            precision.ptr, layer, sample, choice.ignoreAdjustments, coverage,
                            coverage.count, UInt32(side), cropOut.baseAddress, imageOut.baseAddress,
                            imageOut.count, maskOut.baseAddress, maskOut.count, limit, other,
                            control?.ptr, &error)
                    }
                    return dr_doc_remove_model_input(
                        ptr, layer, sample, choice.ignoreAdjustments, coverage, coverage.count,
                        UInt32(side), cropOut.baseAddress, imageOut.baseAddress, imageOut.count,
                        maskOut.baseAddress, maskOut.count, limit, other, control?.ptr, &error)
                }
            }
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Remove was refused."))
        }
        return (crop.map { Int($0) }, image, mask)
    }

    /// The model's `plane` (3 channels of `side × side`) composited over `layer` by `coverage`,
    /// in place or onto a new layer "Remove" above it; nil when nothing changed.
    func removeModelApply(layer: Int, coverage: [UInt16], side: Int, plane: ModelPlane,
                          newLayer: Bool, limit: UInt64, other: UInt64,
                          control: RasterJobControl?) throws -> RasterDocument? {
        guard plane.channels == 3, plane.width == side, plane.height == side else {
            throw DoctoredCoreError(message: "The Remove model's output is not a \(side)² RGB plane.")
        }
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            guard let result = dr_precision_doc_remove_model_apply(
                precision.ptr, layer, coverage, coverage.count, UInt32(side), plane.samples,
                plane.samples.count, newLayer, limit, other, control?.ptr, &error) else {
                guard error != nil else { return nil }
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Remove was refused."))
            }
            return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
        }
        guard let result = dr_doc_remove_model_apply(
            ptr, layer, coverage, coverage.count, UInt32(side), plane.samples, plane.samples.count,
            newLayer, limit, other, control?.ptr, &error) else {
            guard error != nil else { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Remove was refused."))
        }
        return RasterDocument(owning: result)
    }
}
