import AppKit

/// Paint and retouch Action construction from the stroke's captured inputs.
extension EditorViewController {
    /// What a paint or retouch stroke records, built from the canvas's
    /// latched tip and the flattened vertices it actually dabbed along.
    ///
    /// The vertices are the SAME polyline the canvas rendered — the spline
    /// is flattened into vertex spans before any dab is stamped — and the
    /// agent's stroke tools walk a caller's polyline through the identical
    /// `SoftBrush` engine. Native commits attach their exact versioned dabs,
    /// preserving pressure and held airbrush deposits independently of this
    /// point list. Historical byte recordings retain their constant-size,
    /// point-only timing meaning and the existing polyline thinning limit.
    func strokeRecord(_ actionName: String) -> [ActionStep] {
        // Phase 12A: a BYTE pressure/airbrush brush or eraser stroke captured
        // canonical dabs, and the commit re-rasterized THOSE
        // (ImageCanvasView+NativeBrush). The session's life ends here whatever
        // happens, so no later gesture can inherit it.
        let prepared = canvas.preparedByteStroke
        let points = Self.recordablePoints(canvas.recordedStrokePoints)
        guard points.count >= 1 else { return .notACommand }
        let tip = canvas.strokeTip
        let target = strokeTarget.agentName(in: document?.doc)
        var arguments: [String: Any] = [
            "layer": ActionArgs.activeLayer,
            "points": ActionArgs.points(points),
            "size": ActionArgs.number(canvas.brushSize),
            "hardness": ActionArgs.number(tip.hardness * 100),
            "flow": ActionArgs.number(max(tip.flow * 100, 1)),
            "spacing": ActionArgs.number(tip.spacingPercent),
            "angle": ActionArgs.number(tip.angleDegrees),
            "roundness": ActionArgs.number(max(tip.roundness * 100, 1)),
        ]
        // Native commit paths attach the required canonical payload with
        // throwing admission before publishing. Byte recordings retain points.
        // The canonical dabs replace `points` as the replay's footprint: the
        // byte replay rasterizes them through the same `SoftBrush.rasterizeDabs`
        // the commit just used. The point list stays in the step so an older
        // build (and a reader) still sees where the stroke went.
        if let prepared {
            arguments["native_stroke"] = prepared.canonical
            arguments["size"] = Double(canvas.brushSize)
            arguments["hardness"] = Double(tip.hardness * 100)
            arguments["flow"] = Double(max(tip.flow * 100, 1))
            arguments["spacing"] = Double(tip.spacingPercent)
            arguments["angle"] = Double(tip.angleDegrees)
            arguments["roundness"] = Double(max(tip.roundness * 100, 1))
        }
        switch strokeTool {
        case .brush:
            arguments["opacity"] = prepared == nil ? ActionArgs.number(canvas.brushOpacity)
                : Double(canvas.brushOpacity)
            arguments["color"] = ActionArgs.hex(byteStrokeCapture?.color ?? canvas.paintColor)
            // A wide-gamut document records the bytes the tool itself painted,
            // qualified with `color_space: "document"`; an sRGB document's
            // recording is unchanged (ActionArgs.documentHex).
            if let space = document?.nsColorSpace,
               let hex = ActionArgs.documentHex(byteStrokeCapture?.color ?? canvas.paintColor, in: space) {
                arguments["color"] = hex
                arguments["color_space"] = "document"
            }
            arguments["target"] = target
            // Only for a LAYER stroke. A mask, a colour plane and a channel
            // are coverage, not colour, and `brush_stroke` refuses a
            // blend_mode on one outright ("blend_mode does not apply to a
            // mask stroke") — so writing it unconditionally made every
            // recorded mask stroke, the commonest coverage gesture there is,
            // a step that could never replay. The eraser case below omits it
            // for the same kind of reason.
            if !strokeTarget.isCoverage {
                arguments["blend_mode"] = DrBlendMode.displayName(for: strokeBlendMode)
            }
            return .stroke("brush_stroke", arguments)
        case .eraser:
            arguments["opacity"] = prepared == nil ? ActionArgs.number(canvas.brushOpacity)
                : Double(canvas.brushOpacity)
            arguments["target"] = target
            // eraser_stroke refuses a blend_mode by design (erasing removes
            // alpha), so it is not written even though the tip carries one.
            return .stroke("eraser_stroke", arguments)
        case .clone:
            guard let source = canvas.strokeCloneSource else { return .notACommand }
            arguments["opacity"] = prepared == nil ? ActionArgs.number(canvas.brushOpacity)
                : Double(canvas.brushOpacity)
            arguments["source_x"] = prepared == nil ? ActionArgs.number(source.x) : Double(source.x)
            arguments["source_y"] = prepared == nil ? ActionArgs.number(source.y) : Double(source.y)
            arguments["blend_mode"] = DrBlendMode.displayName(for: strokeBlendMode)
            arguments.merge(strokeSamplingRecord(.clone)) { _, new in new }
            return .stroke("clone_stamp", arguments)
        case .dodge:
            let options = byteStrokeCapture?.options ?? ToolOptionsStore.shared.dodge
            arguments["burn"] = options.burn
            arguments["exposure"] = prepared == nil ? ActionArgs.number(options.opacity) : options.opacity
            arguments["range"] = Self.dodgeRangeNames[
                min(max(options.rangeIndex, 0), Self.dodgeRangeNames.count - 1)]
            if options.protectTones == true { arguments["protect_tones"] = true }
            return .stroke("dodge_burn", arguments)
        case .blurBrush, .sharpenBrush, .smudge, .sponge:
            // The four replay through their own stroke tools, whose
            // `strength` is the options bar's percent. The History Brush
            // has no arm: its source is a state of THIS session's history,
            // which no later replay can name, so it takes `default:`.
            guard let tool = Self.retouchBrushToolName(strokeTool) else {
                return .unrecorded(actionName)
            }
            let options = byteStrokeCapture?.options
                ?? ToolOptionsStore.shared.paintOptions(for: strokeTool)
                ?? PaintToolOptions(opacity: 50)
            arguments["strength"] = prepared == nil
                ? ActionArgs.number(options.opacity) : options.opacity
            if strokeTool == .sponge {
                arguments["mode"] = options.saturate == true ? "saturate" : "desaturate"
            }
            // Written only when on, so a step without the key
            // replays as it did before the options existed.
            if Self.spongeVibrance(strokeTool, options: options) { arguments["vibrance"] = true }
            if Self.samplesAllLayers(strokeTool, options: options) {
                arguments["sample_all_layers"] = true
            }
            return .stroke(tool, arguments)
        case .heal:
            guard let source = canvas.strokeCloneSource else {
                return .notACommand
            }
            let options = byteStrokeCapture?.options ?? ToolOptionsStore.shared.heal
            // `flow` is REFUSED by heal_stroke, not ignored — the solve runs
            // over the stroke's coverage, so a flowed-down dab would leave it
            // nothing to work on, and the canvas latches flow to 1 for these
            // two tools for the same reason. Inheriting the shared tip
            // dictionary's key made every recorded healing stroke a step that
            // failed on the first replay.
            arguments.removeValue(forKey: "flow")
            // heal_stroke's opacity is the heal's STRENGTH in percent (1-100),
            // not the 0-1 alpha brush_stroke takes: the overlay is coverage
            // and the strength lives in the op.
            arguments["opacity"] = prepared == nil
                ? ActionArgs.number(min(max(options.opacity, 1), 100))
                : min(max(options.opacity, 1), 100)
            arguments["source_x"] = prepared == nil ? ActionArgs.number(source.x) : Double(source.x)
            arguments["source_y"] = prepared == nil ? ActionArgs.number(source.y) : Double(source.y)
            arguments.merge(strokeSamplingRecord(.heal)) { _, new in new }
            return .stroke("heal_stroke", arguments)
        case .spotHeal:
            let options = byteStrokeCapture?.options ?? ToolOptionsStore.shared.spotHeal
            // Refused here too, for the reason the .heal case gives.
            arguments.removeValue(forKey: "flow")
            // Percent, like heal_stroke's.
            arguments["opacity"] = prepared == nil
                ? ActionArgs.number(min(max(options.opacity, 1), 100))
                : min(max(options.opacity, 1), 100)
            arguments.merge(strokeSamplingRecord(.spotHeal)) { _, new in new }
            // The tool exposes neither dial; the commit passes 0 for both
            // (EditorViewController+Heal), and a replay must do the same or
            // it would inpaint with a different ring and a different seed.
            arguments["ring"] = 0
            arguments["seed"] = 0
            // The latched Type, written only when it is not
            // Content-Aware so a Content-Aware step reads as it always did.
            let type = (precisionStroke?.retouchOptions ?? options).spotHealType ?? .contentAware
            if type != .contentAware { arguments["type"] = type.rawValue }
            return .stroke("spot_heal_stroke", arguments)
        case .artHistoryBrush:
            // Replays through `art_history_brush_stroke` with the latched
            // options and seed, painting from the History panel's source at
            // replay time; never a `state_id`, which names this session's history.
            guard let art = precisionStroke?.art
                    ?? byteStrokeCapture.map({ ArtHistoryOptions(paint: $0.options, seed: $0.artSeed) })
            else { return .unrecorded(actionName) }
            arguments.merge(art.arguments) { _, new in new }
            return .stroke("art_history_brush_stroke", arguments)
        default:
            return .unrecorded(actionName)
        }
    }

