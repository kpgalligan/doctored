import AppKit

/// The Color Sampler tool: the canvas closures, the tool action, View ▸
/// Show / Clear Color Samplers, the options bar, and the "after" readings a
/// tone sheet's preview shows on the markers and in the Info panel. The list
/// is `ImageDocument.colorSamplers` (ColorSamplers.swift); the canvas half is
/// `ImageCanvasView+ColorSamplers.swift`. Every change commits through
/// `ImageDocument.commitColorSamplers`: saved and dirtying, but no
/// undo step and no Action step; a drag moves the live list per tick and
/// commits once at the release. The MCP twins are `get_color_samplers` /
/// `set_color_samplers`.
extension EditorViewController {
    @objc func selectColorSamplerTool(_ sender: Any?) { selectTool(.colorSampler) }

    /// Called once from `loadView`.
    func installColorSamplers() {
        canvas.onColorSamplerAdd = { [weak self] in self?.addColorSampler(at: $0) }
        canvas.onColorSamplerMove = { [weak self] in self?.moveColorSampler($0, to: $1) }
        canvas.onColorSamplerMoveEnd = { [weak self] _ in self?.endColorSamplerMove() }
        canvas.onColorSamplerRemove = { [weak self] in self?.removeColorSampler($0) }
        canvas.onPreviewImageChange = { [weak self] in self?.canvasPreviewImageDidChange($0) }
        NotificationCenter.default.addObserver(
            self, selector: #selector(colorSamplersDidChange(_:)),
            name: .imageDocumentColorSamplersDidChange, object: document)
        NotificationCenter.default.addObserver(
            self, selector: #selector(colorSamplersDidChange(_:)),
            name: .editorViewChromeDidChange, object: nil)
        pushColorSamplerMarkers()
    }

    /// The canvas pixel under `point`, or nil off the canvas.
    private func colorSamplerPixel(_ point: CGPoint) -> (x: Int, y: Int)? {
        guard let doc = document?.doc else { return nil }
        let x = Int(floor(point.x)), y = Int(floor(point.y))
        guard (0..<doc.width).contains(x), (0..<doc.height).contains(y) else { return nil }
        return (x, y)
    }

    /// A placement takes the current Sample Size and shows the Info panel;
    /// an eleventh sampler beeps. Placing one while Show Color Samplers is
    /// off turns it back on, so the click never adds an
    /// invisible marker that can't be dragged or deleted.
    private func addColorSampler(at point: CGPoint) {
        guard let document, let pixel = colorSamplerPixel(point) else { return }
        let index = min(max(ToolOptionsStore.shared.sample.sampleSizeIndex, 0), 2)
        let sampler = ColorSampler(x: pixel.x, y: pixel.y, sampleSize: [1, 3, 5][index])
        guard let next = document.colorSamplers.adding(sampler) else { NSSound.beep(); return }
        guard commitColorSamplers(next) else { return }
        if !ToolOptionsStore.shared.view.colorSamplers { toggleColorSamplers(nil) }
        showInfo(nil)
    }

    /// A drag tick: the LIVE list only; the release commits it.
    private func moveColorSampler(_ index: Int, to point: CGPoint) {
        guard let document, let pixel = colorSamplerPixel(point),
              let next = document.colorSamplers.moving(index, toX: pixel.x, y: pixel.y),
              next != document.colorSamplers else { return }
        document.colorSamplers = next
    }

    /// The drag's release inside the canvas commits the dragged list; an
    /// unchanged one just resyncs.
    private func endColorSamplerMove() {
        guard let document else { return }
        commitColorSamplers(document.colorSamplers)
    }

    private func removeColorSampler(_ index: Int) {
        guard let document, let next = document.colorSamplers.removing(index) else { return }
        commitColorSamplers(next)
    }

    /// The one UI commit: a refusal beeps (`SilentEditRefusal`) and the
    /// document has already put the live list back; true when it committed.
    @discardableResult
    private func commitColorSamplers(_ next: ColorSamplerSet) -> Bool {
        guard let document else { return false }
        do {
            return try document.commitColorSamplers(next)
        } catch {
            document.presentError(error)
            return false
        }
    }

