import AppKit

// Select and Mask: the captured selection's coverage refined by the edge stage (Edge
// Detection radius, Smart Radius, the Refine Edge brush), then Smooth, Shift Edge, Feather and
// Contrast, and written to one of four outputs. The sheet, the `refine_selection` MCP tool and a
// replayed Action share this capture and its one `apply`. The two New Layer outputs and Decontaminate
// Colors live in `SelectionRefinement+NewLayer.swift`; the 16-bit route in
// `NativeSelectionRefinement.swift`.

/// Select and Mask's controls. Every field has a default, and all defaults
/// together reproduce the captured coverage exactly.
struct SelectionRefinementParameters {
    var smooth = 0.0
    var feather = 0.0
    var shift = 0.0
    var contrast = 0.0
    /// Edge Detection: the half-width, in canvas pixels, of the band around
    /// the selection's edge that is re-estimated from the picture.
    var edgeRadius = 0.0
    /// Fade the estimate where the picture has no edge.
    var smartRadius = false
    /// Refine Edge brush strokes, in order: `add` always refines under the
    /// stroke, `subtract` never does.
    var brush: [RefineEdgeStroke] = []
    /// Decontaminate Colors: replace fringe colour with the estimated
    /// foreground. It changes pixels, so it needs a New Layer output.
    var decontaminate = false
    var decontaminateAmount = 1.0

    /// Whether the core's edge stage runs: a radius, or a brush stroke that
    /// adds to the refined area.
    var runsEdgeStage: Bool { edgeRadius > 0 || brush.contains { $0.mode == .add } }

    /// Whether any of the four coverage stages after the edge stage runs.
    var runsPlaneStage: Bool { smooth != 0 || feather != 0 || shift != 0 || contrast != 0 }

    func validated() throws -> Self {
        guard [smooth, feather, shift, contrast].allSatisfy(\.isFinite),
              (0...64).contains(smooth), (0...64).contains(feather),
              (-64...64).contains(shift), (0...1).contains(contrast) else {
            throw DoctoredCoreError(message: "Smooth and Feather must be 0–64 px, Shift −64–64 px, and Contrast 0–100%.")
        }
        guard edgeRadius.isFinite, (0...64).contains(edgeRadius) else {
            throw DoctoredCoreError(message: "Edge Detection Radius must be 0–64 px.")
        }
        guard decontaminateAmount.isFinite, (0...1).contains(decontaminateAmount) else {
            throw DoctoredCoreError(message: "Decontaminate Colors Amount must be 0–100%.")
        }
        try RefineEdgeStroke.validate(brush)
        return self
    }

    /// `validated`, plus the one rule that depends on the output.
    func validated(for output: SelectionRefinementOutput) throws -> Self {
        let checked = try validated()
        guard !decontaminate || output.createsLayer else {
            throw DoctoredCoreError(message: "Decontaminate Colors changes pixel colours; choose output new_layer or new_layer_with_mask.")
        }
        return checked
    }

    /// `dr_selection_refine`'s JSON. The brush travels beside it as the area
    /// plane (`areaPlane`).
    var coreParameters: [String: Any] {
        ["smooth": smooth, "feather": feather, "shift": shift, "contrast": contrast,
         "edge_radius": edgeRadius, "smart_radius": smartRadius]
    }

    /// The `refine_selection` arguments that reproduce these controls.
    var arguments: [String: Any] {
        var values: [String: Any] = ["smooth": smooth, "feather": feather, "shift": shift,
                                     "contrast": contrast, "edge_radius": edgeRadius,
                                     "smart_radius": smartRadius, "decontaminate": decontaminate]
        if !brush.isEmpty { values["brush"] = brush.map(\.arguments) }
        if decontaminate { values["decontaminate_amount"] = decontaminateAmount }
        return values
    }

    /// The typed twin of the four coverage stages. The edge stage is its own
    /// selection kind (`SelectionCapture.refinedEdge`), run before this one.
    var nativeOperation: [String: Any] {
        ["kind": "refine", "smooth": smooth, "feather": feather, "shift": shift, "contrast": contrast]
    }

