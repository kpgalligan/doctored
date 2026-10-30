import Foundation

/// The Duotone document's catalog (#547, `designs/duotone-mode.md`): `get_duotone` and
/// `set_duotone` (`AgentServer+Duotone.swift`, with the `.ado` `path`), `convert_mode`'s `duotone`
/// mode, its `inks`/`overprints` schema and the conversions out, and the notes on `get_document`,
/// `save_copy`, the PSD open and export, the exports through the inks, New from Clipboard and
/// `channel_name`; the tools refused for good in Duotone,
/// `set_proof_setup`'s turn-off exception and the Merge to HDR admission.
/// Behind an explicit non-inlining boundary, for `AgentCatalog.swift`'s reason.
extension AgentServer {
    /// The `inks` and `overprints` schema, shared by `set_duotone` and `convert_mode`.
    private func duotoneProperties() -> [String: Any] {
        let lab: [String: Any] = ["type": "array", "items": ["type": "number"], "minItems": 3, "maxItems": 3,
                                  "description": "D50 CIE Lab: L* 0–100, a* and b* −128–127."]
        let slot: [String: Any] = ["type": ["number", "null"], "minimum": 0, "maximum": 100]
        let ink: [String: Any] = [
            "type": "object",
            "properties": [
                "name": ["type": "string", "minLength": 1, "maxLength": 64],
                "lab": lab,
                "curve": ["type": "array", "items": slot, "minItems": 13, "maxItems": 13,
                          "description": "Output percent at the inputs 0, 5, 10, 20, 30, 40, 50, 60, 70, "
                            + "80, 90, 95 and 100 %; null is blank; the first and last must be set."],
            ],
            "required": ["name", "lab", "curve"],
        ]
        return [
            "inks": ["type": "array", "items": ink, "minItems": 1, "maxItems": 4,
                     "description": "Toward Duotone: 1 to 4 inks (Monotone, Duotone, Tritone, Quadtone), "
                        + "each a name, a D50 Lab colour and a 13-slot curve."],
            "overprints": ["type": "object",
                           "additionalProperties": ["type": ["array", "null"], "items": ["type": "number"]],
                           "description": "Toward Duotone: authored overprint colours as [L, a, b] by "
                            + "combination key (\"1+2\", \"1+3\", \"2+3\", \"1+2+3\", …: every combination "
                            + "of two or more inks); missing or null is the default, the inks' product "
                            + "over paper."],
        ]
    }

    @inline(never) func duotoneCatalog() -> [[String: Any]] {
        let docID: [String: Any] = [
            "type": "string",
            "description": "Which open document; defaults to the frontmost.",
        ]
        var setProperties = duotoneProperties()
        setProperties["document_id"] = docID
        setProperties["path"] = [
            "type": "string",
            "description": "A Duotone Options (.ado) file to load, as the dialog's Load…; exclusive "
                + "with inks and overprints.",
        ]
        return [
            tool(
                "get_duotone",
                "Reads a Duotone document's options, as Image ▸ Mode ▸ Duotone… shows them (#547): "
                    + "type (monotone, duotone, tritone or quadtone), inks [{name, lab (D50 L*, a*, "
                    + "b*), curve (13 output percents at the inputs 0, 5, 10, 20, 30, 40, 50, 60, 70, "
                    + "80, 90, 95 and 100 %, null blank)}], overprints {key: [L, a, b] or null for the "
                    + "default} and overprint_display {key: the effective [L, a, b]}. With path it "
                    + "also writes them as a Duotone Options (.ado) file, as the dialog's Save… (Lab "
                    + "to 0.01, curve points to 0.1 %); an existing file refuses unless overwrite is "
                    + "true. Any other document refuses. channel_name is the Channels panel's name "
                    + "for the one channel: the type (Monotone, Duotone, Tritone or Quadtone).",
                ["document_id": docID,
                 "path": ["type": "string",
                          "description": "Where to write the Duotone Options (.ado) file."],
                 "overwrite": ["type": "boolean",
                               "description": "Replace an existing file at path (default false)."]]),
            tool(
                "set_duotone",
                "Replaces a Duotone document's inks, curves and overprint colours, as Image ▸ Mode ▸ "
                    + "Duotone…'s OK on a Duotone document (#547), one undo step: the gray pixels are "
                    + "kept and only the view changes. Pass inks (1–4 inks, with overprints optional) "
                    + "or path, a Duotone Options (.ado) file, as the dialog's Load…; the same "
                    + "options answer changed false. Any other document refuses (convert_mode duotone "
                    + "converts a Grayscale document).",
                setProperties),
        ]
    }

