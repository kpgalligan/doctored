import AppKit

// MARK: - Retained camera RAW and preserving host decoder intake

extension RasterDocument {
    static func fromRawRGBA16(_ samples: [UInt16], width: Int, height: Int,
                             profile: Data, layerName: String? = nil) throws -> RasterDocument {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              samples.count == width * height * 4 else {
            throw DoctoredCoreError(message: "RAW samples do not match their declared dimensions.")
        }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = samples.withUnsafeBufferPointer { pixels in
            profile.withUnsafeBytes { icc in
                func intake(_ name: UnsafePointer<CChar>?) -> OpaquePointer? {
                    dr_precision_doc_from_rgba16(pixels.baseAddress, pixels.count, UInt32(width), UInt32(height),
                        icc.bindMemory(to: UInt8.self).baseAddress, icc.count, name, context.limit,
                        context.otherRetained, nil, &error)
                }
                return layerName.map { $0.withCString(intake) } ?? intake(nil)
            }
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not preserve the RAW's 16-bit rendering."))
        }
        return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
    }

    /// The 32-bit float (linear) RAW development's intake: straight
    /// RGBA f32, linear light in `profile`'s primaries, as a one-layer 32-bit
    /// float document under the clip tone map. The core refuses a non-finite
    /// sample, an RGB magnitude above 65504 and an alpha outside 0…1; nothing
    /// is repaired.
    static func fromRawRGBA32(_ samples: [Float], width: Int, height: Int,
                             profile: Data, layerName: String? = nil) throws -> RasterDocument {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              samples.count == width * height * 4 else {
            throw DoctoredCoreError(message: "RAW samples do not match their declared dimensions.")
        }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = samples.withUnsafeBufferPointer { pixels in
            profile.withUnsafeBytes { icc in
                func intake(_ name: UnsafePointer<CChar>?) -> OpaquePointer? {
                    dr_precision_doc_from_rgba32(pixels.baseAddress, pixels.count, UInt32(width), UInt32(height),
                        icc.bindMemory(to: UInt8.self).baseAddress, icc.count, name, context.limit,
                        context.otherRetained, nil, &error)
                }
                return layerName.map { $0.withCString(intake) } ?? intake(nil)
            }
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not preserve the RAW's 32-bit float rendering."))
        }
        return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
    }

