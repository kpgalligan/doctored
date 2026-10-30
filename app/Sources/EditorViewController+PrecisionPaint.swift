import AppKit

struct PrecisionCanvasStroke {
    let capture: PrecisionEditorCapture
    let tool: EditorTool
    let ink: PrecisionInk
    let size: CGFloat
    let opacity: Double
    let retouchOptions: PaintToolOptions?
    let sourcePlate: RasterPrecisionImage?
    var blendMode: DrBlendMode = DR_BLEND_NORMAL
    var dodgeRange: Int = 1
    var burn: Bool = false
    /// Dodge/Burn's Protect Tones (#547), latched at mouse-down.
    var protectTones: Bool = false
    /// Sponge's mode and the History Brush's latched source.
    var saturate: Bool = false
    /// The Sponge's Vibrance and Blur, Sharpen and Smudge's Sample
    /// All Layers, latched at mouse-down.
    var vibrance = false
    var sampleAllLayers = false
    var historySource: HistoryBrushSource?
    /// A float layer Brush's above-white foreground: linear light in
    /// the document's primaries, sent as `linear` ink and recorded as
    /// `rgba32`. nil takes `ink`, the document route.
    var linear: [Double]? = nil
    /// A CMYK document Brush's ink (#547 phase 2): straight `[c, m, y, k, alpha]` 0…1, the
    /// foreground's stored ink or its conversion (`CmykPaint.ink`), sent as `cmyk_paint`'s `ink`.
    var cmyk: [Double]? = nil
    /// The Art History Brush's options and seed, latched at mouse-down;
    /// its source is `historySource`.
    var art: ArtHistoryOptions? = nil
}

