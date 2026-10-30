import AppKit

// The canvas side of the five retouch brushes at 8 bits: Blur, Sharpen, Sponge and the History
// Brush recompute their op from the stroke's base on every tick, as Dodge / Burn does; Smudge is
// incremental while the gesture runs (each tick drags the last published document along the new
// dabs) and is recomputed ONCE from the base on the final tick with the whole dab list, which is
// exactly what `smudge_stroke` computes. That final run is synchronous, so the canvas session
// stops ink at Smudge's own 256-million bound (`NativeBrushStroke.maximumSmudgeWork`) and the
// run stays short. The History Brush latches its source state at mouse-down
// (`HistoryBrushSource.latch`), where refusing is still free. The 16-bit strokes commit as one job
// (`EditorViewController+PrecisionPaint.swift`, `PrecisionStrokeWork.Kind`). The options bar's
// Vibrance (Sponge) and Sample All Layers (Blur, Sharpen, Smudge) are latched with the
// stroke's options at mouse-down (`capture.options`) and read here.

/// The state a History Brush stroke paints from, latched for the gesture: a
/// retained history document and the index, in it, of the layer being painted.
struct HistoryBrushSource {
    let document: RasterDocument
    let layer: Int

    /// The source for painting layer `layer` of `current`: history state
    /// `stateID`, else the panel's source, else the oldest retained state.
    /// Throws the refusal's sentence — an unknown id, an empty history, a
    /// state of another bit depth or canvas size, or one that does not hold
    /// the layer (matched by its id, so a reordered stack still resolves).
    /// `tool` names the brush in those sentences (the Art History Brush,
    /// latches the same source).
    static func latch(in document: ImageDocument, current: RasterDocument, layer: Int,
                      stateID: String? = nil,
                      tool: String = "History Brush") throws -> HistoryBrushSource {
        let history = document.history
        let entry: DocumentHistory.Entry
        if let stateID {
            guard let index = history.entryIndex(id: stateID) else {
                throw DoctoredCoreError(message: "No retained history state has the id \(stateID). Call list_history for the current ids.")
            }
            entry = history.entries[index]
        } else {
            guard let source = history.brushSource() else {
                throw DoctoredCoreError(message: "This document has no history state to paint from.")
            }
            entry = source
        }
        let state = entry.state.document
        guard (state.precision == nil) == (current.precision == nil),
              state.isFloat == current.isFloat else {
            throw DoctoredCoreError(message: "History state \"\(entry.name)\" has a different bit depth; the \(tool) paints from a state at the document's own depth.")
        }
        guard state.width == current.width, state.height == current.height else {
            throw DoctoredCoreError(message: "History state \"\(entry.name)\" has a different canvas size; the \(tool) paints from a state of the same size.")
        }
        guard let id = current.layerInfo(layer)?.id,
              let index = (0..<state.layerCount).first(where: { state.layerInfo($0)?.id == id })
        else {
            throw DoctoredCoreError(message: "The layer does not exist in history state \"\(entry.name)\"; choose a state that holds it.")
        }
        return HistoryBrushSource(document: state, layer: index)
    }
}

extension EditorViewController {
    /// The MCP tool a recorded stroke of `tool` replays as; nil for a tool
    /// outside the four recordable retouch brushes.
    static func retouchBrushToolName(_ tool: EditorTool) -> String? {
        switch tool {
        case .blurBrush: return "blur_stroke"
        case .sharpenBrush: return "sharpen_stroke"
        case .smudge: return "smudge_stroke"
        case .sponge: return "sponge_stroke"
        default: return nil
        }
    }

    /// The core kind of an overlay retouch brush under `options`; nil for
    /// Smudge and the History Brush, which are not overlay-and-target ops.
    static func retouchBrushKind(_ tool: EditorTool, options: PaintToolOptions) -> RetouchBrushKind? {
        switch tool {
        case .blurBrush: return .blur
        case .sharpenBrush: return .sharpen
        case .sponge: return options.saturate == true ? .saturate : .desaturate
        default: return nil
        }
    }

    /// The History Brush's source for a canvas stroke on `layer`, or nil
    /// after beeping: the gesture refuses at mouse-down.
    func latchHistoryBrushSource(_ document: ImageDocument, _ doc: RasterDocument,
                                 layer: Int) -> HistoryBrushSource? {
        do { return try HistoryBrushSource.latch(in: document, current: doc, layer: layer) }
        catch {
            NSSound.beep()
            return nil
        }
    }

