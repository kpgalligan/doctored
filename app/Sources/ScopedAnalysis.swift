import AppKit

// Scoped analysis sources are composite, raw layer on canvas or a visible contribution cut below a
// bottom-first record boundary with ancestor group gates intact. At 16 bits the composite scope
// counts a supplied retained projection (`projection:`, read on main beside `document.doc`)
// instead of rendering again, and at 8 bits `byteProjection:` (`retainedByteProjection`, the same
// rule) through `dr_doc_projection_scoped_analysis`; every other scope, and a caller that
// passes none, renders. A 32-bit float source answers the core's float schema (`storage:
// "rgba32_float"`: stop bins with below-zero/above-white weights; a linear sample with stops,
// parsed by `FloatReadout`) through the same reads, every scope and the selection weighting
// included; `ScopedAnalysisRead.bins` dispatches it to `floatBins`. `floatBins: .unit`
// asks a float source for linear 0…1 unit bins (the Levels/Curves sheet axis). U16 coverage carries
// exact weights; current u8 selections promote by ×257. Complete endpoint/sum counts are
// independent of plot stride. Histogram RGB excludes alpha0 while sample means retain hidden RGB.
// Concurrent scans register separate preview scratch leases and subtract only their own reservation
// before core admission.

/// The plot axis of a float histogram: stop bins (Info, MCP) or the linear
/// 0…1 unit bins a float Levels or Curves sheet's handles sit on.
enum FloatHistogramBins: String { case stops, unit }

/// One immutable source, scope and selection for histogram/readout UI and MCP.
/// Only plots may use a stride: exact sums/endpoints always cover the full scope.
struct ScopedAnalysisRead {
    let source: RasterDocument
    let request: String
    let selection: CanvasSelection?
    let capturedCoverage: [UInt8]?
    let control: RasterJobControl
    let scratchEstimate: UInt64
    let lease: PrecisionEditPayload
    let reservation: SelectionWorkBudget.Reservation?
    /// The retained typed projection the composite scope counts instead of
    /// rendering again; nil for every other scope or a byte source.
    let projection: RasterPrecisionImage?
    /// The retained 8-bit projection the composite scope counts instead of
    /// rendering again; nil for every other scope or a typed source.
    /// Pass `ImageDocument.retainedByteProjection`, read on main beside the
    /// same `document.doc` (the `byteComposite(of:)` rule).
    let byteProjection: RasterImage?

    init(source: RasterDocument, scope: HistogramSource, selection: CanvasSelection?,
         stride: Int = 1, point: (x: Int, y: Int)? = nil, reach: Int = 0,
         preview: Bool = false, coverage: [UInt8]? = nil,
         projection: RasterPrecisionImage? = nil,
         byteProjection: RasterImage? = nil, auto: [String: Any]? = nil,
         floatBins: FloatHistogramBins = .stops) throws {
        self.source = source
        self.projection = scope == .composite && source.precision != nil ? projection : nil
        self.byteProjection = scope == .composite && source.precision == nil
            ? byteProjection : nil
        self.selection = selection
        capturedCoverage = coverage
        var object: [String: Any] = ["operation": point == nil ? "histogram" : "sample", "stride": stride]
        switch scope {
        case .composite: object["source"] = "composite"
        case .layer(let index): object["source"] = "layer"; object["layer"] = index
        case .backdrop(let below): object["source"] = "backdrop"; object["below"] = below
        }
        if let point { object["x"] = point.x; object["y"] = point.y; object["reach"] = reach }
        // Levels Auto over a backdrop (`LevelsAuto`); the core refuses
        // it on any other source.
        if let auto { object["auto"] = auto }
        if floatBins == .unit && source.isFloat { object["float_bins"] = floatBins.rawValue }
        let data = try JSONSerialization.data(withJSONObject: object, options: .sortedKeys)
        guard let request = String(data: data, encoding: .utf8) else { throw DoctoredCoreError(message: "Invalid analysis request.") }
        self.request = request
        let plan: [String: Any]
        if let projection = self.projection {
            plan = try projection.scopedAnalysisPlan(request)
        } else if self.byteProjection != nil {
            plan = try source.projectionScopedAnalysisPlan(request)
        } else {
            plan = try source.scopedAnalysisPlan(request)
        }
        scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        // The original byte coverage and exact ×257 promotion overlap during
        // preparation. Captured selection ownership participates in the live set.
        let coverageBytes = selection == nil && coverage == nil ? 0 : UInt64(source.width * source.height) * 3
        lease = try PrecisionEditPayload.admitted(bytes: coverageBytes + UInt64(data.count) * 2 + 4096)
        if preview {
            let reserved = try SelectionWorkBudget.reservePreview(bytes: scratchEstimate, outputBytes: 0)
            reservation = reserved
            control = reserved.control
        } else {
            reservation = nil
            control = try RasterJobControl()
            guard control.setScratchLimit(scratchEstimate) else { throw DoctoredCoreError(message: "Could not reserve scoped analysis storage.") }
        }
    }