    /// A blank 16-bit document (File ▸ New… at 16 bits): the core fills
    /// one raster layer with the four straight RGBA16 samples in `fill`, so
    /// no `width·height` sample buffer is built here. A canvas too large to
    /// reopen at 16 bits throws the core's sentence verbatim.
    static func blankRGBA16(width: Int, height: Int, fill: [UInt16], profile: Data,
                            layerName: String) throws -> RasterDocument {
        let fallback = "Could not create a \(width) × \(height) 16-bit document."
        // The core reads exactly four samples; a shorter array would be read
        // past its end.
        guard fill.count == 4, width > 0, height > 0,
              let w = UInt32(exactly: width), let h = UInt32(exactly: height) else {
            throw DoctoredCoreError(message: fallback)
        }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = fill.withUnsafeBufferPointer { samples in
            profile.withUnsafeBytes { icc in
                layerName.withCString { name in
                    dr_precision_doc_create_blank(w, h, samples.baseAddress,
                        icc.bindMemory(to: UInt8.self).baseAddress, icc.count, name,
                        context.limit, context.otherRetained, &error)
                }
            }
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback))
        }
        return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
    }

    /// Source guards never depend on allocating/expanding recipe inspection.
    func hasRetainedSource(_ index: Int) -> Bool {
        guard isValidIndex(index) else { return false }
        if let precision { return dr_precision_doc_has_retained_source(precision.ptr, index) }
        return dr_doc_has_retained_source(ptr, index)
    }

    func hasRetainedRawSource(_ index: Int) -> Bool {
        guard isValidIndex(index) else { return false }
        if let precision { return dr_precision_doc_raw_source_length(precision.ptr, index, nil) > 0 }
        return dr_doc_raw_source_length(ptr, index, nil) > 0
    }

    func rawSourceBytes(_ index: Int) throws -> Data {
        guard isValidIndex(index) else { throw DoctoredCoreError(message: "Invalid RAW layer.") }
        var error: UnsafeMutablePointer<CChar>?
        let count = precision.map { dr_precision_doc_raw_source_length($0.ptr, index, &error) }
            ?? dr_doc_raw_source_length(ptr, index, &error)
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "RAW source read failed.")) }
        guard count > 0, count <= 256 * 1024 * 1024 else { throw DoctoredCoreError(message: "Retained RAW size is invalid.") }
        _ = try RasterResources.shared.admit(scratch: UInt64(count))
        var data = Data(count: count)
        let ok = data.withUnsafeMutableBytes { bytes in
            let output = bytes.bindMemory(to: UInt8.self).baseAddress
            if let precision { return dr_precision_doc_raw_source_copy(precision.ptr, index, output, count, &error) }
            return dr_doc_raw_source_copy(ptr, index, output, count, &error)
        }
        guard ok else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "RAW source copy failed.")) }
        return data
    }

    func attachingRawSource(_ layer: Int, bytes: Data? = nil, descriptor: String,
                            original: RasterDocument? = nil, originalLayer: Int = 0,
                            control: RasterJobControl? = nil) throws -> RasterDocument {
        guard isValidIndex(layer), (bytes != nil) != (original != nil),
              original == nil || (original?.isHighDepth == isHighDepth && originalLayer >= 0) else {
            throw DoctoredCoreError(message: "Invalid RAW source attachment.")
        }
        try precisionCString(descriptor, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let planJSON: UnsafeMutablePointer<CChar>?
        if let precision {
            if let original = original?.precision {
                planJSON = dr_precision_doc_raw_source_from_plan(precision.ptr, layer, original.ptr, originalLayer, descriptor, &error)
            } else {
                planJSON = dr_precision_doc_raw_source_plan(precision.ptr, layer, bytes?.count ?? 0, descriptor, &error)
            }
        } else if let original {
            planJSON = dr_doc_raw_source_from_plan(ptr, layer, original.ptr, originalLayer, descriptor, &error)
        } else {
            planJSON = dr_doc_raw_source_plan(ptr, layer, bytes?.count ?? 0, descriptor, &error)
        }
        let plan = try precisionJSON(planJSON, error)
        let report = try RasterResources.shared.report()
        let retained = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
        let own = try PrecisionDocumentDescriptor.integer(plan, "retained_bytes")
        let transient = try PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes")
        // Initial caller Data is represented by RawSourceSnapshot's payload
        // lease; From inventories both immutable document owners as one set.
        guard own <= retained else { throw DoctoredCoreError(message: "RAW source bytes are missing from resource accounting.") }
        let other = try PrecisionDocumentDescriptor.sum(retained - own, transient)
        let limit = RasterResources.shared.workingLimit
        let result: OpaquePointer?
        if let precision {
            if let original = original?.precision {
                result = dr_precision_doc_attach_raw_source_from(precision.ptr, layer, original.ptr, originalLayer,
                    descriptor, limit, other, control?.ptr, &error)
            } else {
                result = (bytes ?? Data()).withUnsafeBytes { input in
                    dr_precision_doc_attach_raw_source(precision.ptr, layer, input.bindMemory(to: UInt8.self).baseAddress,
                        input.count, descriptor, limit, other, control?.ptr, &error)
                }
            }
        } else if let original {
            result = dr_doc_attach_raw_source_from(ptr, layer, original.ptr, originalLayer,
                descriptor, limit, other, control?.ptr, &error)
        } else {
            result = (bytes ?? Data()).withUnsafeBytes { input in
                dr_doc_attach_raw_source(ptr, layer, input.bindMemory(to: UInt8.self).baseAddress,
                    input.count, descriptor, limit, other, control?.ptr, &error)
            }
        }
        guard let result else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "RAW attachment failed.")) }
        if isHighDepth { return try RasterDocument(precision: RasterPrecisionDocument(owning: result)) }
        return RasterDocument(owning: result)
    }
}

extension RasterDocument {
    /// Packs actual platform integer samples without any display-image bridge.
    /// The layout fixes true storage depth; alpha/orientation remain explicit.
    static func fromHostPixels(bytes: Data, width: Int, height: Int, rowBytes: Int,
                               layout: String, profile: Data, orientation: Int = 1,
                               control: RasterJobControl? = nil) throws -> RasterDocument {
        let byteLayouts: Set<String> = ["rgba8", "rgba8_premultiplied", "rgb8", "bgra8", "bgra8_opaque", "rgba8_opaque"]
        let typedLayouts: Set<String> = ["rgba16_le", "rgba16_le_premultiplied", "rgb16_le", "rgba16_le_opaque",
                                         "rgb10_packed_le", "argb16_be", "argb16_be_premultiplied",
                                         "argb16_be_opaque", "rgba16_le_left12", "rgba16_le_opaque_left12",
                                         "rgba16_le_premultiplied_left12"]
        guard byteLayouts.contains(layout) || typedLayouts.contains(layout),
              let width = UInt32(exactly: width), let height = UInt32(exactly: height),
              width > 0, height > 0, rowBytes > 0, (1...8).contains(orientation) else {
            throw DoctoredCoreError(message: "Unsupported host sample layout, orientation or extent.")
        }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = bytes.withUnsafeBytes { samples in
            profile.withUnsafeBytes { icc in
                if typedLayouts.contains(layout) {
                    return dr_precision_doc_from_host_pixels(samples.bindMemory(to: UInt8.self).baseAddress,
                        samples.count, width, height, rowBytes, layout, UInt32(orientation),
                        icc.bindMemory(to: UInt8.self).baseAddress, icc.count,
                        context.limit, context.otherRetained, control?.ptr, &error)
                }
                return dr_doc_from_host_pixels(samples.bindMemory(to: UInt8.self).baseAddress,
                    samples.count, width, height, rowBytes, layout, UInt32(orientation),
                    icc.bindMemory(to: UInt8.self).baseAddress, icc.count,
                    context.limit, context.otherRetained, control?.ptr, &error)
            }
        }
        guard let result else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not capture native host samples.")) }
        if typedLayouts.contains(layout) { return try RasterDocument(precision: RasterPrecisionDocument(owning: result)) }
        return RasterDocument(owning: result)
    }
}

