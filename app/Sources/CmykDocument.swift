import AppKit

// The CMYK document's gate (#1): what a CMYK document admits from the canvas, the menus and
// MCP, asked FIRST wherever a depth decides (`DocumentPrecision.refusal`, `allows`,
// `validatePrecisionItem`). The core's own table is `precision_capability::CMYK_FAMILIES`; this
// is its host side, written once. Phase 1 opens, views, saves and converts a CMYK document and
// edits its layer STRUCTURE; phase 2 adds New Layer, layer masks, the Brush, Eraser, Fill,
// Gradient and Paint Bucket in ink and Levels / Curves on ink (`CmykEdit.swift`); Hue/Saturation
// (and Desaturate, its Saturation −100) runs on the ink with K held.
// The selection batch adds the image-reading selections (the wand on ink, the rest on the view),
// Select and Mask, Stroke… in ink and Apply Layer Mask. Phase 4a adds groups, clipping masks,
// blend modes, Fill Opacity, the merges and Stamp Visible (`NativeLayerBlending.cmykModes`). Phase
// 4b adds geometry: Image Size, Canvas Size, Crop, Perspective Crop, Trim, Reveal All, rotation and
// flips, Free Transform / Transform (layers, selected pixels, masks, Transform Mask), Warp (applied
// at once, no re-editable field), Puppet Warp, Perspective Warp, Liquify, the Move tool and
// align/distribute. The adjustments batch adds every Image ▸ Adjustments, New Adjustment Layer,
// Auto and Filter item enabled in CMYK, Edit ▸ Fade and Repeat Last Filter
// (`CmykAdjustFilter.swift`, whose table answers `allowsTagged`, `allowsAdjustment` and the MCP
// narrowing). The channels batch adds alpha channels (Save/Load Selection, the Channels
// menu, painting one), Quick Mask, paths and vector masks, Select ▸ Inverse, Modify, Grow,
// Similar and Transform Selection, the Magnetic Lasso and the Pen, luminosity masks, and the Move
// tool and ⌘T on one ink plate's selected pixels. The retouch batch adds the painting and
// retouching tools enabled in CMYK (Dodge/Burn with Protect Tones, Sponge, Blur,
// Sharpen, Smudge, the History Brush, the Clone Stamp, the healing family, Patch, Remove and
// Content-Aware Move: `CmykRetouch.swift`'s table), the paint and fill blend modes on ink and
// Image ▸ Adjustments ▸ Equalize. The artwork batch adds type, shape and Solid Color / Gradient
// fill layers in ink (`CmykArtwork.swift`), the Type and shape tools, Artwork Colors… and
// Rasterize. The styles batch adds layer effects in ink, knockout, Blend Interior and the
// style clipboard; the Blend If batch Blend If on Gray and the four plates. Auto-Align (registered
// on the layer's view, every ink warped) and Reduce Noise from Stack (per ink) run too; Red
// Eye, Auto-Align's Vignette Removal and Geometric Distortion and Auto-Blend refuse by name
// (`designs/cmyk-mode-status.md`).

extension RasterDocument {
    /// True for a CMYK document — read off the captured descriptor, so a
    /// per-row panel or menu check costs no FFI call.
    var isCmyk: Bool { precisionInfo?["storage"] as? String == PrecisionDocumentDescriptor.cmykStorage }
}

enum CmykDocument {
    /// Every MCP tool a CMYK document answers: each `.cmykTool` entry of the `<prefix>ToolFacts`
    /// tables (`AgentServer+Registry.swift`).
    static let tools: Set<String> = AgentServer.tools(with: .cmykTool)

