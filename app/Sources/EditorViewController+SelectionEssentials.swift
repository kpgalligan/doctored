import AppKit

extension EditorViewController {
    @objc func selectFreehandLassoTool(_ sender: Any?) { selectTool(.freehandLasso) }
    @objc func selectQuickSelectTool(_ sender: Any?) { selectTool(.quickSelect) }

    @objc func showColorRange(_ sender: Any?) {
        guard let document, !isTransforming else { NSSound.beep(); return }
        do { presentAsSheet(try ColorRangeSheetController(editor: self, document: document)) }
        catch { NSApp.presentError(error) }
    }

    @objc func transformSelection(_ sender: Any?) {
        guard let document, document.canvasSelection != nil, !isTransforming else {
            NSSound.beep(); return
        }
        do { presentAsSheet(try SelectionTransformSheetController(editor: self, document: document)) }
        catch { NSApp.presentError(error) }
    }

    func captureSelectionGesture() -> SelectionCapture? {
        guard let document, !isTransforming else { return nil }
        return try? SelectionCapture(
            document: document,
            needsSource: currentTool == .quickSelect || currentTool == .magneticLasso
                || currentTool == .objectSelect,
            edgeTrace: currentTool == .magneticLasso)
    }

    func finishSelectionGesture(_ stroke: SelectionStrokeSession) {
        guard let document else { return }
        do {
            guard !stroke.overflowed else {
                throw DoctoredCoreError(message: "This selection stroke is too long. Use shorter strokes with Add mode.")
            }
            var arguments: [String: Any] = ["mode": stroke.mode.agentName, "feather": stroke.feather]
            if stroke.tool == .freehandLasso {
                guard stroke.points.count >= 3 else { return }
                arguments["points"] = ActionArgs.points(stroke.points)
                // The options bar's Anti-alias, recorded only when off.
                let antiAlias = ToolOptionsStore.shared.select.antiAlias
                if !antiAlias { arguments["anti_alias"] = false }
                try stroke.capture.commit(.polygon(stroke.points), in: document,
                    name: "Freehand Lasso", mode: stroke.mode, feather: stroke.feather,
                    record: .step("select_freehand", arguments, note: "Freehand Lasso tool"),
                    antiAlias: antiAlias)
            } else if stroke.tool == .quickSelect {
                let capture = stroke.capture
                // The step carries the kept seeds and the brush as
                // `quick_select` takes them (`brush_size`, not the core's radius).
                arguments.merge(stroke.quickArguments) { _, new in new }
                let shape = try capture.quickSelectionShape(stroke.quickParameters,
                    enhanceEdge: stroke.enhanceEdge, in: document)
                try capture.commit(shape, in: document, name: "Quick Selection",
                    mode: stroke.mode, feather: stroke.feather,
                    record: .step("quick_select", arguments, note: "Quick Selection tool"))
            }
        } catch { NSApp.presentError(error) }
        syncSelectionHistory()
    }

    func applyColorRange(_ capture: SelectionCapture, parameters: ColorRangeParameters,
                         mode: SelectionCombineMode) {
        guard let document else { return }
        do {
            let parameters = try parameters.validated()
            let gamutSetup = parameters.isOutOfGamut
                ? try ColorRangeGamut.setup(window: proofDisplay.setup) : nil
            let shape = try capture.colorRangeShape(parameters, gamutSetup: gamutSetup,
                                                    in: document)
            var arguments = parameters.arguments
            arguments["mode"] = mode.agentName
            try capture.commit(shape, in: document, name: "Color Range", mode: mode,
                record: .step("select_color_range", arguments, note: "Select ▸ Color Range"))
        } catch { NSApp.presentError(error) }
    }

    func applySelectionTransform(_ capture: SelectionCapture, parameters: SelectionTransformParameters) {
        guard let document, capture.selection.selection != nil else { return }
        do {
            let parameters = try parameters.validated()
            let shape = try capture.transformedShape(parameters, in: document)
            try capture.commit(shape, in: document, name: "Transform Selection",
                record: .step("transform_selection", parameters.arguments,
                              note: "Select ▸ Transform Selection"))
        } catch { NSApp.presentError(error) }
    }
}

extension EditorViewController {
    func quickSelectionClusters() -> [OptionCluster] {
        [OptionCluster([
            OptionDescriptor(id: "quick.size", microLabel: "Size", overflowLabel: "Brush size",
                kind: .field(width: 52, unit: " px", decimals: 0, min: 1, max: 512,
                    quick: [1, 5, 10, 20, 30, 50, 100, 200, 400],
                    get: { QuickSelectionSettings.shared.size },
                    set: { QuickSelectionSettings.shared.size = $0 })),
            OptionDescriptor(id: "quick.enhance", overflowLabel: "Enhance Edge",
                kind: .checkbox(
                    label: "Enhance Edge",
                    get: { QuickSelectionSettings.shared.enhanceEdge },
                    set: { QuickSelectionSettings.shared.enhanceEdge = $0 })),
        ]), OptionCluster([
            OptionDescriptor(id: "quick.tolerance", microLabel: "Tolerance", overflowLabel: "Color tolerance (Lab)",
                kind: .field(width: 48, unit: "", decimals: 0, min: 0, max: 200,
                    quick: [0, 5, 10, 20, 25, 40, 80, 200],
                    get: { QuickSelectionSettings.shared.tolerance },
                    set: { QuickSelectionSettings.shared.tolerance = $0 })),
            OptionDescriptor(id: "quick.edge", microLabel: "Edge", overflowLabel: "Allowed edge step (Lab)",
                kind: .field(width: 48, unit: "", decimals: 0, min: 0, max: 200,
                    quick: [0, 3, 6, 12, 24, 48, 100, 200],
                    get: { QuickSelectionSettings.shared.edge },
                    set: { QuickSelectionSettings.shared.edge = $0 })),
        ])]
    }

    /// The 8-bit wand's region at `point` under the bar's options: the
    /// composite with Sample — All layers, the active layer alone with
    /// Current layer. nil for a refused seed.
    func wandMask(_ doc: RasterDocument, at point: CGPoint, options: SelectToolOptions) -> [UInt8]? {
        let (x, y) = (Int(point.x), Int(point.y))
        let tolerance = Int(options.tolerance.rounded())
        if options.wandSampleAllLayers {
            return doc.magicWand(x: x, y: y, tolerance: tolerance, contiguous: options.contiguous,
                                 sampleSize: options.wandSampleSize)
        }
        guard let layer = document?.activeLayerIndex else { return nil }
        return doc.magicWandLayer(layer, x: x, y: y, tolerance: tolerance, contiguous: options.contiguous,
                                  sampleSize: options.wandSampleSize)
    }
}
