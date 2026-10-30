import AppKit

// The Duotone document's gate (#547, `designs/duotone-mode.md`): a Duotone document IS a Grayscale
// document (8 or 16 bits, layers and all; `isGray` stays true) whose policy carries a duotone
// description, so every edit Gray admits runs unchanged on the gray channel.
// `DocumentPrecision.refusal` asks this table right after its document check and before the
// Indexed, Lab, Gray and CMYK arms: `get_duotone` and `set_duotone` (`tools`) answer only on a
// Duotone document; on one, `refusal(tool:arguments:)` refuses first what Duotone disables
// (`disabledTools`, `disabledFilters`, `disabledSentence`), then a save that
// is not `.doctored` (`saveRefusal`: PSD and PSB are File ▸ Export as PSD's, every other
// format the exports'), and 16 and 32 Bits/Channel for good; anything else falls through to
// Gray's arm unchanged. The exports render the inks to RGB in the core (`exportNote`, carried by
// every export reply); Mode ▸ RGB, CMYK, Lab and Indexed Color convert through the render,
// Mode ▸ Grayscale renders the inks into the gray and Mode ▸ Bitmap… is that Grayscale rule
// followed by Bitmap (`EditorViewController+DuotoneMode.swift`). The menu twin is
// `refuses(selector:proofColorsOn:)`, asked in the Gray branch of `validatePrecisionItem`. The
// view is the core's
// (`duotone_document`): the gray channel through the inks, RGBA16 in the document's own profile,
// then the ordinary preview, measured against Photoshop in take1 ([A], max 0.56 codes). Assign
// and Convert to Profile, Proof Setup and Proof Colors are refused in Duotone, by tool and by
// selector, but for Proof Colors while proofing is on, so a document converted with it on can turn
// it off (`turnsProofingOff`); Merge to HDR is admitted as Gray admits it; the Channels row is
// named after the type (`channelName`). The refused set is `disabledTools`, `disabledFilters` and
// `refusedSelectors` (`designs/duotone-mode.md` lists it by menu).
// Unmeasured (no dialog-free reading): the Layers and Channels thumbnails through the inks
// (`thumbnail`) and New from Clipboard of a Duotone copy as a Grayscale document (the core's
// payload drops the inks).

extension RasterDocument {
    /// True for a Duotone document — read off the captured descriptor (`duotone`), so a per-row
    /// panel or menu check costs no FFI call.
    var isDuotone: Bool { precisionInfo?["duotone"] is [String: Any] }

    /// A Duotone document's ink count (1…4); nil for every other document.
    var duotoneInkCount: Int? { (precisionInfo?["duotone"] as? [String: Any])?["inks"] as? Int }
}

enum DuotoneDocument {
    /// The MCP tools that read and set the inks: admitted on a Duotone document only.
    static let tools: Set<String> = ["get_duotone", "set_duotone"]

    /// What Duotone refuses for good: Edit ▸ Assign Profile and Convert to Profile, View ▸ Proof
    /// Setup and Proof Colors, Content-Aware Fill, Puppet Warp, Perspective Warp, Select ▸ Subject
    /// and Sky, and the Neural Filters Doctored has: Depth Blur, Super Zoom, JPEG Artifacts
    /// Removal and Colorize.
    static let disabledTools: Set<String> = [
        "assign_profile", "convert_profile", "get_proof_setup", "set_proof_setup", "render_proof",
        "content_aware_fill", "preview_content_aware_fill", "discard_content_aware_fill_preview",
        "puppet_warp", "perspective_warp", "select_subject", "select_sky", "depth_blur", "super_zoom",
        "jpeg_artifacts_removal", "neural_colorize",
    ]

    /// The `apply_filter` filters Duotone refuses for good: Develop Filter, Lens Correction and
    /// Tilt-Shift.
    static let disabledFilters: Set<String> = ["develop_filter", "lens_correction", "tilt_shift"]

