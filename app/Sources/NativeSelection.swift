import AppKit

// Quick Mask accepts six-byte legacy or eight-byte native canonical runs; generated brush precision
// remains explicit.

/// Native coverage requests own their input, immutable source and admitted job
/// for their whole worker lifetime. Display conversion happens after the result.
struct NativeSelectionWork {
    let source: RasterPrecisionDocument?
    let width: Int
    let height: Int
    let request: String
    let input: [UInt16]
    let scratchEstimate: UInt64
    let payload: PrecisionEditPayload

    init(source: RasterPrecisionDocument? = nil, width: Int, height: Int,
         operation: [String: Any], input: [UInt16] = [], scope: HistogramSource = .composite) throws {
        self.source = source; self.width = width; self.height = height; self.input = input
        var fields = operation
        if source != nil {
            fields = ["op": "selection", "operation": operation]
            switch scope {
            case .composite: fields["source"] = "composite"
            case .layer(let index): fields["source"] = "layer"; fields["layer"] = index
            case .backdrop(let below): fields["source"] = "backdrop"; fields["below"] = below
            }
        }
        request = try Self.encoding(fields)
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(input.count) * 2 + UInt64(request.utf8.count) * 2 + 4096)
        let context = try Self.context(source)
        let plan = try source?.pixelEditPlan(request: request, workingLimit: context.limit, otherRetained: context.other)
            ?? RasterSelection.nativePlan(width: width, height: height, request: request,
                workingLimit: context.limit, otherRetained: context.other)
        scratchEstimate = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == UInt64(input.count),
              try PrecisionDocumentDescriptor.integer(plan, "output_samples") == UInt64(width * height) else {
            throw DoctoredCoreError(message: "The native selection plan does not match its captured buffers.")
        }
    }

    static func encoding(_ fields: [String: Any]) throws -> String {
        let request = try SelectedAdjustment.json(fields)
        guard request.utf8.count <= 64 * 1024 else { throw DoctoredCoreError(message: "Native scalar request exceeds 64 KiB.") }
        return request
    }

    static func context(_ source: RasterPrecisionDocument?) throws -> (limit: UInt64, other: UInt64) {
        if let source {
            let context = try PrecisionWorkContext(source: source)
            return (context.limit, context.otherRetained)
        }
        let report = try RasterResources.shared.report()
        return (RasterResources.shared.workingLimit, try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes"),
            PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes")))
    }

    /// `replacement` computes over other samples of the planned length in
    /// place of the captured input: a second stage planned on the main thread
    /// before the first stage's result existed (Select and Mask's typed
    /// preview, whose coverage stages follow its edge stage).
    func compute(control: RasterJobControl, ownReservation: UInt64 = 0,
                 over replacement: [UInt16]? = nil) throws -> [UInt16] {
        let input = replacement ?? self.input
        guard input.count == self.input.count else {
            throw DoctoredCoreError(message: "The native selection plan does not match its captured buffers.")
        }
        let context = try Self.context(source)
        guard context.other >= ownReservation else { throw DoctoredCoreError(message: "Native selection reservation is missing.") }
        let other = context.other - ownReservation
        if let source {
            return try source.selection(request: request, input: input, workingLimit: context.limit,
                otherRetained: other, control: control)
        }
        return try RasterSelection.nativeTransform(input, width: width, height: height, request: request,
            workingLimit: context.limit, otherRetained: other, control: control)
    }

    func run(title: String = "Selection") throws -> [UInt16] {
        try PrecisionCapturedWork.run(scratchEstimate: scratchEstimate, title: title) { try compute(control: $0) }
    }
}

extension SelectionCapture {
    func nativeSource(_ operation: [String: Any], in document: ImageDocument,
                      title: String, scope: HistogramSource = .composite,
                      input: [UInt16] = []) throws -> [UInt16] {
        try requireCurrent(in: document)
        guard let source = doc.precision else { throw DoctoredCoreError(message: "Native source selection requires a typed document.") }
        let result = try NativeSelectionWork(source: source, width: doc.width, height: doc.height,
            operation: operation, input: input, scope: scope).run(title: title)
        try requireCurrent(in: document)
        return result
    }
}