    /// The menu items a CMYK document enables; every other document item is disabled
    /// (`validatePrecisionItem`). View-only and window items are answered before this list.
    static let selectors: Set<String> = [
        "convertToRGBMode:", "convertToCMYKMode:",
        "showInfo:", "showHistory:", "toggleLayersPanel:", "showChannels:",
        "undo:", "redo:", "zoomInAction:", "zoomOutAction:", "zoomActualAction:", "zoomFitAction:",
        "duplicateLayer:", "deleteLayer:", "bringToFront:", "bringForward:", "sendBackward:",
        "sendToBack:", "flattenImage:", "setLayerLabel:",
        "lockTransparency:", "lockPixels:", "lockPosition:", "lockAll:",
        "selectAll:", "deselect:", "reselect:", "newGuide:", "clearGuides:", "exportPSD:",
        // Phase 2: New Layer, layer masks, Fill and its chords, Levels and Curves.
        "newLayer:", "addLayerMaskRevealAll:", "addLayerMaskHideAll:", "addLayerMaskFromSelection:",
        "deleteLayerMask:", "toggleLayerMaskEnabled:", "toggleMaskContentLink:", "showMaskProperties:",
        "fillSelection:", "fillSelectionFromDeleteKey:", "fillWithForeground:", "fillWithBackground:",
        "fillWithForegroundPreserving:", "fillWithBackgroundPreserving:",
        "showLevels:", "showCurves:", "newAdjustmentLayerLevels:", "newAdjustmentLayerCurves:",
        "adjustmentOptions:",
        // Hue/Saturation's Saturation −100 (Hue/Saturation itself is a tagged item, `allowsTagged`).
        "applyDesaturate:",
        // The selection batch: Color Range, Select and Mask, Subject / Objects / People / Face
        // Parts, Stroke… and Apply Layer Mask.
        "showColorRange:",
        "refineSelection:",
        "selectSubject:",
        "showSubjectTargets:",
        "showPeopleTargets:",
        "selectPeople:",
        "showFaceParts:",
        "strokeSelection:",
        "applyLayerMask:",
        // Phase 4a: groups, clipping masks, merges, Stamp Visible and Blending Options (Fill).
        "groupLayers:",
        "ungroupLayers:",
        "toggleClippingMask:",
        "mergeDown:",
        "mergeVisible:",
        "stampVisible:",
        "layerStyle:",
        "clearLayerStyle:",
        // The styles batch: every layer effect, knockout and Blend Interior, and the style
        // clipboard; the Blend If batch: Blend If on Gray and the plates (Red, Green and Blue
        // refuse by name in the core).
        "layerStyleEffect:",
        "copyLayerStyle:",
        "pasteLayerStyle:",
        // Phase 5: Assign/Convert to Profile, 8/16 Bits/Channel, Proof Setup, the exports and
        // Levels/Curves presets.
        "assignProfile:",
        "convertToProfile:",
        "convertToEightBits:",
        "convertToSixteenBits:",
        "showProofSetup:",
        "toggleProofColors:",
        "applyProofPresetFromMenu:",
        "applyWorkingProof:",
        "showExportWorkspace:",
        "quickExport:",
        "newAdjustmentLayerFromPhotoshopPreset:",
        // Phase 4b: geometry, transforms, warps, Liquify and align/distribute.
        "resizeImage:",
        "showCanvasSize:",
        "cropToSelection:",
        "showTrim:",
        "revealAllCanvas:",
        "rotateCW:",
        "rotateCCW:",
        "rotate180:",
        "rotateArbitrary:",
        "flipH:",
        "flipV:",
        "freeTransform:",
        "freeTransformCopy:",
        "transformAgain:",
        "transformAgainWithCopy:",
        "showPersistedWarp:",
        "transformMask:",
        "showPuppetWarp:",
        "showPerspectiveWarp:",
        "showLiquify:",
        "alignLeft:",
        "alignCenterX:",
        "alignRight:",
        "alignTop:",
        "alignCenterY:",
        "alignBottom:",
        "distributeHorizontally:",
        "distributeVertically:",
        // The adjustments batch: the adjustment items with their own selectors
        // (`CmykAdjustFilter.legacySelectors`), Image ▸ Auto and Edit ▸ Fade; the tagged items
        // are `allowsTagged`'s and the Filter menu `CmykAdjustFilter.allowsFilterAction`'s.
        "showAdjustments:",
        "applyInvert:",
        "showPosterize:",
        "showThreshold:",
        "newAdjustmentLayerBCS:",
        "newAdjustmentLayerInvert:",
        "newAdjustmentLayerPosterize:",
        "newAdjustmentLayerThreshold:",
        "autoTone:",
        "autoContrast:",
        "autoColor:",
        "showFade:",
        // Phase 4c: copy and paste, Layer Via, Duplicate to Document, Stamp Selected.
        "copy:",
        "copyMerged:",
        "cut:",
        "paste:",
        "pasteAsNewLayer:",
        "pasteInPlace:",
        "pasteInto:",
        "pasteOutside:",
        "layerViaCopy:",
        "layerViaCut:",
        "duplicateToNewDocument:",
        "stampSelected:",
        // The channels batch: Select ▸ Inverse, Modify, Grow, Similar and Transform Selection,
        // Quick Mask, Save/Load Selection, the Channels menu, luminosity masks, paths and vector
        // masks.
        "invertSelection:",
        "featherSelection:",
        "growSelection:",
        "shrinkSelection:",
        "borderSelection:",
        "smoothSelection:",
        "selectGrow:",
        "selectSimilar:",
        "transformSelection:",
        "toggleQuickMask:",
        "saveSelectionSheet:",
        "loadSelectionSheet:",
        "loadChannelAsSelection:",
        "newChannel:",
        "duplicateChannel:",
        "renameChannel:",
        "channelOptions:",
        "deleteChannel:",
        "invertChannel:",
        "addLuminosityMasks:",
        "makeWorkPath:",
        "makeSelectionFromPath:",
        "duplicateCurrentPath:",
        "toggleClippingPath:",
        "deleteCurrentPath:",
        "addVectorMaskFromCurrentPath:",
        "addVectorMaskRevealAll:",
        "deleteVectorMask:",
        "toggleVectorMaskEnabled:",
        "toggleVectorMaskLinked:",
        // The retouch batch: Image ▸ Adjustments ▸ Equalize on ink.
        "equalize:",
        // Edit ▸ Content-Aware Fill… on ink, the `content_aware_fill` mirror (`CmykRetouch`).
        "contentAwareFill:",
        // The artwork batch: Solid Color / Gradient fill layers, Rasterize and Artwork Colors….
        // Pattern… too, its texels separated at render.
        "newSolidColorFillLayer:",
        "newGradientFillLayer:",
        "newPatternFillLayer:",
        // Layer ▸ Layer Content Options… reopens a fill's dialog.
        "layerContentOptions:",
        "rasterizeArtwork:",
        "editArtworkColors:",
        // Layer ▸ Replace Missing Fonts…, its stored inks kept (`replace_missing_fonts`).
        "replaceMissingFonts:",
        // The remaining-gaps batch: Fill and Stroke Path, Apply Image, Calculations and View ▸
        // Gamut Warning.
        "fillCurrentPath:",
        "strokeCurrentPath:",
        "applyImageSheet:",
        "calculationsSheet:",
        "toggleGamutWarning:",
        // Edit ▸ Generative Fill…, Edit ▸ Generative Expand… and Layer ▸
        // Generative Variations… on 8-bit CMYK (`cmyk_gen_fill`; 16-bit CMYK refuses in
        // `GenerativeFillRun.refusal`).
        "showGenerativeFill:",
        "showGenerativeExpand:",
        "showGenerativeVariations:",
        // Layer ▸ Stack Mode ▸ … per ink (`cmyk_stack_mode`).
        "stackMode:",
        // Edit ▸ Auto-Align Layers… (on the layer's view, every ink warped) and Layer ▸
        // Reduce Noise from Stack… (its statistic per ink).
        "autoAlignLayers:",
        "showReduceNoiseFromStack:",
    ]