extension EditorViewController {
    func beginPrecisionStroke() -> Bool {
        guard let document else { return false }
        // A 16-bit stroke commits through the one worker, so it is refused
        // while any job holds it (after settling a finished one).
        guard !DocumentJobs.refusesGesture(on: document, workerBound: true) else { return false }
        do {
            let capture = try PrecisionEditorCapture(document)
            // One colour channel of a float layer is not a stroke
            // target; its mask and the alpha channels are, as at 16 bits.
            if capture.facade.isFloat, capture.target.isPlane {
                throw DoctoredCoreError(message: FloatPaint.planeRefusal)
            }
            if capture.target.isCoverage { return try beginPrecisionScalarStroke(capture) }
            guard capture.target == .layer, !capture.quickMask,
                  capture.facade.layerInfo(capture.layer)?.visible == true,
                  !capture.facade.layerIsGroup(capture.layer), !capture.facade.layerIsAdjustment(capture.layer),
                  [.brush, .eraser, .clone, .heal, .spotHeal, .dodge, .blurBrush, .sharpenBrush,
                   .smudge, .sponge, .historyBrush, .artHistoryBrush].contains(currentTool) else {
                throw DoctoredCoreError(message: "This stroke requires visible raster-layer RGB. Choose Brush or Eraser to edit a mask or scalar plane.")
            }
            // The Brush blends through `paint_blend` and the Clone Stamp
            // through `retouch_clone`'s `blend_mode`; no other stroke
            // tool has a mode to composite through.
            guard [.brush, .clone].contains(currentTool) || paintStrokeBlendMode() == DR_BLEND_NORMAL else {
                throw DoctoredCoreError(message: capture.facade.isFloat
                    ? FloatRetouch.normalOnly : "16-bit retouch supports Normal blending only.")
            }
            // A float layer Brush blends in the float modes only, refused
            // before the stroke starts; a foreground above white is linear ink.
            var linear: [Double]?
            if capture.facade.isFloat, currentTool == .brush {
                if let reason = FloatPaint.modeRefusal(paintStrokeBlendMode()) {
                    throw DoctoredCoreError(message: reason)
                }
                linear = ExtendedRangeColor.aboveWhite(canvas.paintColor, in: capture.facade)
            }
            // #547: a CMYK layer takes the Brush and the Eraser in ink in the CMYK modes, and
            // the retouch tools `CmykRetouch` enables at the document's depth.
            var cmyk: [Double]?
            if capture.source.isCmyk {
                let depth = CmykAdjustFilter.depth(of: capture.facade)
                guard [.brush, .eraser].contains(currentTool)
                        || CmykRetouch.allows(currentTool, depth: depth) else {
                    throw DoctoredCoreError(message: CmykRetouch.tools[currentTool] != nil
                        ? CmykRetouch.refusal(currentTool) : CmykDocument.refusal(tool: currentTool.displayName))
                }
                if let reason = CmykPaint.modeRefusal(paintStrokeBlendMode()) {
                    throw DoctoredCoreError(message: reason)
                }
                if currentTool == .brush {
                    cmyk = try CmykPaint.ink(canvas.paintColor,
                        stored: ToolOptionsStore.shared.sharedState.foregroundInk, in: capture.source)
                }
            }
            let options = Self.retouchOptions(for: currentTool)
            // A clone onto a NEW layer has nothing to blend with, so it is
            // sent, and recorded, as Normal (the `clone_stamp` tool refuses
            // that pair by name).
            let blend = options?.outputToNewLayer == true ? DR_BLEND_NORMAL : paintStrokeBlendMode()
            // A float layer Clone Stamp composites in the float modes
            // only, as the Brush does; the mode asked is the one it is sent.
            if capture.facade.isFloat, currentTool == .clone,
               let reason = FloatPaint.modeRefusal(blend) {
                throw DoctoredCoreError(message: reason)
            }
            guard options?.outputToNewLayer == true || document.confirmRasterize(layer: capture.layer) else { return false }
            try capture.requireCurrent(document)
            // The History Brush's source state, latched while refusing is
            // still free; a refusal beeps, as at 8 bits.
            var historySource: HistoryBrushSource?
            if currentTool == .historyBrush {
                guard let source = latchHistoryBrushSource(document, capture.facade,
                                                           layer: capture.layer) else { return false }
                historySource = source
            }
            // The Art History Brush latches the same source, then asks
            // the core's plan, which refuses by name every depth the tool
            // is disabled at, before the stroke starts.
            var art: ArtHistoryOptions?
            if currentTool == .artHistoryBrush {
                let latched = try latchPrecisionArtHistory(document, capture)
                historySource = latched.0
                art = latched.1
            }
            let plate = try options.flatMap { options in
                currentTool == .spotHeal ? nil : try capture.source.retouchSource(layer: capture.layer,
                    sample: options.sample, ignoreAdjustments: options.ignoreAdjustments)
            }
            // #547 Lab phase 2e: a Lab layer's ink is the foreground's Lab ink when it has one;
            // a coverage target keeps the sRGB swatch, as the Gradient and Fill do.
            let inkColor = LabPaint.color(canvas.paintColor,
                stored: ToolOptionsStore.shared.sharedState.foregroundLab,
                in: capture.target.isCoverage ? nil : document.doc)
            precisionStroke = PrecisionCanvasStroke(capture: capture, tool: currentTool,
                ink: try PrecisionInk(color: inkColor, space: document.nsColorSpace), size: canvas.brushSize,
                opacity: (currentTool == .heal || currentTool == .spotHeal) ? (options?.opacity ?? 100) / 100 : Double(canvas.brushOpacity),
                retouchOptions: options, sourcePlate: plate, blendMode: blend,
                dodgeRange: ToolOptionsStore.shared.dodge.rangeIndex, burn: ToolOptionsStore.shared.dodge.burn,
                protectTones: ToolOptionsStore.shared.dodge.protectTones == true,
                saturate: ToolOptionsStore.shared.sponge.saturate == true, historySource: historySource,
                linear: linear, cmyk: cmyk, art: art)
            if let toolOptions = ToolOptionsStore.shared.paintOptions(for: currentTool) {
                precisionStroke?.vibrance = Self.spongeVibrance(currentTool, options: toolOptions)
                precisionStroke?.sampleAllLayers = Self.samplesAllLayers(currentTool,
                                                                         options: toolOptions)
            }
            strokeTool = currentTool
            strokeTarget = .layer
            strokeBlendMode = blend
            canvas.precisionStrokePreview = true
            return true
        } catch {
            document.presentError(error)
            return false
        }
    }

