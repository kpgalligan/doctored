import AppKit

// `validatedLevelsParams` is the strict MCP validator `add_adjustment_layer` /
// `edit_adjustment_layer` (through `adjustmentParams`) and `apply_filter` share, naming the key and
// the channel; `levelsFilterParams` keeps `apply_filter` "levels" FLAT (r/g/b objects beside
// `filter`), coerces string-spelled numbers through `SelectedAdjustment.numericParameters` and
// refuses a `params` object in the flat filters' words. `levelsAutoArguments` resolves
// `add_adjustment_layer` / `edit_adjustment_layer`'s `auto` {mode, clip} into plain `params` in
// `execute`, before dispatch and recording, so no step records `auto`.

/// The agent side of Levels: the ONE strict validator every
/// levels params object passes, and `apply_filter`'s flat levels arguments.
/// Output levels and the per-channel stages (`r`, `g`, `b`) are the dialog's
/// Output sliders and Channel popup (`LevelsAdjustmentEditor`).
extension AgentServer {
    /// The validated, CANONICAL levels params (`LevelsMeta.params`).
    ///
    /// UI paths: Layer ▸ New Adjustment Layer ▸ Levels… and Layer ▸
    /// Adjustment Options… (`add_adjustment_layer` / `edit_adjustment_layer`
    /// through `adjustmentParams`), and Image ▸ Adjustments ▸ Levels…
    /// (`apply_filter` through `levelsFilterParams`).
    ///
    /// Stricter than the core, which ignores unknown keys: every violation
    /// is refused naming the key — and the channel, for a key inside `r`,
    /// `g` or `b` — so a model corrects itself in one turn.
    func validatedLevelsParams(_ params: [String: Any]) throws -> [String: Any] {
        let stageKeys = Array(LevelsMeta.keys.prefix(5))
        if let first = params.keys.filter({ !LevelsMeta.keys.contains($0) }).sorted().first {
            throw ToolError(
                message: "Unknown levels parameter \"\(first)\" — levels takes "
                    + LevelsMeta.keys.joined(separator: ", ") + ".")
        }
        var stages: [LevelsChannel: LevelsStage] = [.rgb: try levelsStage(params, channel: nil)]
        // `c`/`m`/`y`/`k` are a CMYK document's ink stages (#1); the
        // document's gate refuses the other mode's keys by name
        // (`CmykDocument.toneKeyRefusal`).
        for channel in LevelsChannel.channels {
            let name = channel.rawValue
            guard let raw = params[name] else {
                stages[channel] = .identity
                continue
            }
            guard let object = raw as? [String: Any] else {
                throw ToolError(
                    message: "levels \(name) must be an object of "
                        + stageKeys.joined(separator: ", ") + " (the \(channel.displayName) stage)")
            }
            if let first = object.keys.filter({ !stageKeys.contains($0) }).sorted().first {
                throw ToolError(
                    message: "Unknown levels \(name) parameter \"\(first)\" — a channel stage "
                        + "takes " + stageKeys.joined(separator: ", ") + ".")
            }
            stages[channel] = try levelsStage(object, channel: name)
        }
        return LevelsMeta.params(stages)
    }

    /// One stage's five numbers: each, when present, a finite JSON number in
    /// its range (absent = its default), and `black < white`.
    private func levelsStage(_ object: [String: Any], channel: String?) throws -> LevelsStage {
        let prefix = channel.map { "\($0)." } ?? ""
        func number(_ key: String, _ range: ClosedRange<Double>) throws -> Double? {
            guard let value = object[key] else { return nil }
            guard let n = (value as? NSNumber)?.doubleValue, n.isFinite else {
                throw ToolError(message: "\(prefix)\(key) must be a number")
            }
            guard range.contains(n) else {
                throw ToolError(
                    message: "\(prefix)\(key) must be between \(range.lowerBound) and "
                        + "\(range.upperBound) (got \(n))")
            }
            return n
        }
        var stage = LevelsStage()
        stage.black = try number("black", 0...1) ?? 0
        stage.white = try number("white", 0...1) ?? 1
        stage.gamma = try number("gamma", 0.1...10) ?? 1
        stage.outputBlack = try number("output_black", 0...1) ?? 0
        stage.outputWhite = try number("output_white", 0...1) ?? 1
        guard stage.black < stage.white else {
            let scope = channel.map { "levels \($0)" } ?? "levels"
            throw ToolError(
                message: "\(scope) requires 0 <= black < white <= 1 "
                    + "(got black \(stage.black), white \(stage.white))")
        }
        return stage
    }

    /// `apply_filter` "levels" — UI path: Image ▸ Adjustments ▸ Levels…,
    /// which records exactly this shape. The arguments stay FLAT beside
    /// `filter` (`r`/`g`/`b` included, as objects), so historical Actions
    /// replay unchanged; a `params` object is refused in the flat filters'
    /// own words. String-spelled numbers are accepted here, as `doubleArg`
    /// and the 16-bit path always accepted them, at the top level and
    /// inside each channel object.
    func levelsFilterParams(_ a: [String: Any]) throws -> [String: Any] {
        if a["params"] != nil { throw flatArgumentsRefusal("levels") }
        let keys = Self.flatFilterArguments["levels"] ?? []
        var params = keys.reduce(into: [String: Any]()) { if let value = a[$1] { $0[$1] = value } }
        params = SelectedAdjustment.numericParameters(params)
        for channel in LevelsChannel.channels.map(\.rawValue) {
            if let object = params[channel] as? [String: Any] {
                params[channel] = SelectedAdjustment.numericParameters(object)
            }
        }
        return try validatedLevelsParams(params)
    }

