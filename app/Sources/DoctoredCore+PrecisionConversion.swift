import AppKit

// MARK: - Deliberate document precision conversion and authoritative statistics
extension RasterDocument {
    func promoteFlattenedPlan() throws -> [String: Any] {
        guard let ptr else { throw DoctoredCoreError(message: "This document already stores high-depth pixels.") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_promote_flattened_plan(ptr, &error)
        return try precisionJSON(result, error)
    }

    func promotedFlattened(control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        guard let ptr else { throw DoctoredCoreError(message: "This document already stores high-depth pixels.") }
        let budget = try legacyPrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_promote_flattened(ptr, budget.limit, budget.other, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "High-depth promotion failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The structure-preserving 8→16 plan: preserved, discarded, per-record
    /// rendering differences and blocked rows (`precision_conversion_structure`).
    func promoteStructurePlan() throws -> [String: Any] {
        guard let ptr else { throw DoctoredCoreError(message: "This document already stores high-depth pixels.") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_promote_structure_plan(ptr, &error)
        return try precisionJSON(result, error)
    }

    /// Every supported record carried into a 16-bit document; refuses with
    /// the plan's first blocked row before any plane is promoted.
    func promotedStructure(control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        guard let ptr else { throw DoctoredCoreError(message: "This document already stores high-depth pixels.") }
        let budget = try legacyPrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_promote_structure(ptr, budget.limit, budget.other, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Structure-preserving promotion failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The 8→float plan: the 8→16 rows, the float rows over its typed shape
    /// and the capacity row (`precision_conversion_float`).
    func promoteToFloatPlan() throws -> [String: Any] {
        guard let ptr else { throw DoctoredCoreError(message: "This document already stores high-depth pixels.") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_promote_to_float_plan(ptr, &error)
        return try precisionJSON(result, error)
    }

    /// Every supported record carried into a 32-bit float document, each
    /// byte widened ×257 and decoded through the profile's TRC; refuses with
    /// the plan's first blocked row before any plane is built.
    func promotedToFloat(control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        guard let ptr else { throw DoctoredCoreError(message: "This document already stores high-depth pixels.") }
        let budget = try legacyPrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_promote_to_float(ptr, budget.limit, budget.other, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Conversion to 32-bit float failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Per-channel |a − b| of this byte projection ×257 against the converted
    /// document's u16 render, in codes of 65535.
    func promoteStructurePreview(_ converted: RasterPrecisionDocument,
                                 control: RasterJobControl? = nil) throws -> [String: Any] {
        guard let ptr else { throw DoctoredCoreError(message: "This document already stores high-depth pixels.") }
        let context = try PrecisionWorkContext(source: converted)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_promote_structure_preview(ptr, converted.ptr, context.limit,
                                                      context.otherRetained, control?.ptr, &error)
        return try precisionJSON(result, error)
    }

    static func precisionCapability(operation: String, target: String, depth: UInt32 = 16) throws -> [String: Any] {
        try precisionCString(operation, maximumBytes: 64)
        try precisionCString(target, maximumBytes: 64)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_capability(operation, target, depth, &error)
        return try precisionJSON(result, error)
    }
}

extension RasterPrecisionDocument {
    func downconvertPlan() throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_downconvert_plan(ptr, &error)
        return try precisionJSON(result, error)
    }

    func downconverted(dither: UInt32, workingLimit: UInt64, otherRetained: UInt64,
                       control: RasterJobControl? = nil) throws -> RasterDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_downconvert(ptr, dither, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Eight-bit conversion failed."))
        }
        return RasterDocument(owning: result)
    }

    /// The structure-preserving NARROWING plan (16→8, float→16, float→8;
    /// `precision_conversion_narrow`). `targetDepth` is 8 or 16.
    func narrowStructurePlan(targetDepth: UInt32) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_narrow_structure_plan(ptr, targetDepth, &error)
        return try precisionJSON(result, error)
    }

    /// Every supported record carried into an 8-bit document, RGB dithered
    /// per record (0 none / 1 ordered), alpha nearest.
    func narrowedStructureToU8(dither: UInt32, workingLimit: UInt64, otherRetained: UInt64,
                               control: RasterJobControl? = nil) throws -> RasterDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_narrow_structure_to_u8(
            ptr, dither, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Structure-preserving 8-bit conversion failed."))
        }
        return RasterDocument(owning: result)
    }

    /// A float document's every supported record carried into 16 bits,
    /// each raster tone-mapped per record.
    func narrowedStructureToU16(workingLimit: UInt64, otherRetained: UInt64,
                                control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_narrow_structure_to_u16(
            ptr, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Structure-preserving 16-bit conversion failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The 16→float plan (`precision_conversion_float`); a float document
    /// throws.
    func promoteToFloatPlan() throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_promote_to_float_plan(ptr, &error)
        return try precisionJSON(result, error)
    }

    /// Every supported record carried into a 32-bit float document, each
    /// u16 code decoded through the profile's TRC to linear light.
    func promotedToFloat(workingLimit: UInt64, otherRetained: UInt64,
                         control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_promote_to_float(
            ptr, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Conversion to 32-bit float failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Per-channel |a − b| of this document's render (tone-mapped when
    /// float) against the narrowed document's, in codes of 65535.
    func narrowStructurePreview(_ converted: RasterDocument, workingLimit: UInt64, otherRetained: UInt64,
                                control: RasterJobControl? = nil) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_narrow_structure_preview(
            ptr, converted.precision == nil ? converted.ptr : nil, converted.precision?.ptr,
            workingLimit, otherRetained, control?.ptr, &error)
        return try precisionJSON(result, error)
    }
}

extension RasterPrecisionImage {
    /// Explicit export/display pixels. This image never replaces a typed master.
    func quantized(dither: UInt32, workingLimit: UInt64, otherRetained: UInt64,
                   control: RasterJobControl? = nil) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_quantize(ptr, dither, workingLimit, otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Eight-bit pixel conversion failed."))
        }
        return RasterImage(owning: result)
    }

    func precisionSample(x: Int, y: Int, reach: Int = 0) throws -> [String: Any] {
        guard (Int(Int32.min)...Int(Int32.max)).contains(x), (Int(Int32.min)...Int(Int32.max)).contains(y),
              (0...2).contains(reach) else { throw DoctoredCoreError(message: "Invalid high-depth sample position or reach.") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_image_sample(ptr, Int32(x), Int32(y), UInt32(reach), &error)
        return try precisionJSON(result, error)
    }
}