extension CanvasSelection {
    func nativeModified(_ kind: String, radius: Double = 0) throws -> CanvasSelection? {
        var operation: [String: Any] = ["kind": kind]
        if kind != "invert" { operation["radius"] = radius }
        let samples = try NativeSelectionWork(width: canvasWidth, height: canvasHeight,
            operation: operation, input: maskSamples16()).run(title: "Modify Selection")
        return CanvasSelection(shape: .mask16(samples), canvasWidth: canvasWidth, canvasHeight: canvasHeight)
    }

    /// Old synchronous sheet callbacks cannot throw. A refused native operation
    /// retains its exact original selection and reports the error to the user.
    func nativeModifiedForUI(_ kind: String, radius: Double) -> CanvasSelection? {
        do { return try nativeModified(kind, radius: radius) }
        catch { if Thread.isMainThread { NSApp.presentError(error) }; return self }
    }
}

extension SelectionTransformParameters {
    var nativeOperation: [String: Any] {
        let m = matrix
        return ["kind": "transform", "matrix": [m.a, m.b, m.c, m.d, m.tx, m.ty].map(Double.init), "filter": filter]
    }
}

extension SelectionCapture {
    /// `gamutSetup`: the resolved `ColorRangeGamut.setup(window:)`, read only by Out Of Gamut; nil
    /// for every other range. Out Of Gamut with nil throws "Out Of Gamut needs its proof setup."
    func colorRangeShape(_ parameters: ColorRangeParameters, gamutSetup: ProofSetup?,
                         in document: ImageDocument) throws -> CanvasSelection.Shape {
        if parameters.isOutOfGamut {
            guard let gamutSetup else {
                throw DoctoredCoreError(message: "Out Of Gamut needs its proof setup.")
            }
            return try outOfGamutShape(invert: parameters.invert, setup: gamutSetup, in: document)
        }
        if doc.isHighDepth {
            return .mask16(try nativeSource(["kind": "color_range", "params": parameters.nativeCoreParameters],
                in: document, title: "Color Range"))
        }
        guard let source else { throw DoctoredCoreError(message: "The image has no captured projection.") }
        let core = parameters.coreParameters
        return .mask(try run(in: document, title: "Color Range",
            scratchBytes: UInt64(doc.width * doc.height) * 6 + 262_144) {
            try doc.colorRange(source: source, parameters: core, control: $0)
        })
    }

    /// Quick Selection's coverage for the core's `parameters` (`seeds`,
    /// `tolerance`, `edge`, `radius`, `smooth`). `enhanceEdge` then refines
    /// the result's edge against the picture (radius 3, Smart Radius) and
    /// smooths it by 1 px: the tool's Enhance Edge and `quick_select`'s
    /// `enhance_edge`.
    func quickSelectionShape(_ parameters: [String: Any], enhanceEdge: Bool,
                             in document: ImageDocument) throws -> CanvasSelection.Shape {
        // At float the grow and Enhance Edge (Select and Mask's edge
        // stage) both read the R1 display codes (`display_encoding`, rule E).
        let grown: CanvasSelection.Shape
        if doc.isHighDepth {
            grown = .mask16(try nativeSource(["kind": "quick", "params": parameters], in: document, title: "Quick Selection"))
        } else {
            guard let source else { throw DoctoredCoreError(message: "The image has no captured projection.") }
            grown = .mask(try run(in: document, title: "Quick Selection",
                scratchBytes: UInt64(doc.width * doc.height) * 24 + 1_048_576) {
                try doc.quickSelection(source: source, parameters: parameters, control: $0)
            })
        }
        guard enhanceEdge else { return grown }
        let refined = try refinedEdge(grown, edgeRadius: 3, smartRadius: true, area: nil,
                                      in: document, title: "Quick Selection")
        return try smoothedShape(refined, radius: 1, in: document)
    }

    /// The edge stage's byte budget: the core's 64 bytes a pixel
    /// (`dr_selection_refine`), the refined output and the area plane, the
    /// four stages' line scratch and a megabyte of slack.
    static func edgeRefineScratchBytes(width: Int, height: Int) -> UInt64 {
        UInt64(width * height) * 66 + UInt64(max(width, height)) * 20 + 1_048_576
    }

