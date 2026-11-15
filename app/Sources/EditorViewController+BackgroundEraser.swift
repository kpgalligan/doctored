import AppKit

/// The Background Eraser tool (`designs/background-eraser.md`): its rail action, its options
/// bar (the tip, Sampling, Limits, Tolerance, Protect Foreground), the mouse-down latch and its
/// refusals, the 8-bit tick, and the typed stroke's `erase_native`. An 8-bit stroke reruns from
/// the gesture's base on every tick, over the session's dabs so far and over the prepared
/// stroke's dabs on the final tick (as the Art History Brush does), so it erases live; a typed
/// stroke commits as one job (`PrecisionStrokeWork.Kind.backgroundEraser`). The compositions are
/// `BackgroundEraser.swift`'s; `background_eraser_stroke` (`AgentServer+BackgroundEraser.swift`)
/// mirrors the stroke.
extension EditorViewController {
    @objc func selectBackgroundEraserTool(_ sender: Any?) { selectTool(.backgroundEraser) }

    /// The options bar's values and the two swatches as one stroke's options on `doc`. A
    /// swatch's stored Lab or CMYK ink is app-wide and outlives a switch to a document of
    /// another mode, so it is taken only on a document of its own mode (as `cmykPathInk`
    /// does): a step recorded on an RGB document carries no `_ink`, which a replay there
    /// would refuse.
    func currentBackgroundEraserOptions(in doc: RasterDocument) -> BackgroundEraserOptions {
        let stored = ToolOptionsStore.shared.backgroundEraserOptions
        let shared = ToolOptionsStore.shared.sharedState
        let samplings = BackgroundEraserOptions.Sampling.allCases
        let sampling = samplings[min(max(stored.samplingIndex, 0), samplings.count - 1)]
        let (lab, cmyk) = (doc.isLab, doc.isCmyk)
        return BackgroundEraserOptions(
            tolerance: min(max(stored.tolerance, 0), 255),
            limits: stored.limitsIndex == 1 ? .discontiguous : .contiguous,
            sampling: sampling,
            swatch: sampling == .swatch
                ? BackgroundEraserColor(color: backgroundColor,
                                        lab: lab ? shared.backgroundLab : nil,
                                        ink: cmyk ? shared.backgroundInk : nil) : nil,
            protect: stored.protectForeground
                ? BackgroundEraserColor(color: paintColor, lab: lab ? shared.foregroundLab : nil,
                                        ink: cmyk ? shared.foregroundInk : nil) : nil)
    }

    /// An 8-bit stroke's mouse-down: the target (never a mask, plane, channel or the Quick
    /// Mask) and the transparency lock refuse by name; then the options and their core request
    /// are latched. nil after refusing.
    func latchBackgroundEraser(_ document: ImageDocument, _ doc: RasterDocument)
        -> (options: BackgroundEraserOptions, request: String)? {
        if let reason = backgroundEraserRefusal(
            quickMask: document.quickMaskActive, onLayer: paintTarget == .layer, doc: doc,
            layer: document.activeLayerIndex) {
            refuseBackgroundEraser(reason)
            return nil
        }
        let options = currentBackgroundEraserOptions(in: doc)
        do { return (options, try options.request(in: doc)) } catch {
            refuseBackgroundEraser(error.localizedDescription)
            return nil
        }
    }

    /// A typed stroke's mouse-down: the same refusals, thrown for `beginPrecisionStroke` to
    /// present, then the latched options and request.
    func latchPrecisionBackgroundEraser(_ capture: PrecisionEditorCapture) throws
        -> (options: BackgroundEraserOptions, request: String) {
        if let reason = backgroundEraserRefusal(
            quickMask: capture.quickMask, onLayer: capture.target == .layer, doc: capture.facade,
            layer: capture.layer) {
            throw DoctoredCoreError(message: reason)
        }
        let options = currentBackgroundEraserOptions(in: capture.facade)
        return (options, try options.request(in: capture.facade))
    }

    /// The target and transparency-lock refusal, nil when the stroke may start.
    private func backgroundEraserRefusal(quickMask: Bool, onLayer: Bool, doc: RasterDocument,
                                         layer: Int) -> String? {
        if quickMask { return OperationScope.quickMaskRefusal }
        if !onLayer { return BackgroundEraser.targetRefusal }
        return BackgroundEraser.transparencyRefusal(doc, layer: layer)?.message
    }

