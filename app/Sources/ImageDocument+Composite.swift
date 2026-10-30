import AppKit

// `byteComposite(of:)` reads `retainedByteProjection` when the handle is the document's own and one
// is retained, otherwise `render()` (the core's sentence, never the stored `projectionError`).
// Apply Image / Calculations from Merged (`MergedComposite`, resolved on main before the edit; a
// failure is an alert in the UI and `isError` over MCP), their sheet previews, the Evaluation
// readout and Remove Red Eye read it. So do `PixelReadout.at` (throws the
// sentence; `sample_pixel` answers `isError`, the Info panel's `setCursorFailure` shows "—" and
// names it in the footnote until the next readout or a hand-off to the scoped readout), Copy Merged
// (`mergedCopySource()`: an alert through `document.presentError` in the UI; `copy_merged` answers
// `isError`), the eyedropper (`eyedropperComposite()`: NEVER an alert — one in the press would
// swallow its mouse-up and latch the canvas's eyedropper drag — only a beep on the press, silent on
// drag ticks) and its twin `sample_color` (`isError`), Print (`byteComposite` at 8 bits,
// `currentDisplayProjection()` at 16/32; the sentence is the print's error) and
// `SelectionCapture`'s byte source (the capture throws). `AgentServer.projectionDifference`'s typed
// branch reads the `beforePrecisionProjection` it is handed (`get_evaluation` passes
// `retainedPrecisionProjection`), and so does the typed Evaluation sheet's "before"
// (`retainedPrecisionBefore`, captured at init). The typed readers keep the same
// contract: `PrecisionReadout.at` / `FloatReadout.at` answer nil off the canvas
// (`RasterDocument.sampleBlockTouchesCanvas`, checked before any render) and throw everything else;
// `samplePrecisionColor` calls `eyedropperReadFailed()` (the press beep, at 32 bits too), and
// `setPrecisionCursor` / `setFloatCursor` / `requestScopedCursor` show "—" through
// `showReadoutFailure` with the reason in the footnote at both depths. The scoped analysis reads
// follow the same projection rule: the 16-bit composite scope reads the retained projection through
// `dr_precision_image_scoped_analysis`, the 8-bit one reads `retainedByteProjection` through
// `dr_doc_projection_scoped_analysis`, and the 32-bit float Info panel
// reads its stop-binned histogram and linear readout through the same scoped path — the composite
// scope over the retained float projection, Active layer, Below active layer and selection
// weighting as at 16 bits.

extension ImageDocument {
    /// `doc`'s 8-bit composite: `retainedByteProjection` when `doc === self.doc`
    /// and it is non-nil, else `try doc.render()` (the core's sentence). Never
    /// throws the stored `projectionError`: that describes an earlier render,
    /// and a render now answers for the document as it is. Main thread.
    func byteComposite(of doc: RasterDocument) throws -> RasterImage {
        if doc === self.doc, let retained = retainedByteProjection { return retained }
        return try doc.render()
    }

    /// The byte ANALYSIS image of `doc` (Vision and the selection sheets):
    /// `byteComposite(of:)` at 8 bits; at 16 bits and float an explicit
    /// display derivative of the current precision projection — a float
    /// document's is its TONE-MAPPED display, never a clamped conversion
    /// (`FloatDocument.swift`). Never a source for an edit. Main thread.
    ///
    /// Why this rendering at float: it is what Vision sees for Select
    /// Subject, Select Objects, All People and Object Selection — the
    /// document's OWN tone map applied to the linear composite at view
    /// exposure 0 (`FloatDisplayView.document`), then the profile's tone
    /// curve and 8-bit quantisation. The window's exposure and a candidate
    /// tone map are view state (`ViewExposure.swift`) and are never read
    /// here, so looking at the highlights cannot change what is selected.
    /// No second tone map is made for analysis: a subject that lies entirely
    /// above white under Clip renders flat white and is found poorly, and
    /// the answer is to set the document's tone map to Reinhard first.
    func analysisComposite(of doc: RasterDocument) throws -> RasterImage {
        guard doc.isHighDepth else { return try byteComposite(of: doc) }
        let projection = try currentPrecisionProjection()
        let context = try PrecisionWorkContext(image: projection)
        return try doc.display(of: projection, context: context)
    }

    /// The Magnetic Lasso's TRACE image of `doc` (`display_encoding`'s
    /// rule M): `analysisComposite(of:)` at 8 and 16 bits; at float the
    /// composite through the Clip map at exposure 0 — R1 at 8 bits, so a
    /// value above white traces as white. Never the document's own tone map
    /// or the window's exposure: the edge the lasso snaps to must not change
    /// with how the document is viewed. Vision callers keep
    /// `analysisComposite`. Main thread.
    func edgeTraceComposite(of doc: RasterDocument) throws -> RasterImage {
        guard doc.precision?.isFloat == true else { return try analysisComposite(of: doc) }
        let projection = try currentPrecisionProjection()
        let context = try PrecisionWorkContext(image: projection)
        return try doc.display(of: projection, context: context,
                               view: FloatDisplayView(toneMap: ToneMap(operator: .clip),
                                                      exposureStops: 0))
    }
}