    /// The sampling and output arguments for a recorded retouch stroke,
    /// taken from what the GESTURE LATCHED rather than from the options store.
    ///
    /// The store is what the user has selected NOW, and `strokeRecord` runs
    /// at mouse-up: a mode changed mid-drag — by a panel click, an agent call
    /// or a preset — would otherwise be recorded as the mode the stroke
    /// sampled with, which it was not. The stroke's own plate carries the
    /// choice it was built from (`RetouchPlate`), so it is the honest source;
    /// the store is the fallback for the one path that latches no plate.
    ///
    /// At every depth the recorded mode is the one the gesture latched;
    /// 16-bit retouch supports all three sampling modes (phase 9D).
    private func strokeSamplingRecord(_ tool: EditorTool) -> [String: Any] {
        let stored = precisionStroke?.retouchOptions ?? byteStrokeCapture?.options
            ?? Self.retouchOptions(for: tool)
        let choice = strokePlate?.choice
            ?? RetouchSamplingChoice(
                mode: stored?.sample ?? .all,
                ignoreAdjustments: stored?.ignoreAdjustments ?? false)
        var out = ActionArgs.sampling(choice.mode, choice.ignoreAdjustments)
        if precisionStroke == nil, tool == .clone || tool == .heal { out["footprint"] = "dabs" }
        if let geometry = canvas.strokeCloneGeometry, tool == .clone || tool == .heal {
            out["source_transform"] = geometry.values
        }
        if stored?.expandDestination == true { out["expand_destination"] = true }
        // Written only when the gesture actually went somewhere else, so a
        // step recorded for the ordinary in-place retouch is byte-identical
        // to one recorded before this argument existed.
        if stored?.outputToNewLayer == true { out["output"] = RetouchOutput.newLayer.rawValue }
        return out
    }

