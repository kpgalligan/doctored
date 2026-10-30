import AppKit

/// Auto analysis reads the captured scalar storage independently of selected
/// writeback. The final edit applies the reported levels without measuring
/// again, through the same linked cancellation owner and one publication.
struct PrecisionScalarAuto {
    let edit: PrecisionScalarWork
    let target: (String, Int)
    let selected: Bool

    init(capture: PrecisionEditorCapture, layer: Int, target: PaintTarget,
         mode: DrAutoMode, scope: ToneAnalysisScope, clip: Double) throws {
        guard clip.isFinite, (0...0.1).contains(clip), scope != .selection || capture.selection != nil else {
            throw DoctoredCoreError(message: "Auto scalar analysis needs clip 0…0.1 and an active selection for selection analysis.")
        }
        let op = mode == DR_AUTO_CONTRAST ? "auto_contrast" : mode == DR_AUTO_COLOR ? "auto_color" : "auto_tone"
        let operation: [String: Any] = ["kind": "filter", "edit": ["layer": 0, "op": op,
            "params": ["clip": clip], "analysis_scope": scope.rawValue]]
        edit = try PrecisionScalarOperations.edit(capture, target: target, layer: layer, operation: operation)
        self.target = try PrecisionScalarOperations.target(target, layer: layer)
        selected = capture.selection != nil
    }

    func run(title: String) throws -> (document: RasterPrecisionDocument?, derived: [String: Any]) {
        try PrecisionCapturedWork.run(scratchEstimate: edit.scratchEstimate, title: title) { control in
            let budget = try PrecisionWorkContext(source: edit.source)
            let derived = try control.withChildOperation(from: 0, to: 0.45) { child in
                try edit.source.scalarAutoLevels(request: edit.command.request, input: edit.samples,
                    workingLimit: budget.limit, otherRetained: budget.otherRetained, control: child)
            }
            if (derived["identity"] as? Bool) == true { return (nil, derived) }
            let request = try PrecisionPixelCommand(["op": "scalar_edit", "target": target.0,
                "index": target.1, "selection": selected, "operation": ["kind": "filter",
                    "edit": ["layer": 0, "op": "levels_channels", "params": [
                        "black": derived["black"] ?? [], "white": derived["white"] ?? [], "gamma": derived["gamma"] ?? []]]]])
            let result = try control.withChildOperation(from: 0.45, to: 1) { child in
                try edit.source.editingPixels(request: request.request, samples: edit.samples,
                    workingLimit: budget.limit, otherRetained: budget.otherRetained, control: child)
            }
            return (result, derived)
        }
    }
}

extension AgentServer {
    func selectedPrecisionScalarAuto(_ document: ImageDocument, index: Int, target: PaintTarget,
                                     mode: DrAutoMode, scope: ToneAnalysisScope, clip: Double, action: String) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        if !capture.quickMask, let reason = scopeRefusal(.filter, target: target,
            doc: capture.facade, layer: index) { throw ToolError(message: reason) }
        let work = try PrecisionScalarAuto(capture: capture, layer: index, target: target, mode: mode, scope: scope, clip: clip)
        let result = try work.run(title: action)
        let changed = try capture.publish(result.document, to: document, name: action)
        return try jsonResult(result.derived.merging(["ok": true, "changed": changed, "layer": index,
            "target": target.agentName(in: capture.facade), "storage": "u16", "analysis_scope": scope.rawValue,
            "analysis_region": scope.rawValue, "clip": clip,
            "write_scope": capture.selection == nil ? "whole_target" : "selection"]) { _, new in new })
    }
}