    /// `apply_filter` "levels" / "curves" on a scalar target (mask, plane,
    /// channel): both depths write the Rec. 709 gray of the per-channel
    /// result, so an `r`, `g` or `b` stage — or a curves `r_map`, `g_map`
    /// or `b_map` — would land at a fraction of its weight, and a CMYK
    /// stage (`cmyk`, `c`…`k`, or their `_map` forms) would not apply at all. Refused naming
    /// the keys — a scalar target takes only the composite stage, as the
    /// Levels dialog's master-only rule does. Identity stages change
    /// nothing and pass.
    func refuseChannelStagesOnScalarTarget(
        _ filter: String, _ params: [String: Any]?, target: PaintTarget
    ) throws {
        guard target != .layer, let params, filter == "levels" || filter == "curves" else { return }
        // `c`/`m`/`y`/`k` (and Curves' `cmyk`) are a CMYK layer's ink channels (#1): a
        // mask is gray coverage at every mode.
        var keys = ["r", "g", "b", "c", "m", "y", "k", "cmyk"].filter { name in
            if filter == "levels" {
                guard let channel = LevelsChannel(rawValue: name) else { return false }
                return LevelsMeta.stages(in: params)[channel]?.isIdentity == false
            }
            guard let channel = CurveChannel(rawValue: name) else { return false }
            return CurvesMeta.points(of: channel, in: AdjustmentLayerPayload(op: filter, params: params))
                != CurvePoint.identity
        }
        if filter == "curves",
            let maps = CurvesMeta.maps(in: AdjustmentLayerPayload(op: filter, params: params)) {
            keys += [CurveChannel.red, .green, .blue]
                .filter { maps[$0] != CurvesMeta.identityMap }
                .map(\.mapKey)
        }
        // The CMYK map keys, refused like their point-form twins: the scalar path parses
        // the RGB keys only, so they would otherwise be ignored as identity.
        if filter == "curves",
            let maps = CurvesMeta.maps(
                in: AdjustmentLayerPayload(op: filter, params: params), cmyk: true) {
            keys += CurveChannel.cases(cmyk: true)
                .filter { maps[$0] != CurvesMeta.identityMap }
                .map(\.mapKey)
        }
        guard !keys.isEmpty else { return }
        let composite = filter == "levels" ? "the top-level black/white/gamma/output keys" : "rgb or rgb_map"
        let named = keys.map { "\"\($0)\"" }.joined(separator: ", ")
        throw ToolError(
            message: "\(filter) \(named) cannot apply to a mask, plane or channel "
                + "target — a scalar target takes only the composite stage (\(composite)). "
                + "Nothing was applied.")
    }

    /// `add_adjustment_layer` / `edit_adjustment_layer` with `auto`
    /// {mode, clip?} — UI path: the Levels dialog's Auto Tone and
    /// Auto Color buttons, in its create (Layer ▸ New Adjustment Layer ▸
    /// Levels…) and edit (Layer ▸ Adjustment Options…) modes. Resolved BEFORE
    /// dispatch and recording (`execute`), so the call runs and records plain
    /// `params` — a replay reproduces the numbers, it never re-measures.
    /// Other tools pass through unchanged.
    ///
    /// Create measures the backdrop where the new layer lands, gated by the
    /// live selection (the mask it gets); edit measures the backdrop below the
    /// edited layer, gated by its own mask (`LevelsAuto`).
    func levelsAutoArguments(
        tool: String, arguments: [String: Any], document: ImageDocument?
    ) throws -> [String: Any] {
        // #1 phase 5: a CMYK document's Levels and Curves Auto — on a layer; `apply_filter` on
        // one ink plate or an alpha channel is a scalar target and falls through as on RGB.
        let target = stringArg(arguments, "target") ?? "layer"
        let scalarTarget = tool == "apply_filter"
            && (CmykPlane(key: target) != nil || target.hasPrefix("channel:"))
        if let document, document.doc?.isCmyk == true, !scalarTarget {
            return try cmykAutoArguments(tool: tool, arguments: arguments, document: document)
        }
        guard tool == "add_adjustment_layer" || tool == "edit_adjustment_layer",
              let raw = arguments["auto"] else { return arguments }
        guard let object = raw as? [String: Any] else {
            throw ToolError(
                message: "auto must be an object {\"mode\": \"tone\" or \"color\", "
                    + "\"clip\": optional number 0…0.1}.")
        }
        if let first = object.keys.filter({ $0 != "mode" && $0 != "clip" }).sorted().first {
            throw ToolError(message: "Unknown auto key \"\(first)\" — auto takes mode and clip.")
        }
        guard let name = object["mode"] as? String, let mode = LevelsAutoMode(rawValue: name) else {
            throw ToolError(message: "auto mode must be \"tone\" or \"color\".")
        }
        var clip = LevelsAuto.defaultClip
        if object["clip"] != nil {
            guard let value = doubleArg(object, "clip") else {
                throw ToolError(message: "auto clip must be a number between 0 and 0.1.")
            }
            clip = value
        }
        guard arguments["params"] == nil else {
            throw ToolError(message: "auto derives params; pass auto or params, not both.")
        }
        guard let document, let doc = document.doc else {
            throw ToolError(message: "Document has no image")
        }
        let edited = tool == "edit_adjustment_layer" ? try paintLayerIndex(arguments, document) : nil
        let op = stringArg(arguments, "op") ?? edited.flatMap { doc.adjustmentPayload($0)?.op }
        guard op == AdjustmentLayerOp.levels.rawValue else {
            throw ToolError(message: "auto applies to Levels layers only.")
        }
        let stages: [LevelsChannel: LevelsStage]?
        if let edited {
            stages = try LevelsAuto.derive(source: doc, below: edited, insert: nil,
                selection: nil, coverage: nil, mode: mode, clip: clip)
        } else {
            let placement = try insertionArguments(arguments, document: document)
            stages = try LevelsAuto.derive(
                source: doc,
                below: placement.position.landingIndex(in: doc, anchor: placement.anchor),
                insert: (placement.anchor, placement.position),
                selection: document.canvasSelection, coverage: nil, mode: mode, clip: clip)
        }
        guard let stages else {
            throw ToolError(message: "Auto found nothing visible below the adjustment layer to measure.")
        }
        var resolved = arguments
        resolved.removeValue(forKey: "auto")
        resolved["params"] = LevelsMeta.params(stages)
        return resolved
    }

