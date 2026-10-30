import AppKit

// Native foreground source alpha stays distinct from the u16 refinement mask. Capture never
// materializes a byte source; only the separately prepared display is reduced. Native assembly,
// authoritative rendering and byte-preview quantization use separately admitted cancellable
// children; the shared `PreparedDocumentEdit` optional-control path checks cancellation around
// CoreGraphics construction. Unregistered raw matte retention follows host admission and the native
// render/preview work contexts; registered scalar buffers add zero extra bytes. The foreground
// estimate (Decontaminate Colors) is a separate job that runs before this one
// (`SelectionRefinement+NewLayer.swift`), and coverage refinement is
// `NativeSelectionRefinement.swift`'s; neither is called here.

/// The New Layer output's 16- and 32-bit assembly (RGBA16, or RGBA32 float in a
/// float document, rule C): native source alpha and refinement coverage
/// remain distinct.
extension RefinementCutoutCapture {
    func prepare(foreground: RasterPrecisionImage, matte: [UInt16], in document: ImageDocument,
                 session: RefinementTrimapSession,
                 control: RasterJobControl? = nil) throws -> PreparedRefinementCutout? {
        try prepareNative(foreground: foreground, matte: matte, registeredMatte: nil,
            in: document, session: session, control: control)
    }

    /// Native selection/mask work already owns a registered scalar payload.
    /// Keep that lease alive and let the core count its borrowed samples once.
    func prepare(foreground: RasterPrecisionImage, matte: PrecisionScalarBuffer,
                 in document: ImageDocument, session: RefinementTrimapSession,
                 control: RasterJobControl? = nil) throws -> PreparedRefinementCutout? {
        guard matte.width == selection.doc.width, matte.height == selection.doc.height,
              matte.planeCount == 1 else {
            throw DoctoredCoreError(message: "Native refinement requires one canvas-sized coverage plane.")
        }
        return try withExtendedLifetime(matte) {
            try prepareNative(foreground: foreground, matte: matte.samples,
                registeredMatte: matte.retainedPayload, in: document, session: session, control: control)
        }
    }

    private func prepareNative(foreground: RasterPrecisionImage, matte: [UInt16],
                               registeredMatte: PrecisionEditPayload?, in document: ImageDocument,
                               session: RefinementTrimapSession,
                               control: RasterJobControl?) throws -> PreparedRefinementCutout? {
        try validate(in: document, session: session)
        guard let captured = selection.doc.precision,
              matte.count == selection.doc.width * selection.doc.height else {
            throw DoctoredCoreError(message: "Native refinement requires a high-depth document and canvas-sized coverage.")
        }
        let preparation = try RefinementCutoutPreparation(document: document, control: control)
        defer { preparation.finish() }
        let token = preparation.control
        let plan = try captured.refinementCutoutPlan(foreground: foreground)
        let coreScratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        let borrowed = try PrecisionDocumentDescriptor.integer(plan, "borrowed_matte_bytes")
        let registeredBytes = registeredMatte == nil ? 0 : borrowed
        guard registeredMatte == nil || (registeredMatte?.bytes ?? 0) >= borrowed else {
            throw DoctoredCoreError(message: "Native refinement coverage is missing its retained payload.")
        }
        let unregisteredBytes = borrowed - registeredBytes
        let scratch = try PrecisionDocumentDescriptor.sum(coreScratch, unregisteredBytes)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let report = try RasterResources.shared.report()
        let all = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
        let own = try PrecisionDocumentDescriptor.integer(plan, "retained_bytes")
        let countedByCore = try PrecisionDocumentDescriptor.sum(own, registeredBytes)
        guard all >= countedByCore else {
            throw DoctoredCoreError(message: "Native refinement inputs are missing from resource accounting.")
        }
        let other = try PrecisionDocumentDescriptor.sum(all - countedByCore,
            PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes"))
        let limit = RasterResources.shared.workingLimit
        let native = try BackgroundWork.wait(title: "Creating Cutout", control: token) {
            try WorkerTask(scratchEstimate: scratch) {
                try token.withChildOperation(from: 0, to: 0.25) { child in
                    guard child.setScratchLimit(coreScratch) else {
                        throw DoctoredCoreError(message: "Could not configure native cutout memory.")
                    }
                    return try captured.addingRefinementCutout(foreground: foreground, matte: matte,
                        workingLimit: limit, otherRetained: other, control: child)
                }
            }
        }
        try validate(in: document, session: session)
        guard !token.isCancelled else { throw JobCancelled() }
        guard let native else { return nil }
        let result = try RasterDocument(precision: native)
        let projectionScratch = try PrecisionDocumentDescriptor.sum(result.projectionScratchEstimate(), unregisteredBytes)
        _ = try RasterResources.shared.admit(scratch: projectionScratch)
        let prepared = try BackgroundWork.wait(title: "Preparing Cutout Display", control: token) {
            try WorkerTask(scratchEstimate: projectionScratch) {
                try token.withChildOperation(from: 0.25, to: 1) { child in
                    let prepared = PreparedDocumentEdit(result, control: child,
                        borrowedInputBytes: unregisteredBytes)
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
