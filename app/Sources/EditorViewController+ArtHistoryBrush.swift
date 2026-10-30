import AppKit

// The canvas side of the Art History Brush. It paints the History Brush's source state
// (`HistoryBrushSource.latch`, latched at mouse-down where refusing is still free) as seeded,
// stylized strokes the core generates (`core/src/art_history_stroke.rs`). At 8 bits each tick
// reruns the stroke from the gesture's base: on the final tick over the prepared stroke's dabs
// (`canvas.preparedByteStroke`, what `art_history_brush_stroke` replays), before it over the
// session's dabs so far. The seed is drawn once at mouse-down, so every tick lays the same strokes
// and a dab prefix gives a stroke prefix. A typed (8-bit CMYK or Gray) stroke commits as one job
// (`PrecisionStrokeWork.Kind.artHistory`); every other typed depth is refused at mouse-down by the
// core's own sentence.

extension EditorViewController {
    /// The tool's name in the source latch's sentences.
    static let artHistoryToolName = "Art History Brush"

    /// A fresh stroke seed: one per gesture, recorded with the step.
    static func artHistorySeed() -> UInt32 { UInt32.random(in: 0...UInt32.max) }

    /// The Art History Brush's source for a byte stroke on `layer`, or nil
    /// after beeping: the gesture refuses at mouse-down.
    func latchArtHistorySource(_ document: ImageDocument, _ doc: RasterDocument,
                               layer: Int) -> HistoryBrushSource? {
        do {
            return try HistoryBrushSource.latch(in: document, current: doc, layer: layer,
                                                tool: Self.artHistoryToolName)
        } catch {
            NSSound.beep()
            return nil
        }
    }

    /// The core plan's refusal of a typed document's depth (`precision_art_history.rs`), thrown
    /// before anything else is asked: the gate reads no tip, no dab and no state, so an empty
    /// stroke from the document itself plans it.
    static func refuseArtHistoryDepth(_ source: RasterPrecisionDocument, layer: Int,
                                      request: String) throws {
        let context = try PrecisionWorkContext(source: source)
        _ = try source.artHistoryBrushPlan(layer: layer, source: source, sourceLayer: layer,
            request: request,
            brushRequest: "{\"version\":1,\"hardness\":1,\"roundness\":1,\"angle_degrees\":0,\"pixel\":false}",
            dabs: [], selection: false, workingLimit: context.limit,
            otherRetained: context.otherRetained)
    }

    /// A typed stroke's source and options, latched at mouse-down: throws the
    /// core plan's refusal of the document's depth, or the latch's sentence.
    func latchPrecisionArtHistory(_ document: ImageDocument, _ capture: PrecisionEditorCapture)
        throws -> (HistoryBrushSource, ArtHistoryOptions) {
        let options = ToolOptionsStore.shared.paintOptions(for: .artHistoryBrush)
            ?? PaintToolOptions()
        let art = ArtHistoryOptions(paint: options, seed: Self.artHistorySeed(),
                                    document: document.doc)
        try Self.refuseArtHistoryDepth(capture.source, layer: capture.layer, request: art.request)
        let source = try HistoryBrushSource.latch(in: document, current: capture.facade,
            layer: capture.layer, tool: Self.artHistoryToolName)
        return (source, art)
    }

    /// One tick of a byte Art History Brush stroke (module comment): the
    /// strokes of every dab so far over `base`. nil when the core changed
    /// nothing; throws the core's sentence (the work bound, a bad stroke).
    func artHistoryTick(_ capture: BytePaintStrokeCapture,
                        base: RasterDocument) throws -> RasterDocument? {
        guard let source = capture.historySource else { return nil }
        let stroke: NativeBrushStroke
        if let prepared = canvas.preparedByteStroke {
            stroke = prepared.stroke
        } else {
            guard let session = canvas.nativeStrokeSession else { return nil }
            if let refusal = session.refusal { throw DoctoredCoreError(message: refusal) }
            guard !session.dabs.isEmpty else { return nil }
            stroke = try NativeBrushStroke(tip: session.tip, pixel: session.size <= 1,
                                           dabs: session.dabs)
        }
        let art = ArtHistoryOptions(paint: capture.options, seed: capture.artSeed)
        return try base.artHistoryBrushLayer(capture.layer, source: source.document,
            sourceLayer: source.layer, request: art.request, brushRequest: stroke.request,
            dabs: stroke.dabs, selection: canvas.strokeByteSelection?.maskBytes())
    }
}