    /// The menu items a Duotone document refuses beyond Gray's own rules: those of
    /// `disabledTools` and `disabledFilters`, every Proof Setup entry (its presets and working
    /// plates) and Gamut Warning (which turns Proof Colors on), and 16 and 32 Bits/Channel (for
    /// good).
    static let refusedSelectors: Set<String> = [
        "convertToFloatPrecision:", "convertToSixteenBits:", "assignProfile:", "convertToProfile:",
        "showProofSetup:", "toggleProofColors:", "applyProofPresetFromMenu:", "applyWorkingProof:",
        "toggleGamutWarning:", "contentAwareFill:", "showPuppetWarp:", "showPerspectiveWarp:",
        "selectSubject:", "selectSky:", "showDepthBlur:", "showSuperZoom:",
        "showJpegArtifactsRemoval:", "showColorize:", "showDevelopFilter:", "showLensCorrection:",
        "showTiltShift:",
    ]

    /// Whether a Duotone document refuses the menu item `selector`. Proof Colors is let through
    /// while proofing is on, so a document converted into Duotone with Proof Colors on can turn
    /// it off; the item can then only turn proofing off.
    static func refuses(selector: String, proofColorsOn: Bool) -> Bool {
        if selector == "toggleProofColors:" && proofColorsOn { return false }
        return refusedSelectors.contains(selector)
    }

    /// Whether a `set_proof_setup` call only turns proofing off (Proof Colors' menu exception's
    /// MCP twin): every key but `document_id` is `proof_colors` or `gamut_warning`, at least one
    /// is present, and each is false. State-free: on a document already unproofed it changes
    /// nothing.
    static func turnsProofingOff(_ arguments: [String: Any]) -> Bool {
        let keys = arguments.keys.filter { $0 != "document_id" }
        guard !keys.isEmpty else { return false }
        // `AgentServer.boolArg`'s spellings of false (the number, "false" or "0").
        func isFalse(_ value: Any?) -> Bool {
            if let number = value as? NSNumber { return !number.boolValue }
            return value as? String == "false" || value as? String == "0"
        }
        return keys.allSatisfy { key in
            (key == "proof_colors" || key == "gamut_warning") && isFalse(arguments[key])
        }
    }

    /// The for-good sentence for `name`, a tool Duotone disables.
    static func disabledSentence(_ name: String) -> String {
        "\(name) is not available in Duotone."
    }

    /// The Channels panel's name for a Duotone document's one channel: its type (Monotone,
    /// Duotone, Tritone or Quadtone).
    static func channelName(_ doc: RasterDocument) -> String {
        let index = (doc.duotoneInkCount ?? 2) - 1
        return DuotoneOptions.typeTitles.indices.contains(index)
            ? DuotoneOptions.typeTitles[index] : "Duotone"
    }

    /// A Duotone layer's (or composite's) thumbnail: fitted at u16 first, then through the inks in
    /// the core (`dr_precision_doc_duotone_thumbnail_rgba8`), RGBA8 in the document's profile.
    static func thumbnail(of source: RasterPrecisionImage, precision: RasterPrecisionDocument,
                          maxSide: Int, context: PrecisionWorkContext) throws -> RasterImage {
        try precision.duotoneThumbnailRGBA8(of: source, maxSide: maxSide, workingLimit: context.limit,
                                            otherRetained: context.otherRetained)
    }

    /// 32 Bits/Channel, for good.
    static let depthRefusal = "32 Bits/Channel is unavailable in a Duotone document: "
        + "the 32-bit modes are RGB and Grayscale."

    /// 16 Bits/Channel, for good (the core's `DEPTH16_REFUSAL`; 16 → 8 keeps the inks).
    static let sixteenBitRefusal = "16 Bits/Channel is not available in Duotone."

    /// Save As / Save a Copy in a format other than `.doctored` (`CapturedSave`, `save_copy`).
    static let saveRefusal = "A Duotone document saves as .doctored, or as PSD or PSB through File ▸ Export "
        + "as PSD; File ▸ Export renders its inks to RGB."