    /// The tagged adjustment items a CMYK document enables (Image ▸ Adjustments ▸ <op>… and Layer ▸
    /// New Adjustment Layer ▸ <op>…, whose shared selectors take every `AdjustmentMenuOrder` op by
    /// tag): the ops `CmykAdjustFilter.adjustments` runs at `depth`, as a layer where the op
    /// has the layer kind.
    static func allowsTagged(_ action: String, tag: Int, depth: Int = 16) -> Bool {
        ["showAdjustmentSheet:", "newAdjustmentLayerOp:"].contains(action)
            && CmykAdjustFilter.allowsAdjustmentAction(action, tag: tag, depth: depth) == true
    }

    /// What a CMYK document can do in this version, and the way past it.
    static let limitation = "A CMYK document opens, displays, saves, exports, proofs and converts "
        + "(Image ▸ Mode, Assign / Convert to Profile, 8/16 Bits/Channel); its layers can be "
        + "added, reordered, renamed, shown or hidden, duplicated, deleted, labelled, locked, set to an "
        + "opacity and a blend mode, given a layer style (Fill Opacity, every layer effect in ink, "
        + "knockout and Blend Interior, with the Global Light, and Blend If on Gray, Cyan, Magenta, "
        + "Yellow or Black), masked (and the mask applied), grouped "
        + "(Normal or Pass Through), clipped, merged (a group with Merge Group), stamped (Stamp "
        + "Visible and Stamp Selected) and flattened; Cut, Copy, Copy Merged and Paste (in Place, "
        + "Into, Outside) work on and between CMYK documents, keeping ink between documents of one "
        + "profile and converting across modes and profiles (relative colorimetric with black-point "
        + "compensation); Layer Via Copy / Cut and Duplicate to Document copy layers; the Brush, "
        + "Eraser, Fill, Gradient, Paint Bucket and Edit ▸ Stroke… paint in ink in every blend mode "
        + "but Pass Through; Dodge/Burn, Sponge, Blur, Sharpen, Smudge, the History Brush, the Clone "
        + "Stamp, the Healing Brush, Spot Healing, Patch, Remove, Content-Aware Fill and Move "
        + "retouch ink; type, shape and Solid Color and Gradient fill layers are made, edited and "
        + "rasterized with their colours in ink; the adjustments (Equalize included) adjust ink, "
        + "destructively or, where the op has the layer kind, as an adjustment layer (not "
        + "Exposure, Vibrance, Black & White, Hue Rotate, Grayscale, Sepia or White Balance), with "
        + "Image ▸ Auto Tone, Auto Contrast and Auto Color; the filters run on ink (not Lens "
        + "Correction, the Develop Filter, Depth Blur or Remove Red Eye), with Edit ▸ Fade and "
        + "Repeat Last Filter; "
        + "the Magic Wand (on ink), Quick Selection, Object Selection, Select Subject, People, Face "
        + "Parts, Color Range, Select and Mask, the Magnetic Lasso, Select ▸ Inverse, Grow, "
        + "Similar, Modify and Transform Selection, Quick Mask, alpha channels (Save/Load "
        + "Selection, from a plate or the composite) and luminosity masks make selections; paths "
        + "and vector masks work; Image Size, Canvas Size, Crop, Perspective Crop, Trim, Reveal "
        + "All, rotation, flips, Free Transform, Warp, Puppet Warp, Perspective Warp, Liquify, the "
        + "Move tool and align/distribute reshape the ink, and the Move tool and ⌘T move one ink "
        + "plate's selected pixels; one ink plate or an alpha channel is painted, filled, filtered "
        + "and adjusted as a colour channel is, Apply Image and Calculations read and write plates "
        + "and channels, paths fill and stroke in ink, View ▸ Gamut Warning flags what the "
        + "proof profile cannot print, and layer effects, knockout, Blend If and Global Light work "
        + "in ink, and Layer ▸ Stack Mode takes its statistic per ink; Auto-Align (but for "
        + "Vignette Removal and Geometric Distortion) registers on the view and warps every ink, "
        + "and Reduce Noise from Stack takes its Median or Mean per ink. Red Eye and Auto-Blend "
        + "are not available in CMYK: use Image ▸ Mode ▸ RGB Color to edit in RGB, and CMYK Color "
        + "to return."