    @inline(never) func annotateDuotoneCatalog(_ catalog: inout [[String: Any]]) {
        let note = " mode \"duotone\" (#547) converts an 8- or 16-bit Grayscale document to "
            + "Duotone, as Image ▸ Mode ▸ Duotone…, one undo step: the gray pixels, layers and depth "
            + "are kept and the view shows the gray channel through the inks (each ink's coverage = "
            + "its curve, a natural spline through the set slots, of the gray's ink percentage, "
            + "through the Gray tone curve; the inks and overprints mixed over paper in D50 XYZ). It "
            + "takes inks (required) and overprints only; on a Duotone document it replaces them. "
            + "Any other document refuses. On a Duotone document mode \"gray\" renders the inks into "
            + "the gray: each gray code becomes the luminance of its ink mix, "
            + "encoded through the document's own Gray profile at its depth (within one 8-bit code "
            + "of Photoshop's on the two- and three-ink takes), one undo step (gray_space refuses); "
            + "mode bitmap (8-bit) is that Grayscale rule followed by the Bitmap conversion and its "
            + "options, one undo step, composed and unmeasured against Photoshop; modes rgb, cmyk, "
            + "lab and indexed (8-bit) convert it through the render of its inks (each layer through "
            + "the view into RGB, then the mode's own route), one undo step. An adjustment layer or "
            + "a layer style needs flatten true. The reply's duotone holds type and inks."
        let renders = " A Duotone document (#547) exports through its inks: the file is RGB in the "
            + "document's profile, rendered as the canvas shows it, and the reply's duotone says so."
        let disabled = " A Duotone document refuses it for good (#547)."
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            let existing = (catalog[index]["description"] as? String) ?? ""
            switch name {
            case "convert_mode":
                catalog[index]["description"] = existing + note
                guard var schema = catalog[index]["inputSchema"] as? [String: Any],
                      var current = schema["properties"] as? [String: Any] else { continue }
                if var mode = current["mode"] as? [String: Any] {
                    var options = (mode["enum"] as? [String]) ?? []
                    if !options.contains("duotone") { options.append("duotone") }
                    mode["enum"] = options
                    current["mode"] = mode
                }
                current.merge(duotoneProperties()) { existing, _ in existing }
                schema["properties"] = current
                catalog[index]["inputSchema"] = schema
            case "get_document":
                catalog[index]["description"] = existing + " A Duotone document (#547) reports "
                    + "color_mode duotone and duotone {type, inks}; its gray block and pixels are the "
                    + "Grayscale document's."
            case "save_copy":
                catalog[index]["description"] = existing + " A Duotone document (#547) saves rz only, "
                    + "keeping its inks; PSD and PSB are export_psd's and every other format is an "
                    + "export through its inks, so other formats refuse by that sentence."
            case "export_psd", "inspect_psd_export":
                catalog[index]["description"] = existing + " A Duotone document (#547) writes a "
                    + "Duotone PSD or PSB (colour mode 8, its gray channel and its inks as Photoshop's "
                    + "duotone specification; inks unchanged since the open keep the file's duotone "
                    + "data byte for byte) at 8 bits, Photoshop's Duotone depth: a 16-bit document is "
                    + "written from an 8-bit copy (bit_depth 8, one loss line; the document keeps 16 "
                    + "bits); a layered TIFF refuses."
            case "inspect_psd", "open_document":
                catalog[index]["description"] = existing + " A Duotone PSD or PSB (colour mode 8, 8 or "
                    + "16 bits, #547) answers import_precision \"gray\", color_mode Duotone and duotone "
                    + "(its options), and opens as a Duotone document."
            case "export_image", "export_preview", "quick_export", "export_layers", "export_pdf",
                 "contact_sheet":
                catalog[index]["description"] = existing + renders
            case "set_proof_setup":
                catalog[index]["description"] = existing + disabled + " One exception: a call whose "
                    + "only keys besides document_id are proof_colors and gamut_warning, each false, "
                    + "is admitted, so a document converted into Duotone with Proof Colors on can turn "
                    + "proofing off."
            case "assign_profile", "convert_profile", "get_proof_setup", "render_proof",
                 "content_aware_fill", "preview_content_aware_fill", "discard_content_aware_fill_preview",
                 "puppet_warp", "perspective_warp", "select_subject", "select_sky", "depth_blur",
                 "super_zoom", "jpeg_artifacts_removal", "neural_colorize":
                catalog[index]["description"] = existing + disabled
            case "apply_filter":
                catalog[index]["description"] = existing + " A Duotone document (#547) refuses filter "
                    + "develop_filter, lens_correction and tilt_shift for good."
            case "convert_precision":
                catalog[index]["description"] = existing + " A Duotone document (#547) refuses 16 and "
                    + "32 Bits/Channel for good (mode rgba16 and rgba32, and gray16 from 8 bits and "
                    + "gray32); gray8 from 16 bits keeps the "
                    + "inks."
            case "merge_layers_to_hdr":
                catalog[index]["description"] = existing + " A Duotone document (#547) merges as a "
                    + "Grayscale document does."
            case "new_from_clipboard":
                catalog[index]["description"] = existing + " A pixel copy of a Duotone document "
                    + "(#547) makes a Grayscale document of the copied gray at the source's Gray "
                    + "profile and depth, without the inks."
            default:
                break
            }
        }
    }
}
