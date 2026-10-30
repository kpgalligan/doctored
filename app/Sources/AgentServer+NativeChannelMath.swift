import AppKit

extension AgentServer {
    /// Image > Apply Image, with the same native capture as the visible sheet.
    func nativeApplyImage(_ document: ImageDocument, parameters: ApplyImageParameters) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        if let reason = scopeRefusal(.filter, target: parameters.target, doc: capture.facade,
            layer: parameters.targetLayer) { throw ToolError(message: reason) }
        let scratch = try NativeChannelMath.scratch(capture.facade, apply: parameters)
        let result = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Apply Image") {
            try NativeChannelMath.apply(capture, parameters: parameters, control: $0)
        }
        let changed = try capture.publish(result, to: document, name: "Apply Image")
        return try jsonResult(["ok": true, "changed": changed, "target": parameters.target.agentName(in: capture.facade),
            "layer": parameters.targetLayer, "storage": "u16", "write_scope": capture.selection == nil ? "whole_target" : "selection",
            "working_memory_plan_bytes": scratch])
    }

    /// Image > Calculations: native output stays scalar through either landing.
    func nativeCalculations(_ document: ImageDocument, parameters: CalculationsParameters,
                            result: CalculationsResult) throws -> String {
        let capture = try PrecisionEditorCapture(document)
        guard !capture.quickMask else { throw ToolError(message: OperationScope.quickMaskRefusal) }
        let samples = try PrecisionCapturedWork.run(scratchEstimate: NativeChannelMath.scratch(capture.facade, calculations: parameters), title: "Calculations") {
            try NativeChannelMath.calculated(capture.facade, parameters: parameters, control: $0)
        }
        try capture.requireCurrent(document)
        if result == .selection {
            document.setCanvasSelection(CanvasSelection(shape: .mask16(samples), canvasWidth: capture.facade.width,
                canvasHeight: capture.facade.height), actionName: "Calculations Selection")
            return try jsonResult(selectionHistorySummary(document).merging(["result": "selection", "storage": "u16"]) { _, new in new })
        }
        let work = try PrecisionScalarWork(source: capture.source, command: .setChannel(name: parameters.name), samples: samples)
        let changed = try capture.publish(work.run(title: "Save Calculations Channel"), to: document, name: "Calculations")
        return try jsonResult(["ok": true, "changed": changed, "result": "new_channel", "channel": capture.facade.channelCount,
            "name": parameters.name, "storage": "u16"])
    }
}

extension NativeChannelMath {
    static func luminosity(_ doc: RasterDocument, control: RasterJobControl) throws -> RasterPrecisionDocument? {
        guard let source = doc.precision else { throw DoctoredCoreError(message: "Native luminosity masks require a 16-bit source.") }
        try source.requireChannelCapacity(additional: 9)
        let reader = NativePlaneReader(doc, control: control)
        // A Lab document's luminosity is its Lightness, the stored red codes (#1 Lab close).
        let luma = try reader.plane(layer: nil, choice: .plane(doc.isLab ? .red : .luma))
        var current = source
        for band in ["lights", "darks", "midtones"] {
            for level in 1...3 {
                let work = try NativeSelectionWork(width: doc.width, height: doc.height,
                    operation: ["kind": "luminosity", "band": band, "level": level], input: luma)
                let values = try reader.stage { try work.compute(control: $0) }
                let context = try PrecisionWorkContext(source: current)
                let request = try PrecisionPixelCommand.setChannel(name: "\(band.capitalized) \(level)").request
                guard let next = try reader.stage({ try current.editingPixels(request: request, samples: values,
                    workingLimit: context.limit, otherRetained: context.otherRetained, control: $0) }) else {
                    throw DoctoredCoreError(message: "Could not append the complete native luminosity mask set.")
                }
                current = next
            }
        }
        return current
    }
}

extension EditorViewController {
    func addNativeLuminosityMasks() -> Bool {
        guard let document, document.doc?.isHighDepth == true else { return false }
        do {
            let capture = try PrecisionEditorCapture(document)
            guard !capture.quickMask else { throw DoctoredCoreError(message: OperationScope.quickMaskRefusal) }
            let result = try PrecisionCapturedWork.run(scratchEstimate: NativeChannelMath.scratch(capture.facade), title: "Add Luminosity Masks") {
                try NativeChannelMath.luminosity(capture.facade, control: $0)
            }
            _ = try capture.publish(result, to: document, name: "Add Luminosity Masks", record: .addLuminosityMasks)
        } catch { document.presentError(error) }
        return true
    }
}
