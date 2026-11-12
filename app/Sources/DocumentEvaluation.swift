import Foundation

// `EvaluationPolicy` is the one vocabulary the menu, the sheet, the MCP tools and the editing-state
// archive (v4) speak. TWO GUARDS make a core refusal arrive on the edit rather than on the next
// render: one in `ImageDocument.applyEdit` (an alert) and one in `AgentServer.performGroupedEdit`
// (an in-band `ToolError`); both are `updated.linearLightRefusal()`, which the core answers nil for
// an encoded document without comparing anything. Linear light evaluates every operator, so what
// those guards still catch is a float document's own table and the core's stale-transfer net.

/// Image ▸ Evaluation… — the per-document choice between compositing the
/// stored code values (`encoded`, what every document has always done) and
/// decoding through the document profile's tone curve, compositing in linear
/// light and re-encoding once at the output (`linear_light`).
///
/// The policy is the CORE's (`doc_evaluation` / `precision_evaluation`); this
/// file is the host's vocabulary for it: the enum the menu, the sheet, the MCP
/// tools and the editing-state archive all speak, the operator table the sheet
/// shows, and the projection difference the sheet's readout reports. Every
/// `dr_*` call behind these lives in `DoctoredCore+Evaluation.swift`.
///
/// The invariant worth restating here, because two host guards depend on it:
/// whatever a linear-light document refuses — today only a held transfer that
/// no longer belongs to its profile — it refuses at the render as well as at
/// the edit. The guards in
/// `ImageDocument.applyEdit` and `AgentServer.performGroupedEdit` are there so
/// the user meets the sentence when they make the edit rather than when the
/// canvas next tries to draw.
enum EvaluationPolicy: String, CaseIterable {
    case encoded
    case linearLight = "linear_light"

    /// The `DR_EVALUATION_*` value, asked of the core rather than hardcoded —
    /// once per process, not once per read. `RasterDocument.evaluation` scans
    /// these on `get_document`, on both resize guards and on every sheet
    /// refresh, so the string lookup is resolved here and cached.
    var code: Int32 { Self.codes[self] ?? 0 }

    private static let codes: [EvaluationPolicy: Int32] = Dictionary(
        uniqueKeysWithValues: allCases.map { ($0, EvaluationCore.code(forName: $0.rawValue) ?? 0) }
    )

    var displayName: String {
        switch self {
        case .encoded: return "Encoded (default)"
        case .linearLight: return "Linear light"
        }
    }

    /// What the sheet says the choice means.
    var summary: String {
        switch self {
        case .encoded:
            return "Layers composite on the stored numbers, exactly as they always have."
        case .linearLight:
            return "Layers are decoded through this document's profile, composited in "
                + "linear light, and re-encoded once at the output."
        }
    }

    /// The editing-state archive's `evaluation` value. A byte document's
    /// encoded policy is spelled `legacy`, which is what every archive
    /// written before Phase 11E means; a typed document uses the same tag its
    /// own descriptor carries, so the two cross-check.
    func archiveTag(isHighDepth: Bool) -> String {
        switch self {
        case .encoded: return isHighDepth ? "encoded_rgb_f32_v1" : "legacy"
        case .linearLight: return "linear_light_f32_v1"
        }
    }

    /// The archive value read back; nil for a tag this build does not know,
    /// which the archive reader refuses rather than repairs.
    static func fromArchiveTag(_ tag: String) -> EvaluationPolicy? {
        switch tag {
        case "legacy", "encoded_rgb_f32_v1": return .encoded
        case "linear_light_f32_v1": return .linearLight
        default: return nil
        }
    }
}

/// One `dr_image_compare` / `dr_precision_image_compare` result: how far two
/// projections of the same document sit apart, in the depth's own codes.
struct ProjectionDifference: Decodable {
    let depth: Int
    let width: Int
    let height: Int
    let maxAbs: Int
    let meanAbs: Double
    let changedFraction: Double

    enum CodingKeys: String, CodingKey {
        case depth, width, height
        case maxAbs = "max_abs"
        case meanAbs = "mean_abs"
        case changedFraction = "changed_fraction"
    }

    /// The readout the Evaluation sheet shows under its preview.
    var summary: String {
        let unit = depth == 16 ? "codes of 65535" : "codes"
        return String(format: "Mean %.3f, max %d %@ over %.1f%% of samples.",
                      meanAbs, maxAbs, unit, changedFraction * 100)
    }
}

struct RefusedOperator: Decodable {
    let name: String
    let phase: Int

    enum CodingKeys: String, CodingKey {
        case name = "operator"
        case phase
    }
}

/// The core's operator table: what linear-light evaluation evaluates — every
/// operator — and what it refuses by name, an empty list kept for the reply's
/// shape. Constant for the life of the process, so it is read once.
struct EvaluationOperators: Decodable {
    let migrated: [String]
    let refused: [RefusedOperator]

    private static let cached: EvaluationOperators? = try? load()

    static func load() throws -> EvaluationOperators {
        let json = try EvaluationCore.operatorsJSON()
        return try JSONDecoder().decode(
            EvaluationOperators.self, from: JSONSerialization.data(withJSONObject: json))
    }

    static var table: EvaluationOperators? { cached }
}

extension RasterDocument {
    /// This document's evaluation policy.
    var evaluation: EvaluationPolicy {
        EvaluationPolicy.allCases.first { $0.code == evaluationCode } ?? .encoded
    }

    /// Why the document cannot evaluate under its policy, as the sentence to
    /// show; nil when there is nothing to refuse — which the core answers
    /// without any comparison for an encoded document, so this is cheap
    /// enough to ask on every edit.
    func linearLightRefusal() -> String? { linearLightRefusalText() }

    /// The document under `policy`; nil — no error — when it is already in it.
    func withEvaluation(_ policy: EvaluationPolicy) throws -> RasterDocument? {
        try settingEvaluation(policy.code)
    }

    static func compare(_ a: RasterImage, _ b: RasterImage) throws -> ProjectionDifference {
        try decodeDifference(a.differenceJSON(to: b))
    }

    static func compare(_ a: RasterPrecisionImage,
                        _ b: RasterPrecisionImage) throws -> ProjectionDifference {
        try decodeDifference(a.differenceJSON(to: b))
    }

    private static func decodeDifference(_ json: [String: Any]) throws -> ProjectionDifference {
        try JSONDecoder().decode(
            ProjectionDifference.self, from: JSONSerialization.data(withJSONObject: json))
    }
}
