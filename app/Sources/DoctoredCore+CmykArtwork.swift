import AppKit

// Type, shape and fill layers on a CMYK document (#1): the two `dr_*` wrappers of
// `core/src/ffi_cmyk_artwork.rs` — the ink publication and the colour leg a description's numbers
// cross the CMYK boundary through. Their callers are `CmykArtwork.swift`'s.

enum RasterCmykArtworkCore {
    /// `dr_cmyk_artwork_colors`: `values` from one end to the other under `options`' intent and
    /// black-point compensation. An sRGB end is `[r, g, b]`, a CMYK end `[c, m, y, k]`, each 0…1.
    static func colors(_ values: [[Double]], from: ArtworkColorEnd, to: ArtworkColorEnd,
                       options: CmykConversionOptions) throws -> [[Double]] {
        guard !values.isEmpty else { return [] }
        let request: [String: Any] = [
            "values": values, "intent": options.intent.rawValue,
            "black_point_compensation": options.blackPointCompensation,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: request),
              let text = String(data: data, encoding: .utf8) else {
            throw DoctoredCoreError(message: "Could not encode the artwork colour request.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = from.profile.withUnsafeBytes { source in
            to.profile.withUnsafeBytes { destination in
                dr_cmyk_artwork_colors(
                    source.bindMemory(to: UInt8.self).baseAddress, source.count,
                    destination.bindMemory(to: UInt8.self).baseAddress, destination.count, text, &error)
            }
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert the artwork colours."))
        }
        defer { dr_string_free(result) }
        guard let reply = try? JSONSerialization.jsonObject(with: Data(String(cString: result).utf8))
                as? [String: Any],
              let answered = reply["values"] as? [[NSNumber]], answered.count == values.count else {
            throw DoctoredCoreError(message: "The artwork colour reply is malformed.")
        }
        return answered.map { $0.map(\.doubleValue) }
    }
}

extension RasterPrecisionDocument {
    /// `dr_precision_doc_cmyk_artwork`: assembles the ink `components` over `samples` (coverage
    /// planes, component-major) and `intrinsic` (RGBA8 sRGB glyph planes) into a CMYK raster and
    /// publishes it with `description`. `replacing` rewrites `layer`; otherwise `layer` is the
    /// anchor and `position` the slot. nil when a replacement changes nothing.
    func publishingCmykArtwork(layer: Int, replacing: Bool, position: LayerInsertPosition,
                               width: Int, height: Int, components: String, samples: [UInt16],
                               intrinsic: [UInt8], x: Int, y: Int, name: String, description: String,
                               control: RasterJobControl?) throws -> RasterPrecisionDocument? {
        guard layer >= 0, width > 0, height > 0, let x = Int32(exactly: x), let y = Int32(exactly: y),
              let w = UInt32(exactly: width), let h = UInt32(exactly: height) else {
            throw DoctoredCoreError(message: "Artwork target or position is invalid.")
        }
        try precisionCString(components, maximumBytes: NativeArtwork.maximumDescriptionBytes)
        try precisionCString(name, maximumBytes: 64 * 1024)
        try precisionCString(description, maximumBytes: NativeArtwork.maximumDescriptionBytes)
        let context = try PrecisionWorkContext(source: self)
        var error: UnsafeMutablePointer<CChar>?
        let result = samples.withUnsafeBufferPointer { values in
            intrinsic.withUnsafeBufferPointer { glyphs in
                dr_precision_doc_cmyk_artwork(ptr, layer, replacing, position.code, w, h, components,
                    values.baseAddress, values.count, glyphs.baseAddress, glyphs.count, x, y, name,
                    description, context.limit, context.otherRetained, control?.ptr, &error)
            }
        }
        if let error {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not publish CMYK artwork."))
        }
        return result.map { RasterPrecisionDocument(owning: $0) }
    }
}
