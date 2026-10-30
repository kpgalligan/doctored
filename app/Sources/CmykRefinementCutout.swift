import AppKit

// Select and Mask's New Layer outputs on a CMYK document (#547). The core assembles the cutout
// (`cmyk_cutout`: the visible composite's ink on a new top CMYK raster, the refined u16 coverage
// as its mask, Decontaminate Colors on ink when asked), so no foreground image crosses the
// boundary; the preparation, the display and the one publication are `NativeRefinementCutout`'s
// (`prepareNative`), and New Layer bakes the mask through the CMYK Apply Layer Mask
// (`SelectionRefinement+NewLayer.swift`'s `publish`).

extension SelectionRefinementCapture {
    /// The CMYK New Layer outputs: `matte` is the refined u16 coverage; Decontaminate Colors runs
    /// on ink at `parameters.decontaminateAmount`.
    func applyNewLayerCmyk(matte: [UInt16], parameters: SelectionRefinementParameters,
                           output: SelectionRefinementOutput, in document: ImageDocument,
                           record: [ActionStep]) throws -> SelectionRefinementResult {
        let strength = parameters.decontaminate ? min(max(parameters.decontaminateAmount, 0), 1) : 0
        let session = try RefinementTrimapSession(width: selection.doc.width, height: selection.doc.height)
        let cutout = try RefinementCutoutCapture(document: document, session: session)
        guard let prepared = try cutout.prepareCmyk(matte: matte, decontaminate: strength,
                                                    in: document, session: session) else {
            return SelectionRefinementResult(changed: false, decontaminated: false)
        }
        return try publish(prepared, session: session, output: output, in: document, record: record,
                           decontaminated: strength > 0)
    }
}

extension RefinementCutoutCapture {
    /// `prepareNative`'s twin over `addingCmykCutout`: the core's plan admitted beside the
    /// unregistered matte, the assembly and the display each a cancellable child.
    func prepareCmyk(matte: [UInt16], decontaminate: Double, in document: ImageDocument,
                     session: RefinementTrimapSession,
                     control: RasterJobControl? = nil) throws -> PreparedRefinementCutout? {
        try validate(in: document, session: session)
        guard let captured = selection.doc.precision, captured.isCmyk,
              matte.count == selection.doc.width * selection.doc.height else {
            throw DoctoredCoreError(message: "The CMYK refinement requires a CMYK document and canvas-sized coverage.")
        }
        let preparation = try RefinementCutoutPreparation(document: document, control: control)
        defer { preparation.finish() }
        let token = preparation.control
        let plan = try captured.cmykCutoutPlan(decontaminate: decontaminate)
        let coreScratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        let borrowed = try PrecisionDocumentDescriptor.integer(plan, "borrowed_matte_bytes")
        let scratch = try PrecisionDocumentDescriptor.sum(coreScratch, borrowed)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let report = try RasterResources.shared.report()
        let all = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
        let own = try PrecisionDocumentDescriptor.integer(plan, "retained_bytes")
        guard all >= own else {
            throw DoctoredCoreError(message: "Native refinement inputs are missing from resource accounting.")
        }
        let other = try PrecisionDocumentDescriptor.sum(all - own,
            PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes"))
        let limit = RasterResources.shared.workingLimit
        let native = try BackgroundWork.wait(title: "Creating Cutout", control: token) {
            try WorkerTask(scratchEstimate: scratch) {
                try token.withChildOperation(from: 0, to: 0.25) { child in
                    guard child.setScratchLimit(coreScratch) else {
                        throw DoctoredCoreError(message: "Could not configure native cutout memory.")
                    }
                    return try captured.addingCmykCutout(matte: matte, decontaminate: decontaminate,
                        workingLimit: limit, otherRetained: other, control: child)
                }
            }
        }
        try validate(in: document, session: session)
        guard !token.isCancelled else { throw JobCancelled() }
        guard let native else { return nil }
        let result = try RasterDocument(precision: native)
        let projectionScratch = try PrecisionDocumentDescriptor.sum(result.projectionScratchEstimate(), borrowed)
        _ = try RasterResources.shared.admit(scratch: projectionScratch)
        let prepared = try BackgroundWork.wait(title: "Preparing Cutout Display", control: token) {
            try WorkerTask(scratchEstimate: projectionScratch) {
                try token.withChildOperation(from: 0.25, to: 1) { child in
                    let prepared = PreparedDocumentEdit(result, control: child, borrowedInputBytes: borrowed)
                    if let error = prepared.error { throw DoctoredCoreError(message: error) }
                    return prepared
                }
            }
        }
        try validate(in: document, session: session)
        guard !token.isCancelled else { throw JobCancelled() }
        guard prepared.precisionProjection != nil, prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "The cutout display could not be prepared.")
        }
        return PreparedRefinementCutout(capture: self, edit: prepared, control: token)
    }
}
