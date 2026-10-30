import AppKit

extension AgentServer {
    /// Filters / Image > Adjustments share the sheet's captured native source,
    /// selection and one central publication. Explicit MCP targets are checked
    /// independently from the ambient editor target retained by the capture.
    func selectedPrecisionFilter(_ document: ImageDocument, filter: String, params: [String: Any],
                                 index: Int, target: PaintTarget) throws -> String {
        if target.isCoverage { return try precisionScalarFilter(document, filter: filter, params: params, layer: index, target: target) }
        let capture = try PrecisionEditorCapture(document)
        if let reason = scopeRefusal(.filter, target: target, doc: capture.facade, layer: index) {
            throw ToolError(message: reason)
        }
        let params = SelectedAdjustment.numericParameters(params)
        let edit = try SelectedAdjustment.edit(capture, layer: index, filter: filter, params: params, target: target)
        let result = try edit.run(title: "Applying \(filter)")
        let changed = try capture.publish(result, to: document, name: "Apply \(filter)")
        return try jsonResult(["ok": true, "changed": changed, "layer": index, "target": "layer",
            "filter": filter, "storage": capture.facade.isFloat ? "rgba32_float" : "rgba16_unorm", "write_scope": capture.selection == nil ? "whole_target" : "selection"])
    }

    /// Record the effective scope explicitly; old recordings with no field
    /// continue to use whole_target on both storage depths.
    func scopedToneArguments(tool: String, arguments: [String: Any]) throws -> [String: Any] {
        guard ["auto_tone", "auto_contrast", "auto_color"].contains(tool) else { return arguments }
        let allowed: Set<String> = ["layer", "target", "clip", "analysis_scope", "document_id"]
        guard Set(arguments.keys).isSubset(of: allowed) else {
            throw ToolError(message: "Unknown Auto adjustment argument. Use layer, target, clip or analysis_scope.")
        }
        var resolved = arguments
        resolved["analysis_scope"] = try ToneAnalysisScope.parse(arguments["analysis_scope"]).rawValue
        return resolved
    }

    func selectedPrecisionAuto(_ document: ImageDocument, index: Int, mode: DrAutoMode,
                               scope: ToneAnalysisScope, clip: Double, action: String) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        if let reason = scopeRefusal(.filter, target: .layer, doc: capture.facade, layer: index) {
            throw ToolError(message: reason)
        }
        let edit = try SelectedAutoAdjustment(capture: capture, layer: index, mode: mode, scope: scope, clip: clip, target: .layer)
        let result = try edit.run(title: action)
        let changed = try capture.publish(result.document, to: document, name: action)
        return try jsonResult(result.derived.merging(["ok": true, "changed": changed, "layer": index,
            "target": "layer", "analysis_scope": scope.rawValue, "analysis_region": scope.rawValue, "clip": clip,
            "write_scope": capture.selection == nil ? "whole_target" : "selection"]) { _, new in new })
    }
}

/// The automatic measurement and write share one admitted captured worker.
/// All writeback math remains native, including fractional coverage.
struct SelectedAutoAdjustment {
    let edit: PrecisionCapturedEdit
    let request: String
    let coverage: PrecisionScalarBuffer?
    let layer: Int

    init(capture: PrecisionEditorCapture, layer: Int, mode: DrAutoMode, scope: ToneAnalysisScope, clip: Double, target: PaintTarget? = nil) throws {
        self.layer = layer
        if let reason = OperationScope.refusalReason(.filter, target: target ?? capture.target, doc: capture.facade, layerIndex: layer) {
            throw DoctoredCoreError(message: reason)
        }
        guard clip.isFinite, (0...0.1).contains(clip) else { throw DoctoredCoreError(message: "clip must be in 0…0.1.") }
        guard scope != .selection || capture.selection != nil else {
            throw DoctoredCoreError(message: "Selection analysis requires an active selection.")
        }
        let op = mode == DR_AUTO_CONTRAST ? "auto_contrast" : mode == DR_AUTO_COLOR ? "auto_color" : "auto_tone"
        request = try SelectedAdjustment.json(["layer": layer, "op": op, "params": ["clip": clip], "analysis_scope": scope.rawValue])
        edit = try SelectedAdjustment.edit(capture, request: request, target: target)
        if case .selected(_, let mask) = edit.operation { coverage = mask } else { coverage = nil }
    }

    func run(title: String) throws -> (document: RasterPrecisionDocument?, derived: [String: Any]) {
        try PrecisionCapturedWork.run(scratchEstimate: edit.scratchEstimate, title: title) { control in
            let budget = try PrecisionWorkContext(source: edit.source)
            let derived = try control.withChildOperation(from: 0, to: 0.45) { child in
                try edit.source.selectedAutoLevels(request: request, coverage: coverage?.samples ?? [],
                    workingLimit: budget.limit, otherRetained: budget.otherRetained, control: child)
            }
            let resolved = try SelectedAdjustment.json(["layer": layer, "op": "levels_channels", "params": [
                "black": derived["black"] ?? [], "white": derived["white"] ?? [], "gamma": derived["gamma"] ?? []]])
            let result = try control.withChildOperation(from: 0.45, to: 1) { child in
                try edit.source.selectedEdit(request: resolved, coverage: coverage?.samples ?? [],
                    workingLimit: budget.limit, otherRetained: budget.otherRetained, control: child)
            }
            return (result, derived)
        }
    }
}
