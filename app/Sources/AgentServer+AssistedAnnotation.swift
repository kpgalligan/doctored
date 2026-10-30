import AppKit

/// The assisted annotation tools (internal Phase 9I experiment):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `assistedWorkspaceStatus` | the case list the File ▸ Open Assisted Annotation Case… panel picks from |
/// | `assistedOpenCase` | File ▸ Open Assisted Annotation Case…, up to the task card |
/// | `assistedSessionState` | the annotation window's own time/gesture readout |
/// | `assistedSyntheticStroke` | one brush drag on the annotation canvas |
/// | `assistedFinalizeCase` | the annotation window's Submit button |
///
/// NO `document_id`: an annotation session is not an `ImageDocument`, and the
/// window holds its own `RasterImage`.
///
/// BLINDING IS ENFORCED HERE, NOT ONLY DOCUMENTED. A HUMAN session refuses a
/// synthesized stroke and an agent finalize outright: a replay that claims
/// human origin must record the annotator's own gestures, and an agent that
/// could drive one would make every human replay unfalsifiable. A synthetic
/// session never presents the task card, records `annotator_id: null` and is
/// marked `scoreable: false` — it exists to exercise the pipeline, never to
/// contribute a measurement.
///
/// Every handler refuses with `AssistedAnnotation.disabledMessage` when the
/// flag is off, so an ordinary build exposes the tools but can do nothing.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let assistedAnnotationHandlers: [String: ToolHandler] = [
        // Assisted annotation, the internal Phase 9I experiment
        // (AgentServer+AssistedAnnotation.swift). No document_id: the
        // annotation window owns its own image.
        "assisted_workspace_status": { $0.assistedWorkspaceStatus },
        "assisted_open_case": { $0.assistedOpenCase },
        "assisted_session_state": { $0.assistedSessionState },
        "assisted_synthetic_stroke": { $0.assistedSyntheticStroke },
        "assisted_finalize_case": { $0.assistedFinalizeCase },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let assistedAnnotationToolFacts: [String: ToolFacts] = [
        // Recording sweep: `get_warp` only reads. The five assisted-annotation
        // tools drive their own gated study window over an external
        // workspace — they read it, open a case, draw in the session or
        // write its replay file — and edit no document an action runs on.
        "assisted_workspace_status": [.nonRecordable, .documentFree],
        "assisted_open_case": [.nonRecordable, .documentFree],
        "assisted_session_state": [.nonRecordable, .documentFree],
        "assisted_synthetic_stroke": [.nonRecordable, .documentFree],
        "assisted_finalize_case": [.nonRecordable, .documentFree],
    ]

    private func requireAssistedAnnotation() throws {
        guard AssistedAnnotation.isEnabled else {
            throw ToolError(message: AssistedAnnotation.disabledMessage)
        }
    }

    private func assistedWorkspaceRoot(_ a: [String: Any]) throws -> URL {
        URL(fileURLWithPath: try requiredString(a, "workspace")).standardizedFileURL
    }

    private func liveAssistedSession() throws -> AssistedAnnotationWindowController {
        guard let controller = AssistedAnnotationWindowController.current else {
            throw ToolError(message: "No annotation session is open. Call assisted_open_case.")
        }
        return controller
    }

    /// The workspace's cases and which of them still have no replay.
    func assistedWorkspaceStatus(_ a: [String: Any]) throws -> String {
        try requireAssistedAnnotation()
        let workspace: AssistedWorkspace
        do {
            workspace = try AssistedWorkspace(root: try assistedWorkspaceRoot(a))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let cases = workspace.cases.map { entry -> [String: Any] in
            [
                "case_id": entry.anonymousID, "width": entry.width, "height": entry.height,
                "locked": workspace.isLocked(entry.anonymousID),
            ]
        }
        return try jsonResult([
            "ok": true, "workspace_id": workspace.id, "annotator_id": workspace.annotatorID,
            "cases": cases,
            "pending": workspace.cases.filter { !workspace.isLocked($0.anonymousID) }
                .map(\.anonymousID),
        ])
    }

    /// Mirrors File ▸ Open Assisted Annotation Case…. A human session stops at
    /// the task card (`awaiting_start`); a synthetic one starts its clock now.
    func assistedOpenCase(_ a: [String: Any]) throws -> String {
        try requireAssistedAnnotation()
        let synthetic = boolArg(a, "synthetic") ?? false
        let controller: AssistedAnnotationWindowController
        do {
            controller = try AssistedAnnotationWindowController.open(
                workspace: try assistedWorkspaceRoot(a), caseID: stringArg(a, "case"),
                synthetic: synthetic)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let session = controller.session
        return try jsonResult([
            "ok": true, "case_id": session.caseID, "width": session.width,
            "height": session.height, "origin": session.origin.rawValue,
            "state": session.stateName,
        ])
    }

    /// The window's own readout, as data.
    func assistedSessionState(_ a: [String: Any]) throws -> String {
        try requireAssistedAnnotation()
        guard let controller = AssistedAnnotationWindowController.current else {
            return try jsonResult(["ok": true, "active": false])
        }
        var fields = controller.session.stateJSON()
        fields["ok"] = true
        fields["active"] = controller.session.isActive
        return try jsonResult(fields)
    }

    /// One brush gesture on a synthetic session. `accepted` false with a
    /// `rejected:` result is a RECORDED refusal that still cost a gesture,
    /// not a failed call.
    func assistedSyntheticStroke(_ a: [String: Any]) throws -> String {
        try requireAssistedAnnotation()
        let controller = try liveAssistedSession()
        guard let tool = AssistedLabel(rawValue: try requiredString(a, "tool")) else {
            throw ToolError(message: "tool is foreground, background or unknown.")
        }
        guard let diameter = intArg(a, "diameter"),
              AssistedAnnotation.diameters.contains(diameter)
        else {
            throw ToolError(message: "diameter is one of \(AssistedAnnotation.diameters).")
        }
        guard let raw = a["points"] as? [[Any]], !raw.isEmpty else {
            throw ToolError(message: "points is a non-empty list of [x, y] canvas pixels.")
        }
        let points: [[Int]] = try raw.map { pair in
            guard pair.count == 2, let x = (pair[0] as? NSNumber)?.intValue,
                  let y = (pair[1] as? NSNumber)?.intValue
            else { throw ToolError(message: "Each point is [x, y] in canvas pixels.") }
            return [x, y]
        }
        let session = controller.session
        let accepted: Bool
        do {
            accepted = try controller.synthesizeStroke(
                tool: tool, diameter: diameter, points: points)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try jsonResult([
            "ok": true, "accepted": accepted,
            "result": session.events.last?.result ?? "accepted",
            "modifying_gestures": session.modifyingGestures,
            "label_plane_sha256": session.labelPlaneSHA256, "state": session.stateName,
        ])
    }

    /// Mirrors the window's Submit button, for a synthetic session only.
    func assistedFinalizeCase(_ a: [String: Any]) throws -> String {
        try requireAssistedAnnotation()
        let controller = try liveAssistedSession()
        let path: URL
        do {
            path = try controller.finalizeFromAgent()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let session = controller.session
        return try jsonResult([
            "ok": true, "replay_path": path.path, "outcome": session.stateName,
            "human_origin": session.origin == .human,
            "scoreable": session.origin == .human,
        ])
    }
}