    func finishPrecisionStroke(_ name: String) {
        let pin = canvas.strokeGhostPin
        guard let stroke = precisionStroke, let document else { clearStrokeGhostPin(pin); return }
        defer { precisionStroke = nil; canvas.precisionStrokePreview = false; canvas.nativeStrokeSession = nil; strokeTarget = .layer }
        do {
            if stroke.capture.target.isCoverage { try finishPrecisionScalarStroke(stroke, name: name); return }
            try stroke.capture.requireCurrent(document)
            let source = stroke.capture.facade
            guard let session = canvas.nativeStrokeSession else { throw DoctoredCoreError(message: "Native stroke capture is missing.") }
            let native = try session.resolved()
            let selected = stroke.capture.selection != nil
            let command: PrecisionPixelCommand
            var kind = PrecisionStrokeWork.Kind.edit
            switch stroke.tool {
            case .brush where stroke.cmyk != nil:
                // #547: ink on a CMYK layer (`cmyk_paint`).
                command = try CmykPaint.command(["op": "paint_blend", "layer": stroke.capture.layer,
                    "ink": stroke.cmyk ?? [], "opacity": stroke.opacity,
                    "blend_mode": Int(stroke.blendMode.rawValue), "selection": selected])
            case .brush:
                command = try PrecisionPixelCommand(["op": "paint_blend", "layer": stroke.capture.layer,
                    "color": stroke.linear ?? stroke.ink.rgba,
                    "color_space": stroke.linear == nil ? "document" : "linear", "opacity": stroke.opacity,
                    "blend_mode": stroke.blendMode.rawValue, "selection": selected])
            case .eraser:
                // #547 Indexed phase 3b: an erased pixel with no transparent index takes the
                // entry nearest the background swatch.
                command = try PrecisionPixelCommand(["op": "erase_native", "layer": stroke.capture.layer,
                    "opacity": stroke.opacity, "selection": selected]
                    .merging(IndexedDocument.backgroundFields(backgroundColor, in: source)) { $1 })
            case .dodge:
                command = try PrecisionPixelCommand(["op": "dodge_burn", "layer": stroke.capture.layer,
                    "exposure": stroke.opacity, "range": stroke.dodgeRange, "burn": stroke.burn,
                    "protect_tones": stroke.protectTones, "selection": selected])
            case .clone, .heal, .spotHeal:
                guard let options = stroke.retouchOptions else { throw DoctoredCoreError(message: "Missing captured retouch options.") }
                let choice = RetouchSamplingChoice(mode: options.sample, ignoreAdjustments: options.ignoreAdjustments)
                let output: RetouchOutput = options.outputToNewLayer ? .newLayer : .layer
                if stroke.tool == .spotHeal {
                    let bounds = native.healBounds(width: source.width, height: source.height)
                    command = try .spotHeal(layer: stroke.capture.layer, choice: choice, output: output,
                        expand: options.expandDestination == true, opacity: stroke.opacity,
                        type: options.spotHealType ?? .contentAware, selection: selected, bounds: bounds)
                } else {
                    guard let geometry = canvas.strokeCloneGeometry else {
                        throw DoctoredCoreError(message: "Choose a clone/heal source before painting.")
                    }
                    command = try .retouch(layer: stroke.capture.layer, heal: stroke.tool == .heal,
                        geometry: geometry, choice: choice, output: output,
                        expand: options.expandDestination == true, opacity: stroke.opacity,
                        blendMode: stroke.blendMode, selection: selected,
                        bounds: stroke.tool == .heal ? native.healBounds(width: source.width, height: source.height) : nil)
                }
            case .blurBrush, .sharpenBrush, .sponge, .smudge, .historyBrush:
                // `retouch_brush` is the planned request of Blur,
                // Sharpen and Sponge; for Smudge and the History Brush it
                // only names the stroke, and `kind` carries the work.
                var options = PaintToolOptions()
                options.saturate = stroke.saturate
                let brush = Self.retouchBrushKind(stroke.tool, options: options) ?? .blur
                var request: [String: Any] = ["op": "retouch_brush", "layer": stroke.capture.layer,
                    "kind": brush.requestName, "strength": stroke.opacity, "selection": selected]
                if stroke.vibrance { request["vibrance"] = true }
                if stroke.sampleAllLayers, stroke.tool != .smudge {
                    request["sample_all_layers"] = true
                }
                command = try PrecisionPixelCommand(request)
                if stroke.tool == .smudge {
                    kind = .smudge(strength: stroke.opacity, sampleAllLayers: stroke.sampleAllLayers)
                } else if stroke.tool == .historyBrush {
                    guard let history = stroke.historySource, let state = history.document.precision else {
                        throw DoctoredCoreError(message: "The History Brush's source state is missing.")
                    }
                    kind = .historyBrush(source: state, sourceLayer: history.layer, opacity: stroke.opacity)
                }
            case .artHistoryBrush:
                // The request only names the stroke; `kind` carries the work.
                command = try PrecisionPixelCommand(["op": "retouch_brush",
                    "layer": stroke.capture.layer, "kind": "blur", "strength": stroke.opacity,
                    "selection": selected])
                guard let history = stroke.historySource, let state = history.document.precision,
                      let art = stroke.art else {
                    throw DoctoredCoreError(message: "The Art History Brush's source state is missing.")
                }
                kind = .artHistory(source: state, sourceLayer: history.layer, request: art.request)
            default: throw DoctoredCoreError(message: "This tool is unavailable in 16-bit mode.")
            }
            let work = try PrecisionStrokeWork(kind: kind, layer: stroke.capture.layer,
                source: stroke.capture.source, stroke: native, command: command,
                selection: stroke.capture.selection?.maskSamples16(), width: source.width, height: source.height)
            var record = ActionRecorder.shared.gestureRecord(strokeRecord(name))
            let canonical = try native.canonical
            // A History Brush stroke is `.unrecorded("History Brush")`: it is
            // passed on as it is, with no arguments written into it.
            for index in record.indices where stroke.tool != .historyBrush {
                record[index].arguments["native_stroke"] = canonical
                record[index].arguments["size"] = Double(stroke.size)
                if stroke.tool == .dodge {
                    record[index].arguments["exposure"] = stroke.opacity * 100
                    record[index].arguments["range"] = ["shadows", "midtones", "highlights"][stroke.dodgeRange]
                    record[index].arguments["burn"] = stroke.burn
                    if stroke.protectTones { record[index].arguments["protect_tones"] = true }
                } else if Self.retouchBrushToolName(stroke.tool) != nil {
                    // The four stroke tools take `strength` in percent, and
                    // Sponge the mode this gesture latched at mouse-down.
                    record[index].arguments["strength"] = stroke.opacity * 100
                    if stroke.tool == .sponge {
                        record[index].arguments["mode"] = stroke.saturate ? "saturate" : "desaturate"
                    }
                    // Written only when on, so a step without the key replays as before.
                    record[index].arguments["vibrance"] = stroke.vibrance ? true : nil
                    record[index].arguments["sample_all_layers"] = stroke.sampleAllLayers ? true : nil
                } else { record[index].arguments["opacity"] = [.heal, .spotHeal, .artHistoryBrush].contains(stroke.tool) ? stroke.opacity * 100 : stroke.opacity }
            }
            if stroke.tool == .brush {
                for index in record.indices {
                    if let cmyk = stroke.cmyk {
                        // `brush_stroke`'s `ink`: C, M, Y, K in percent (#547).
                        record[index].arguments["ink"] = cmyk.prefix(4).map { $0 * 100 }
                        for key in ["color", "rgba", "color_space"] {
                            record[index].arguments.removeValue(forKey: key)
                        }
                        continue
                    }
                    if let linear = stroke.linear {
                        // `brush_stroke`'s `rgba32` replaces every other
                        // colour key (`linearInkArgument`).
                        record[index].arguments["rgba32"] = linear
                        for key in ["color", "rgba", "color_space"] {
                            record[index].arguments.removeValue(forKey: key)
                        }
                        continue
                    }
                    record[index].arguments["rgba"] = stroke.ink.rgba
                    record[index].arguments["color_space"] = "document"
                }
            }
            let rasterizes = [.brush, .eraser, .dodge].contains(stroke.tool) || stroke.tool.strokeIsCoverage
                || stroke.retouchOptions?.outputToNewLayer != true
            try startPrecisionStrokeJob(work, stroke: stroke, name: name, record: record, rasterizesTarget: rasterizes)
        } catch {
            clearStrokeGhostPin(pin)
            document.presentError(error)
        }
    }

