import Foundation

/// Generative Fill: the core's frame, plan, the provider's input and the apply at 8 and 16
/// bits (`dr_gen_fill_frame`, `dr_doc_gen_fill_plan` / `_input` / `_apply` and their
/// `dr_precision_doc_` twins), and Generative Expand's coverage (`dr_doc_gen_expand_coverage`,
/// `dr_precision_doc_gen_expand_coverage`). The only callers of those nine exports; the run is
/// `GenerativeFillRun` (GenerativeFill.swift).
extension RasterDocument {
    /// Generative Expand's canvas-sized u16 coverage: 65535 − each pixel's full-composite alpha,
    /// so the area the picture leaves transparent is the hole. `layer` is the anchor; `limit` and
    /// `other` are the working-memory context measured on main. Throws the core's refusal (no
    /// transparent pixel among them).
    func genExpandCoverage(layer: Int, limit: UInt64, other: UInt64,
                           control: RasterJobControl?) throws -> [UInt16] {
        var coverage = [UInt16](repeating: 0, count: width * height)
        var error: UnsafeMutablePointer<CChar>?
        let ok = coverage.withUnsafeMutableBufferPointer { out in
            if let precision {
                return dr_precision_doc_gen_expand_coverage(
                    precision.ptr, layer, out.baseAddress, out.count, limit, other, control?.ptr,
                    &error)
            }
            return dr_doc_gen_expand_coverage(ptr, layer, out.baseAddress, out.count, limit, other,
                                              control?.ptr, &error)
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Expand was refused."))
        }
        return coverage
    }

    /// The native size whose aspect is nearest the covered rect's among `sizes`, and the crop
    /// `[x, y, w, h]` sent at it, for the canvas-sized `coverage`. Throws the core's refusal.
    func genFillFrame(coverage: [UInt16], sizes: [(width: Int, height: Int)]) throws
        -> (crop: [Int], native: (width: Int, height: Int)) {
        let flat = sizes.flatMap { [UInt32($0.width), UInt32($0.height)] }
        var frame = [UInt32](repeating: 0, count: 6)
        var error: UnsafeMutablePointer<CChar>?
        let ok = frame.withUnsafeMutableBufferPointer { out in
            dr_gen_fill_frame(coverage, coverage.count, UInt32(width), UInt32(height), flat,
                              flat.count, out.baseAddress, &error)
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
        }
        let values = frame.map { Int($0) }
        return (Array(values[0..<4]), (values[4], values[5]))
    }

    /// The core's working set for one Generative Fill at `native` keeping `variations`
    /// pictures. Throws the core's refusal.
    func genFillPlan(native: (width: Int, height: Int), variations: Int) throws -> UInt64 {
        var error: UnsafeMutablePointer<CChar>?
        let bytes: UInt64
        let count = UInt32(max(0, variations))
        if let precision {
            bytes = dr_precision_doc_gen_fill_plan(
                precision.ptr, UInt32(native.width), UInt32(native.height), count, &error)
        } else {
            bytes = dr_doc_gen_fill_plan(ptr, UInt32(native.width), UInt32(native.height), count,
                                         &error)
        }
        guard bytes > 0 else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
        }
        return bytes
    }

    /// The provider's two straight RGBA8 pictures at `native` for the canvas-sized `coverage`
    /// anchored at `layer`: the crop `[x, y, w, h]`, the image and the mask, both with alpha 0
    /// where the provider fills (the hole) and 255 elsewhere. `limit` and `other` are the
    /// working-memory context measured on main.
    func genFillInput(layer: Int, coverage: [UInt16], native: (width: Int, height: Int),
                      limit: UInt64, other: UInt64, control: RasterJobControl?) throws
        -> (crop: [Int], image: [UInt8], mask: [UInt8]) {
        let bytes = 4 * native.width * native.height
        var crop = [UInt32](repeating: 0, count: 4)
        var image = [UInt8](repeating: 0, count: bytes)
        var mask = [UInt8](repeating: 0, count: bytes)
        var error: UnsafeMutablePointer<CChar>?
        let (nw, nh) = (UInt32(native.width), UInt32(native.height))
        let ok = crop.withUnsafeMutableBufferPointer { cropOut in
            image.withUnsafeMutableBufferPointer { imageOut in
                mask.withUnsafeMutableBufferPointer { maskOut in
                    if let precision {
                        return dr_precision_doc_gen_fill_input(
                            precision.ptr, layer, coverage, coverage.count, nw, nh,
                            cropOut.baseAddress, imageOut.baseAddress, imageOut.count,
                            maskOut.baseAddress, maskOut.count, limit, other, control?.ptr, &error)
                    }
                    return dr_doc_gen_fill_input(
                        ptr, layer, coverage, coverage.count, nw, nh, cropOut.baseAddress,
                        imageOut.baseAddress, imageOut.count, maskOut.baseAddress, maskOut.count,
                        limit, other, control?.ptr, &error)
                }
            }
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
        }
        return (crop.map { Int($0) }, image, mask)
    }

    /// The provider's straight RGBA8 `results` (1 to 4) on a new masked layer `name`, tagged
    /// `tag`, directly above `layer`, the first rendered and every one kept as a variation; the
    /// document and the new layer's index, nil when nothing changed.
    func genFillApply(layer: Int, coverage: [UInt16], native: (width: Int, height: Int),
                      results: [GenerativePicture], name: String, tag: String, limit: UInt64,
                      other: UInt64, control: RasterJobControl?) throws -> (document: RasterDocument, layer: Int)? {
        var error: UnsafeMutablePointer<CChar>?
        var index = 0
        let (nw, nh) = (UInt32(native.width), UInt32(native.height))
        let (bytes, sizes) = GenerativePicture.flattened(results)
        if let precision {
            guard let out = dr_precision_doc_gen_fill_apply(
                precision.ptr, layer, coverage, coverage.count, nw, nh, bytes, bytes.count, sizes,
                sizes.count, name, tag, &index, limit, other, control?.ptr, &error) else {
                guard error != nil else { return nil }
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
            }
            return (try RasterDocument(precision: RasterPrecisionDocument(owning: out)), index)
        }
        guard let out = dr_doc_gen_fill_apply(
            ptr, layer, coverage, coverage.count, nw, nh, bytes, bytes.count, sizes, sizes.count,
            name, tag, &index, limit, other, control?.ptr, &error) else {
            guard error != nil else { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
        }
        return (RasterDocument(owning: out), index)
    }
}