    func read() throws -> [String: Any] {
        try withExtendedLifetime(lease) {
            guard !control.isCancelled else { throw JobCancelled() }
            let coverage = capturedCoverage?.map { UInt16($0) * 257 } ?? selection?.maskSamples16() ?? []
            let budget: (limit: UInt64, other: UInt64)
            if let projection {
                let context = try PrecisionWorkContext(image: projection)
                budget = (context.limit, context.otherRetained)
            } else if let byteProjection {
                let context = try PrecisionWorkContext(byteImage: byteProjection)
                budget = (context.limit, context.otherRetained)
            } else if let typed = source.precision {
                let context = try PrecisionWorkContext(source: typed)
                budget = (context.limit, context.otherRetained)
            } else { budget = try source.legacyPrecisionWorkContext() }
            // The complete preview live set includes THIS reservation already.
            // Core adds this job's own plan, so subtract precisely this lease;
            // concurrent histogram/readout/preview reservations remain counted.
            let own = reservation == nil ? 0 : scratchEstimate
            guard budget.other >= own else { throw DoctoredCoreError(message: "Scoped analysis reservation is missing from the live set.") }
            let other = budget.other - own
            var result: [String: Any]
            if let projection {
                result = try projection.scopedAnalysis(request, coverage: coverage,
                    workingLimit: budget.limit, otherRetained: other, control: control)
            } else if let byteProjection {
                result = try source.projectionScopedAnalysis(byteProjection, request,
                    coverage: coverage, workingLimit: budget.limit, otherRetained: other,
                    control: control)
            } else {
                result = try source.scopedAnalysis(request, coverage: coverage,
                    workingLimit: budget.limit, otherRetained: other, control: control)
            }
            result["ok"] = true
            return result
        }
    }

    func run() throws -> [String: Any] {
        _ = try RasterResources.shared.admit(scratch: scratchEstimate)
        let task = try WorkerTask(scratchEstimate: scratchEstimate) { try read() }
        return try BackgroundWork.wait(task, title: "Reading Scoped Samples", control: control)
    }

    static func bins(_ report: [String: Any]) -> HistogramBins? {
        if report["storage"] as? String == "rgba32_float" { return floatBins(report) }
        guard let rows = report["bins"] as? [[NSNumber]], rows.count == 4,
              rows.allSatisfy({ $0.count == 256 }),
              let zeros = report["exact_zero_weight_units_rgba"] as? [NSNumber], zeros.count == 4,
              let maximum = report["exact_max_weight_units_rgba"] as? [NSNumber], maximum.count == 4,
              let total = report["full_rgb_weight_units"] as? NSNumber,
              let denominator = report["coverage_denominator"] as? NSNumber else { return nil }
        let divisor = denominator.doubleValue
        guard divisor > 0 else { return nil }
        return HistogramBins(values: rows.flatMap { $0 }.map(\.doubleValue),
            total: total.doubleValue / divisor,
            sampled: (report["stride"] as? NSNumber)?.intValue != 1,
            exactShadow: zeros.prefix(3).map(\.doubleValue).max().map { $0 / divisor },
            exactHighlight: maximum.prefix(3).map(\.doubleValue).max().map { $0 / divisor },
            completeEndpoints: true)
    }

