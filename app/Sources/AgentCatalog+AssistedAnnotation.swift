import Foundation

extension AgentServer {
    /// The five assisted annotation tools. Behind its own non-inlining
    /// boundary like every other catalog group, so Release WMO never sees one
    /// giant literal. Names must match the `handlers` keys exactly — the
    /// parity check at server start (Debug and Release) enforces it.
    @inline(never)
    func assistedAnnotationCatalog() -> [[String: Any]] {
        let workspace: [String: Any] = [
            "type": "string",
            "description": "Absolute path of the prepared workspace directory (the one holding manifest.json, cases/ and replays/).",
        ]
        let caseID: [String: Any] = [
            "type": "string",
            "description": "Anonymous case id from assisted_workspace_status. Omit for the first case with no replay.",
        ]
        let synthetic: [String: Any] = [
            "type": "boolean",
            "description": "Drive the case synthetically (default false). A synthetic session records annotator_id null, origin \"synthetic\" and scoreable false, and never contributes a measurement. A human session refuses every agent stroke and agent finalize.",
        ]
        let tool: [String: Any] = [
            "type": "string", "enum": ["foreground", "background", "unknown"],
            "description": "The categorical label this stroke deposits. Unknown RELEASES a constraint rather than adding one.",
        ]
        let diameter: [String: Any] = [
            "type": "integer", "enum": AssistedAnnotation.diameters,
            "description": "Brush diameter in native pixels; only these four are offered.",
        ]
        let points: [String: Any] = [
            "type": "array", "minItems": 1,
            "description": "Polyline samples [[x,y],…] in canvas pixels, clamped to the canvas. More than 256 samples are decimated to 256 keeping the first and last; the recorded points are exactly what is rasterized.",
            "items": ["type": "array", "minItems": 2, "maxItems": 2,
                      "items": ["type": "integer"]] as [String: Any],
        ]
        return [
            self.tool(
                "assisted_workspace_status",
                "Internal experiment: refuses unless assisted annotation is enabled for this launch (DR_ASSISTED_ANNOTATION=1, or the AssistedAnnotationEnabled default). The cases in a prepared assisted annotation workspace and which still have no replay. No document is involved.",
                ["workspace": workspace], required: ["workspace"]),
            self.tool(
                "assisted_open_case",
                "Internal experiment: refuses unless assisted annotation is enabled for this launch (DR_ASSISTED_ANNOTATION=1, or the AssistedAnnotationEnabled default). Mirrors File > Open Assisted Annotation Case…, which only such a launch shows. Opens one case in the gated annotation window; a human session stops at the task card and a synthetic session starts its 5-minute, 20-gesture budget at once. Refuses a locked case or a live session.",
                ["workspace": workspace, "case": caseID, "synthetic": synthetic],
                required: ["workspace"]),
            self.tool(
                "assisted_session_state",
                "Internal experiment: refuses unless assisted annotation is enabled for this launch (DR_ASSISTED_ANNOTATION=1, or the AssistedAnnotationEnabled default). The open annotation session's elapsed time, gesture budget, applied strokes, recorded events and current label-plane hash.",
                [:]),
            self.tool(
                "assisted_synthetic_stroke",
                "Internal experiment: refuses unless assisted annotation is enabled for this launch (DR_ASSISTED_ANNOTATION=1, or the AssistedAnnotationEnabled default). One brush gesture on a SYNTHETIC session. accepted:false with result \"rejected:time_budget\" or \"rejected:rasterizer_limit\" is a recorded refusal that still spent a gesture and left the label plane unchanged.",
                ["tool": tool, "diameter": diameter, "points": points],
                required: ["tool", "diameter", "points"]),
            self.tool(
                "assisted_finalize_case",
                "Internal experiment: refuses unless assisted annotation is enabled for this launch (DR_ASSISTED_ANNOTATION=1, or the AssistedAnnotationEnabled default). Mirrors the annotation window's Submit button for a synthetic session: writes <workspace>/replays/<case_id>.json and locks the case.",
                [:]),
        ]
    }
}
