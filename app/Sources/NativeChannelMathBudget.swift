import AppKit

// Compound child tokens preserve cancellation; operation-specific core source/read/write/blend
// plans plus simultaneous native buffers and display work bound the parent reservation. Flattened
// inputs and selected write payloads are admitted before allocation.

extension NativeChannelMath {
    /// Compound jobs must cover their largest core stage as well as the native
    /// planes retained between stages. Source extraction has its own admitted
    /// compositor/statistics plan; a canvas-only estimate cannot bound it.
    static func scratch(_ doc: RasterDocument, apply: ApplyImageParameters? = nil,
                        calculations: CalculationsParameters? = nil) throws -> UInt64 {
        guard let source = doc.precision else {
            throw DoctoredCoreError(message: "Native channel arithmetic requires a typed source.")
        }
        let budget = try PrecisionWorkContext(source: source)
        let standalone = try NativeSelectionWork.context(nil)
        func pixelPlan(_ fields: [String: Any]) throws -> UInt64 {
            let request = try PrecisionPixelCommand(fields).request
            let plan = try source.pixelEditPlan(request: request, workingLimit: budget.limit,
                otherRetained: budget.otherRetained)
            return try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        }
        func sourcePlan(layer: Int?, choice: PlaneChoice) throws -> UInt64 {
            if case .channel(let index) = choice {
                return try pixelPlan(["op": "read_plane", "target": "channel", "index": index])
            }
            // #547: a CMYK layer's plate is its stored samples (`read_plane`); the composite's
            // is the `color` selection, bounded with the other selection kinds below.
            if case .ink(let plate) = choice, let layer {
                return try pixelPlan(["op": "read_plane", "target": plate.key, "index": layer,
                                      "canvas": true])
            }
            // Alpha, RGB and luma extraction share this source-stage plan; a Lab document plans
            // its Lightness `color` read, since it refuses `luminance` (#547 Lab phase 6b-i).
            let kind: [String: Any] = doc.isLab ? ["kind": "color", "channel": "red"]
                : ["kind": "luminance"]
            var fields: [String: Any] = ["op": "selection", "source": "composite",
                "operation": kind]
            if let layer { fields["source"] = "layer"; fields["layer"] = layer }
            return try pixelPlan(fields)
        }
        func planePlan(_ fields: [String: Any]) throws -> UInt64 {
            let request = try NativeSelectionWork.encoding(fields)
            let plan = try RasterSelection.nativePlan(width: doc.width, height: doc.height,
                request: request, workingLimit: standalone.limit, otherRetained: standalone.other)
            return try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        }
        var stage: UInt64 = 0
        if let apply, doc.isGray, apply.target == .layer {
            // #547 phase 6b: a Gray layer is read as its luminance (R = G = B) and written once
            // through the Gray scalar target (`NativeChannelMath.applyGray`).
            stage = try max(sourcePlan(layer: apply.targetLayer, choice: .rgb),
                sourcePlan(layer: apply.source, choice: apply.sourcePlane))
            stage = try max(stage, pixelPlan(["op": "scalar_edit", "target": "gray",
                "index": apply.targetLayer, "selection": true, "operation": ["kind": "replace_canvas"]]))
        } else if let apply {
            // A CMYK layer target writes its plates (#547), an RGB one its colour planes.
            let target = apply.target == .layer
                ? (doc.isCmyk ? PaintTarget.ink(.cyan) : PaintTarget.plane(.red)) : apply.target
            let addressed = try PrecisionScalarOperations.target(target, layer: apply.targetLayer)
            let choice: PlaneChoice
            if case .channel(let index) = target { choice = .channel(index) }
            else if case .ink(let plate) = target { choice = .ink(plate) }
            else { choice = .rgb }
            stage = try max(sourcePlan(layer: apply.targetLayer, choice: choice),
                sourcePlan(layer: apply.source, choice: apply.sourcePlane))
            stage = try max(stage, pixelPlan(["op": "scalar_edit", "target": addressed.0,
                "index": addressed.1, "selection": true, "operation": ["kind": "replace_canvas"]]))
        } else if let calculations {
            stage = try max(sourcePlan(layer: calculations.source1.layer, choice: calculations.source1.plane),
                sourcePlan(layer: calculations.source2.layer, choice: calculations.source2.plane))
        } else {
            stage = try sourcePlan(layer: nil, choice: .plane(.luma))
            stage = try max(stage, planePlan(["kind": "luminosity", "band": "midtones", "level": 3]))
            stage = try max(stage, pixelPlan(["op": "set_channel", "index": NSNull(), "name": "Luminosity"]))
        }
        // A CMYK layer blends plate by plate, one plane at a time (#547); a Gray layer is one plane.
        let channels = apply?.target == .layer && !doc.isCmyk && !doc.isGray ? 3 : 1
        stage = try max(stage, planePlan(["kind": "blend", "channels": channels,
            "mode": Int((apply?.blend ?? calculations?.blend ?? DR_BLEND_NORMAL).rawValue),
            "opacity": apply?.opacity ?? calculations?.opacity ?? 1,
            "invert_base": calculations?.source2.invert ?? false,
            "invert_source": apply?.invert ?? calculations?.source1.invert ?? false]))
        // Six cached planes, flattened blend input/output and split arrays,
        // selected write payloads, Swift COW overlap and the nine-channel
        // luminosity result remain covered while a child kernel runs. Newly
        // returned native handles also enter the ordinary retained registry.
        let resident = UInt64(doc.width * doc.height) * 128 + 1_048_576
        return try PrecisionDocumentDescriptor.sum(stage,
            PrecisionDocumentDescriptor.sum(resident, doc.projectionScratchEstimate()))
    }
}