    /// `get_precision`'s (and `get_document`'s `precision_support`) answer for a CMYK document:
    /// its colour mode, the core's depth, storage, CMYK and view profiles, this gate's tools and
    /// what the mode cannot do yet (`limitation`).
    static func support(_ doc: RasterDocument) -> [String: Any] {
        let info = doc.precision?.cmykInfo() ?? [:]
        return [
            "color_mode": "cmyk", "bits_per_channel": info["depth"] ?? 16,
            "storage": info["storage"] ?? NSNull(), "cmyk_profile": info["profile"] ?? NSNull(),
            "view_profile": info["view_profile"] ?? NSNull(), "supported_tools": tools.sorted(),
            "limitations": limitation,
        ]
    }

    /// The refusal of an ink key (`ink`, `start_ink`, `end_ink`) on one ink plate or an alpha
    /// channel: a single plane holds a grey, so the colour's grey is painted, as on a colour plane.
    static let plateInkRefusal = "A plate or alpha channel paints in grey: pass color (its grey is "
        + "painted; white leaves no ink)."

    /// Auto-Align's lens corrections on a CMYK document: the core's
    /// `precision_align_vignette::cmyk_refusal` sentences verbatim, asked before any
    /// registration. nil when neither is requested. The core keeps its own refusal.
    static func autoAlignLensRefusal(vignette: Bool, distortion: Bool) -> String? {
        if vignette { return "Vignette Removal does not work in CMYK." }
        if distortion { return "Geometric Distortion does not work in CMYK." }
        return nil
    }

