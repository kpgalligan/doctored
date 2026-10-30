import Foundation

/// Edit ▸ Auto-Blend Layers: the core's plan and edit at every depth
/// (`dr_doc_auto_blend*` at 8 bits, `dr_precision_doc_auto_blend*` at 16 bits
/// and 32-bit float). The only callers of those four exports; the job that
/// runs them is `AutoBlendWork` (AutoBlend.swift).
extension RasterDocument {
    /// The core's plan for `request` (`AutoBlend.command`): `retained_bytes`,
    /// `scratch_bytes`, `work_units` and `merged_index`. Throws the core's
    /// sentence for every refusal the edit would give. Reads in place, so it
    /// is safe on a worker.
    func autoBlendPlan(request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(
                dr_precision_doc_auto_blend_plan(precision.ptr, request, &error), error)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for Auto-Blend Layers.")
        }
        return try precisionJSON(dr_doc_auto_blend_plan(ptr, request, &error), error)
    }

    /// The blended document and the core's report (`layers`, `covered_pixels`,
    /// `canvas_pixels`, `merged_layer`). `budget` is the byte working-memory
    /// context; nil reads `legacyPrecisionWorkContext()`, which is main-only,
    /// so an off-main byte caller MUST pass it. Ignored for a typed document.
    /// A cancel throws the core's `operation cancelled`.
    func autoBlending(request: String, budget: (limit: UInt64, other: UInt64)?,
                      control: RasterJobControl?) throws
        -> (document: RasterDocument, report: [String: Any])
    {
        try precisionCString(request, maximumBytes: 64 * 1024)
        let refused = "Auto-Blend Layers was refused."
        var error: UnsafeMutablePointer<CChar>?
        var report: UnsafeMutablePointer<CChar>?
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            guard let result = dr_precision_doc_auto_blend(
                precision.ptr, request, context.limit, context.otherRetained, control?.ptr,
                &report, &error) else {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: refused))
            }
            let owner = RasterPrecisionDocument(owning: result)
            let fields = try precisionJSON(report, nil)
            return (try RasterDocument(precision: owner), fields)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for Auto-Blend Layers.")
        }
        let budget = try budget ?? legacyPrecisionWorkContext()
        guard let document = wrap(dr_doc_auto_blend(
            ptr, request, budget.limit, budget.other, control?.ptr, &report, &error)) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: refused))
        }
        return (document, try precisionJSON(report, nil))
    }
}
