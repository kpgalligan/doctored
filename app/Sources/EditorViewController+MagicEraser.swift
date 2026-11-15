import AppKit

/// The Magic Eraser tool (`designs/magic-eraser.md`): its rail action, its click and its options
/// bar (Tolerance, Contiguous, Sample All Layers, Opacity). A click erases the Magic Wand's region
/// from the clicked pixel on the active layer as one undo step and records `magic_eraser`; the
/// compositions are `MagicEraser.swift`'s, and `magic_eraser` (`AgentServer+MagicEraser.swift`)
/// mirrors the click.
extension EditorViewController {
    @objc func selectMagicEraserTool(_ sender: Any?) { selectTool(.magicEraser) }

    /// The click's gates, in the order `fillClicked` asks them: a layer target (never a mask,
    /// plane, channel or the Quick Mask), the operation scope (a group, an adjustment layer, a
    /// retained source, a pixels lock), the transparency lock, then the rasterize prompt of a
    /// described layer.
    func magicEraserClicked(_ point: CGPoint) {
        guard let document, let doc = document.doc, !doc.isBitmap else { NSSound.beep(); return }
        if document.quickMaskActive || paintTarget != .layer {
            refuseMagicEraser(document.quickMaskActive
                ? OperationScope.quickMaskRefusal : MagicEraser.targetRefusal)
            return
        }
        guard !refuseOperationScope(.pixelOnly) else { return }
        let idx = document.activeLayerIndex
        if let refusal = MagicEraser.transparencyRefusal(doc, layer: idx) {
            refuseMagicEraser(refusal.message)
            return
        }
        let options = ToolOptionsStore.shared.magicEraser
        let request = MagicEraserRequest(
            x: Int(point.x.rounded(.down)), y: Int(point.y.rounded(.down)),
            tolerance: min(max(options.tolerance, 0), 255), contiguous: options.contiguous,
            sampleAllLayers: options.sampleAllLayers,
            opacity: min(max(options.opacity, 1), 100) / 100)
        if doc.isHighDepth {
            magicErasePrecision(document, request: request)
            return
        }
        guard let capture = document.captureOperation() else { NSSound.beep(); return }
        document.applyCapturedRewrite(capture, MagicEraser.name, record: request.record) {
            capture.doc.magicErased(capture.layerIndex, request: request, mask: capture.selection)
        }
    }

    /// The typed click: the region and `erase_native` over the captured revision, one publish.
    /// An Indexed document's erased pixels take its transparent index, else the entry nearest
    /// the background swatch, as the Eraser's do.
    private func magicErasePrecision(_ document: ImageDocument, request: MagicEraserRequest) {
        do {
            let capture = try PrecisionEditorCapture(document)
            guard document.confirmRasterize(layer: capture.layer) else { return }
            let result = try capture.source.magicErased(
                layer: capture.layer, request: request, selection: capture.selection,
                width: capture.facade.width, height: capture.facade.height,
                extra: IndexedDocument.backgroundFields(backgroundColor, in: capture.facade))
            if try !capture.publish(result, to: document, name: MagicEraser.name,
                                    record: request.record) {
                NSSound.beep()
            }
        } catch { document.presentError(error) }
    }

    private func refuseMagicEraser(_ reason: String) {
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = reason
        if let window = view.window { alert.beginSheetModal(for: window) } else { alert.runModal() }
    }

    /// The options bar: the Magic Wand's Tolerance and Contiguous, Sample All Layers, and the
    /// erase's Opacity.
    func magicEraserClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "magicEraser.tolerance", microLabel: "Tolerance", overflowLabel: "Tolerance",
                    kind: .field(
                        width: 46, unit: "", decimals: 0, min: 0, max: 255,
                        quick: Self.quickTolerance,
                        get: { ToolOptionsStore.shared.magicEraser.tolerance },
                        set: { ToolOptionsStore.shared.magicEraser.tolerance = $0 })),
                OptionDescriptor(
                    id: "magicEraser.contiguous", overflowLabel: "Contiguous",
                    kind: .checkbox(
                        label: "Contiguous",
                        get: { ToolOptionsStore.shared.magicEraser.contiguous },
                        set: { ToolOptionsStore.shared.magicEraser.contiguous = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "magicEraser.samplealllayers", overflowLabel: "Sample all layers",
                    kind: .checkbox(
                        label: "Sample all layers",
                        get: { ToolOptionsStore.shared.magicEraser.sampleAllLayers },
                        set: { ToolOptionsStore.shared.magicEraser.sampleAllLayers = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "magicEraser.opacity", microLabel: "Opacity", overflowLabel: "Opacity",
                    kind: .field(
                        width: 52, unit: "%", decimals: 0, min: 1, max: 100,
                        quick: Self.quickPercent,
                        get: { ToolOptionsStore.shared.magicEraser.opacity },
                        set: { ToolOptionsStore.shared.magicEraser.opacity = $0 })),
            ]),
        ]
    }
}