    /// `dodge_burn`'s `range` vocabulary, in the options bar's segment order.
    private static let dodgeRangeNames = ["shadows", "midtones", "highlights"]

    /// The most points a recorded stroke may carry — the stroke tools' own
    /// limit (`AgentServer.parsePoints`: "Too many points (10,000 max)").
    private static let maxRecordedStrokePoints = 10_000

    /// `points` thinned, uniformly and keeping both ends, to something the
    /// stroke tools will accept.
    ///
    /// The canvas flattens a spline at roughly one vertex per 1.5 px of arc
    /// and appends every one of them, so a path longer than ~15,000 px — three
    /// diagonal sweeps across a 4000 px canvas — produced more points than
    /// any stroke tool takes: the step was ~200 KB of JSON that failed on its
    /// first replay and, with the default `on_error: "stop"`, took the rest
    /// of the action down with it. Thinning is the honest repair rather than
    /// a refusal to record: the flattening step is far below the dab spacing
    /// of any tip, so the same dabs land in the same places.
    static func recordablePoints(_ points: [CGPoint]) -> [CGPoint] {
        guard points.count > maxRecordedStrokePoints else { return points }
        // One less than the cap, so re-adding the final point cannot push the
        // result back over it.
        let step = Int(
            (Double(points.count) / Double(maxRecordedStrokePoints - 1)).rounded(.up))
        var thinned: [CGPoint] = []
        thinned.reserveCapacity(maxRecordedStrokePoints)
        var index = 0
        while index < points.count {
            thinned.append(points[index])
            index += step
        }
        if let last = points.last, thinned.last != last { thinned.append(last) }
        return thinned
    }
}
