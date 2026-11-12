import Foundation

/// The Phase 11E catalog: the two evaluation tools, plus the annotation that
/// teaches `image_size` what linear-light evaluation does to it.
///
/// Kept in its own file behind an explicit non-inlining boundary, for the
/// reason `AgentCatalog.swift`'s header gives: one giant literal made Release
/// WMO spend minutes in `COWArrayOpts` / `ColdBlockInfo`.
extension AgentServer {
    @inline(never) func evaluationCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        return [
            tool(
                "get_evaluation",
                "How this document's projection evaluates its stack: \"encoded\" (the "
                    + "default — layers composite on the stored numbers) or \"linear_light\" "
                    + "(each layer is decoded through the document's colour profile, "
                    + "composited in linear light and re-encoded once at the output). The "
                    + "reply also carries the operator table — what linear light evaluates, "
                    + "every operator, with an empty refused list — and linear_light_refusal, "
                    + "null unless the document's linear-light tables no longer belong to its "
                    + "colour profile. With preview:true it adds a MEASURED "
                    + "difference between the projection on screen and the one the other "
                    + "policy would produce (mean_abs / max_abs in the document's own codes, "
                    + "changed_fraction over all RGBA samples). Reads only.",
                [
                    "document_id": docID,
                    "preview": [
                        "type": "boolean",
                        "description": "Measure what switching would change. Renders the "
                            + "document twice, so leave it off unless the number is wanted.",
                    ],
                ]),
            tool(
                "set_evaluation",
                "Chooses this document's evaluation policy — Image ▸ Evaluation…. One undo "
                    + "step; a policy the document already has is reported unchanged rather "
                    + "than failing. \"linear_light\" changes what every blend, opacity and "
                    + "mask means: each layer's pixels and each layer-style effect's colour "
                    + "are decoded as they composite; Blend If ranges, adjustment kernels and "
                    + "the Shadows/Highlights guide still read display levels; and Image Size, "
                    + "Free Transform, Warp and a RAW layer's re-render resample in linear "
                    + "light. Painting, fills, filters, Liquify, Puppet Warp and Perspective "
                    + "Warp still edit the stored numbers. It refuses only a colour profile "
                    + "with no matrix/TRC model.",
                [
                    "document_id": docID,
                    "policy": [
                        "type": "string", "enum": ["encoded", "linear_light"],
                        "description": "The evaluation policy to apply.",
                    ],
                ], required: ["policy"]),
        ]
    }

    @inline(never) func annotateEvaluationCatalog(_ catalog: inout [[String: Any]]) {
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            switch name {
            case "image_size":
                let existing = (catalog[index]["description"] as? String) ?? ""
                let note: String = " On a document whose evaluation policy is "
                    + "linear_light, layer colour resamples in premultiplied linear light "
                    + "(Nearest still copies the stored numbers). A 32-bit float document "
                    + "is linear light by construction: its samples are already linear, so "
                    + "it resamples them directly."
                catalog[index]["description"] = existing + note
            case "get_document":
                catalog[index]["description"] = ((catalog[index]["description"] as? String) ?? "")
                    + " The reply carries an \"evaluation\" block beside \"color_profile\": the "
                    + "policy and projection_error, the sentence naming why this document "
                    + "cannot evaluate under it (null when it can)."
            default: continue
            }
        }
    }
}