    /// The brush as the core's area plane, or nil with no strokes.
    func areaPlane(width: Int, height: Int) throws -> [UInt8]? {
        try RefineEdgeStroke.areaPlane(brush, width: width, height: height)
    }
}

enum SelectionRefinementOutput: String {
    case selection, mask
    case newLayer = "new_layer"
    case newLayerWithMask = "new_layer_with_mask"

    /// The two outputs that add a "Refined Cutout" layer.
    var createsLayer: Bool { self == .newLayer || self == .newLayerWithMask }
}

/// What one `apply` did: whether the document or selection changed, the new
/// layer's index for a New Layer output, and whether Decontaminate Colors
/// changed any colour.
struct SelectionRefinementResult {
    var changed: Bool
    var newLayer: Int? = nil
    var decontaminated = false
}

struct SelectionRefinementCapture {
    let selection: SelectionCapture
    let coverage: [UInt8]
    let coverage16: [UInt16]?
    let layer: Int
    let fingerprint: String?
    let layerName: String
    let hadMask: Bool
    let maskOutputBytes: UInt64

    init(_ document: ImageDocument, layer: Int? = nil) throws {
        selection = try SelectionCapture(document: document)
        guard let coverage = selection.selection.selection?.maskBytes() else {
            throw DoctoredCoreError(message: "Make a selection before opening Select and Mask.")
        }
        self.coverage = coverage
        // A 16-bit document refines its exact captured u16 coverage; the byte
        // projection above stays the display and byte-ABI input. A selection
        // that has no native samples promotes the bytes ALREADY captured
        // rather than rasterizing its geometry a second time — `maskSamples16`
        // would re-run `maskBytes`, and the promotion is the same ×257.
        coverage16 = selection.doc.isHighDepth
            ? (selection.selection.selection?.hasNativeCoverage == true
                ? selection.selection.selection?.maskSamples16()
                : coverage.map { UInt16($0) * 257 })
            : nil
        self.layer = layer ?? document.activeLayerIndex
        guard let info = selection.doc.layerInfo(self.layer) else {
            throw DoctoredCoreError(message: "The output layer does not exist.")
        }
        fingerprint = selection.doc.layerFingerprint(self.layer)
        layerName = info.name
        hadMask = selection.doc.layerHasMask(self.layer)
        // Typed masks are written canvas-sized at (0,0), as addPrecisionMask does.
        maskOutputBytes = coverage16 != nil || selection.doc.layerIsGroup(self.layer)
            ? UInt64(selection.doc.width * selection.doc.height)
            : UInt64(info.width) * UInt64(info.height)
    }

    var isNative: Bool { coverage16 != nil }

    /// The byte refinement's worker budget, its output included: the edge
    /// stage's (`SelectionCapture.edgeRefineScratchBytes`) when it runs, else
    /// the four stages' 10 bytes a pixel and line scratch.
    func scratchEstimate(for parameters: SelectionRefinementParameters) -> UInt64 {
        let width = selection.doc.width, height = selection.doc.height
        guard !parameters.runsEdgeStage else {
            return SelectionCapture.edgeRefineScratchBytes(width: width, height: height)
        }
        return UInt64(coverage.count) * 10 + UInt64(max(width, height)) * 20 + 1_048_576
    }

    func validate(in document: ImageDocument, output: SelectionRefinementOutput) throws {
        try selection.requireCurrent(in: document)
        guard output == .mask else { return }
        guard document.doc?.layerFingerprint(layer) == fingerprint else {
            throw DoctoredCoreError(message: "The captured mask output layer changed.")
        }
        if let refusal = selection.doc.lockRefusal(layer, kind: DR_EDIT_MASK) {
            throw DoctoredCoreError(message: refusal.message)
        }
    }

