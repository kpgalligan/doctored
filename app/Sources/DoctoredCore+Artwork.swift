import AppKit

// `publishingArtwork` gained `position:` and calls the `_at` exports when it is NOT replacing, so
// the typed artwork path lands above the active entry instead of appending a top root, and
// `addingDescribedLayer(above:)` passes `.above`.

// MARK: - Retained native artwork
enum RasterArtworkCore {
    /// `evenOdd` fills by the even-odd rule, nonzero winding otherwise (`dr_artwork_path_coverage`).
    static func coverage(commands: [UInt8], points: [Double], width: Int, height: Int,
                         evenOdd: Bool = false,
                         control: RasterJobControl? = nil) throws -> [UInt16] {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              points.count % 2 == 0, points.count / 2 <= 262_144, commands.count <= 262_144 else {
            throw DoctoredCoreError(message: "Artwork path exceeds its geometry limits.")
        }
        let context = try PrecisionWorkContext()
        _ = try RasterResources.shared.admit(scratch: UInt64(width * height) * 2)
        var output = [UInt16](repeating: 0, count: width * height)
        var error: UnsafeMutablePointer<CChar>?
        let success = commands.withUnsafeBufferPointer { verbs in
            points.withUnsafeBufferPointer { coordinates in
                output.withUnsafeMutableBufferPointer { samples in
                    dr_artwork_path_coverage(verbs.baseAddress, verbs.count, coordinates.baseAddress,
                        coordinates.count / 2, UInt32(width), UInt32(height), evenOdd, samples.baseAddress,
                        samples.count, context.limit, context.otherRetained, control?.ptr, &error)
                }
            }
        }
        guard success else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not generate native artwork coverage.")) }
        return output
    }

    /// `float` renders the 32-bit float twin (`dr_artwork_render_float`):
    /// f32 linear light, the colour numbers taken as linear.
    static func render(width: Int, height: Int, components: String, samples: [UInt16],
                       intrinsic: [UInt8], profile: Data, float: Bool = false,
                       control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        try precisionCString(components, maximumBytes: NativeArtwork.maximumDescriptionBytes)
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "Invalid artwork dimensions.")
        }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = samples.withUnsafeBufferPointer { values in
            intrinsic.withUnsafeBufferPointer { glyphs in
                profile.withUnsafeBytes { bytes in
                    float
                        ? dr_artwork_render_float(UInt32(width), UInt32(height), components,
                            values.baseAddress, values.count, glyphs.baseAddress, glyphs.count,
                            bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, context.limit,
                            context.otherRetained, control?.ptr, &error)
                        : dr_artwork_render(UInt32(width), UInt32(height), components, values.baseAddress, values.count,
                            glyphs.baseAddress, glyphs.count, bytes.bindMemory(to: UInt8.self).baseAddress,
                            bytes.count, context.limit, context.otherRetained, control?.ptr, &error)
                }
            }
        }
        guard let result else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not assemble native artwork.")) }
        return RasterPrecisionImage(owning: result)
    }
}

extension RasterDocument {
    /// Publishes rendered artwork pixels and their description together.
    /// `replacing` rewrites `layer`'s own pixels; otherwise `layer` is the
    /// placement ANCHOR and `position` names the slot (12E-2b, the core's one
    /// rule in `core/src/layer_insert.rs`). The default `.above` is the UI's
    /// decision — above the entry the user has selected.
    func publishingArtwork(_ image: RasterPrecisionImage, layer: Int, replacing: Bool,
                           position: LayerInsertPosition = .above,
                           x: Int, y: Int, name: String, description: String,
                           control: RasterJobControl? = nil) throws -> RasterDocument? {
        guard isValidIndex(layer), let x = Int32(exactly: x), let y = Int32(exactly: y) else {
            throw DoctoredCoreError(message: "Artwork target or position is invalid.")
        }
        try precisionCString(name, maximumBytes: 64 * 1024)
        try precisionCString(description, maximumBytes: NativeArtwork.maximumDescriptionBytes)
        let report = try RasterResources.shared.report()
        let own = try Self.mixedResourceReport(documents: [self], images: [],
            precisionDocuments: precision.map { [$0] } ?? [], precisionImages: [image], scratch: 0, limit: 0)
        let allBytes = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
        let ownBytes = try PrecisionDocumentDescriptor.integer(own, "unique_retained_bytes")
        guard ownBytes <= allBytes else { throw DoctoredCoreError(message: "Artwork source is missing from resource accounting.") }
        let other = try PrecisionDocumentDescriptor.sum(allBytes - ownBytes,
            PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes"))
        let limit = RasterResources.shared.workingLimit
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer?
        if let precision {
            result = replacing
                ? dr_precision_doc_artwork(precision.ptr, layer, true, image.ptr, x, y,
                    name, description, limit, other, control?.ptr, &error)
                : dr_precision_doc_artwork_at(precision.ptr, layer, position.code, image.ptr, x, y,
                    name, description, limit, other, control?.ptr, &error)
        } else {
            result = replacing
                ? dr_doc_artwork(ptr, layer, true, image.ptr, x, y,
                    name, description, limit, other, control?.ptr, &error)
                : dr_doc_artwork_at(ptr, layer, position.code, image.ptr, x, y,
                    name, description, limit, other, control?.ptr, &error)
        }
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not publish retained artwork.")) }
        guard let result else { return nil }
        if isHighDepth { return try RasterDocument(precision: RasterPrecisionDocument(owning: result)) }
        return RasterDocument(owning: result)
    }
}