    /// Starts a typed stroke's job with the progress panel (so
    /// Esc/⌘Z cancel it) and the canvas's ghost pin up until it ends.
    func startPrecisionStrokeJob(_ work: PrecisionStrokeWork, stroke: PrecisionCanvasStroke, name: String,
                                 record: [ActionStep], rasterizesTarget: Bool) throws {
        guard let document else { throw DoctoredCoreError(message: "The stroke's document is no longer open.") }
        let pin = canvas.strokeGhostPin
        let tool = stroke.tool
        let job = try DocumentJobs.startPrecisionStroke(work, document: document, capture: stroke.capture,
            layer: stroke.capture.layer, title: name, record: record, showProgress: true,
            rasterizesTarget: rasterizesTarget, completion: { [weak self, weak document] job in
                self?.clearStrokeGhostPin(pin)
                if job.state == "failed" || job.state == "stale" {
                    document?.presentError(DoctoredCoreError(message: job.message ?? "The stroke could not be applied."))
                } else if job.state == "unchanged", tool == .heal {
                    NSSound.beep()
                }
            })
        // Edit ▸ Fade weighs a layer stroke by the samples it changed.
        if stroke.capture.target == .layer {
            job.fade = FadeHint(kind: .stroke, layer: stroke.capture.layer)
        }
    }

    /// Takes down the ghost pin `pin` names, and only that one: a later
    /// stroke's pin stays.
    func clearStrokeGhostPin(_ pin: StrokeGhostPin?) {
        guard let pin, canvas.strokeGhostPin?.overlay === pin.overlay else { return }
        canvas.strokeGhostPin = nil
    }

    func cancelPrecisionStroke() {
        precisionStroke = nil
        canvas.precisionStrokePreview = false
    }

    func applyPrecisionParameterless(_ op: AdjustmentLayerOp, create: Bool = false) -> Bool {
        guard let document, document.doc?.precision != nil else { return false }
        let mode: AdjustmentSheetMode = create ? .create(selection: canvas.selection?.maskBytes()) : .destructive
        let session = PrecisionAdjustmentSession(document, canvas: canvas, mode: mode)
        session?.commit(op: op, params: [:], onCommitted: create ? { [weak self] in self?.didCommitAdjustmentLayer($0) } : nil)
        return true
    }
}
