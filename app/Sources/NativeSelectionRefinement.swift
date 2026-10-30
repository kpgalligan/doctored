import AppKit

// On a 16-bit document the `SelectionRefinementCapture` refines its exact u16 coverage: the edge
// stage through the typed `refine_edge` selection kind (`SelectionCapture.refinedEdge`, the document
// itself as the guide), then the four coverage stages through `NativeSelectionWork`. A mask output
// is a canvas-sized u16 mask through `PrecisionPixelEdit(.setMask)` and the one
// `PrecisionEditorCapture.publish`, so the typed facade never reaches
// `dr_doc_adding_layer_mask`/`dr_selection_refine`; typed previews are explicit byte display
// derivatives and never enter a commit. A 32-bit float document refines the same way (the edge stage
// on its R1 display codes, rule E); Decontaminate Colors refuses there before any job.

/// The typed twin of SelectionRefinementCapture's byte path: a 16-bit document
/// refines its captured u16 coverage natively and writes a canvas-sized u16
/// mask, so the layer's RGBA16 is the unchanged input and output. Previews stay
/// explicit byte display derivatives and never enter a commit.
extension SelectionRefinementCapture {
    /// The four coverage stages over `input` (the captured coverage when nil).
    func nativeWork(_ parameters: SelectionRefinementParameters,
                    input: [UInt16]? = nil) throws -> NativeSelectionWork {
        guard let coverage16 else {
            throw DoctoredCoreError(message: "Native refinement requires a 16-bit document's captured coverage.")
        }
        return try NativeSelectionWork(width: selection.doc.width, height: selection.doc.height,
                                       operation: parameters.nativeOperation, input: input ?? coverage16)
    }

    /// The edge stage's typed work over the captured coverage, or nil when
    /// `parameters` runs no edge stage. `area` is the brush's plane.
    func nativeEdgeWork(_ parameters: SelectionRefinementParameters,
                        area: [UInt8]?) throws -> NativeSelectionWork? {
        guard parameters.runsEdgeStage else { return nil }
        guard let coverage16, let source = selection.doc.precision else {
            throw DoctoredCoreError(message: "Native refinement requires a 16-bit document's captured coverage.")
        }
        let operation: [String: Any] = ["kind": "refine_edge", "edge_radius": parameters.edgeRadius,
                                        "smart_radius": parameters.smartRadius, "area": area != nil]
        return try NativeSelectionWork(source: source, width: selection.doc.width,
            height: selection.doc.height, operation: operation,
            input: coverage16 + (area ?? []).map(UInt16.init))
    }

    /// The maskBytes() projection rule, applied to a result that never became
    /// a CanvasSelection. This is display only; the commit keeps every code.
    static func displayBytes(_ samples: [UInt16]) -> [UInt8] {
        samples.map { UInt8((UInt32($0) + 128) / 257) }
    }

    func nativeDisplay(_ samples: [UInt16], background: RefinementCanvasView.Background) -> CGImage? {
        let bytes = Self.displayBytes(samples)
        if background == .mask {
            return CanvasSelection.grayImage(bytes, selection.doc.width, selection.doc.height)
        }
        return preview(bytes, output: .selection)
    }

    /// The core's sentence for Decontaminate Colors in a 32-bit float document
    /// (`float_document::decontaminate_refusal`, the operator table's row).
    static let floatDecontaminateRefusal =
        "Select and Mask's Decontaminate Colors is unavailable in a 32-bit float document"

    func applyNative(_ parameters: SelectionRefinementParameters, output: SelectionRefinementOutput,
                     in document: ImageDocument, record: [ActionStep]) throws -> SelectionRefinementResult {
        try validate(in: document, output: output)
        let parameters = try parameters.validated(for: output)
        // Refused before any job, so no work runs for an edit that cannot land.
        if parameters.decontaminate, selection.doc.isFloat {
            throw DoctoredCoreError(message: Self.floatDecontaminateRefusal)
        }
        guard var samples = coverage16 else {
            throw DoctoredCoreError(message: "Native refinement requires a 16-bit document's captured coverage.")
        }
        if parameters.runsEdgeStage {
            let area = try parameters.areaPlane(width: selection.doc.width, height: selection.doc.height)
            guard case .mask16(let refined) = try selection.refinedEdge(.mask16(samples),
                edgeRadius: parameters.edgeRadius, smartRadius: parameters.smartRadius, area: area,
                in: document, title: "Select and Mask") else {
                throw DoctoredCoreError(message: "The refined edge lost its native coverage.")
            }
            samples = refined
        }
        // Zero controls copy exactly, so a refinement with only an edge stage
        // skips the second job.
        if parameters.runsPlaneStage || !parameters.runsEdgeStage {
            samples = try nativeWork(parameters, input: samples).run(title: "Refine Selection")
        }
        try validate(in: document, output: output)
        switch output {
        case .selection:
            guard samples != coverage16 else { return SelectionRefinementResult(changed: false) }
            try selection.commit(.mask16(samples), in: document, name: "Refine Selection", record: record)
            return SelectionRefinementResult(changed: true)
        case .newLayer, .newLayerWithMask:
            return try applyNewLayerNative(matte: samples, parameters: parameters, output: output,
                                           in: document, record: record)
        case .mask:
            break
        }
        let editor = try PrecisionEditorCapture(document)
        guard editor.facade === selection.doc else {
            throw DoctoredCoreError(message: "The captured document changed before the refined mask was written.")
        }
        let width = selection.doc.width, height = selection.doc.height
        let input = try PrecisionScalarBuffer(samples: samples, width: width, height: height)
        let work = try PrecisionPixelEdit(editor.source,
            command: .setMask(layer: layer, width: width, height: height), input: input)
        let result = try work.run(title: "Creating Layer Mask")
        // An identical mask returns no new document, which is not a change.
        let changed = try editor.publish(result, to: document, name: "Refine to Layer Mask", record: record)
        return SelectionRefinementResult(changed: changed)
    }
}
