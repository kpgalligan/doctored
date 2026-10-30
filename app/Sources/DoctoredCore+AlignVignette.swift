import Foundation

/// Edit ▸ Auto-Align Layers' Vignette Removal
/// (`ffi_align_vignette.rs`): the `remove_vignette` plan and edit at both
/// depths. Both take the budget read on main (`meshWarpBudget`), so both may
/// run on the worker.
extension RasterDocument {
    /// `{"retained_bytes","scratch_bytes","output_bytes","coefficient",
    /// "gain","samples","layers"}` for a `remove_vignette` request; it runs
    /// the fit. Throws the core's refusal.
    func removeVignettePlan(request: String, budget: (limit: UInt64, other: UInt64)) throws
        -> [String: Any]
    {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_remove_vignette_plan(
                precision.ptr, request, budget.limit, budget.other, &error), error)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for \(AutoAlign.title).")
        }
        return try precisionJSON(dr_doc_remove_vignette_plan(ptr, request, budget.limit,
                                                             budget.other, &error), error)
    }

    /// The `remove_vignette` edit under `budget`: the document with the
    /// fitted vignette divided out of every listed layer, or nil when no
    /// sample changed. Throws the core's refusal or `operation cancelled`.
    func editingRemoveVignette(request: String, budget: (limit: UInt64, other: UInt64),
                               control: RasterJobControl?) throws -> RasterDocument? {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let fallback = "\(AutoAlign.title) could not remove the vignette."
        if let precision {
            let result = dr_precision_doc_remove_vignette(precision.ptr, request, budget.limit,
                                                          budget.other, control?.ptr, &error)
            guard error == nil else {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback))
            }
            guard let result else { return nil }
            return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for \(AutoAlign.title).")
        }
        let result = dr_doc_remove_vignette(ptr, request, budget.limit, budget.other,
                                            control?.ptr, &error)
        guard error == nil else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback))
        }
        guard let result else { return nil }
        guard let document = wrap(result) else {
            throw DoctoredCoreError(message: fallback)
        }
        return document
    }
}
