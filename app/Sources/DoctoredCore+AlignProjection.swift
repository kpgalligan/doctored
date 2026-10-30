import Foundation

/// Edit ▸ Auto-Align Layers' Cylindrical and Spherical reprojection
/// (`ffi_align_projection.rs`): one layer's `project_layer` plan
/// and edit at both depths. Both take the budget read on main
/// (`meshWarpBudget`), so both may run on the worker.
extension RasterDocument {
    /// `{"retained_bytes","scratch_bytes","rect","vertices","output_bytes"}`
    /// for a `project_layer` request, `rect` the projected layer's and
    /// `output_bytes` the new layer the result holds; throws the core's
    /// refusal.
    func projectLayerPlan(request: String, budget: (limit: UInt64, other: UInt64)) throws
        -> [String: Any]
    {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_project_layer_plan(
                precision.ptr, request, budget.limit, budget.other, &error), error)
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for \(AutoAlign.title).") }
        return try precisionJSON(dr_doc_project_layer_plan(ptr, request, budget.limit, budget.other,
                                                           &error), error)
    }

    /// The `project_layer` edit under `budget`: the document with the layer
    /// reprojected. Throws the core's refusal or `operation cancelled`.
    func editingProjectLayer(request: String, budget: (limit: UInt64, other: UInt64),
                             control: RasterJobControl?) throws -> RasterDocument {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let fallback = "\(AutoAlign.title) could not project the layer."
        if let precision {
            let result = dr_precision_doc_project_layer(precision.ptr, request, budget.limit,
                                                        budget.other, control?.ptr, &error)
            guard error == nil, let result else {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback))
            }
            return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for \(AutoAlign.title).") }
        let result = dr_doc_project_layer(ptr, request, budget.limit, budget.other, control?.ptr,
                                          &error)
        guard error == nil, let document = wrap(result) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback))
        }
        return document
    }
}
