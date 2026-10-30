import AppKit

extension RasterDocument {
    // MARK: - Colour samplers

    /// The document's colour samplers, in label order — document state
    /// the core saves in `.rz` and PSD and moves through every geometry op
    /// (`ffi_sampler`). Read in one pass, three values per sampler.
    var colorSamplers: ColorSamplerSet {
        let count = precision.map { dr_precision_doc_color_sampler_count($0.ptr) }
            ?? dr_doc_color_sampler_count(ptr)
        var items: [ColorSampler] = []
        var triple = [UInt32](repeating: 0, count: 3)
        for i in 0..<count {
            let ok = triple.withUnsafeMutableBufferPointer { buffer in
                precision.map { dr_precision_doc_color_sampler($0.ptr, i, buffer.baseAddress) }
                    ?? dr_doc_color_sampler(ptr, i, buffer.baseAddress)
            }
            guard ok else { continue }
            items.append(ColorSampler(x: Int(triple[0]), y: Int(triple[1]),
                                      sampleSize: Int(triple[2])))
        }
        return ColorSamplerSet(items: items)
    }

    /// The same document holding exactly `set`. nil when the core refuses the
    /// list (more than 10, an entry off the canvas, a size other than 1/3/5)
    /// or it is the list the document already holds; callers validate first
    /// to say which.
    func withColorSamplers(_ set: ColorSamplerSet) throws -> RasterDocument? {
        var raw: [UInt32] = []
        for sampler in set.items {
            guard let x = UInt32(exactly: sampler.x), let y = UInt32(exactly: sampler.y),
                  let size = UInt32(exactly: sampler.sampleSize) else { return nil }
            raw += [x, y, size]
        }
        let count = set.items.count
        return try raw.withUnsafeBufferPointer { buffer in
            if let precision {
                return try wrapTyped(
                    dr_precision_doc_with_color_samplers(precision.ptr, buffer.baseAddress, count))
            }
            return wrap(dr_doc_with_color_samplers(ptr, buffer.baseAddress, count))
        }
    }
}
