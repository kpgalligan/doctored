import AppKit

// Select and Mask's two New Layer outputs and Decontaminate Colors, at both depths. The
// foreground is the captured composite, its own alpha kept (8 bits: the capture's byte composite;
// 16 bits: the document's typed projection; 32-bit float: its float projection, every value kept,
// above white included), or that composite with its fringe colour replaced by the
// estimated foreground (8 and 16 bits; a float document refuses Decontaminate Colors by name). The refined coverage becomes the new "Refined Cutout" layer's mask
// through `RefinementCutoutCapture` (assembly, then the display it publishes with); New Layer bakes
// that mask into the layer's alpha before the one publication. Every other top-level layer is
// hidden, not changed, and the whole output is one Undo step.

extension SelectionRefinementCapture {
    /// The 8-bit New Layer outputs: `alpha` is the refined coverage.
    func applyNewLayer(alpha: [UInt8], parameters: SelectionRefinementParameters,
                       output: SelectionRefinementOutput, in document: ImageDocument,
                       record: [ActionStep]) throws -> SelectionRefinementResult {
        guard let composite = selection.source else {
            throw DoctoredCoreError(message: "No refinement source.")
        }
        var foreground = composite
        var decontaminated = false
        if parameters.decontaminate {
            let doc = selection.doc
            let estimate: [String: Any] = ["color_space": "encoded",
                                           "strength": parameters.decontaminateAmount]
            // The solver's 128 bytes a pixel and megabyte (`dr_image_estimate_foreground`),
            // and the estimated image it returns.
            let scratch = UInt64(alpha.count) * 132 + 1_048_576
            if let cleaned = try PrecisionCapturedWork.run(scratchEstimate: scratch,
                title: "Decontaminating Colors", compute: { control in
                    try composite.estimatingForeground(in: doc, alpha: alpha, parameters: estimate,
                                                       control: control)
                }) {
                foreground = cleaned
                decontaminated = true
            }
            try validate(in: document, output: output)
        }
        let session = try RefinementTrimapSession(width: selection.doc.width, height: selection.doc.height)
        let cutout = try RefinementCutoutCapture(document: document, session: session)
        guard let prepared = try cutout.prepare(foreground: foreground, alpha: alpha, in: document,
                                                session: session) else {
            return SelectionRefinementResult(changed: false, decontaminated: decontaminated)
        }
        return try publish(prepared, session: session, output: output, in: document, record: record,
                           decontaminated: decontaminated)
    }

    /// The 16- and 32-bit New Layer outputs: `matte` is the refined u16 coverage.
    func applyNewLayerNative(matte: [UInt16], parameters: SelectionRefinementParameters,
                             output: SelectionRefinementOutput, in document: ImageDocument,
                             record: [ActionStep]) throws -> SelectionRefinementResult {
        guard let source = selection.doc.precision else {
            throw DoctoredCoreError(message: "Native refinement requires a 16-bit or 32-bit document.")
        }
        // #1: a CMYK document's cutout is assembled in ink (`CmykRefinementCutout.swift`).
        if source.isCmyk {
            return try applyNewLayerCmyk(matte: matte, parameters: parameters, output: output,
                                         in: document, record: record)
        }
        var foreground = try document.currentPrecisionProjection()
        var decontaminated = false
        if parameters.decontaminate {
            let composite = foreground
            let estimate: [String: Any] = ["color_space": "encoded",
                                           "strength": parameters.decontaminateAmount]
            let plan = try source.foregroundEstimatePlan(source: composite)
            let scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
            if let cleaned = try PrecisionCapturedWork.run(scratchEstimate: scratch,
                title: "Decontaminating Colors", compute: { control in
                    let context = try PrecisionWorkContext(source: source)
                    return try source.estimatingForeground(source: composite, matte: matte,
                        parameters: estimate, workingLimit: context.limit,
                        otherRetained: context.otherRetained, control: control)
                }) {
                foreground = cleaned
                decontaminated = true
            }
            try validate(in: document, output: output)
        }
        let session = try RefinementTrimapSession(width: selection.doc.width, height: selection.doc.height)
        let cutout = try RefinementCutoutCapture(document: document, session: session)
        guard let prepared = try cutout.prepare(foreground: foreground, matte: matte, in: document,
                                                session: session) else {
            return SelectionRefinementResult(changed: false, decontaminated: decontaminated)
        }
        return try publish(prepared, session: session, output: output, in: document, record: record,
                           decontaminated: decontaminated)
    }

    /// Publishes the assembled cutout as one edit. New Layer with Layer Mask
    /// publishes it as assembled; New Layer first applies the new top layer's
    /// mask to its alpha (Layer ▸ Apply Layer Mask's own operation at each
    /// depth) and prepares that document's display.
    func publish(_ prepared: PreparedRefinementCutout, session: RefinementTrimapSession,
                 output: SelectionRefinementOutput, in document: ImageDocument,
                 record: [ActionStep], decontaminated: Bool) throws -> SelectionRefinementResult {
        let top = prepared.document.layerCount - 1
        guard output == .newLayer else {
            try prepared.commit(in: document, session: session,
                                name: "Refine to New Layer with Layer Mask", record: record)
            return SelectionRefinementResult(changed: true, newLayer: top, decontaminated: decontaminated)
        }
        guard let baked = try prepared.document.removingLayerMask(top, apply: true) else {
            throw DoctoredCoreError(message: "The refined coverage could not be applied to the new layer.")
        }
        try prepared.capture.validate(in: document, session: session)
        let scratch = try baked.projectionScratchEstimate() + 1_048_576
        _ = try RasterResources.shared.admit(scratch: scratch)
        let reservation = BackgroundWork.scratch
        let task = try WorkerTask(scratchEstimate: scratch) {
            PreparedDocumentEdit(baked, scratch: reservation)
        }
        let edit = try BackgroundWork.wait(task, title: "Preparing Layer Display")
        if let error = edit.error { throw DoctoredCoreError(message: error) }
        try PreparedRefinementCutout(capture: prepared.capture, edit: edit, control: prepared.control)
            .commit(in: document, session: session, name: "Refine to New Layer", record: record)
        return SelectionRefinementResult(changed: true, newLayer: top, decontaminated: decontaminated)
    }
}
