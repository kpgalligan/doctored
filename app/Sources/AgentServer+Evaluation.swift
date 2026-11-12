import AppKit

/// The evaluation-policy tools (`DocumentEvaluation.swift`):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `getEvaluation` | the Image ▸ Evaluation… card as it opens |
/// | `setEvaluation` | Image ▸ Evaluation… ▸ Apply |
///
/// `get_evaluation` reads and records nothing. `set_evaluation` IS a step:
/// it is a per-document, undoable, `.rz`-persisted property, which is the
/// test `AgentServer+Guides.swift`'s header states for the view-preference
/// exemption, and this passes all four parts of it.
///
/// Linear-light evaluation evaluates every operator (P7-24); what it can still
/// refuse, at the RENDER as well as at the switch, is a profile with no
/// matrix/TRC model and a held transfer that no longer belongs to the
/// document's profile. `performGroupedEdit`'s pre-check turns a refusal into
/// an in-band `ToolError` on the tool that caused it, leaving the document
/// exactly as it was.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let evaluationHandlers: [String: ToolHandler] = [
        "get_evaluation": { $0.getEvaluation },
        "set_evaluation": { $0.setEvaluation },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let evaluationToolFacts: [String: ToolFacts] = [
        // Phase 11E: `set_evaluation` IS a step — per-document, undoable,
        // `.rz`-persisted — and only the read is dropped, beside the other
        // `get_*` tools above.
        // Phase 11E: the evaluation policy is a DOCUMENT property with a
        // typed implementation of its own (`precision_evaluation`), so both
        // tools answer at either depth.
        "get_evaluation": [.nonRecordable, .commonTool, .floatTool],
        "set_evaluation": [.commonTool, .floatTool],
    ]

    /// The document's policy, what the other one would cost, and why it may
    /// be refused. With `preview: true` the reply carries a MEASURED
    /// `difference` between the projection on screen and the one switching
    /// would produce — the same numbers the sheet's readout shows.
    func getEvaluation(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let policy = doc.evaluation
        let other: EvaluationPolicy = policy == .encoded ? .linearLight : .encoded
        var result: [String: Any] = [
            "ok": true,
            "policy": policy.rawValue,
            "available": EvaluationPolicy.allCases.map(\.rawValue),
            "linear_light_refusal": doc.linearLightRefusal() ?? NSNull(),
        ]
        if let table = EvaluationOperators.table {
            result["operators"] = [
                "migrated": table.migrated,
                "refused": table.refused.map { ["operator": $0.name, "phase": $0.phase] },
            ] as [String: Any]
        }
        if boolArg(a, "preview") == true {
            do {
                if let candidate = try doc.withEvaluation(other) {
                    result["difference"] = try Self.projectionDifference(
                        doc, candidate, beforeProjection: document.retainedByteProjection,
                        beforePrecisionProjection: document.retainedPrecisionProjection)
                    result["difference_policy"] = other.rawValue
                }
            } catch {
                result["preview_refusal"] = error.localizedDescription
            }
        }
        return try jsonResult(result)
    }

    /// Image ▸ Evaluation… ▸ Apply. One undo step; a policy the document
    /// already has is a no-op rather than a failure.
    func setEvaluation(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let raw = stringArg(a, "policy"),
              let policy = EvaluationPolicy(rawValue: raw) else {
            throw ToolError(
                message: "policy must be one of "
                    + EvaluationPolicy.allCases.map(\.rawValue).joined(separator: ", "))
        }
        let previous = doc.evaluation
        if previous == policy {
            return try jsonResult([
                "ok": true, "policy": policy.rawValue, "previous": previous.rawValue,
                "unchanged": true,
            ])
        }
        // The core's refusal is a SENTENCE naming the layer and the operator.
        // `performGroupedEdit` can only report "check the parameters" for a
        // nil, so it is carried out of the non-throwing transform.
        var refusal: Error?
        do {
            try performGroupedEdit(document, "Evaluation") { source in
                do {
                    return try source.withEvaluation(policy)
                } catch {
                    refusal = error
                    return nil
                }
            }
        } catch {
            if let refusal { throw ToolError(message: refusal.localizedDescription) }
            throw error
        }
        return try jsonResult([
            "ok": true, "policy": policy.rawValue, "previous": previous.rawValue,
        ])
    }

    /// How far two projections of one document sit apart, at its own depth.
    /// `beforeProjection` is `before`'s retained 8-bit projection and
    /// `beforePrecisionProjection` its retained typed one, when the caller
    /// holds them; nil renders it. Each is read only at its own
    /// depth: the byte branch ignores the typed one and the typed branch the
    /// byte one.
    static func projectionDifference(_ before: RasterDocument, _ after: RasterDocument,
                                     beforeProjection: RasterImage?,
                                     beforePrecisionProjection: RasterPrecisionImage? = nil)
        throws -> [String: Any]
    {
        let measured: ProjectionDifference
        if before.isHighDepth {
            measured = try RasterDocument.compare(
                beforePrecisionProjection ?? before.renderPrecision(), after.renderPrecision())
        } else {
            measured = try RasterDocument.compare(
                beforeProjection ?? before.render(), after.render())
        }
        return [
            "depth": measured.depth,
            "width": measured.width,
            "height": measured.height,
            "mean_abs": measured.meanAbs,
            "max_abs": measured.maxAbs,
            "changed_fraction": measured.changedFraction,
        ]
    }

    /// The `evaluation` block `get_document` carries, beside `color_profile`.
    static func evaluationFields(_ doc: RasterDocument) -> [String: Any] {
        [
            "policy": doc.evaluation.rawValue,
            "projection_error": doc.linearLightRefusal() ?? NSNull(),
        ]
    }
}