    /// The refusal of a tool outside `tools`.
    static func refusal(tool: String) -> String {
        "\(tool) is unavailable in a CMYK document. " + limitation
    }

    /// Whether a CMYK document takes adjustment `op` at `depth`, as a layer when `layer`
    /// (`CmykAdjustFilter.adjustments`).
    static func allowsAdjustment(_ op: String, depth: Int = 16, layer: Bool = false) -> Bool {
        CmykAdjustFilter.allowsAdjustment(op, depth: depth, layer: layer)
    }

    /// The refusal of an adjustment op outside `allowsAdjustment`: its own reason
    /// (`CmykAdjustFilter.refusal`), then the way past it.
    static func adjustmentRefusal(_ op: String, layer: Bool = false) -> String {
        CmykAdjustFilter.refusal(op: op, layer: layer) + ". " + limitation
    }

    /// An RGB document's refusal of CMYK-only arguments (#1): the ink keys and the CMYK tone
    /// keys mean nothing there. nil when none is passed.
    static func rgbDocumentRefusal(tool: String, arguments: [String: Any]) -> String? {
        if AgentServer.cmykInkTools.contains(tool),
           AgentServer.cmykInkKeys.contains(where: { arguments[$0] != nil })
            || nestedInk(arguments) {
            return "ink is a CMYK document's colour. Pass color."
        }
        if tool == "apply_filter",
           let key = ["foreground_ink", "background_ink"].first(where: { arguments[$0] != nil }) {
            return "\(key) is a CMYK document's clouds ink. Pass "
                + key.replacingOccurrences(of: "_ink", with: "_color") + "."
        }
        if let key = toneKeys(arguments).first(where: { AgentServer.cmykToneKeys.contains($0) }) {
            return "\"\(key)\" is a CMYK document's Levels/Curves channel; an RGB document takes "
                + "rgb, r, g and b (and Curves' rgb_map, r_map, g_map and b_map)."
        }
        return nil
    }

    /// Whether an artwork call's rows carry an `ink` (#1): `color_runs[]`, `stops[]` and
    /// `gradient.stops[]`.
    private static func nestedInk(_ arguments: [String: Any]) -> Bool {
        let gradient = (arguments["gradient"] as? [String: Any]) ?? [:]
        let rows = AgentServer.cmykInkRows.flatMap { (arguments[$0] as? [[String: Any]]) ?? [] }
            + ((gradient["stops"] as? [[String: Any]]) ?? [])
        return rows.contains { $0["ink"] != nil }
    }

    /// The Levels/Curves keys a call naming `levels` or `curves` passes: `params`' keys, and
    /// `apply_filter` levels' flat ones (an `edit_adjustment_layer` naming no op is the core's to
    /// refuse).
    private static func toneKeys(_ arguments: [String: Any]) -> [String] {
        let op = (arguments["op"] as? String) ?? (arguments["filter"] as? String)
        guard let op, ["levels", "curves"].contains(op) else { return [] }
        var keys = Array(((arguments["params"] as? [String: Any]) ?? [:]).keys)
        if arguments["filter"] as? String == "levels" { keys += arguments.keys }
        return keys.sorted()
    }