    /// The list or View ▸ Show Color Samplers changed.
    @objc private func colorSamplersDidChange(_ note: Notification) {
        colorSamplerPreviewChanged(canvas.previewImage)
    }

    /// The markers, with `after` labels when a sheet previews; none while
    /// Show Color Samplers is off.
    private func pushColorSamplerMarkers(after: [String]? = nil) {
        guard ToolOptionsStore.shared.view.colorSamplers, let document else {
            if !canvas.colorSamplerMarkers.isEmpty { canvas.colorSamplerMarkers = [] }
            return
        }
        let markers = document.colorSamplers.items.enumerated().map { index, sampler in
            ColorSamplerMarker(
                label: index + 1,
                point: CGPoint(x: Double(sampler.x) + 0.5, y: Double(sampler.y) + 0.5),
                after: after.flatMap { $0.indices.contains(index) ? $0[index] : nil })
        }
        if markers != canvas.colorSamplerMarkers { canvas.colorSamplerMarkers = markers }
    }

    /// A sheet's preview landed or went away. "After" values are read only
    /// while a sheet is attached and no channel is on display: at 16 bits
    /// from the authoritative projection the preview was made from, at 8
    /// bits from the preview image itself; anything else reads "—".
    func colorSamplerPreviewChanged(_ image: CGImage?) {
        // A ⌥-drag threshold display is not the edit's pixels.
        if image != nil, image === canvas.clipDisplayPreview { return }
        guard let document, let doc = document.doc, let image,
              !document.colorSamplers.items.isEmpty,
              view.window?.attachedSheet != nil, canvas.channelDisplay == nil else {
            pushColorSamplerMarkers()
            infoPanel?.setColorSamplerAfter(nil)
            return
        }
        let set = document.colorSamplers
        let unavailable = set.items.map { _ in ColorSamplerReading.unavailable(nil) }
        let readings: [ColorSamplerReading]
        if doc.isHighDepth, !doc.isFloat {
            if let source = canvas.previewPrecisionSource, source.preview === image {
                readings = ColorSamplerReader.read(set, precisionPreview: source.projection,
                                                   lab: doc.isLab)
            } else {
                readings = unavailable
            }
        } else if !doc.isHighDepth {
            readings = ColorSamplerReader.read(set, preview: image, doc: doc) ?? unavailable
        } else {
            readings = unavailable
        }
        pushColorSamplerMarkers(after: readings.map { $0.text(.rgb) })
        infoPanel?.setColorSamplerAfter(readings)
    }

    /// View ▸ Clear Color Samplers and the options bar's Clear All.
    @objc func clearColorSamplers(_ sender: Any?) {
        guard let document, !document.colorSamplers.items.isEmpty else { NSSound.beep(); return }
        commitColorSamplers(ColorSamplerSet())
    }

    /// View ▸ Show Color Samplers: an app-wide view preference, so every
    /// editor answers the chrome broadcast.
    @objc func toggleColorSamplers(_ sender: Any?) {
        ToolOptionsStore.shared.view.colorSamplers.toggle()
        viewChromeChanged()
    }

    func validateColorSamplerItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        switch item.action {
        case #selector(clearColorSamplers(_:)):
            return !(document?.colorSamplers.items.isEmpty ?? true)
        case #selector(toggleColorSamplers(_:)):
            (item as? NSMenuItem)?.state = ToolOptionsStore.shared.view.colorSamplers ? .on : .off
            return document?.doc != nil
        default:
            return nil
        }
    }

    func colorSamplerClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "colorSampler.size", microLabel: "Sample", overflowLabel: "Sample size",
                    kind: .popup(
                        width: 76, items: ["Point", "3 × 3", "5 × 5"],
                        get: { ToolOptionsStore.shared.sample.sampleSizeIndex },
                        set: { ToolOptionsStore.shared.sample.sampleSizeIndex = $0 })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "colorSampler.clear", overflowLabel: "Clear All",
                    kind: .button(
                        title: "Clear All",
                        action: { [weak self] in self?.clearColorSamplers(nil) })),
            ]),
        ]
    }
}
