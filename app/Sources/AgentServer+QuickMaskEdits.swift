import AppKit

/// `apply_filter filter: "blur"` and `gradient` in Quick Mask: the MCP
/// mirrors of Filters ▸ Blur… and the Gradient tool's drag there
/// (EditorViewController+QuickMaskEdits.swift). Both edit the Quick Mask
/// coverage when the document is in Quick Mask and the call names no
/// target, or when `target` is `"quick_mask"`, which pins the call there and
/// refuses by name outside Quick Mask (`fill_selection`'s rule). A
/// call that names a layer target while Quick Mask is on keeps the standing
/// refusal (`OperationScope.quickMaskRefusal`), and `gradient` refuses a
/// `blend_mode` other than Normal there (coverage has no colour to
/// blend with). The dispatcher records the
/// call, pinned (`ActionRecorder.pinningModeDefaults`), so these commits
/// record nothing themselves.
extension AgentServer {
    /// Whether this call edits the Quick Mask: `target: "quick_mask"`, or no
    /// target while the document is in Quick Mask. Throws when the target
    /// pins a document that is not in Quick Mask, or when the call names a
    /// layer, which a Quick Mask edit has none of.
    private func editsQuickMask(
        _ a: [String: Any], _ document: ImageDocument, tool: String
    ) throws -> Bool {
        let named = stringArg(a, "target")
        guard named == "quick_mask" || (a["target"] == nil && document.quickMaskActive) else {
            return false
        }
        guard document.quickMaskActive else {
            throw ToolError(
                message: "target \"quick_mask\" edits the Quick Mask, and this document is not "
                    + "in Quick Mask. Enter it with set_quick_mask first, or pass another target.")
        }
        guard a["layer"] == nil, a["index"] == nil else {
            throw ToolError(message: "In Quick Mask, \(tool) edits the Quick Mask coverage, "
                + "which belongs to no layer: pass no layer.")
        }
        return true
    }

    /// `apply_filter` in Quick Mask: Gaussian Blur over the coverage, sigma
    /// 0.65…75.5 (default 4). nil when the call does not edit the Quick Mask.
    /// Every other filter is refused there by name.
    func quickMaskBlurFilter(_ a: [String: Any], _ document: ImageDocument) throws -> String? {
        let filter = stringArg(a, "filter")
        // Without a target, only Blur is rerouted: another filter in Quick
        // Mask keeps the standing refusal.
        guard filter == "blur" || stringArg(a, "target") == "quick_mask",
              try editsQuickMask(a, document, tool: "apply_filter")
        else { return nil }
        guard filter == "blur" else {
            throw ToolError(message: "In Quick Mask, apply_filter supports filter \"blur\" only: "
                + "Gaussian Blur over the Quick Mask coverage.")
        }
        let changed: Bool
        do {
            changed = try document.applyQuickMaskBlur(
                sigma: doubleArg(a, "sigma") ?? 4, record: .notACommand)
        } catch {
            throw error as? ToolError ?? ToolError(message: error.localizedDescription)
        }
        return try quickMaskEditResult(document, changed: changed)
    }

    /// `gradient` in Quick Mask: the ramp — `stops`, or `start_color` to
    /// `end_color` — laid over the coverage in any of the five shapes, each
    /// colour as its coverage gray with its alpha (times `opacity`) as the
    /// strength. nil when the call does not edit the Quick Mask.
    func quickMaskGradient(_ a: [String: Any], _ document: ImageDocument) throws -> String? {
        guard try editsQuickMask(a, document, tool: "gradient") else { return nil }
        try refuseNullOrBooleanNumbers(a, ["x0", "y0", "x1", "y1", "opacity"], in: "gradient")
        try requireGradientRamp(a, numericEndpoints: false)
        // The Quick Mask is coverage, which a blend mode has nothing to
        // composite into (the Gradient tool's own refusal there).
        if let blend = try blendModeArg(a), blend != DR_BLEND_NORMAL {
            throw ToolError(message: GradientRamp.coverageTargetRefusal)
        }
        guard let x0 = doubleArg(a, "x0"), let y0 = doubleArg(a, "y0"),
              let x1 = doubleArg(a, "x1"), let y1 = doubleArg(a, "y1"),
              [x0, y0, x1, y1].allSatisfy(\.isFinite)
        else { throw ToolError(message: "gradient requires finite x0, y0, x1 and y1") }
        guard let kind = GradientRamp.kind(named: stringArg(a, "shape") ?? "linear") else {
            throw ToolError(message:
                "shape must be \"linear\", \"radial\", \"angle\", \"reflected\" or \"diamond\"")
        }
        let opacity = doubleArg(a, "opacity") ?? 1
        guard opacity.isFinite, (0...1).contains(opacity) else {
            throw ToolError(message: "opacity must be in 0…1")
        }
        let smoothness = try gradientSmoothnessArgument(a)
        let faded: (NSColor, Double) -> NSColor = { color, strength in
            color.withAlphaComponent(color.alphaComponent * CGFloat(strength * opacity))
        }
        var stops: [(position: Double, color: NSColor)]
        var midpoints: [GradientRamp.StopMidpoint]?
        if let listed = try gradientStopArguments(a) {
            stops = try listed.map { stop in
                (stop.position,
                 faded(try coverageColor(stop.color, "color", in: document, fallback: .black),
                       stop.opacity))
            }
            midpoints = listed.map(\.midpoint)
        } else {
            let end = stringArg(a, "end_color") == nil
                ? NSColor.clear
                : try coverageColor(a, "end_color", in: document, fallback: .clear)
            stops = [
                (0, faded(try coverageColor(a, "start_color", in: document, fallback: .black), 1)),
                (1, faded(end, 1)),
            ]
        }
        let changed: Bool
        do {
            changed = try document.applyQuickMaskGradient(
                GradientRamp(
                    stops: stops, style: kind, midpoints: midpoints, smoothness: smoothness),
                from: CGPoint(x: x0, y: y0), to: CGPoint(x: x1, y: y1), record: .notACommand)
        } catch {
            throw error as? ToolError ?? ToolError(message: error.localizedDescription)
        }
        return try quickMaskEditResult(document, changed: changed)
    }

    private func quickMaskEditResult(_ document: ImageDocument, changed: Bool) throws -> String {
        var result = selectionHistorySummary(document)
        result["changed"] = changed
        result["target"] = "quick_mask"
        if !changed { result["note"] = "No Quick Mask coverage changed. No undo step was added." }
        return try jsonResult(result)
    }
}