    /// The gate's answer for one MCP call: nil to admit it, else the refusal. Admitted tools whose
    /// ARGUMENTS can still reach an RGB-only path are narrowed here.
    static func refusal(tool: String, arguments: [String: Any], depth: Int = 16) -> String? {
        // The adjustments batch: Depth Blur and Remove Red Eye are RGB-only tools.
        if tool == "depth_blur" { return CmykAdjustFilter.refusal(op: "depth_blur", layer: false) }
        if tool == "red_eye" || tool == "red_eye_auto" {
            return CmykAdjustFilter.refusal(op: "red_eye", layer: false)
        }
        guard tools.contains(tool) else { return refusal(tool: tool) }
        // The retouch batch: the retouch tools' own table and narrowing (`CmykRetouch`).
        if let reason = CmykRetouch.refusal(tool: tool, arguments: arguments, depth: depth) {
            return reason + " " + limitation
        }
        // Phase 2's paint and tone tools: a layer or its mask, ink (no rgba32), in every blend
        // mode but Pass Through (the retouch batch).
        let painting: Set<String> = ["brush_stroke", "eraser_stroke", "fill", "gradient", "fill_selection",
                                     "apply_filter", "stroke_selection"]
        let target = arguments["target"] as? String
        // The channels batch: an alpha channel paints and fills as at 16 bits; the remaining-gaps
        // batch: one ink plate as a colour channel is, and filters and adjustments on either.
        let scalarTarget = target.map { CmykPlane(key: $0) != nil || $0.hasPrefix("channel:") } ?? false
        if painting.contains(tool) {
            if scalarTarget, AgentServer.cmykInkKeys.contains(where: { arguments[$0] != nil }) {
                return plateInkRefusal
            }
            if let target, !["layer", "mask"].contains(target), !scalarTarget {
                return "A CMYK document paints and adjusts a layer, its mask, one ink plate or an "
                    + "alpha channel: target \"\(target)\" is unavailable. " + limitation
            }
            if let blend = arguments["blend_mode"] as? String,
               blend.lowercased().replacingOccurrences(of: " ", with: "") == "passthrough" {
                return CmykPaint.modeRefusal(DR_BLEND_PASS_THROUGH).map { $0 + " " + limitation }
            }
            if arguments["rgba32"] != nil {
                return "rgba32 is a 32-bit float document's colour; a CMYK document paints in ink. "
                    + "Pass ink (C, M, Y, K percent) or color."
            }
        }
        // The adjustments and filters `CmykAdjustFilter` admits at the document's depth, on ink
        // keys.
        if ["apply_filter", "add_adjustment_layer", "edit_adjustment_layer"].contains(tool) {
            if let named = (arguments["filter"] as? String) ?? (arguments["op"] as? String) {
                let op = named == "adjust" ? "bcs" : named
                let layer = tool != "apply_filter"
                if CmykAdjustFilter.adjustments[op] != nil {
                    if !allowsAdjustment(op, depth: depth, layer: layer) {
                        return adjustmentRefusal(op, layer: layer)
                    }
                } else if !layer, CmykAdjustFilter.filters[op] != nil {
                    if !CmykAdjustFilter.allowsFilter(op, depth: depth) {
                        return CmykAdjustFilter.refusal(op: op, layer: false) + ". " + limitation
                    }
                    // A filter runs on a layer's ink, one ink plate or an alpha channel, as the
                    // Filter menu's gate has it.
                    if let target, target != "layer", !scalarTarget {
                        return "A CMYK document runs filters on a layer's ink, one ink plate or an "
                            + "alpha channel: target \"\(target)\" is unavailable. " + limitation
                    }
                } else {
                    return tool == "apply_filter" ? refusal(tool: "apply_filter \(named)")
                        : adjustmentRefusal(op, layer: layer)
                }
            }
            // #1 phase 5: Auto is admitted on Levels and Curves (`cmykAutoArguments`).
            if arguments["auto"] != nil,
               !["levels", "curves"].contains((arguments["filter"] as? String) ?? (arguments["op"] as? String) ?? "levels") {
                return "Auto applies to Levels and Curves in a CMYK document."
            }
            // The CMYK tone keys belong to a layer (or no target); one ink plate or an alpha channel
            // is a scalar target and takes the RGB scalar vocabulary, as the sheets do. A mask is
            // asked neither (#1).
            if target == nil || target == "layer",
               let key = toneKeys(arguments).first(where: { AgentServer.rgbToneKeys.contains($0) }) {
                return "The RGB channel key \"\(key)\" is unavailable in a CMYK document: its Levels and "
                    + "Curves channels are cmyk (the composite), c, m, y and k (and Curves' cmyk_map, "
                    + "c_map, m_map, y_map and k_map)."
            }
            if scalarTarget,
               let key = toneKeys(arguments).first(where: { AgentServer.cmykToneKeys.contains($0) }) {
                return "\"\(key)\" is a CMYK layer's Levels/Curves channel; a plate or alpha channel "
                    + "target takes rgb (and Curves' rgb_map)."
            }
        }
        switch tool {
        case "save_copy":
            let format = (arguments["format"] as? String)?.lowercased() ?? ImageDocument.nativeFormatName
            if !ImageDocument.isNativeFormatName(format), !["tiff", "tif", "jpeg", "jpg"].contains(format) {
                return "Saving a CMYK document as \(format) is unavailable: it saves as .doctored, TIFF or "
                    + "JPEG, and Export as PSD (export_psd) writes it as a PSD, PSB or layered TIFF. "
                    + "Convert to RGB Color for other formats."
            }
        case "render":
            if let channel = arguments["channel"] as? String, CmykPlane(key: channel) == nil,
               channel != "composite" {
                return "A CMYK document renders its composite or one ink plane: channel is "
                    + "cyan, magenta, yellow or black."
            }
        default:
            break
        }
        return nil
    }

