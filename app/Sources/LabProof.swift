import AppKit

// The Lab Color document's proof and its for-good profile refusals (#1 Lab phase 5b).
// A Lab document proofs through the unchanged RGB proof of its Display P3 view: the canvas
// (`ProofSource.display`) already proofs the view's display codes (`RasterDocument.display(of:)`
// routes Lab to `LabDocument.display`), so it only needs the view's profile in place of the twin
// (`proofSourceProfile`); `render_proof exact:true` proofs the u16 view (`exactProofImage`). The
// core refuses the twin itself as a proof's document profile (`soft_proof::Proofer::new`,
// `lab_document::PROOF_VIEW_REFUSAL`). Results report `space` "display-p3"
// (`reportedProofSpace`), which `ProofRenderer.space` tags. `assign_profile` and
// `auto_blend_layers` refuse for good (`disabledTools`); Convert to Profile…, Proof Setup, Proof
// Colors and Gamut Warning run.

extension RasterDocument {
    /// The profile a proof reads this document's display codes in: the built-in Display P3 for a
    /// Lab document (its view space, `LabDocument.viewSpace`), else the document's own profile.
    var proofSourceProfile: Data {
        isLab ? RasterProfile.builtin(.displayP3) : iccProfile ?? Data()
    }
}

extension LabDocument {
    /// The space a proof reports: a Lab document's "document" space is its Display P3 view.
    static func reportedProofSpace(_ space: String, isLab: Bool) -> String {
        isLab && space == "document" ? "display-p3" : space
    }

    /// `render_proof exact:true`'s source on a Lab document: the projection's u16 Display P3 view
    /// (`dr_precision_image_lab_display`), which the typed proof then takes as an RGB projection.
    static func exactProofImage(of projection: RasterPrecisionImage, context: PrecisionWorkContext,
                                control: RasterJobControl?) throws -> RasterPrecisionImage {
        try projection.labDisplay(workingLimit: context.limit, otherRetained: context.otherRetained,
                                  control: control)
    }

    /// Phase 5b's proof tools (`AgentServer+SoftProof.swift`, `AgentServer+ProofPresets.swift`).
    static let proofTools: Set<String> = ["get_proof_setup", "set_proof_setup", "render_proof",
                                          "save_proof_preset"]

    /// Phase 5b's View ▸ Proof Setup, Proof Colors and Gamut Warning items.
    static let proofSelectors: Set<String> = ["showProofSetup:", "toggleProofColors:",
                                              "toggleGamutWarning:", "applyProofPresetFromMenu:",
                                              "applyWorkingProof:"]

    /// The MCP tools of the items unavailable in Lab: Edit ▸ Assign Profile… and Edit ▸
    /// Auto-Blend Layers. They refuse for good (`disabledRefusal`), never naming a phase.
    static let disabledTools: Set<String> = ["assign_profile", "auto_blend_layers"]

    /// The refusal of a `disabledTools` tool.
    static func disabledRefusal(tool: String) -> String {
        "\(tool) is unavailable in a Lab Color document."
    }
}
