import AppKit

// MCP `histogram(source: "scalar")` at 32-bit float measures masks and channels (u16 coverage)
// exactly as at 16 bits while the core refuses the f32 red/green/blue/alpha by name.
// `NativeScalarStatistics` uses the shared core weighted counter, maps canvas selection to raw
// offsets, keeps complete endpoint/sum counts independently of plot stride, and rejects stale sheet
// delivery.

/// Captured raw scalar statistics share the core's complete weighted histogram
/// counters. The opaque gray interpretation counts zero-valued masks/channels;
/// neither source alpha nor an 8-bit display can hide their samples.
struct NativeScalarStatisticsRead {
    let source: RasterPrecisionDocument
    let request: String
    let coverage: [UInt16]
    let scratchEstimate: UInt64
    let control: RasterJobControl
    let lease: PrecisionEditPayload
    let reservation: SelectionWorkBudget.Reservation?

    init(source document: RasterDocument, target: PaintTarget, layer: Int,
         selection: CanvasSelection?, stride: Int = 1, preview: Bool = false) throws {
        guard let source = document.precision, stride > 0, stride <= Int(UInt32.max) else {
            throw DoctoredCoreError(message: "Native scalar statistics require a 16-bit or 32-bit float document and a positive stride.")
        }
        self.source = source
        let addressed = try PrecisionScalarOperations.target(target, layer: layer)
        request = try PrecisionPixelCommand(["op": "scalar_statistics", "target": addressed.0,
            "index": addressed.1, "selection": selection != nil, "stride": stride]).request
        lease = try PrecisionEditPayload.admitted(bytes: UInt64(selection == nil ? 0 : document.width * document.height) * 2
            + UInt64(request.utf8.count) * 2 + 8192)
        coverage = selection?.maskSamples16() ?? []
        let budget = try PrecisionWorkContext(source: source)
        let plan = try source.pixelEditPlan(request: request, workingLimit: budget.limit, otherRetained: budget.otherRetained)
        guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == UInt64(coverage.count) else {
            throw DoctoredCoreError(message: "Scalar histogram coverage differs from its captured plan.")
        }
        scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        if preview {
            let reservation = try SelectionWorkBudget.reservePreview(bytes: scratchEstimate, outputBytes: 0)
            self.reservation = reservation; control = reservation.control
        } else {
            reservation = nil; control = try RasterJobControl()
            guard control.setScratchLimit(scratchEstimate) else { throw DoctoredCoreError(message: "Could not reserve scalar histogram storage.") }
        }
    }

    func read() throws -> [String: Any] {
        let budget = try PrecisionWorkContext(source: source)
        let own = reservation == nil ? 0 : scratchEstimate
        guard budget.otherRetained >= own else { throw DoctoredCoreError(message: "Scalar histogram reservation is missing.") }
        return try source.scalarStatistics(request: request, input: coverage, workingLimit: budget.limit,
            otherRetained: budget.otherRetained - own, control: control)
    }

    func run() throws -> [String: Any] {
        let task = try WorkerTask(scratchEstimate: scratchEstimate) { try read() }
        return try BackgroundWork.wait(task, title: "Reading Native Scalar Statistics", control: control)
    }
}

extension AgentServer {
    /// Mirrors the exact raw scalar histogram behind native Levels controls.
    func nativeScalarHistogram(_ arguments: [String: Any], document: ImageDocument) throws -> String {
        try jsonResult(nativeScalarHistogramReport(arguments, document: document))
    }

    /// `nativeScalarHistogram`'s report before it is wrapped; a Lab Color document renames a
    /// colour plane's (`LabDocument.scalarReport`, #547 phase 2d).
    func nativeScalarHistogramReport(_ arguments: [String: Any],
                                     document: ImageDocument) throws -> [String: Any] {
        let allowed: Set<String> = ["document_id", "source", "target", "layer", "selection", "stride"]
        guard Set(arguments.keys).isSubset(of: allowed), arguments["target"] != nil else {
            throw ToolError(message: "Scalar histogram requires target and accepts layer, selection and stride.")
        }
        guard arguments["selection"] == nil || boolArg(arguments, "selection") != nil,
              arguments["stride"] == nil || intArg(arguments, "stride") != nil else {
            throw ToolError(message: "Scalar histogram selection must be boolean and stride must be an integer.")
        }
        let target = try paintTarget(arguments, document)
        let layer = try structuralLayerIndex(arguments, document)
        let read = try NativeScalarStatisticsRead(source: document.doc, target: target, layer: layer,
            selection: (boolArg(arguments, "selection") ?? true) ? document.canvasSelection : nil,
            stride: intArg(arguments, "stride") ?? 1)
        return try read.run()
    }
}