    /// A CMYK document's `auto` (#1 phase 5) — UI path: the Levels and Curves dialogs' Auto and
    /// Options… (`CmykToneControls`), in every mode: `add_adjustment_layer`,
    /// `edit_adjustment_layer` and `apply_filter` levels or curves take `auto` true or
    /// `{"algorithm","shadows","highlights"}` (Auto Options, over the saved defaults), measured
    /// on the scope the dialog plots — the layer `apply_filter` rewrites within the selection,
    /// the backdrop where a new layer lands within the selection, the backdrop below an edited
    /// one — and resolved into plain params before dispatch and recording.
    private func cmykAutoArguments(tool: String, arguments: [String: Any],
                                   document: ImageDocument) throws -> [String: Any] {
        guard ["add_adjustment_layer", "edit_adjustment_layer", "apply_filter"].contains(tool),
              let raw = arguments["auto"] else { return arguments }
        guard let doc = document.doc, let typed = doc.precision else {
            throw ToolError(message: "Document has no image")
        }
        let options: CmykAutoOptions
        do { options = try CmykAutoOptions(arguments: raw) } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let edited = tool == "edit_adjustment_layer" ? try paintLayerIndex(arguments, document) : nil
        let op = tool == "apply_filter" ? stringArg(arguments, "filter")
            : stringArg(arguments, "op") ?? edited.flatMap { doc.adjustmentPayload($0)?.op }
        guard op == "levels" || op == "curves" else {
            throw ToolError(message: "On a CMYK document auto applies to Levels and Curves.")
        }
        guard arguments["params"] == nil else {
            throw ToolError(message: "auto derives params; pass auto or params, not both.")
        }
        var request: [String: Any]
        var coverage: [UInt16] = document.canvasSelection?.maskSamples16() ?? []
        switch tool {
        case "apply_filter":
            request = ["source": "layer", "layer": try paintLayerIndex(arguments, document)]
        case "edit_adjustment_layer":
            request = ["source": "backdrop", "below": edited ?? 0]
            coverage = []
        default:
            let placement = try insertionArguments(arguments, document: document)
            request = ["source": "backdrop",
                       "below": placement.position.landingIndex(in: doc, anchor: placement.anchor)]
        }
        request["auto"] = options.request
        let analysis: [String: Any]
        do {
            let context = try PrecisionWorkContext(source: typed)
            analysis = try typed.cmykToneAnalysis(request: request, coverage: coverage, workingLimit: context.limit,
                                                  otherRetained: context.otherRetained, control: nil)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        guard let auto = analysis["auto"] as? [String: Any], auto["empty"] == nil else {
            throw ToolError(message: "Auto found nothing visible to measure.")
        }
        var resolved = arguments
        resolved.removeValue(forKey: "auto")
        if op == "levels" {
            let params = LevelsMeta.params(CmykToneTools.levelsStages(fromAuto: auto) ?? [:])
            if tool == "apply_filter" {
                // `apply_filter` levels stays FLAT beside `filter` (`levelsFilterParams`).
                resolved.merge(params) { _, derived in derived }
            } else {
                resolved["params"] = params
            }
        } else {
            resolved["params"] = CurvesMeta.params(CmykToneTools.curvePoints(fromAuto: auto) ?? [:])
        }
        return resolved
    }
}
