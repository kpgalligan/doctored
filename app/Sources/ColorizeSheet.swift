import AppKit

/// The sheet of Filter ▸ Neural Filters ▸ Colorize…: Saturation, Cyan/Red, Magenta/Green,
/// Yellow/Blue (−50…50), Color Artifact Reduction (0…100), Output To (Current Layer, New Layer, New
/// Document) and Preview — over the whole layer (its 8-bit display, fitted to 1024 px). On
/// `PreviewRenderer` the first render runs the core's input and DDColor once and keeps the chroma
/// (touched only on that queue); a slider change re-runs only the byte apply. OK starts
/// `ColorizeRun.start` (layer outputs, one undo step) or runs `ColorizeRun.runNewDocument`, and
/// records `neural_colorize` with the settings.
final class ColorizeSheetController: NSViewController {
    static let previewMaxSide = 1024

    /// The preview's model and the chroma it predicted for `source`; preview queue only.
    private final class PreviewState {
        let models = NeuralPreviewModels()
        var chroma: [Float]?
    }

    private let document: ImageDocument
    private let layer: Int
    private let source: RasterImage?
    private let colorSpace: CGColorSpace
    private let profile: Data
    private let budget: (limit: UInt64, other: UInt64)
    private let renderer = PreviewRenderer()
    private let state = PreviewState()
    private(set) var settings = ColorizeSettings()
    private var sliders: [(key: String, slider: NSSlider, label: NSTextField)] = []
    private let output = NSPopUpButton(frame: .zero, pullsDown: false)
    private let showPreview = NSButton(checkboxWithTitle: "Preview", target: nil, action: nil)
    private let preview = NSImageView()

