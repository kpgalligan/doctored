import Foundation

/// The regional render's PLAN (13B-7, `ffi_regional.rs`): the admission
/// `renderRegion` runs, allocating no pixels, as the core's JSON
/// (`RegionalPlan::json`'s contract). MCP `render` reports it beside a
/// regional image (AgentServer+RegionalPlan.swift).
extension RasterDocument {
    /// The plan of the regional render `render` would run for `request`: the
    /// typed one (`dr_precision_doc_render_region_plan`, CMYK included) for a
    /// high-depth or colour-mode document, else the 8-bit one against
    /// `maxOutputBytes` (`dr_doc_render_region_plan`). Throws the render's
    /// own refusal.
    func renderRegionPlan(_ request: RegionRenderRequest, maxOutputBytes: UInt64) throws
        -> [String: Any]
    {
        try request.validate()
        let (x, y, w, h, d) = (UInt32(request.x), UInt32(request.y), UInt32(request.width),
                               UInt32(request.height), UInt32(request.previewDivisor))
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(
                dr_precision_doc_render_region_plan(precision.ptr, x, y, w, h, d, &error), error)
        }
        guard let ptr else { throw DoctoredCoreError(message: "Could not plan the region.") }
        return try precisionJSON(
            dr_doc_render_region_plan(ptr, x, y, w, h, d, maxOutputBytes, &error), error)
    }
}
