import AppKit

/// The sheet of Filter ▸ Neural Filters ▸ Super Zoom…: the panel — the zoom (×2,
/// ×4), Enhance Image Details, Remove JPEG Artifacts, Noise Reduction and Sharpen — over a 100 %
/// preview of a 192² crop of the layer centred on the selection's bounds (else the canvas),
/// rendered through the run's own stages on `PreviewRenderer`. ×4 is disabled, with the core's
/// refusal shown, when the result would pass the 100-megapixel limit. OK runs `SuperZoomRun.run`
/// and records `super_zoom` with the settings.
final class SuperZoomSheetController: NSViewController {
    static let previewSide = 192

    private let document: ImageDocument
    private let layer: Int
    private let crop: RasterImage?
    private let colorSpace: CGColorSpace
    private let budget: (limit: UInt64, other: UInt64)
    private let renderer = PreviewRenderer()
    private let models = NeuralPreviewModels()
    private(set) var settings = SuperZoomSettings()
    private let zoom = NSSegmentedControl(labels: ["×2", "×4"], trackingMode: .selectOne, target: nil, action: nil)
    private let enhance = NSButton(checkboxWithTitle: "Enhance Image Details", target: nil, action: nil)
    private let removeJpeg = NSButton(checkboxWithTitle: "Remove JPEG Artifacts", target: nil, action: nil)
    private let noise = NSSlider(value: 0, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let sharpen = NSSlider(value: 0, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let noiseLabel = NSTextField(labelWithString: "0")
    private let sharpenLabel = NSTextField(labelWithString: "0")
    private let preview = NSImageView()
    private let scroll = NSScrollView()
    private let status = NSTextField(labelWithString: "")
    private var fourRefusal: String?

    init(document: ImageDocument, layer: Int) throws {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        self.document = document
        self.layer = layer
        colorSpace = doc.colorSpace
        budget = try NeuralFilterRun.budget(doc)
        crop = try autoreleasepool { try Self.crop(document, doc: doc, layer: layer) }
        let entry = ModelGate.ready(SuperZoomRun.capability)?.entry
        if let entry {
            do {
                var four = SuperZoomSettings()
                four.zoom = 4
                _ = try SuperZoomRun.plan(width: doc.width, height: doc.height,
                                          bytesPerSample: doc.isHighDepth ? 2 : 1, settings: four,
                                          superResolution: entry, jpeg: nil)
            } catch {
                fourRefusal = error.localizedDescription
            }
        }
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] image in self?.showPreview(image) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SuperZoomSheetController does not support NSCoder")
    }

    /// The preview's source: a `previewSide`² crop of the layer's display on its canvas, centred
    /// on the selection's bounds, else the canvas, and slid inside it.
    private static func crop(_ document: ImageDocument, doc: RasterDocument, layer: Int) throws -> RasterImage? {
        let display: RasterImage?
        if doc.isHighDepth { display = try doc.nativeDisplay(layer: layer) } else { display = doc.layerCanvasImage(layer) }
        guard let display else { return nil }
        let side = min(previewSide, display.width, display.height)
        let centre = document.canvasSelection?.bounds
            ?? CGRect(x: 0, y: 0, width: display.width, height: display.height)
        let x = min(max(0, Int(centre.midX) - side / 2), display.width - side)
        let y = min(max(0, Int(centre.midY) - side / 2), display.height - side)
        return display.cropped(x: x, y: y, w: side, h: side)
    }

    override func loadView() {
        zoom.selectedSegment = 0
        zoom.target = self
        zoom.action = #selector(controlChanged(_:))
        zoom.setEnabled(fourRefusal == nil, forSegment: 1)
        zoom.setAccessibilityIdentifier("super-zoom-zoom")
        for (button, identifier, on) in [(enhance, "super-zoom-enhance", true), (removeJpeg, "super-zoom-jpeg", false)] {
            button.target = self
            button.action = #selector(controlChanged(_:))
            button.state = on ? .on : .off
            button.setAccessibilityIdentifier(identifier)
        }
        for (slider, label, identifier) in [(noise, noiseLabel, "super-zoom-noise"), (sharpen, sharpenLabel, "super-zoom-sharpen")] {
            slider.target = self
            slider.action = #selector(controlChanged(_:))
            slider.isContinuous = true
            slider.widthAnchor.constraint(equalToConstant: 180).isActive = true
            slider.setAccessibilityIdentifier(identifier)
            label.font = DS.mono(12)
            label.textColor = DS.textMuted
            label.alignment = .right
            label.widthAnchor.constraint(equalToConstant: 40).isActive = true
        }
        let grid = NSGridView(views: [
            [fieldLabel("Zoom:"), zoom, NSView()],
            [fieldLabel("Noise Reduction:"), noise, noiseLabel],
            [fieldLabel("Sharpen:"), sharpen, sharpenLabel],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 120
        preview.imageScaling = .scaleNone
        preview.imageAlignment = .alignTopLeft
        scroll.documentView = preview
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.borderType = .lineBorder
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.widthAnchor.constraint(equalToConstant: 376).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 280).isActive = true
        status.textColor = .systemRed
        status.font = DS.sans(11)
        status.lineBreakMode = .byWordWrapping
        status.maximumNumberOfLines = 3
        status.preferredMaxLayoutWidth = 376
        status.stringValue = fourRefusal.map { "×4: " + $0 } ?? ""
        status.isHidden = fourRefusal == nil
        let content = NSStackView(views: [scroll, grid, enhance, removeJpeg, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        apply.title = "OK"
        view = makeSheetView(
            title: "Super Zoom",
            hint: "Enlarges the layer into a new document; the preview shows the result at 100 %.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancel, apply: apply, leading: [sheetFootnote("new document")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        requestPreview()
    }

    private func showPreview(_ image: CGImage?) {
        guard let image else { return }
        let size = NSSize(width: image.width, height: image.height)
        preview.image = NSImage(cgImage: image, size: size)
        preview.frame = NSRect(origin: .zero, size: size)
    }

    /// One preview through the run's stages on the crop (worker queue).
    private func requestPreview() {
        guard let crop, let superResolution = ModelGate.ready(SuperZoomRun.capability) else { return }
        let settings = settings, models = models, budget = budget, space = colorSpace
        let jpeg = settings.removeJpegArtifacts ? ModelGate.ready(JpegArtifactsRemovalRun.capability) : nil
        renderer.request {
            do {
                var image = NeuralImage.byte(crop)
                if let jpeg {
                    let model = try models.model(jpeg.compiled, side: NeuralFilterRun.jpegTile,
                                                 outputSide: NeuralFilterRun.jpegTile,
                                                 qualityFactor: JpegArtifactsStrength.medium.qualityFactor)
                    image = try NeuralFilterRun.run(NeuralFilterRun.jpegStage(), model: model, image: image,
                                                    limit: budget.limit, other: budget.other, control: nil)
                }
                let model = settings.enhanceDetails ? try models.model(
                    superResolution.compiled, side: NeuralFilterRun.superResolutionTile,
                    outputSide: NeuralFilterRun.superResolutionTile * NeuralFilterRun.superResolutionScale,
                    qualityFactor: nil) : nil
                let stage = NeuralFilterRun.superResolutionStage(
                    zoom: settings.zoom, enhance: settings.enhanceDetails,
                    noiseReduction: settings.noiseReduction, sharpen: settings.sharpen)
                image = try NeuralFilterRun.run(stage, model: model, image: image, limit: budget.limit,
                                                other: budget.other, control: nil)
                guard case .byte(let out) = image else { return nil }
                return out.makeCGImage(in: space)
            } catch {
                return nil
            }
        }
    }

    @objc private func controlChanged(_ sender: Any?) {
        if sender as? NSButton === removeJpeg, removeJpeg.state == .on,
           ModelGate.ready(JpegArtifactsRemovalRun.capability) == nil {
            removeJpeg.state = .off
            ModelGate.offerSettings(ModelGate.notInstalled(JpegArtifactsRemovalRun.capability))
        }
        noise.doubleValue = noise.doubleValue.rounded()
        sharpen.doubleValue = sharpen.doubleValue.rounded()
        settings.zoom = zoom.selectedSegment == 1 ? 4 : 2
        settings.enhanceDetails = enhance.state == .on
        settings.removeJpegArtifacts = removeJpeg.state == .on
        settings.noiseReduction = noise.doubleValue
        settings.sharpen = sharpen.doubleValue
        noiseLabel.stringValue = String(format: "%.0f", noise.doubleValue)
        sharpenLabel.stringValue = String(format: "%.0f", sharpen.doubleValue)
        requestPreview()
    }

    @objc private func cancelClicked(_ sender: Any?) {
        renderer.cancel()
        dismiss(self)
    }

    @objc private func applyClicked(_ sender: Any?) {
        let settings = settings
        renderer.cancel()
        renderer.waitUntilIdle()
        dismiss(self)
        do {
            let result = try SuperZoomRun.run(document: document, layer: layer, settings: settings)
            let note = ModelGate.note(SuperZoomRun.menu, result.models[0])
            ActionRecorder.shared.record(.superZoom(settings, note: note), on: document)
        } catch is JobCancelled {
        } catch {
            document.presentError(error)
        }
    }
}

extension EditorViewController {
    /// Filter ▸ Neural Filters ▸ Super Zoom…: offers Settings ▸ Models when Real-ESRGAN is not
    /// installed, else presents the sheet.
    @objc func showSuperZoom(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        guard document.planeEditTarget == .layer, !document.quickMaskActive else { NSSound.beep(); return }
        guard ModelGate.ready(SuperZoomRun.capability) != nil else {
            ModelGate.offerSettings(ModelGate.notInstalled(SuperZoomRun.capability))
            return
        }
        if let reason = NeuralFilterRun.refusal(doc, name: SuperZoomRun.name) {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        do {
            presentAsSheet(try SuperZoomSheetController(document: document, layer: document.activeLayerIndex))
        } catch {
            document.presentError(error)
        }
    }
}