/// The core's one-image cap: a still a document could not keep at its
/// depth is refused by the core's sentence before it is decoded.
enum HostImageCapacity {
    static func require(width: Int, height: Int, depth: Int) throws {
        guard let w = UInt32(exactly: width), let h = UInt32(exactly: height),
              let d = UInt32(exactly: depth) else {
            throw DoctoredCoreError(message: "This image is too large to keep.")
        }
        var err: UnsafeMutablePointer<CChar>? = nil
        guard dr_single_image_capacity(w, h, d, &err) else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "This image is too large to keep."))
        }
    }
}

/// The primary item's colour facts an ISO-BMFF/HEIF file states in its own
/// boxes: the first `colr`/`nclx` matrix associated with it (nil when
/// none) and whether an `iref` `prem` reference starts at it.
struct HeifPrimaryColorFacts: Equatable {
    let matrixCoefficients: Int?
    let premultipliedAlpha: Bool
}

/// Reads `HeifPrimaryColorFacts` through the core's bounded box reader; the
/// file is not decoded.
enum HeifContainer {
    static func primaryColorFacts(_ data: Data) throws -> HeifPrimaryColorFacts {
        var matrix: Int32 = -1
        var premultiplied = false
        var err: UnsafeMutablePointer<CChar>? = nil
        let ok = data.withUnsafeBytes { buffer in
            dr_heif_primary_color_facts(buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                &matrix, &premultiplied, &err)
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not read the image container."))
        }
        return HeifPrimaryColorFacts(matrixCoefficients: matrix < 0 ? nil : Int(matrix),
                                     premultipliedAlpha: premultiplied)
    }

    /// A copy of `data` whose primary item no longer has a `prem` reference:
    /// the bytes of that reference's `from_item_ID` field are set to
    /// 0xFF, so the reference names no item and nothing else in the file
    /// moves. nil when the file has no such reference. A file with a second
    /// one still reads as premultiplied afterwards; the caller checks.
    static func detachingPremultipliedReference(_ data: Data) throws -> Data? {
        var offset = 0
        var width = 0
        var err: UnsafeMutablePointer<CChar>? = nil
        let ok = data.withUnsafeBytes { buffer in
            dr_heif_premultiplied_reference(buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count,
                &offset, &width, &err)
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not read the image container."))
        }
        guard width > 0, offset >= 0, offset <= data.count - width else { return nil }
        var copy = Data(data)
        let start = copy.startIndex + offset
        copy.replaceSubrange(start..<start + width, with: [UInt8](repeating: 0xFF, count: width))
        return copy
    }
}

enum RasterSourceAttachmentKind: UInt32 {
    case none = 0, hostMedia = 1, rawLocalMasks = 2, other = 3
}

extension RasterDocument {
    func sourceAttachmentKind(_ layer: Int) -> RasterSourceAttachmentKind {
        guard isValidIndex(layer) else { return .none }
        let value = precision.map { dr_precision_doc_source_attachment_kind($0.ptr, layer) }
            ?? dr_doc_source_attachment_kind(ptr, layer)
        return RasterSourceAttachmentKind(rawValue: value) ?? .other
    }
    func hasSourceAttachment(_ layer: Int) -> Bool {
        guard isValidIndex(layer) else { return false }
        if let precision { return dr_precision_doc_has_source_attachment(precision.ptr, layer) }
        return dr_doc_has_source_attachment(ptr, layer)
    }

    func sourceAttachmentInfo(_ layer: Int) throws -> [String: Any]? {
        guard hasSourceAttachment(layer) else { return nil }
        let scratch = precision.map { dr_precision_doc_source_attachment_scratch_bytes($0.ptr, layer) }
            ?? dr_doc_source_attachment_scratch_bytes(ptr, layer)
        let lease = try PrecisionEditPayload.admitted(bytes: scratch)
        defer { withExtendedLifetime(lease) {} }
        var error: UnsafeMutablePointer<CChar>?
        let text = precision.map { dr_precision_doc_source_attachment_info($0.ptr, layer, &error) }
            ?? dr_doc_source_attachment_info(ptr, layer, &error)
        return try precisionJSON(text, error)
    }

}
