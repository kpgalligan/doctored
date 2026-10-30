import Foundation

/// Generative Layer variations: the core's info, select, the input of a new set
/// and its add at 8 bits and on the typed document (`dr_doc_gen_fill_variations` /
/// `_select_variation` / `_more_input` / `_add_variations` and their `dr_precision_doc_` twins).
/// The only callers of those eight exports; the runs are `GenerativeVariationsRun`
/// (GenerativeVariations.swift).
extension RasterDocument {
    /// Layer `layer`'s live variations, nil when it holds none. Throws the core's refusal (an
    /// out-of-range index).
    func genFillVariations(layer: Int) throws -> GenerativeVariationsInfo? {
        var info = [UInt32](repeating: 0, count: 8)
        var error: UnsafeMutablePointer<CChar>?
        let found = info.withUnsafeMutableBufferPointer { out in
            if let precision {
                return dr_precision_doc_gen_fill_variations(precision.ptr, layer, out.baseAddress, &error)
            }
            return dr_doc_gen_fill_variations(ptr, layer, out.baseAddress, &error)
        }
        guard found else {
            guard error == nil else {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The variations could not be read."))
            }
            return nil
        }
        let v = info.map { Int($0) }
        return GenerativeVariationsInfo(count: v[0], selected: v[1], native: (v[2], v[3]),
                                        crop: Array(v[4..<8]))
    }

    /// Layer `layer` re-rendered from variation `index`; nil when it is already selected.
    func genFillSelectVariation(layer: Int, index: Int, limit: UInt64, other: UInt64,
                                control: RasterJobControl?) throws -> RasterDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let k = UInt32(max(0, index))
        if let precision {
            guard let out = dr_precision_doc_gen_fill_select_variation(
                precision.ptr, layer, k, limit, other, control?.ptr, &error) else {
                guard error != nil else { return nil }
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The variation was refused."))
            }
            return try RasterDocument(precision: RasterPrecisionDocument(owning: out))
        }
        guard let out = dr_doc_gen_fill_select_variation(ptr, layer, k, limit, other, control?.ptr,
                                                         &error) else {
            guard error != nil else { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The variation was refused."))
        }
        return RasterDocument(owning: out)
    }

    /// The provider's two straight RGBA8 pictures at the stored `native` size for a new set of
    /// layer `layer`'s variations.
    func genFillMoreInput(layer: Int, native: (width: Int, height: Int), limit: UInt64,
                          other: UInt64, control: RasterJobControl?) throws -> (image: [UInt8], mask: [UInt8]) {
        let bytes = 4 * native.width * native.height
        var image = [UInt8](repeating: 0, count: bytes)
        var mask = [UInt8](repeating: 0, count: bytes)
        var error: UnsafeMutablePointer<CChar>?
        let ok = image.withUnsafeMutableBufferPointer { imageOut in
            mask.withUnsafeMutableBufferPointer { maskOut in
                if let precision {
                    return dr_precision_doc_gen_fill_more_input(
                        precision.ptr, layer, imageOut.baseAddress, imageOut.count,
                        maskOut.baseAddress, maskOut.count, limit, other, control?.ptr, &error)
                }
                return dr_doc_gen_fill_more_input(
                    ptr, layer, imageOut.baseAddress, imageOut.count, maskOut.baseAddress,
                    maskOut.count, limit, other, control?.ptr, &error)
            }
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
        }
        return (image, mask)
    }

    /// `results` appended to layer `layer`'s variations, tagged `tag`, the first new one
    /// selected; nil when nothing changed.
    func genFillAddVariations(layer: Int, results: [GenerativePicture], tag: String, limit: UInt64,
                              other: UInt64, control: RasterJobControl?) throws -> RasterDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let (bytes, sizes) = GenerativePicture.flattened(results)
        if let precision {
            guard let out = dr_precision_doc_gen_fill_add_variations(
                precision.ptr, layer, bytes, bytes.count, sizes, sizes.count, tag, limit, other,
                control?.ptr, &error) else {
                guard error != nil else { return nil }
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
            }
            return try RasterDocument(precision: RasterPrecisionDocument(owning: out))
        }
        guard let out = dr_doc_gen_fill_add_variations(
            ptr, layer, bytes, bytes.count, sizes, sizes.count, tag, limit, other, control?.ptr,
            &error) else {
            guard error != nil else { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Generative Fill was refused."))
        }
        return RasterDocument(owning: out)
    }
}