    init(document: ImageDocument, layer: Int) throws {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        self.document = document
        self.layer = layer
        colorSpace = doc.colorSpace
        profile = NeuralFilterRun.modelProfile(doc)
        budget = try NeuralFilterRun.budget(doc)
        source = try autoreleasepool { try Self.source(doc, layer: layer) }
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] image in self?.show(image) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ColorizeSheetController does not support NSCoder")
    }

    /// The layer's own pixels (its 8-bit display at 16 bits), fitted to `previewMaxSide`.
    private static func source(_ doc: RasterDocument, layer: Int) throws -> RasterImage? {
        let pixels: RasterImage?
        if let precision = doc.precision {
            // A Lab layer previews its Display P3 view, the model's input (#1 Lab phase 6b-iii).
            let limit = RasterResources.shared.workingLimit
            pixels = try precision.layerSource(layer).map {
                try NeuralFilterRun.modelInput($0, document: doc, limit: limit, other: 0, control: nil)
            }?.previewRGBA8(workingLimit: limit, otherRetained: 0)
        } else {
            pixels = doc.layerImage(layer)
        }
        guard let pixels, pixels.width > 0, pixels.height > 0 else { return nil }
        let longest = max(pixels.width, pixels.height)
        guard longest > previewMaxSide else { return pixels }
        let scale = Double(previewMaxSide) / Double(longest)
        let w = max(1, Int((Double(pixels.width) * scale).rounded()))
        let h = max(1, Int((Double(pixels.height) * scale).rounded()))
        return pixels.resized(w: w, h: h, filter: DR_FILTER_BILINEAR)
    }

    override func loadView() {
        var rows: [[NSView]] = []
        let titles = ["Saturation:", "Cyan / Red:", "Magenta / Green:", "Yellow / Blue:", "Artifact Reduction:"]
        for (slider, title) in zip(ColorizeSettings.sliders, titles) {
            let control = NSSlider(value: 0, minValue: slider.range.lowerBound, maxValue: slider.range.upperBound,
                                   target: self, action: #selector(controlChanged(_:)))
            control.isContinuous = true
            control.widthAnchor.constraint(equalToConstant: 180).isActive = true
            control.setAccessibilityIdentifier("colorize-" + slider.key)
            let label = NSTextField(labelWithString: "0")
            label.font = DS.mono(12)
            label.textColor = DS.textMuted
            label.alignment = .right
            label.widthAnchor.constraint(equalToConstant: 40).isActive = true
            sliders.append((slider.key, control, label))
            rows.append([fieldLabel(title), control, label])
        }
        output.addItems(withTitles: ColorizeOutput.allCases.map(\.title))
        output.target = self
        output.action = #selector(controlChanged(_:))
        output.setAccessibilityIdentifier("colorize-output")
        rows.append([fieldLabel("Output To:"), output, NSView()])
        showPreview.state = .on
        showPreview.target = self
        showPreview.action = #selector(controlChanged(_:))
        showPreview.setAccessibilityIdentifier("colorize-preview")
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 130
        preview.imageScaling = .scaleProportionallyUpOrDown
        preview.imageFrameStyle = .grayBezel
        preview.translatesAutoresizingMaskIntoConstraints = false
        preview.widthAnchor.constraint(equalToConstant: 376).isActive = true
        preview.heightAnchor.constraint(equalToConstant: 260).isActive = true
        let content = NSStackView(views: [preview, showPreview, grid])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        apply.title = "OK"
        view = makeSheetView(
            title: "Colorize",
            hint: "Colors the layer from its luminance; the layer's own lightness and detail are kept.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancel, apply: apply, leading: [sheetFootnote("one undo step")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        requestPreview()
    }

    private func show(_ image: CGImage?) {
        guard let image else { return }
        preview.image = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
    }

    /// One preview on the worker queue: the model once (its chroma kept), then the byte apply
    /// with the current sliders; the layer itself with Preview off.
    private func requestPreview() {
        guard let source else { return }
        let space = colorSpace
        guard showPreview.state == .on, let entry = ModelGate.ready(ColorizeRun.capability) else {
            renderer.request { source.makeCGImage(in: space) }
            return
        }
        let settings = settings, state = state, budget = budget, profile = profile
        renderer.request {
            do {
                let chroma: [Float]
                if let kept = state.chroma {
                    chroma = kept
                } else {
                    let model = try state.models.model(
                        entry.compiled, side: NeuralFilterRun.colorizeSide, outputSide: NeuralFilterRun.colorizeSide,
                        qualityFactor: nil, outputChannels: 2)
                    chroma = try ColorizeRun.chroma(.byte(source), profile: profile, model: model,
                                                    limit: budget.limit, other: budget.other, control: nil)
                    state.chroma = chroma
                }
                let out = try ColorizeRun.apply(.byte(source), profile: profile, chroma: chroma, settings: settings,
                                                limit: budget.limit, other: budget.other, control: nil)
                guard case .byte(let image)? = out else { return source.makeCGImage(in: space) }
                return image.makeCGImage(in: space)
            } catch {
                return nil
            }
        }
    }

    @objc private func controlChanged(_ sender: Any?) {
        for entry in sliders {
            entry.slider.doubleValue = entry.slider.doubleValue.rounded()
            settings.set(entry.key, entry.slider.doubleValue)
            entry.label.stringValue = String(format: "%.0f", entry.slider.doubleValue)
        }
        settings.output = ColorizeOutput.allCases[max(0, output.indexOfSelectedItem)]
        if sender as? NSPopUpButton !== output { requestPreview() }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        renderer.cancel()
        dismiss(self)
    }

    @objc private func applyClicked(_ sender: Any?) {
        let settings = settings
        // Only output Current Layer writes the source layer, so only it meets a pixel lock (as MCP).
        if settings.output == .layer,
           (presentingViewController as? EditorViewController)?.refuseLockedEdit(
               layer: layer, kind: DR_EDIT_PIXELS) == true {
            return
        }
        if settings.output == .layer, !document.confirmRasterize(layer: layer) { return }
        renderer.cancel()
        renderer.waitUntilIdle()
        dismiss(self)
        do {
            guard let entry = ModelGate.ready(ColorizeRun.capability)?.entry else {
                throw DoctoredCoreError(message: ModelGate.notInstalled(ColorizeRun.capability))
            }
            let note = ModelGate.note(ColorizeRun.menu, entry)
            if settings.output == .newDocument {
                _ = try ColorizeRun.runNewDocument(document: document, layer: layer, settings: settings)
                ActionRecorder.shared.record(.colorize(settings, note: note), on: document)
            } else {
                _ = try ColorizeRun.start(document: document, layer: layer, settings: settings,
                                          record: .colorize(settings, note: note), showProgress: true)
            }
        } catch is JobCancelled {
        } catch {
            document.presentError(error)
        }
    }
}

extension EditorViewController {
    /// Filter ▸ Neural Filters ▸ Colorize…: offers Settings ▸ Models when DDColor is not
    /// installed, else presents the sheet.
    @objc func showColorize(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        guard document.planeEditTarget == .layer, !document.quickMaskActive else { NSSound.beep(); return }
        guard ModelGate.ready(ColorizeRun.capability) != nil else {
            ModelGate.offerSettings(ModelGate.notInstalled(ColorizeRun.capability))
            return
        }
        if let reason = NeuralFilterRun.refusal(doc, name: ColorizeRun.name) {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        let layer = document.activeLayerIndex
        if doc.hasRetainedSource(layer) {
            document.presentError(DoctoredCoreError(message: RetainedImagePolicy.rasterizeFirst))
            return
        }
        do {
            presentAsSheet(try ColorizeSheetController(document: document, layer: layer))
        } catch {
            document.presentError(error)
        }
    }
}