    /// Every export reply's note on a Duotone document (#547 Duotone phase 1b).
    static let exportNote = "A Duotone document exports through its inks: the file is RGB, rendered "
        + "as the canvas shows it."

    /// `fields` with the export note when `doc` is a Duotone document.
    static func noting(_ fields: [String: Any], for doc: RasterDocument?) -> [String: Any] {
        guard doc?.isDuotone == true else { return fields }
        var out = fields
        out["duotone"] = exportNote
        return out
    }

    /// `get_duotone` / `set_duotone` on any document but a Duotone one.
    static func notDuotoneRefusal(tool: String) -> String {
        "\(tool) applies to a Duotone document, as Image ▸ Mode ▸ Duotone… does; convert_mode duotone "
            + "converts a Grayscale document."
    }

    /// Why `doc` cannot become a Duotone document: only an 8- or 16-bit Grayscale document (or a
    /// Duotone one, whose inks the item edits) converts. nil when it can.
    static func sourceRefusal(_ doc: RasterDocument) -> String? {
        guard doc.isGray else {
            return "Duotone converts an 8- or 16-bit Grayscale document; convert to "
                + "Grayscale first."
        }
        guard doc.grayDepth != 32 else {
            return "Duotone converts an 8- or 16-bit Grayscale document; convert to "
                + "8 or 16 Bits/Channel first."
        }
        return nil
    }

    /// The gate's answer for one MCP call on a Duotone document: a refusal, or nil to hand the
    /// tool to Gray's arm unchanged.
    static func refusal(tool: String, arguments: [String: Any]) -> String? {
        // Proof Colors' menu exception's twin: a call that only turns proofing off is admitted.
        if tool == "set_proof_setup" && turnsProofingOff(arguments) { return nil }
        if disabledTools.contains(tool) { return disabledSentence(tool) }
        switch tool {
        case "save_copy":
            let format = (arguments["format"] as? String)?.lowercased()
                ?? (arguments["path"] as? String).map { ($0 as NSString).pathExtension.lowercased() }
                ?? ImageDocument.nativeFormatName
            if !ImageDocument.isNativeFormatName(format) { return saveRefusal }
        case "apply_filter":
            if let filter = arguments["filter"] as? String, disabledFilters.contains(filter) {
                return disabledSentence(filter)
            }
        case "convert_precision":
            switch (arguments["mode"] as? String)?.lowercased() {
            case "rgba32": return depthRefusal
            case "rgba16": return sixteenBitRefusal
            default: break
            }
        default:
            break
        }
        return nil
    }

    /// A Duotone projection's display bytes: the core's view through the inks at u16, then the
    /// ordinary preview (Lab's `LabDocument.display` shape). `RasterDocument.display(of:)` routes a
    /// Duotone document here. `control` is single-use, so each core call takes a child of it.
    static func display(of image: RasterPrecisionImage, precision: RasterPrecisionDocument,
                        context: PrecisionWorkContext, control: RasterJobControl?) throws -> RasterImage {
        guard let control else {
            let view = try precision.duotoneView(of: image, workingLimit: context.limit,
                                                 otherRetained: context.otherRetained)
            let narrowing = try PrecisionWorkContext(image: view)
            return try view.previewRGBA8(workingLimit: narrowing.limit,
                                         otherRetained: narrowing.otherRetained)
        }
        let view = try control.withChildOperation(from: 0, to: 0.6) {
            try precision.duotoneView(of: image, workingLimit: context.limit,
                                      otherRetained: context.otherRetained, control: $0)
        }
        let narrowing = try PrecisionWorkContext(image: view)
        return try control.withChildOperation(from: 0.6, to: 1) {
            try view.previewRGBA8(workingLimit: narrowing.limit,
                                  otherRetained: narrowing.otherRetained, control: $0)
        }
    }
}
