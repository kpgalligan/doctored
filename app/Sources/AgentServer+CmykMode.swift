import AppKit
import UniformTypeIdentifiers

// The CMYK document over MCP (#1 phase 1): `convert_mode` — Image ▸ Mode ▸ CMYK Color… / RGB
// Color… (`EditorViewController+CmykMode.swift`'s conversion and undo step) — and the CMYK route
// `dispatchPrecision` asks FIRST: the ink readout beside the view's RGB on `sample_color` and
// `sample_pixel` (the Info panel's CMYK numbers), `save_copy`'s CMYK TIFF/JPEG/.rz and
// `get_precision`'s CMYK reply. The gate (`CmykDocument`) has already refused every other tool.

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let cmykModeHandlers: [String: ToolHandler] = [
        "convert_mode": { $0.convertMode },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let cmykModeToolFacts: [String: ToolFacts] = [
        "convert_mode": [.commonTool, .floatTool, .cmykTool],
    ]

    private static let multichannelRefusal =
        "Multichannel documents are not supported."

    /// Mirrors Image ▸ Mode ▸ CMYK Color… and RGB Color… (`CmykModeSheetController`) and
    /// Grayscale (`convertToGrayscaleMode:`, #1 phase 1a) and Lab Color (`convertToLabMode:`,
    /// #1 Lab phase 1) and Indexed Color… (`convertToIndexedMode:`, `IndexedModeSheetController`,
    /// #1 Indexed phase 1a): the same capture, plan, job and one undo step. A Grayscale, Lab
    /// Color or Indexed Color conversion that must flatten answers the core's sentence in-band;
    /// `flatten: true` converts.
    /// Since #1 Bitmap phase 1a also Bitmap… (`convertToBitmapMode:`, `BitmapModeSheetController`)
    /// with `method`, `resolution` and `flatten` (since phase 1b also Halftone Screen's `frequency`,
    /// `angle` and `shape` and Custom Pattern's `pattern`), and Grayscale on a Bitmap document with
    /// `size_ratio` (`BitmapToGraySheetController`).
    /// `mode: multichannel` refuses by name (`designs/multichannel-mode.md`: not built).
    func convertMode(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let modeName = try requiredString(a, "mode")
        if modeName.lowercased() == "multichannel" {
            throw ToolError(message: Self.multichannelRefusal)
        }
        guard let mode = ColorModeTarget(rawValue: modeName) else {
            throw ToolError(message: "mode must be gray, rgb, cmyk, lab, indexed, duotone or bitmap.")
        }
        // #1 Duotone phase 1a: Duotone's options apply to mode duotone only, and mode duotone (and
        // mode gray on a Duotone document) is AgentServer+Duotone.swift's.
        if mode != .duotone, let key = ["inks", "overprints"].first(where: { a[$0] != nil }) {
            throw ToolError(message: "\(key) applies to mode duotone only.")
        }
        if let result = try convertModeDuotone(a, mode: mode, document: document) { return result }
        var options = CmykConversionOptions()
        if let name = stringArg(a, "intent") {
            guard let intent = RenderingIntent(rawValue: name) else {
                throw ToolError(message: "intent must be perceptual, relative, saturation or absolute.")
            }
            options.intent = intent
        }
        options.blackPointCompensation = boolArg(a, "black_point_compensation")
            ?? options.blackPointCompensation
        options.flatten = boolArg(a, "flatten") ?? false
        // Indexed → RGB is the RGB document the storage already is, in its own profile.
        if mode == .rgb, document.doc?.isIndexed == true, a["rgb_space"] != nil {
            throw ToolError(message: "rgb_space does not apply to an Indexed Color document: RGB Color "
                + "keeps the document's own profile.")
        }
        let profile = try modeProfile(a, mode: mode)
        var graySpace: String?
        if let name = stringArg(a, "gray_space") {
            guard mode == .gray else { throw ToolError(message: "gray_space applies to mode gray only.") }
            guard GrayWorkingSpace(rawValue: name) != nil else {
                throw ToolError(message: "gray_space must be one of "
                    + GrayWorkingSpace.allCases.map(\.rawValue).joined(separator: ", ") + ".")
            }
            graySpace = name
        }
        // #1 Indexed phase 1a: the sheet's options (`IndexedDocument.options`).
        let indexedKeys = ["palette", "table", "colors", "forced", "transparency", "matte", "matte_rgb",
                           "dither", "amount", "preserve_exact"]
        if mode != .indexed, let key = indexedKeys.first(where: { a[$0] != nil }) {
            throw ToolError(message: "\(key) applies to mode indexed only.")
        }
        var indexed: [String: Any] = [:]
        if mode == .indexed {
            do {
                indexed = try IndexedDocument.options(from: a)
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
        }
        // #1 Bitmap phase 1a: the Bitmap sheet's options (`BitmapDocument.options`), and the Size
        // Ratio toward Grayscale from a Bitmap document.
        let fromBitmap = document.doc?.isBitmap == true
        let bitmapKeys = ["method", "resolution", "frequency", "angle", "shape", "pattern"]
        if mode != .bitmap, let key = bitmapKeys.first(where: { a[$0] != nil }) {
            throw ToolError(message: "\(key) applies to mode bitmap only.")
        }
        if a["size_ratio"] != nil, !(mode == .gray && fromBitmap) {
            throw ToolError(message: "size_ratio applies to mode gray on a Bitmap document only.")
        }
        var bitmap: [String: Any] = [:]
        do {
            if mode == .bitmap { bitmap = try BitmapDocument.options(from: a) }
            if mode == .gray, fromBitmap { bitmap = ["size_ratio": try BitmapDocument.sizeRatio(from: a)] }
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let conversion: CmykModeConversion
        let prepared: PreparedDocumentEdit
        do {
            conversion = try CmykModeConversion(document, target: mode, profile: profile.data, options: options,
                                                graySpace: graySpace, indexedOptions: indexed,
                                                bitmapOptions: bitmap)
            prepared = try conversion.compute()
            try conversion.publish(prepared, to: document, record: .notACommand)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let doc = document.doc
        // A Bitmap result is 1 bit deep, in its Gray profile.
        let bitmapInfo: [String: Any]? = doc?.isBitmap == true
            ? ["depth": 1, "profile": (doc?.precisionInfo?["gray"] as? [String: Any])?["profile"] ?? NSNull()]
            : nil
        let info = bitmapInfo ?? doc?.precision?.cmykInfo() ?? doc?.precisionInfo?["gray"] as? [String: Any]
            ?? doc?.precisionInfo?["lab"] as? [String: Any]
            ?? (doc?.isIndexed == true ? ["depth": 8] : nil)
        return try jsonResult([
            "ok": true, "color_mode": mode.rawValue,
            "depth": info?["depth"] ?? (doc?.isFloat == true ? 32 : doc?.isHighDepth == true ? 16 : 8),
            "profile": info?["profile"] ?? doc?.profileName ?? NSNull(),
            // A Lab conversion always runs relative colorimetric without BPC (`lab_convert`), so
            // the reply reports what ran; the arguments are still accepted so recorded steps replay.
            "intent": mode == .lab ? RenderingIntent.relative.rawValue : options.intent.rawValue,
            "black_point_compensation": mode == .lab ? false : options.blackPointCompensation,
            "flattened": options.flatten && (doc?.layerCount ?? 0) == 1,
            "working_memory_plan_bytes": conversion.scratch,
            // #1 Indexed phase 1a: the palette size and transparent index of an Indexed result.
            "indexed": doc?.precisionInfo?["indexed"] ?? NSNull(),
            "document": summary(document),
        ])
    }

    /// The destination profile `convert_mode` names: toward CMYK the working, an installed or a
    /// file CMYK profile; toward RGB a working space.
    private func modeProfile(_ a: [String: Any], mode: ColorModeTarget) throws -> (data: Data?, name: String?) {
        switch mode {
        case .cmyk:
            if a["rgb_space"] != nil { throw ToolError(message: "rgb_space applies to mode rgb only.") }
            switch stringArg(a, "profile") ?? "working" {
            case "working":
                return (nil, WorkingCMYK.installedName)
            case "installed":
                guard let name = stringArg(a, "name"), let installed = InstalledProfiles.named(name),
                      installed.description.space == "cmyk" else {
                    throw ToolError(message: "profile installed needs name, an installed CMYK profile "
                        + "(list_installed_profiles kinds [\"cmyk\"]).")
                }
                return (try Data(contentsOf: installed.url), installed.name)
            case "file":
                let path = try requiredString(a, "path")
                let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
                guard let data = try? Data(contentsOf: url) else {
                    throw ToolError(message: "Could not read the CMYK profile at \(url.path).")
                }
                return (data, url.lastPathComponent)
            default:
                throw ToolError(message: "profile must be working, installed or file.")
            }
        case .gray:
            if a["profile"] != nil || a["name"] != nil || a["path"] != nil || a["rgb_space"] != nil {
                throw ToolError(message: "mode gray takes gray_space; profile, name, path and rgb_space "
                    + "apply to modes cmyk and rgb.")
            }
            let space = stringArg(a, "gray_space").flatMap(GrayWorkingSpace.init(rawValue:))
                ?? ColorSettings.workingGray
            return (nil, space.displayName)
        case .lab:
            if a["profile"] != nil || a["name"] != nil || a["path"] != nil || a["rgb_space"] != nil {
                throw ToolError(message: "mode lab takes only flatten; profile, name, path, rgb_space "
                    + "and gray_space apply to modes cmyk, rgb and gray.")
            }
            return (nil, ColorModeTarget.lab.title)
        case .duotone:
            // `convertModeDuotone` answers every Duotone call first.
            throw ToolError(message: "mode duotone takes inks and overprints.")
        case .indexed:
            if a["profile"] != nil || a["name"] != nil || a["path"] != nil || a["rgb_space"] != nil {
                throw ToolError(message: "mode indexed takes the Indexed Color options and flatten; "
                    + "profile, name, path, rgb_space and gray_space apply to modes cmyk, rgb and gray.")
            }
            return (nil, ColorModeTarget.indexed.title)
        case .bitmap:
            if a["profile"] != nil || a["name"] != nil || a["path"] != nil || a["rgb_space"] != nil {
                throw ToolError(message: "mode bitmap takes method, resolution, frequency, angle, "
                    + "shape, pattern and flatten; profile, name, path, rgb_space and gray_space apply "
                    + "to modes cmyk, rgb and gray.")
            }
            return (nil, ColorModeTarget.bitmap.title)
        case .rgb:
            if a["profile"] != nil || a["name"] != nil || a["path"] != nil {
                throw ToolError(message: "profile, name and path apply to mode cmyk; mode rgb takes rgb_space.")
            }
            var space = ColorSettings.workingSpace
            if let name = stringArg(a, "rgb_space") {
                guard let parsed = WorkingSpace(rawValue: name) else {
                    throw ToolError(message: "rgb_space must be srgb or displayP3.")
                }
                space = parsed
            }
            return (space.profileData, space.displayName)
        }
    }

    /// The CMYK route `dispatchPrecision` asks first; nil hands the tool to its ordinary handler.
    func dispatchCmyk(_ tool: String, _ a: [String: Any], document: ImageDocument) throws -> String? {
        guard let doc = document.doc, let precision = doc.precision, precision.isCmyk else { return nil }
        // #1 phase 5: the colour workflow's routes (AgentServer+CmykColour.swift).
        if let result = try dispatchCmykColour(tool, a, document: document, precision: precision) {
            return result
        }
        switch tool {
        case "sample_color", "sample_pixel":
            guard let x = intArg(a, "x"), let y = intArg(a, "y") else {
                throw ToolError(message: "\(tool) requires x and y")
            }
            let size = intArg(a, "sample_size") ?? 1
            guard [1, 3, 5].contains(size) else { throw ToolError(message: "Sample size must be 1, 3 or 5 pixels.") }
            // The view's RGB (the scoped report over the Display P3 projection), then the ink.
            var report = (try? scopedSampleReport(a, document: document)) ?? ["x": x, "y": y]
            let ink: [String: Any]
            do {
                let context = try PrecisionWorkContext(source: precision)
                ink = try precision.cmykSample(x: x, y: y, reach: size / 2, workingLimit: context.limit,
                                               otherRetained: context.otherRetained)
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
            for key in ["ink_percent", "ink16", "alpha16", "total_ink_percent"] { report[key] = ink[key] }
            report["color_mode"] = "cmyk"
            report["cmyk_profile"] = precision.cmykInfo()?["profile"] ?? NSNull()
            report["view_space"] = "Display P3"
            return try jsonResult(report)
        case "save_copy":
            return try saveCMYKCopy(a, document: document, precision: precision)
        case "get_precision":
            // The plans each CMYK operation admits (`scripts/cmyk-admission`'s figures).
            let descriptor = (try? precision.descriptor()) ?? [:]
            var plans = ["render_scratch_bytes", "cmyk_save_scratch_bytes", "cmyk_psd_save_scratch_bytes",
                         "native_scratch_bytes",
                         "decode_scratch_bytes", "reopen_retained_bytes", "cmyk_flatten_scratch_bytes",
                         "cmyk_open_bytes_per_pixel", "retained_bytes",
                         // #1 phase 2: the paint, tone and New Layer plans.
                         "cmyk_paint_scratch_bytes", "cmyk_gradient_scratch_bytes",
                         "cmyk_bucket_scratch_bytes", "cmyk_adjust_apply_scratch_bytes",
                         "cmyk_adjust_add_scratch_bytes", "cmyk_add_layer_scratch_bytes",
                         // #1: Hue/Saturation's destructive and record plans.
                         "cmyk_huesat_apply_scratch_bytes", "cmyk_huesat_add_scratch_bytes",
                         // #1: the selections', Stroke…'s, Apply Layer Mask's and the cutout's.
                         "cmyk_wand_scratch_bytes", "cmyk_range_scratch_bytes",
                         "cmyk_quick_scratch_bytes", "cmyk_refine_scratch_bytes",
                         "cmyk_stroke_edge_scratch_bytes", "cmyk_apply_mask_scratch_bytes",
                         "cmyk_cutout_scratch_bytes", "cmyk_cutout_decontaminate_scratch_bytes",
                         // #1 phase 4a: Merge Visible's and Stamp Visible's plans.
                         "cmyk_merge_scratch_bytes", "cmyk_stamp_scratch_bytes",
                         // #1 styles batch: Merge Down of the top raster (seeded under a
                         // knockout).
                         "cmyk_merge_down_scratch_bytes",
                         // #1 phase 5: New in CMYK, the proof, the tone analysis and the ⌥
                         // clipping display.
                         "cmyk_new_scratch_bytes", "cmyk_proof_scratch_bytes",
                         "cmyk_tone_analysis_scratch_bytes", "cmyk_clip_display_scratch_bytes",
                         // #1 phase 4b: the geometry plans (`cmyk_geometry_plans`).
                         "cmyk_resize_scratch_bytes",
                         "cmyk_affine_scratch_bytes",
                         "cmyk_perspective_crop_scratch_bytes",
                         "cmyk_warp_scratch_bytes",
                         "cmyk_liquify_scratch_bytes",
                         "cmyk_transform_selected_scratch_bytes",
                         // #1 adjustments and filters: three filters, a destructive
                         // Shadows/Highlights and Fade.
                         "cmyk_filter_gaussian_scratch_bytes",
                         "cmyk_filter_surface_scratch_bytes",
                         "cmyk_filter_dust_scratch_bytes",
                         "cmyk_shadows_apply_scratch_bytes",
                         "cmyk_fade_scratch_bytes",
                         // #1 phase 4c: copy and paste, Layer Via, Stamp Selected, Merge Group.
                         "cmyk_copy_scratch_bytes", "cmyk_copy_merged_scratch_bytes",
                         "cmyk_cut_scratch_bytes", "cmyk_paste_scratch_bytes",
                         "cmyk_paste_convert_scratch_bytes", "cmyk_paste_to_rgb_scratch_bytes",
                         "cmyk_layer_via_copy_scratch_bytes", "cmyk_layer_via_cut_scratch_bytes",
                         "cmyk_paste_layers_convert_scratch_bytes",
                         "cmyk_stamp_selected_scratch_bytes", "cmyk_merge_group_scratch_bytes",
                         // #1's channels batch: the plate and composite loads, Similar, the
                         // backdrop wand and one ink plate's move and transform.
                         "cmyk_plate_load_scratch_bytes",
                         "cmyk_luminance_scratch_bytes",
                         "cmyk_similar_scratch_bytes",
                         "cmyk_backdrop_wand_scratch_bytes",
                         "cmyk_move_plate_scratch_bytes",
                         "cmyk_transform_plate_scratch_bytes",
                         // #1's retouch batch: the retouch tools' and Equalize's plans
                         // (`cmyk_retouch_plate::cmyk_retouch_plans`).
                         "cmyk_clone_scratch_bytes", "cmyk_heal_scratch_bytes",
                         "cmyk_spot_heal_scratch_bytes", "cmyk_dodge_scratch_bytes",
                         "cmyk_smudge_scratch_bytes", "cmyk_history_brush_scratch_bytes",
                         "cmyk_equalize_scratch_bytes"]
                .reduce(into: [String: Any]()) { $0[$1] = descriptor[$1] ?? NSNull() }
            // The view's charge: the render, then the display conversion of
            // its output, with `ProjectionMargin` (`projectionScratchEstimate`).
            let projection = try? doc.projectionScratchEstimate()
            plans["projection_scratch_bytes"] = projection.map { $0 as Any } ?? NSNull()
            var result = CmykDocument.support(doc)
            result["plans"] = plans
            result["document"] = summary(document)
            result["mode"] = doc.precisionName
            result["mode_description"] = "CMYK ink, displayed through its profile in Display P3"
            return try jsonResult(result)
        default:
            return nil
        }
    }

    /// `save_copy` of a CMYK document: doctored (every layer; `rz` is its legacy name), tiff or jpeg (the composite on paper).
    private func saveCMYKCopy(_ a: [String: Any], document: ImageDocument,
                              precision: RasterPrecisionDocument) throws -> String {
        try commitPendingCanvasSessions(document)
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var name = (stringArg(a, "format") ?? url.pathExtension).lowercased()
        if name == "tif" { name = "tiff" }
        if name == "jpg" { name = "jpeg" }
        if ImageDocument.isNativeFormatName(name) { name = ImageDocument.nativeFormatName }
        let type: String
        switch name {
        case ImageDocument.nativeFormatName: type = ImageDocument.nativeTypeIdentifier
        case "tiff": type = UTType.tiff.identifier
        case "jpeg": type = UTType.jpeg.identifier
        default:
            throw ToolError(message: "A CMYK document saves as doctored, tiff or jpeg.")
        }
        var scratch: UInt64 = 0
        var losses: [String] = []
        do {
            // What the copy has no place for: Save's own list (`ImageDocument.saveLosses`).
            losses = try document.saveLosses(for: type)
            let captured = try CapturedSave(document, type: type)
            scratch = captured.scratchEstimate
            _ = try RasterResources.shared.admit(scratch: captured.scratchEstimate,
                own: captured.ownRetainedBytes, remedy: captured.refusalRemedy)
            try captured.run(to: url, title: "Saving CMYK Copy")
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let info = precision.cmykInfo() ?? [:]
        var result: [String: Any] = ["ok": true, "path": url.path, "format": name, "color_mode": "cmyk",
                                     "bit_depth": name == "jpeg" ? 8 : info["depth"] ?? 16,
                                     "flattened": name != ImageDocument.nativeFormatName,
                                     "flat_save_losses": losses]
        // A TIFF keeps the spot channels (#1 1c-2a) and names the plan it was admitted at.
        if name == "tiff", document.doc?.hasSpotChannels == true { result["working_memory_plan_bytes"] = scratch }
        return try jsonResult(result)
    }
}