    /// What a CMYK TIFF or JPEG of `doc` loses: the file is the composite flattened onto paper,
    /// with no alpha sample, so the core's editable-state list (layers, masks, styles, layer
    /// properties and bounds, saved channels, guides, paths — which a TIFF keeps in its 8BIM run
    /// unless one lies past 128 canvases — transparency), plus the 16-bit ink
    /// a JPEG writes at 8 bits, the profile when it is not embedded and the capture metadata when
    /// it is stripped. The spot channels a JPEG composites are `ImageDocument.saveLosses`' line.
    /// `[]` only when the file reopens as the same document, which Save keeps in place on
    /// (`ImageDocument.canPreserveState`); any other format is the `.doctored`/TIFF/JPEG line.
    static func saveLosses(_ doc: RasterDocument, format: ExportFormat?, embedProfile: Bool,
                           stripMetadata: Bool) throws -> [String] {
        guard let format, format == .tiff || format == .jpeg else {
            return ["editable CMYK document state; a CMYK document saves as .doctored, TIFF or JPEG"]
        }
        guard let typed = doc.precision else { throw DoctoredCoreError(message: "No CMYK document is loaded.") }
        var losses = try typed.flatStateLosses(alphaKept: false, tiff: format == .tiff)
        if format == .jpeg, typed.cmykInfo()?["depth"] as? Int == 16 {
            losses.append("16-bit CMYK precision (JPEG holds 8 bits)")
        }
        if !embedProfile { losses.append("embedded color profile") }
        return losses + doc.flatPacketLosses(format: format, stripMetadata: stripMetadata)
    }

    /// The open's profile sentence of a CMYK document: an untagged file's assignment of the
    /// working CMYK profile (`cmyk_io::untagged_sentence`), carried in its import warnings; nil
    /// for a file that brought its own profile. What `on_open_refusal` reports.
    static func assignedSentence(_ info: [String: Any]?) -> String? {
        (info?["import_warnings"] as? [String])?.first {
            $0.hasPrefix("The image is CMYK with no embedded colour profile")
        }
    }

    /// The canvas tools a CMYK document admits at `depth`: selections, the view, the eyedropper,
    /// (phase 2) the Brush, Eraser, Fill and Gradient tools, which paint in ink, (the selection
    /// batch) the Magic Wand on ink and Quick Selection, Object Selection and Select Subject on the
    /// view, (phase 4b) the Move, Crop, Perspective Crop and Ruler tools, and (the retouch batch)
    /// the retouch tools `CmykRetouch` enables.
    static func allows(_ tool: EditorTool, depth: Int = 16) -> Bool {
        if CmykRetouch.tools[tool] != nil { return CmykRetouch.allows(tool, depth: depth) }
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .hand, .zoom, .eyedropper,
             .brush, .eraser, .fill, .gradient:
            return true
        case .wand:
            return true
        case .quickSelect:
            return true
        case .objectSelect:
            return true
        case .subject:
            return true
        case .move:
            return true
        case .crop:
            return true
        case .perspectiveCrop:
            return true
        case .ruler:
            return true
        case .magneticLasso:
            return true
        case .pen:
            return true
        // The artwork batch: the Type tool and the shape tools, in ink.
        case .text, .shapeRect, .shapeEllipse, .shapeLine:
            return true
        default:
            return false
        }
    }
}