    /// The sheet's picture of `mask`. The New Layer outputs show the coverage
    /// over the composite, as Selection does: a decontaminated preview is not
    /// drawn.
    func preview(_ mask: [UInt8], output: SelectionRefinementOutput) -> CGImage? {
        switch output {
        case .selection, .newLayer, .newLayerWithMask:
            return selection.source?.masked(by: mask)?.makeCGImage(in: selection.doc.colorSpace)
        case .mask:
            // A typed facade never reaches the byte mask constructor; its
            // preview is the explicit byte display derivative instead.
            guard !isNative else { return preview(mask, output: .selection) }
            // Canvas-sized, never cropped to the layer; an identical
            // mask is the core's no-op, so the document already shows it.
            let doc = selection.doc
            let result = doc.addingLayerMask(layer, fromCoverage: mask)
                ?? (doc.hasLayerMask(layer, fromCoverage: mask) ? doc : nil)
            return result?.flattened()?.makeCGImage(in: doc.colorSpace)
        }
    }

    /// Both UI and MCP compute from the original captured coverage. Reopening
    /// the workspace starts from the resulting selection; Undo restores either.
    @discardableResult
    func apply(_ parameters: SelectionRefinementParameters, output: SelectionRefinementOutput,
               in document: ImageDocument, record: [ActionStep]) throws -> Bool {
        try applyReporting(parameters, output: output, in: document, record: record).changed
    }

    /// `apply`, answering what it did.
    func applyReporting(_ parameters: SelectionRefinementParameters,
                        output: SelectionRefinementOutput, in document: ImageDocument,
                        record: [ActionStep]) throws -> SelectionRefinementResult {
        if isNative { return try applyNative(parameters, output: output, in: document, record: record) }
        try validate(in: document, output: output)
        let parameters = try parameters.validated(for: output)
        guard let source = selection.source else { throw DoctoredCoreError(message: "No refinement source.") }
        let original = coverage
        let core = parameters.coreParameters
        let area = try parameters.areaPlane(width: selection.doc.width, height: selection.doc.height)
        // One core call: the edge stage (the area plane is its `brush`), then
        // the four coverage stages.
        let mask = try selection.run(in: document, title: "Select and Mask",
                                     scratchBytes: scratchEstimate(for: parameters)) { token in
            try source.refiningSelection(original, brush: area, parameters: core, control: token)
        }
        try validate(in: document, output: output)
        switch output {
        case .selection:
            guard mask != coverage else { return SelectionRefinementResult(changed: false) }
            try selection.commit(.mask(mask), in: document, name: "Refine Selection", record: record)
            return SelectionRefinementResult(changed: true)
        case .newLayer, .newLayerWithMask:
            return try applyNewLayer(alpha: mask, parameters: parameters, output: output,
                                     in: document, record: record)
        case .mask:
            break
        }
        let capturedDoc = selection.doc
        let index = layer
        let scratch = try capturedDoc.projectionScratchEstimate() + UInt64(mask.count) * 8
            + maskOutputBytes * 2 + 1_048_576
        _ = try RasterResources.shared.admit(scratch: scratch)
        let reservation = BackgroundWork.scratch
        let task = try WorkerTask(scratchEstimate: scratch) { () -> PreparedDocumentEdit? in
            // Canvas-sized at (0, 0), never cropped to the layer.
            guard let result = capturedDoc.addingLayerMask(index, fromCoverage: mask) else {
                if capturedDoc.hasLayerMask(index, fromCoverage: mask) { return nil }
                throw DoctoredCoreError(message: "The refined coverage could not become this layer's mask.")
            }
            guard !capturedDoc.hasSameLayerMask(as: result, index: index) else { return nil }
            return PreparedDocumentEdit(result, scratch: reservation)
        }
        let result = try BackgroundWork.wait(task, title: "Creating Layer Mask")
        try validate(in: document, output: output)
        guard let result else { return SelectionRefinementResult(changed: false) }
        document.preparedEdit = result
        try document.preflightPublication(result.document, record: record)
        withUndoGroup(document.undoManager) { document.applyEdit("Refine to Layer Mask", record: record) { _ in result.document } }
        return SelectionRefinementResult(changed: true)
    }
}

/// The preview queue returns images only; this tag lets the UI reject an older
/// render after controls changed, without sharing AppKit state with the worker.
final class RefinementPreviewStamp {
    private let lock = NSLock()
    private var serial = 0
    func set(_ value: Int) { lock.lock(); serial = value; lock.unlock() }
    var value: Int { lock.lock(); defer { lock.unlock() }; return serial }
}