    /// The 32-bit float schema: stop bins, with the end markers
    /// counting values at or below zero and at or above white over the
    /// complete scope — the ⌥ clipping display's inclusive `<= 0` / `>= 1`
    /// rule: the exact counts plus the beyond counts.
    private static func floatBins(_ report: [String: Any]) -> HistogramBins? {
        guard let rows = report["bins"] as? [[NSNumber]], rows.count == 4,
              rows.allSatisfy({ $0.count == 256 }),
              let below = report["below_zero_weight_units_rgba"] as? [NSNumber], below.count == 4,
              let above = report["above_white_weight_units_rgba"] as? [NSNumber], above.count == 4,
              let zero = report["exact_zero_weight_units_rgba"] as? [NSNumber], zero.count == 4,
              let one = report["exact_one_weight_units_rgba"] as? [NSNumber], one.count == 4,
              let total = report["full_rgb_weight_units"] as? NSNumber,
              let denominator = report["coverage_denominator"] as? NSNumber else { return nil }
        let divisor = denominator.doubleValue
        guard divisor > 0 else { return nil }
        return HistogramBins(values: rows.flatMap { $0 }.map(\.doubleValue),
            total: total.doubleValue / divisor,
            sampled: (report["stride"] as? NSNumber)?.intValue != 1,
            exactShadow: (0..<3).map { zero[$0].doubleValue + below[$0].doubleValue }.max()
                .map { $0 / divisor },
            exactHighlight: (0..<3).map { one[$0].doubleValue + above[$0].doubleValue }.max()
                .map { $0 / divisor },
            completeEndpoints: true)
    }
}

extension AgentServer {
    /// Info > source: raw layer or visible backdrop below a named record.
    func analysisScope(_ a: [String: Any], document: ImageDocument) throws -> HistogramSource {
        switch stringArg(a, "source") ?? "composite" {
        case "composite":
            guard a["below"] == nil, a["layer"] == nil else { throw ToolError(message: "Composite analysis takes no layer or below boundary.") }
            return .composite
        case "layer":
            guard a["below"] == nil else { throw ToolError(message: "Layer analysis takes no below boundary.") }
            return .layer(try structuralLayerIndex(a, document))
        case "backdrop":
            guard a["layer"] == nil, let below = intArg(a, "below"), (0...document.doc.layerCount).contains(below) else {
                throw ToolError(message: "Backdrop analysis requires below between 0 and layer_count, without layer.")
            }
            return .backdrop(below: below)
        default: throw ToolError(message: "Analysis source must be composite, layer or backdrop.")
        }
    }

    func scopedHistogram(_ a: [String: Any], document: ImageDocument) throws -> String {
        if stringArg(a, "source") == "scalar" { return try nativeScalarHistogram(a, document: document) }
        guard a["target"] == nil else { throw ToolError(message: "Histogram target requires source scalar.") }
        let stride = intArg(a, "stride") ?? 1
        guard stride > 0, stride <= Int(UInt32.max) else { throw ToolError(message: "Histogram stride must be a positive uint32.") }
        let read = try ScopedAnalysisRead(source: document.doc, scope: analysisScope(a, document: document),
            selection: (boolArg(a, "selection") ?? true) ? document.canvasSelection : nil, stride: stride,
            projection: document.retainedPrecisionProjection,
            byteProjection: document.retainedByteProjection)
        return try jsonResult(read.run())
    }

    func scopedSample(_ a: [String: Any], document: ImageDocument) throws -> String {
        try jsonResult(scopedSampleReport(a, document: document))
    }

    func scopedSampleReport(_ a: [String: Any], document: ImageDocument) throws -> [String: Any] {
        guard let x = intArg(a, "x"), let y = intArg(a, "y"),
              Int32(exactly: x) != nil, Int32(exactly: y) != nil else { throw ToolError(message: "Sampling requires int32 canvas x and y.") }
        let size = intArg(a, "sample_size") ?? 1
        guard [1, 3, 5].contains(size) else { throw ToolError(message: "Sample size must be 1, 3 or 5 pixels.") }
        let read = try ScopedAnalysisRead(source: document.doc, scope: analysisScope(a, document: document),
            selection: (boolArg(a, "selection") ?? false) ? document.canvasSelection : nil,
            point: (x, y), reach: size / 2, projection: document.retainedPrecisionProjection,
            byteProjection: document.retainedByteProjection)
        return try read.run()
    }
}
