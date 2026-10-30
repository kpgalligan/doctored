import Foundation

// MARK: - Merge to HDR
//
// The only `dr_*merge_hdr*` calls. Each dispatches on `precision`, as `geometryEditPlan` does: an
// 8-bit document goes through `dr_doc_merge_hdr*` and a 16-bit one through
// `dr_precision_doc_merge_hdr*`; a 32-bit float document is refused by the core, naming the layer.
// The request JSON is `MergeHDR.requestJSON`'s (`core/include/doctored_core.h` holds the
// contract). No call changes this document.
extension RasterDocument {
    /// The relative exposures of `layers`, estimated from their pixels: one
    /// EV per layer relative to the first, or `.unknown` with the core's
    /// sentence when two frames share too few well-exposed pixels (a reply,
    /// not an error). Throws the core's refusal for a layer that is not an
    /// 8- or 16-bit raster.
    func mergeHDRExposures(layers: [Int], control: RasterJobControl? = nil) throws
        -> MergeHDR.Estimate
    {
        let request = try MergeHDR.requestJSON(layers: layers)
        try precisionCString(request, maximumBytes: MergeHDR.requestMaximumBytes)
        var error: UnsafeMutablePointer<CChar>?
        let reply: [String: Any]
        if let precision {
            reply = try precisionJSON(dr_precision_doc_merge_hdr_exposures(
                precision.ptr, request, control?.ptr, &error), error)
        } else {
            guard let ptr else {
                throw DoctoredCoreError(message: "A document is required to merge to HDR.")
            }
            reply = try precisionJSON(
                dr_doc_merge_hdr_exposures(ptr, request, control?.ptr, &error), error)
        }
        if let evs = reply["evs"] as? [NSNumber], evs.count == layers.count {
            return .evs(evs.map(\.doubleValue))
        }
        guard let reason = reply["reason"] as? String else {
            throw DoctoredCoreError(message: "The exposure estimate could not be read.")
        }
        return .unknown(reason: reason)
    }

    /// The merge's plan for `request`: `{width, height, frames, output_bytes,
    /// scratch_bytes}`, after every refusal the merge gives before it
    /// allocates.
    func mergeHDRPlan(request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: MergeHDR.requestMaximumBytes)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(
                dr_precision_doc_merge_hdr_plan(precision.ptr, request, &error), error)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required to merge to HDR.")
        }
        return try precisionJSON(dr_doc_merge_hdr_plan(ptr, request, &error), error)
    }

    /// The layers `request` names merged into a NEW 32-bit float document,
    /// with the core's report (`{reference, all_clipped_pixels,
    /// all_dark_pixels, deghosted_pixels, noise_sigma}`). `budget` is the byte
    /// working-memory context; nil reads `legacyPrecisionWorkContext()`, which
    /// is main-only, so an off-main byte caller MUST pass it
    /// (`editingGeometry`'s rule). Ignored for a typed document.
    func mergingHDR(request: String, budget: (limit: UInt64, other: UInt64)? = nil,
                    control: RasterJobControl? = nil) throws
        -> (document: RasterDocument, report: [String: Any])
    {
        try precisionCString(request, maximumBytes: MergeHDR.requestMaximumBytes)
        var error: UnsafeMutablePointer<CChar>?
        var report: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer?
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            result = dr_precision_doc_merge_hdr(
                precision.ptr, request, context.limit, context.otherRetained, control?.ptr,
                &report, &error)
        } else {
            guard let ptr else {
                throw DoctoredCoreError(message: "A document is required to merge to HDR.")
            }
            let budget = try budget ?? legacyPrecisionWorkContext()
            result = dr_doc_merge_hdr(
                ptr, request, budget.limit, budget.other, control?.ptr, &report, &error)
        }
        guard let result else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "\(MergeHDR.title) failed."))
        }
        let merged = RasterPrecisionDocument(owning: result)
        let parsed = try precisionJSON(report, nil)
        return (try RasterDocument(precision: merged), parsed)
    }
}
