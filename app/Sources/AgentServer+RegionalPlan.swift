import Foundation

/// MCP `render`'s regional plan (13B-7): a render with `region` or
/// `preview_divisor` also returns, as a second text item, the plan the core's
/// admission makes for that window (DoctoredCore+Regional.swift) —
/// `{"regional_plan": {…}}` with its top-level keys (`global_analysis`,
/// `required_input_region`, `neighborhood`, `stages`, `preview_only`,
/// `scratch_heap_bytes`, `work_units`, `depth` and the rest) and without the
/// per-layer `layers` rows, which would swamp a vision reply. Like the
/// regional render it mirrors no menu item: the canvas display still renders
/// the whole projection.
extension AgentServer {
    /// The plan item for `region`, read BEFORE the render so a refused stack
    /// surfaces once, with the render's own sentence.
    static func regionalPlanItem(_ doc: RasterDocument, region: RegionRenderRequest) throws
        -> [String: Any]
    {
        var plan = try doc.renderRegionPlan(
            region, maxOutputBytes: RasterResources.shared.workingLimit)
        plan.removeValue(forKey: "layers")
        let data = try JSONSerialization.data(
            withJSONObject: ["regional_plan": plan], options: [.sortedKeys])
        return ["type": "text", "text": String(decoding: data, as: UTF8.self)]
    }
}
