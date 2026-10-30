import AppKit

/// The agent side of the phase-5 adjustments: the destructive twin
/// `apply_filter` routes to, the one place a Color Lookup layer's table is
/// kept out of a reply, and the three Auto commands.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let adjustmentsHandlers: [String: ToolHandler] = [
        // Auto Tone / Contrast / Color (AgentServer+Adjustments.swift):
        // mirrors Image > Auto Tone, > Auto Contrast, > Auto Color.
        "auto_tone": { $0.autoTone },
        "auto_contrast": { $0.autoContrast },
        "auto_color": { $0.autoColor },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let adjustmentsToolFacts: [String: ToolFacts] = [
        // #547: a CMYK document's Auto is CMYK Levels from its tone analysis.
        "auto_tone": [.repeatable, .selectionSensitive, .typedTool, .rasterTool, .cmykTool],
        "auto_contrast": [.repeatable, .selectionSensitive, .typedTool, .rasterTool, .cmykTool],
        "auto_color": [.repeatable, .selectionSensitive, .typedTool, .rasterTool, .cmykTool],
    ]

    /// The destructive twin of the adjustment layer of the same name: the
    /// SAME core op (`dr_image_adjust_op`) the compositor runs, so the two
    /// can never drift. `params` is already validated — `applyFilter`
    /// validates once, where it can still throw a message naming the
    /// offending key — so this is non-throwing and can be called from the
    /// plane path's non-throwing closure.
    ///
    /// nil only when the core refuses the op outright, which after
    /// validation means an image it could not allocate.
    func adjustmentFilter(
        _ image: RasterImage, _ filter: String, _ params: [String: Any]
    ) -> RasterImage? {
        image.applyingAdjustment(op: filter, params: params)
    }

    /// The `params` object of a tool call ([:] when omitted), before any
    /// op-specific validation. `apply_filter` needs it a step earlier than
    /// `adjustmentParams` does — it has to validate before the plane/pixel
    /// branch, where it can still throw — so the read lives here rather
    /// than being duplicated inside the frozen file.
    func adjustmentParamsObject(_ a: [String: Any]) throws -> [String: Any] {
        guard let raw = a["params"] else { return [:] }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: "params must be a JSON object, e.g. {\"density\": 0.4}")
        }
        return object
    }

    /// The flat argument names each pre-phase-5 `apply_filter` filter reads,
    /// mirroring `AgentServer.filtered`'s arms one for one. A filter absent
    /// from this table is either an adjustment op (which takes `params`) or
    /// not a filter at all.
    static let flatFilterArguments: [String: [String]] = [
        "grayscale": [], "invert": [], "sepia": [], "edge_detect": [], "emboss": [],
        "blur": ["sigma"],
        "sharpen": ["amount"],
        "unsharp_mask": ["radius", "amount", "threshold"],
        "high_pass": ["radius"],
        "reduce_noise": ["radius", "luminance", "detail", "color"],
        "dust_and_scratches": ["radius", "threshold"],
        "adjust": ["brightness", "contrast", "saturation", "legacy"],
        // `c`/`m`/`y`/`k`: a CMYK document's ink stages (#547).
        "levels": ["black", "white", "gamma", "output_black", "output_white", "r", "g", "b",
                   "c", "m", "y", "k"],
        "hue_rotate": ["degrees"],
        "threshold": ["level"],
        "posterize": ["levels"],
        "pixelate": ["block"],
        "add_noise": ["amount", "seed"],
        "box_blur": ["radius"],
        "surface_blur": ["radius", "threshold"],
        "add_grain": ["amount", "size", "roughness", "seed"],
        "radial_blur": ["amount", "method", "quality", "center_x", "center_y"],
        // `foreground_ink` / `background_ink`: a CMYK document's clouds inks, percent (#547).
        "clouds": ["seed", "foreground_color", "background_color", "high_contrast",
                   "foreground_ink", "background_ink"],
        "difference_clouds": ["seed", "foreground_color", "background_color", "high_contrast",
                              "foreground_ink", "background_ink"],
        "offset": ["horizontal", "vertical", "undefined_areas", "background_color"],
        "motion_blur": ["angle", "distance"],
        "lens_blur": ["radius", "brightness", "threshold"],
        "tilt_shift": ["radius", "center_x", "center_y", "angle", "focus", "transition"],
        "lens_correction": ["distortion", "chromatic_red_cyan", "chromatic_blue_yellow", "vignette_amount",
                            "vignette_midpoint", "vertical_perspective", "horizontal_perspective", "angle",
                            "scale", "profile"],
        // Filters ▸ Presence… (`PresenceSheet.swift`): each −100…100, 0 off.
        "presence": ["texture", "clarity", "dehaze"],
        // Filters ▸ Develop Filter… (`DevelopFilterSheet.swift`): exposure −4…4
        // stops, the rest −100…100, 0 off; not re-editable. `masks` are its local adjustments
        // and `range_profile` the document's profile, which only the app writes
        // (`AgentServer+DevelopFilterMasks.swift`).
        "develop_filter": ["temperature", "tint", "exposure", "contrast", "highlights", "shadows",
                           "whites", "blacks", "texture", "clarity", "dehaze", "vibrance",
                           "saturation", "masks", "range_profile"],
        // Filter ▸ Sharpen ▸ Smart Sharpen… (`SmartSharpenSheet.swift`).
        "smart_sharpen": SmartSharpenSettings.keys,
    ]

    /// `apply_filter`'s own arguments, as opposed to any filter's: what may
    /// legitimately sit beside `filter` whatever the filter is. Everything
    /// else at the top level belongs to ONE of the two argument styles, and
    /// is refused by whichever of the two does not read it.
    static let filterEnvelopeKeys: Set<String> = [
        "filter", "layer", "index", "target", "params", "document_id",
    ]

    /// The flat filter arguments that are NOT numbers — a boolean, a
    /// method/quality/edge-mode name, a colour, or levels' per-channel
    /// objects — so `refuseUnknownFilterArguments` leaves their JSON type to
    /// the filter's own parser.
    static let nonNumericFilterArguments: Set<String> = [
        "legacy", "high_contrast", "method", "quality", "undefined_areas",
        "foreground_color", "background_color", "r", "g", "b", "profile", "remove",
        "foreground_ink", "background_ink", "masks", "range_profile",
    ]

    /// `apply_filter`'s strict-keys check for the flat filters (the ones in
    /// `flatFilterArguments`): a key that is neither the envelope's nor the
    /// filter's own is refused by name rather than ignored, and a numeric
    /// argument given as JSON null or a boolean is refused rather than read
    /// as absent (and so as the default). Adjustment ops and `curves` keep
    /// their own stray refusals in `adjustmentFilterParams`; an unknown
    /// filter name keeps the dispatch's refusal. Runs before the Quick Mask
    /// blur branch, so every depth and target passes through it.
    func refuseUnknownFilterArguments(_ a: [String: Any]) throws {
        // Depth Blur runs the depth model first, so it is its own tool.
        if a["filter"] as? String == "depth_blur" {
            throw ToolError(message: "depth_blur is its own tool: call depth_blur")
        }
        // Every filter, adjustment op and depth names its layer this way.
        try refuseNullOrBooleanNumbers(a, ["layer", "index"], in: "apply_filter")
        guard let filter = a["filter"] as? String,
              let flat = Self.flatFilterArguments[filter] else { return }
        let allowed = Self.filterEnvelopeKeys.union(flat)
        let unknown = a.keys.filter { !allowed.contains($0) }.sorted()
        if !unknown.isEmpty {
            let takes = flat.isEmpty ? "no arguments" : flat.joined(separator: ", ")
            throw ToolError(
                message: "apply_filter: \"\(filter)\" has no argument \(quoted(unknown)) — it takes "
                    + "\(takes). Nothing was applied.")
        }
        try refuseNullOrBooleanNumbers(
            a, flat.filter { !Self.nonNumericFilterArguments.contains($0) }, in: "apply_filter")
    }

    /// `"a", "b" and "c"` — the offending keys of a refusal, in the wording
    /// both directions of the params/flat mismatch use.
    private func quoted(_ keys: [String]) -> String {
        let names = keys.map { "\"\($0)\"" }
        guard let last = names.last, names.count > 1 else { return names.first ?? "" }
        return names.dropLast().joined(separator: ", ") + " and " + last
    }

    /// The whole of `apply_filter`'s params story, so the frozen file holds
    /// one call: the VALIDATED params object when `filter` is one of the
    /// twelve adjustment ops, curves or levels (whose arguments stay flat but
    /// run the shared adjustment kernel), and nil for the other older
    /// filters — which read their arguments flat.
    ///
    /// **Both directions of the mismatch are refused, and neither is
    /// ignored.** `params` is a top-level property of the one `apply_filter`
    /// schema whose `filter` enum lists `levels`, `posterize` and `adjust`
    /// beside the twelve ops that honour it, and three of those legacy names
    /// are themselves adjustment-layer ops — so passing `params` to a legacy
    /// filter is a likely mistake. The inverse is likelier still: `gamma`,
    /// `levels`, `black`, `white`, `degrees`, `level` and `saturation` are
    /// declared top-level properties of this same tool, so spelling an
    /// adjustment op's parameters flat looks exactly like spelling a legacy
    /// filter's. Either way, dropping what was passed silently reported
    /// `ok: true`, applied the filter's DEFAULTS — which for `photo_filter`
    /// meant warming an image the caller asked to cool — and registered an
    /// undo step for an edit nobody asked for. Both refusals are in-band and
    /// name the keys, so a model corrects itself in one turn.
    func adjustmentFilterParams(_ a: [String: Any], filter: String,
                                documentProfile: Data? = nil) throws -> [String: Any]? {
        if filter == "curves" {
            // UI path: Image ▸ Adjustments ▸ Curves…, which records
            // exactly this shape — the point lists (or maps) inside `params`.
            guard Set(a.keys).isSubset(of: Self.filterEnvelopeKeys) else {
                throw ToolError(message: "curves takes rgb/r/g/b point lists, or rgb_map/r_map/g_map/b_map 256-entry maps, inside params, in the historical 0…255 coordinate space.")
            }
            return try adjustmentParams(a, for: .curves, documentProfile: documentProfile)
        }
        // UI path: Image ▸ Adjustments ▸ Levels… — flat, onto the shared
        // adjustment kernel (AgentServer+Levels.swift).
        if filter == "levels" { return try levelsFilterParams(a) }
        if let op = AdjustmentSchema.op(forFilter: filter) {
            let stray = a.keys.filter { !Self.filterEnvelopeKeys.contains($0) }.sorted()
            if !stray.isEmpty {
                let shape = AdjustmentSchema.inputKeys(for: op).prefix(3)
                    .map { "\"\($0)\": …" }.joined(separator: ", ")
                throw ToolError(
                    message: "apply_filter: \"\(filter)\" takes its parameters inside a params "
                        + "object, not beside \"filter\" — {\"params\": {\(shape)}}. "
                        + quoted(stray) + " would have been ignored and \(filter)'s DEFAULTS "
                        + "applied instead. Nothing was applied.")
            }
            // #547: a CMYK layer reads the CMYK keys; one ink plate or an alpha channel is a
            // scalar target and reads the RGB scalar vocabulary.
            let scalarTarget = (stringArg(a, "target") ?? "layer") != "layer"
            return try AdjustmentSchema.validate(adjustmentParamsObject(a), for: op,
                                                 documentProfile: documentProfile,
                                                 cmyk: (try? target(a))?.doc?.isCmyk == true && !scalarTarget)
        }
        // UI path: Image ▸ Adjustments ▸ Brightness/Contrast/Saturation…'s
        // Use Legacy checkbox — `legacy` is flat beside the other
        // three, and only a JSON boolean, so a stray "false" string never
        // silently applies the legacy curve.
        if filter == "adjust", let legacy = a["legacy"], !Self.isJSONBoolean(legacy) {
            throw ToolError(message: "legacy must be true or false")
        }
        // An unknown filter is not this function's refusal to make: the
        // dispatch below already fails and names it.
        guard Self.flatFilterArguments[filter] != nil, a["params"] != nil else { return nil }
        throw flatArgumentsRefusal(filter)
    }

    /// The refusal of a `params` object handed to a filter that reads its
    /// arguments flat, naming them.
    func flatArgumentsRefusal(_ filter: String) -> ToolError {
        let flat = Self.flatFilterArguments[filter] ?? []
        let takes = flat.isEmpty
            ? "takes no arguments"
            : "takes its arguments flat, beside \"filter\": " + flat.joined(separator: ", ")
        return ToolError(
            message: "apply_filter: \"\(filter)\" \(takes) — not inside a params object, "
                + "which only the adjustment ops read. Nothing was applied.")
    }

    /// An adjustment layer's params as `get_document` reports them: every
    /// key intact except a Color Lookup's `table`, which is up to ~575 KB of
    /// base64 and would swamp the reply (and every later one, since a model
    /// re-reads the document). What is left — `kind`, `size`, `source_size`,
    /// the domain, `strength`, `title` — is everything an agent needs to
    /// reason about the layer, and `edit_adjustment_layer` either keeps the
    /// stored LUT (omit the table keys, or echo this placeholder back) or
    /// replaces the whole thing from a `file` path — which is what the
    /// placeholder text says, since it is the only instruction an agent
    /// reading a document reply gets.
    ///
    /// Used by `get_document` AND by `edit_adjustment_layer`'s own reply, so
    /// the two read paths agree and neither can put ~575 KB of base64 into
    /// a conversation.
    func elidedAdjustmentParams(_ payload: AdjustmentLayerPayload) -> [String: Any] {
        var params = payload.params
        guard payload.op == AdjustmentLayerOp.colorLookup.rawValue,
              params["table"] is String
        else { return params }
        let size = (params["size"] as? NSNumber)?.intValue ?? 0
        let nodes = payload.string("kind", default: "3d") == "3d" ? size * size * size : size
        params["table"] =
            "<\(nodes) entries elided — omit it (or pass it back unchanged) to keep this LUT, "
            + "or pass file with a .cube or .icc path to replace it>"
        // An abstract look's kept profile is elided the same way.
        if params["abstract_profile"] is String {
            params["abstract_profile"] =
                "<abstract profile elided — omit it, or pass it back unchanged, to keep it>"
        }
        return params
    }

    /// The keys that describe a `color_lookup` layer's stored LUT, as
    /// opposed to how it is APPLIED (`strength`, the only key left out).
    /// `title` is in the set because it travels with the table everywhere
    /// else too: loading a `file` replaces it along with the numbers
    /// (`AdjustmentSchema.validate`), and it names the .cube, not the
    /// layer. `abstract_profile` is an abstract look's own bytes,
    /// which the table is re-baked through. Exactly what `file` replaces
    /// wholesale, and exactly what the merge below carries forward.
    static let lutTableKeys = [
        "kind", "size", "table", "source_size", "domain_min", "domain_max", "title",
        "abstract_profile",
    ]

    /// `edit_adjustment_layer`'s ONE merge, on the ONE op that needs it.
    ///
    /// `params` is otherwise a wholesale replacement, deliberately. But a
    /// `color_lookup` layer's `table` is up to ~575 KB of base64, so
    /// `get_document` hands out a placeholder instead of it, and the layer
    /// stores no .cube path — which between them made the obvious flow
    /// ("read the params, change one key, write them back") impossible:
    /// omitting `table` was refused for want of a `kind`, and echoing the
    /// placeholder back was refused as invalid base64. An agent that did
    /// not itself load the LUT — a reopened .rz, a LUT the user picked in
    /// the sheet, a pruned conversation — could never touch `strength`
    /// again, while the UI's own sheet edited the layer fine.
    ///
    /// So: when the layer already IS a `color_lookup` and the call names no
    /// replacement table — no `file`, and either no `table` or just the
    /// elision placeholder (which can never be confused with base64, having
    /// angle brackets) — the stored table and the keys describing it are
    /// carried forward. Everything else still comes from the call.
    ///
    /// Takes and returns the whole argument dictionary so the frozen file's
    /// call site stays one line.
    func mergingStoredLut(
        _ a: [String: Any], op: AdjustmentLayerOp, current: AdjustmentLayerPayload?
    ) -> [String: Any] {
        guard op == .colorLookup,
              let current = current,
              current.op == AdjustmentLayerOp.colorLookup.rawValue,
              var params = a["params"] as? [String: Any],
              params["file"] == nil
        else { return a }
        for key in ["table", "abstract_profile"] {
            if let elided = params[key] as? String, elided.hasPrefix("<"), elided.hasSuffix(">") {
                params.removeValue(forKey: key)
            }
        }
        for key in Self.lutTableKeys where params[key] == nil {
            if let value = current.params[key] { params[key] = value }
        }
        var merged = a
        merged["params"] = params
        return merged
    }

    /// `add_adjustment_layer`'s POSITION pair (12E-2): where the new entry
    /// lands and the ANCHOR it lands relative to. The anchor defaults to the
    /// active layer and is resolved by `structuralLayerIndex`, so a GROUP is a
    /// legal target — which `into_group` requires. It is ignored for `top`,
    /// but still range-checked, exactly as the core range-checks it.
    ///
    /// Lives here rather than in the frozen file so `addAdjustmentLayer` keeps
    /// one call, and is the parser both depths read, so the byte and typed
    /// paths cannot disagree about what was asked for.
    func insertionArguments(
        _ a: [String: Any], document: ImageDocument
    ) throws -> (anchor: Int, position: LayerInsertPosition) {
        let position = try LayerInsertPosition.parse(a["position"])
        let anchor = try structuralLayerIndex(a, document)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = position.refusal(in: doc, anchor: anchor) {
            throw ToolError(message: reason)
        }
        return (anchor, position)
    }

    // MARK: - Auto Tone / Auto Contrast / Auto Color

    /// auto_tone: the agent mirror of Image ▸ Auto Tone (⇧⌘L).
    func autoTone(_ a: [String: Any]) throws -> String {
        try autoLevels(a, mode: DR_AUTO_TONE, action: "Auto Tone")
    }

    /// auto_contrast: the agent mirror of Image ▸ Auto Contrast (⌥⇧⌘L).
    func autoContrast(_ a: [String: Any]) throws -> String {
        try autoLevels(a, mode: DR_AUTO_CONTRAST, action: "Auto Contrast")
    }

    /// auto_color: the agent mirror of Image ▸ Auto Color (⇧⌘B).
    func autoColor(_ a: [String: Any]) throws -> String {
        try autoLevels(a, mode: DR_AUTO_COLOR, action: "Auto Color")
    }

    /// The three share one body: derive Levels parameters from the layer's
    /// own histogram (the core's counting rule, so a transparent surround
    /// never pins the black point at 0), then apply the SAME levels math the
    /// Levels dialog and the levels adjustment run. One undo step; an image
    /// already at full range writes nothing and reports `changed: false`.
    ///
    /// A GROUP is refused, by `paintLayerIndex` and in its words: a group has
    /// no pixels of its own to stretch, only a projection of the layers
    /// inside it, and writing that projection back would flatten the group.
    /// The refusal names the group's children so a model can auto-level them
    /// one at a time — or it can add a levels ADJUSTMENT LAYER inside the
    /// group, which recolors everything below it there without touching a
    /// single stored pixel.
    private func autoLevels(
        _ a: [String: Any], mode: DrAutoMode, action: String
    ) throws -> String {
        let document = try target(a)
        let index = try structuralLayerIndex(a, document)
        let editTarget = try paintTarget(a, document)
        let scope = try ToneAnalysisScope.parse(a["analysis_scope"])
        let clip = doubleArg(a, "clip") ?? 0.001
        guard clip.isFinite, clip >= 0, clip <= 0.1 else {
            throw ToolError(message: "clip must be a finite number between 0 and 0.1")
        }
        if document.doc?.isCmyk == true {
            // #547: one ink plate or an alpha channel takes the scalar Auto, as a 16-bit colour
            // plane or channel does.
            if editTarget.isInk || editTarget.isChannel {
                return try selectedPrecisionScalarAuto(document, index: index, target: editTarget,
                    mode: mode, scope: scope, clip: clip, action: action)
            }
            guard editTarget == .layer else {
                let named = stringArg(a, "target") ?? "layer"
                throw ToolError(message: CmykAdjustFilter.autoTargetRefusal(named))
            }
            return try cmykAutoLevels(document, index: index, scope: scope, clip: clip, action: action)
        }
        if document.doc.isHighDepth {
            if editTarget.isCoverage {
                return try selectedPrecisionScalarAuto(document, index: index, target: editTarget,
                    mode: mode, scope: scope, clip: clip, action: action)
            }
            return try selectedPrecisionAuto(document, index: index, mode: mode, scope: scope, clip: clip, action: action)
        }
        let capture = try operationCapture(document, operation: .filter, target: editTarget, layer: index)
        let mask = try capture.analysisCoverage(scope)
        guard let derived = try capture.source.scopedAutoLevels(mask: mask, mode: mode, clip: clip) else {
            return try commitScopedResult(
                document, capture: capture, updated: nil, actionName: action,
                extra: ["analysis_region": scope.rawValue, "analysis_scope": scope.rawValue])
        }
        guard let corrected = capture.source.levelsChannels(
            black: derived.black, white: derived.white, gamma: derived.gamma)
        else { throw ToolError(message: "Could not apply the derived levels") }
        return try commitScopedResult(
            document, capture: capture,
            updated: capture.writing(corrected, selection: capture.selection), actionName: action,
            extra: [
                "clip": clip, "analysis_region": scope.rawValue, "analysis_scope": scope.rawValue,
                "black": derived.black, "white": derived.white, "gamma": derived.gamma,
            ])
    }

    /// A CMYK document's Auto Tone / Auto Contrast / Auto Color (#547) — UI path: Image ▸ Auto
    /// Tone, Auto Contrast and Auto Color (`EditorViewController+AutoAdjust.swift`): the CMYK tone
    /// analysis's Auto on the layer (`CmykAdjustFilter.autoLevelsParams`, clipping `clip` at each
    /// end; a mask, plane or channel target is refused before this runs), applied as a
    /// destructive CMYK Levels through the selection, one undo step; `changed` false when there
    /// is nothing to measure.
    private func cmykAutoLevels(_ document: ImageDocument, index: Int, scope: ToneAnalysisScope,
                                clip: Double, action: String) throws -> String {
        guard let precision = document.doc?.precision else { throw ToolError(message: "Document has no image") }
        let tool = action == "Auto Contrast" ? "auto_contrast" : action == "Auto Color" ? "auto_color" : "auto_tone"
        let coverage: [UInt16]
        if scope == .selection {
            guard let selection = document.canvasSelection else {
                throw ToolError(message: "Selection analysis requires an active selection.")
            }
            coverage = selection.maskSamples16()
        } else {
            coverage = []
        }
        let params: [String: Any]?
        do {
            params = try CmykAdjustFilter.autoLevelsParams(precision, layer: index, coverage: coverage,
                                                           form: CmykAdjustFilter.autoForm(tool),
                                                           clipPercent: clip * 100)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        guard let params else {
            return try jsonResult(["ok": true, "changed": false, "layer": index,
                                   "analysis_scope": scope.rawValue])
        }
        let capture = try PrecisionEditorCapture(document)
        if let reason = scopeRefusal(.filter, target: .layer, doc: capture.facade, layer: index) {
            throw ToolError(message: reason)
        }
        let edit = try SelectedAdjustment.edit(capture, layer: index, filter: "levels", params: params,
                                               target: .layer)
        let result = try edit.run(title: action)
        let changed = try capture.publish(result, to: document, name: action)
        return try jsonResult(["ok": true, "changed": changed, "layer": index, "target": "layer",
                               "params": params, "form": CmykAdjustFilter.autoForm(tool), "clip": clip,
                               "analysis_scope": scope.rawValue,
                               "write_scope": capture.selection == nil ? "whole_target" : "selection"])
    }
}