    private func refuseBackgroundEraser(_ reason: String) {
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = reason
        if let window = view.window { alert.beginSheetModal(for: window) } else { alert.runModal() }
    }

    /// One tick of an 8-bit stroke (type comment): every dab so far, erased over `base`. nil
    /// when the core changed nothing; throws the core's sentence.
    func backgroundEraserTick(_ capture: BytePaintStrokeCapture,
                              base: RasterDocument) throws -> RasterDocument? {
        guard let request = capture.backgroundEraserRequest else { return nil }
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
        return try base.backgroundEraserLayer(capture.layer, request: request,
            brushRequest: stroke.request, dabs: stroke.dabs,
            selection: canvas.strokeByteSelection?.maskBytes())
    }

    /// A typed stroke's erase: `erase_native` at full opacity over the coverage plane, the
    /// selection as its second gate; an Indexed document's erased pixels take its transparent
    /// index, else the entry nearest the background swatch, as the Eraser's do.
    func backgroundEraserCommand(layer: Int, selected: Bool,
                                 source: RasterDocument) throws -> PrecisionPixelCommand {
        try PrecisionPixelCommand(
            (["op": "erase_native", "layer": layer, "opacity": 1, "selection": selected]
                as [String: Any])
                .merging(IndexedDocument.backgroundFields(backgroundColor, in: source)) { $1 })
    }

    /// The tip clusters the tool reads: Size, Hardness, Spacing, Angle, Roundness, Smoothing and
    /// Pressure size (its opacity, flow, blend and airbrush are pinned,
    /// `ToolOptionsStore.backgroundEraserTip`).
    private static let backgroundEraserTipIDs: Set<String> = [
        "paint.size", "paint.hardness", "paint.spacing", "paint.angle", "paint.roundness",
        "paint.smoothing", "paint.pressure",
    ]

    /// The options bar: the tip, then Sampling, Limits, Tolerance and Protect Foreground.
    func backgroundEraserClusters() -> [OptionCluster] {
        let tip = paintClusters(.backgroundEraser).compactMap { cluster -> OptionCluster? in
            let kept = cluster.descriptors.filter { Self.backgroundEraserTipIDs.contains($0.id) }
            return kept.isEmpty ? nil : OptionCluster(kept)
        }
        return tip + [
            OptionCluster([
                OptionDescriptor(
                    id: "backgroundEraser.sampling", microLabel: "Sampling",
                    overflowLabel: "Sampling",
                    kind: .popup(
                        width: 150, items: ["Continuous", "Once", "Background Swatch"],
                        get: { ToolOptionsStore.shared.backgroundEraserOptions.samplingIndex },
                        set: { ToolOptionsStore.shared.backgroundEraserOptions.samplingIndex = $0 })),
                OptionDescriptor(
                    id: "backgroundEraser.limits", microLabel: "Limits", overflowLabel: "Limits",
                    kind: .popup(
                        width: 120, items: ["Contiguous", "Discontiguous"],
                        get: { ToolOptionsStore.shared.backgroundEraserOptions.limitsIndex },
                        set: { ToolOptionsStore.shared.backgroundEraserOptions.limitsIndex = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "backgroundEraser.tolerance", microLabel: "Tolerance",
                    overflowLabel: "Tolerance",
                    kind: .field(
                        width: 46, unit: "", decimals: 0, min: 0, max: 255,
                        quick: Self.quickTolerance,
                        get: { ToolOptionsStore.shared.backgroundEraserOptions.tolerance },
                        set: { ToolOptionsStore.shared.backgroundEraserOptions.tolerance = $0 })),
                OptionDescriptor(
                    id: "backgroundEraser.protect", overflowLabel: "Protect Foreground",
                    kind: .checkbox(
                        label: "Protect Foreground",
                        get: { ToolOptionsStore.shared.backgroundEraserOptions.protectForeground },
                        set: {
                            ToolOptionsStore.shared.backgroundEraserOptions.protectForeground = $0
                        })),
            ]),
        ]
    }
}
