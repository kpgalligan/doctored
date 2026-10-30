import Foundation

/// Edit ▸ Auto-Align Layers' Geometric Distortion
/// (`ffi_align_distortion.rs`): the `fit_distortion` plan and fit at both
/// depths. Neither changes the document. Both take the budget read on main
/// (`meshWarpBudget`), so both may run on the worker.
extension RasterDocument {
    /// `{"retained_bytes","scratch_bytes"}` for a `fit_distortion` request;
    /// it checks every listed layer and runs no matching. Throws the core's
    /// refusal.
    func fitDistortionPlan(request: String, budget: (limit: UInt64, other: UInt64)) throws
        -> [String: Any]
    {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_fit_distortion_plan(
                precision.ptr, request, budget.limit, budget.other, &error), error)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for \(AutoAlign.title).")
        }
        return try precisionJSON(dr_doc_fit_distortion_plan(ptr, request, budget.limit,
                                                            budget.other, &error), error)
    }

    /// The `fit_distortion` fit under `budget`: `{"coefficient","matches",
    /// "residual","layers","maps"}`, each map a layer's corrected canvas
    /// point to the reference's. Throws the core's refusal or `operation
    /// cancelled`.
    func fittingDistortion(request: String, budget: (limit: UInt64, other: UInt64),
                           control: RasterJobControl?) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_fit_distortion(
                precision.ptr, request, budget.limit, budget.other, control?.ptr, &error), error)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for \(AutoAlign.title).")
        }
        return try precisionJSON(dr_doc_fit_distortion(ptr, request, budget.limit, budget.other,
                                                       control?.ptr, &error), error)
    }
}