    /// Whether a stroke of `tool` under `options` reads the sampled picture:
    /// the options bar's Sample All Layers on Blur, Sharpen or Smudge.
    static func samplesAllLayers(_ tool: EditorTool, options: PaintToolOptions) -> Bool {
        switch tool {
        case .blurBrush, .sharpenBrush, .smudge: return options.sampleAllLayers == true
        default: return false
        }
    }

    /// Whether a stroke of `tool` under `options` takes the Sponge's Vibrance
    /// target (the checkbox is on unless turned off).
    static func spongeVibrance(_ tool: EditorTool, options: PaintToolOptions) -> Bool {
        tool == .sponge && options.vibrance != false
    }

    /// One tick of a byte Blur, Sharpen, Sponge or History Brush stroke: the
    /// op over `base` (the stroke's pre-stroke document) with the accumulated
    /// overlay as coverage. nil when the core changed nothing; throws the
    /// core's sentence (an option the kind refuses, a failed picture render).
    func retouchBrushTick(_ capture: BytePaintStrokeCapture, base: RasterDocument,
                          overlay data: UnsafePointer<UInt8>) throws -> RasterDocument? {
        let strength = min(max(capture.options.opacity, 0), 100) / 100
        if capture.tool == .historyBrush {
            guard let source = capture.historySource else { return nil }
            return base.historyBrushLayer(capture.layer, overlay: data, w: base.width,
                h: base.height, source: source.document, sourceLayer: source.layer,
                opacity: strength)
        }
        guard let kind = Self.retouchBrushKind(capture.tool, options: capture.options) else {
            return nil
        }
        return try base.retouchBrushLayer(capture.layer, overlay: data, w: base.width,
            h: base.height, kind: kind, strength: strength,
            vibrance: Self.spongeVibrance(capture.tool, options: capture.options),
            sampleAllLayers: Self.samplesAllLayers(capture.tool, options: capture.options))
    }

    /// One tick of a byte Smudge stroke.
    ///
    /// While the gesture runs it is INCREMENTAL: the last published document
    /// is dragged along the dabs stamped since the previous tick, starting one
    /// dab back (that dab only anchors the first move). `.unchanged` keeps the
    /// published document — no new dab, or a move over a flat area. On the
    /// final tick (`canvas.preparedByteStroke` is set) the whole resolved dab
    /// list is run once over `base`, so the committed pixels are a function of
    /// the stroke alone and equal `smudge_stroke`'s; `.final(nil)` there means
    /// the stroke changed nothing and the caller restores the base. Under
    /// Sample All Layers each incremental tick samples the published
    /// document's own projection, so the preview carries the running smear,
    /// and the final tick samples the base's.
    enum SmudgeTick {
        case unchanged
        case incremental(RasterDocument)
        case final(RasterDocument?)
    }

    func smudgeTick(_ capture: BytePaintStrokeCapture, base: RasterDocument) throws -> SmudgeTick {
        let strength = min(max(capture.options.opacity, 0), 100) / 100
        let selection = canvas.strokeByteSelection?.maskBytes()
        let sampled = Self.samplesAllLayers(capture.tool, options: capture.options)
        if let prepared = canvas.preparedByteStroke {
            return .final(try base.smudgeLayer(capture.layer, request: prepared.stroke.request,
                dabs: prepared.stroke.dabs, strength: strength, selection: selection,
                sampleAllLayers: sampled))
        }
        guard let session = canvas.nativeStrokeSession, let published = byteLastPublished else {
            return .unchanged
        }
        if let refusal = session.refusal { throw DoctoredCoreError(message: refusal) }
        let total = session.dabs.count / 4
        guard total > capture.smudgedDabs, total >= 2 else { return .unchanged }
        let first = max(capture.smudgedDabs - 1, 0)
        let slice = try NativeBrushStroke(tip: session.tip, pixel: session.size <= 1,
            dabs: Array(session.dabs[(first * 4)...]))
        let updated = try published.smudgeLayer(capture.layer, request: slice.request,
            dabs: slice.dabs, strength: strength, selection: selection, sampleAllLayers: sampled)
        byteStrokeCapture?.smudgedDabs = total
        return updated.map { .incremental($0) } ?? .unchanged
    }
}