    /// The one Swift entry to Select and Mask's edge stage, at both depths:
    /// `coverage` re-estimated from the captured composite inside `edgeRadius`
    /// of its 50% contour and under the area plane's INCLUDE pixels (`area`:
    /// 0 never, 128 automatic, 255 always; nil = all automatic). 8 bits runs
    /// `dr_selection_refine` on the captured byte composite; 16 and 32 bits the
    /// typed `refine_edge` selection kind on the document itself (at 32 bits on
    /// its R1 display codes, rule E). An empty coverage
    /// comes back unchanged.
    func refinedEdge(_ coverage: CanvasSelection.Shape, edgeRadius: Double, smartRadius: Bool,
                     area: [UInt8]?, in document: ImageDocument,
                     title: String) throws -> CanvasSelection.Shape {
        let width = doc.width, height = doc.height
        guard area == nil || area?.count == width * height else {
            throw DoctoredCoreError(message: "The refinement area does not match the canvas.")
        }
        if doc.isHighDepth {
            let samples: [UInt16]
            switch coverage {
            case .mask16(let native):
                samples = native
            case .mask(let bytes):
                // `maskSamples16`'s lossless promotion.
                samples = bytes.map { UInt16($0) * 257 }
            case .rect, .ellipse, .polygon:
                guard let selection = CanvasSelection(shape: coverage, canvasWidth: width,
                                                      canvasHeight: height) else { return coverage }
                samples = selection.maskSamples16()
            }
            let operation: [String: Any] = ["kind": "refine_edge", "edge_radius": edgeRadius,
                                            "smart_radius": smartRadius, "area": area != nil]
            return .mask16(try nativeSource(operation, in: document, title: title,
                                            input: samples + (area ?? []).map(UInt16.init)))
        }
        guard let source else { throw DoctoredCoreError(message: "The image has no captured projection.") }
        let mask: [UInt8]
        switch coverage {
        case .mask(let bytes):
            mask = bytes
        case .mask16(let samples):
            // `maskBytes`' display projection.
            mask = samples.map { UInt8((UInt32($0) + 128) / 257) }
        case .rect, .ellipse, .polygon:
            guard let selection = CanvasSelection(shape: coverage, canvasWidth: width,
                                                  canvasHeight: height) else { return coverage }
            mask = selection.maskBytes()
        }
        let parameters: [String: Any] = ["edge_radius": edgeRadius, "smart_radius": smartRadius]
        return .mask(try run(in: document, title: title,
            scratchBytes: Self.edgeRefineScratchBytes(width: width, height: height)) {
            try source.refiningSelection(mask, brush: area, parameters: parameters, control: $0)
        })
    }

    /// `shape` smoothed by `radius` px at its own depth (Select ▸ Modify ▸
    /// Smooth's kernel); geometric shapes come back unchanged.
    private func smoothedShape(_ shape: CanvasSelection.Shape, radius: Double,
                               in document: ImageDocument) throws -> CanvasSelection.Shape {
        switch shape {
        case .mask16(let samples):
            try requireCurrent(in: document)
            let result = try NativeSelectionWork(width: doc.width, height: doc.height,
                operation: ["kind": "smooth", "radius": radius], input: samples).run(title: "Quick Selection")
            try requireCurrent(in: document)
            return .mask16(result)
        case .mask(var bytes):
            guard RasterSelection.smoothMask(&bytes, width: doc.width, height: doc.height, radius: radius) else {
                throw DoctoredCoreError(message: "The selection edge could not be smoothed.")
            }
            return .mask(bytes)
        case .rect, .ellipse, .polygon:
            return shape
        }
    }

    func transformedShape(_ parameters: SelectionTransformParameters, in document: ImageDocument) throws -> CanvasSelection.Shape {
        guard let selection = selection.selection else { throw DoctoredCoreError(message: "Make a selection first.") }
        let width = doc.width, height = doc.height
        if doc.isHighDepth || selection.hasNativeCoverage {
            try requireCurrent(in: document)
            let result = try NativeSelectionWork(width: width, height: height,
                operation: parameters.nativeOperation, input: selection.maskSamples16()).run(title: "Transform Selection")
            try requireCurrent(in: document)
            return .mask16(result)
        }
        let mask = selection.maskBytes()
        return .mask(try run(in: document, title: "Transform Selection", scratchBytes: UInt64(width * height) * 8 + 4096) {
            try RasterImage.transformingSelection(mask, width: width, height: height,
                matrix: parameters.matrix, filter: parameters.sampler, control: $0)
        })
    }
}
