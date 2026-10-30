import AppKit

extension RasterDocument {
    /// The one 8-bit display of a typed document's composite (`layer` nil) or
    /// of one record's raw pixels on the canvas (a group rendered alone):
    /// a float document's own tone map, else the u16 preview
    /// (`display(of:context:control:)`). A disposable derivative that cannot
    /// enter a typed edit, save or source binding. On the main thread it runs
    /// as admitted, cancellable worker work; off it, directly — the
    /// `PrecisionScalarOperations.sourcePlane` rule.
    func nativeDisplay(layer: Int?) throws -> RasterImage {
        guard precision != nil else {
            throw DoctoredCoreError(message: "Native display requires a typed document.")
        }
        guard Thread.isMainThread else {
            return try nativeDisplayWork(layer: layer, control: try RasterJobControl())
        }
        // Each stage reserves against the whole limit on its own child, so the
        // estimate is the larger stage. The display: an f32 canvas at 16 B/px,
        // display scratch at 12 and the output at 4. A record's read: the
        // core's own layer-source plan plus one canvas at up to 16 B/px.
        let pixels = UInt64(width * height)
        var scratch = try projectionScratchEstimate() + pixels * 32
        if let layer {
            let request = "{\"layer\":\(layer),\"operation\":\"histogram\",\"source\":\"layer\"}"
            let plan = try PrecisionDocumentDescriptor.integer(scopedAnalysisPlan(request), "scratch_bytes")
            scratch = max(scratch, plan + pixels * 16)
        }
        return try PrecisionCapturedWork.run(scratchEstimate: scratch, title: "Render Native Display") {
            try self.nativeDisplayWork(layer: layer, control: $0)
        }
    }

    /// A `RasterJobControl` is single-use in the core, so the render and the
    /// display each take a child of `control`.
    private func nativeDisplayWork(layer: Int?, control: RasterJobControl) throws -> RasterImage {
        guard let precision else {
            throw DoctoredCoreError(message: "Native display requires a typed document.")
        }
        let context = try PrecisionWorkContext(source: precision)
        let image = try control.withChildOperation(from: 0, to: 0.7) { child -> RasterPrecisionImage in
            if let layer {
                return try precision.layerCanvasImage(layer, workingLimit: context.limit,
                                                      otherRetained: context.otherRetained, control: child)
            }
            return try precision.render(workingLimit: context.limit,
                                        otherRetained: context.otherRetained, control: child)
        }
        return try control.withChildOperation(from: 0.7, to: 1) {
            try display(of: image, context: PrecisionWorkContext(image: image), control: $0)
        }
    }
}

extension AgentServer {
    /// Public render is explicitly a display derivative: the record's raw
    /// pixels on the canvas, through the document's tone map at float.
    func nativeLayerDisplay(_ doc: RasterDocument, layer: Int) throws -> RasterImage {
        try doc.nativeDisplay(layer: layer)
    }
}
