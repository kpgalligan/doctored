import AppKit

/// The sheet of Filter ▸ Neural Filters ▸ JPEG Artifacts Removal…: the panel —
/// Strength (Low, Medium, High) and Output To (Current Layer, New Layer) — over a 100 % preview of
/// a 384² crop of the layer centred on the selection's bounds (else the canvas), its origin on the
/// layer's 8×8 block grid, rendered through the run's own stage on `PreviewRenderer`; Preview off
/// shows the crop as it is. OK starts `JpegArtifactsRemovalRun.start` (one undo step) and records
/// `jpeg_artifacts_removal` with the settings.
final class JpegArtifactsRemovalSheetController: NSViewController {
    static let previewSide = 384

    private let document: ImageDocument
    private let layer: Int
    private let crop: RasterImage?
    private let colorSpace: CGColorSpace
    private let budget: (limit: UInt64, other: UInt64)
    private let renderer = PreviewRenderer()
    private let models = NeuralPreviewModels()
    private(set) var settings = JpegArtifactsSettings()
    private let strength = NSPopUpButton(frame: .zero, pullsDown: false)
    private let output = NSPopUpButton(frame: .zero, pullsDown: false)
    private let showPreview = NSButton(checkboxWithTitle: "Preview", target: nil, action: nil)
    private let preview = NSImageView()
    private let scroll = NSScrollView()

    init(document: ImageDocument, layer: Int) throws {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        self.document = document
        self.layer = layer
        colorSpace = doc.colorSpace
        budget = try NeuralFilterRun.budget(doc)
        crop = try autoreleasepool { try Self.crop(document, doc: doc, layer: layer) }
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] image in self?.show(image) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("JpegArtifactsRemovalSheetController does not support NSCoder")
    }

    /// A `previewSide`² crop of the layer's own pixels (its 8-bit display at 16 bits), centred on
    /// the selection's bounds, else the canvas, on the layer's block grid.
    private static func crop(_ document: ImageDocument, doc: RasterDocument, layer: Int) throws -> RasterImage? {
        guard let placement = doc.layerPlacement(layer) else { return nil }
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
        let side = min(previewSide, pixels.width, pixels.height)
        let centre = document.canvasSelection?.bounds
            ?? CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        let cx = Int(centre.midX) - placement.x, cy = Int(centre.midY) - placement.y
        let x = min(max(0, (cx - side / 2) / 8 * 8), (pixels.width - side) / 8 * 8)
        let y = min(max(0, (cy - side / 2) / 8 * 8), (pixels.height - side) / 8 * 8)
        return pixels.cropped(x: x, y: y, w: side, h: side)
    }

    override func loadView() {
        strength.addItems(withTitles: JpegArtifactsStrength.allCases.map(\.title))
        strength.selectItem(at: 1)
        output.addItems(withTitles: ["Current Layer", "New Layer"])
        for (control, identifier) in [(strength, "jpeg-artifacts-strength"), (output, "jpeg-artifacts-output")] {
            control.target = self
            control.action = #selector(controlChanged(_:))
            control.setAccessibilityIdentifier(identifier)
        }
        showPreview.state = .on
        showPreview.target = self
        showPreview.action = #selector(controlChanged(_:))
        showPreview.setAccessibilityIdentifier("jpeg-artifacts-preview")
        let grid = NSGridView(views: [
            [fieldLabel("Strength:"), strength],
            [fieldLabel("Output To:"), output],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        preview.imageScaling = .scaleNone
        preview.imageAlignment = .alignTopLeft
        scroll.documentView = preview
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.borderType = .lineBorder
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.widthAnchor.constraint(equalToConstant: 376).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 280).isActive = true
        let content = NSStackView(views: [scroll, showPreview, grid])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        apply.title = "OK"
        view = makeSheetView(
            title: "JPEG Artifacts Removal",
            hint: "Removes JPEG blocking and ringing from the layer; the preview shows it at 100 %.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancel, apply: apply, leading: [sheetFootnote("one undo step")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        requestPreview()
    }

    private func show(_ image: CGImage?) {
        guard let image else { return }
        let size = NSSize(width: image.width, height: image.height)
        preview.image = NSImage(cgImage: image, size: size)
        preview.frame = NSRect(origin: .zero, size: size)
    }

    /// One preview through the run's stage on the crop (worker queue), or the crop itself with
    /// Preview off.
    private func requestPreview() {
        guard let crop else { return }
        let space = colorSpace
        guard showPreview.state == .on, let entry = ModelGate.ready(JpegArtifactsRemovalRun.capability) else {
            renderer.request { crop.makeCGImage(in: space) }
            return
        }
        let quality = settings.strength.qualityFactor, models = models, budget = budget
        renderer.request {
            do {
                let model = try models.model(entry.compiled, side: NeuralFilterRun.jpegTile,
                                             outputSide: NeuralFilterRun.jpegTile, qualityFactor: quality)
                let out = try NeuralFilterRun.run(NeuralFilterRun.jpegStage(), model: model, image: .byte(crop),
                                                  limit: budget.limit, other: budget.other, control: nil)
                guard case .byte(let image) = out else { return nil }
                return image.makeCGImage(in: space)
            } catch {
                return nil
            }
        }
    }

    @objc private func controlChanged(_ sender: Any?) {
        settings.strength = JpegArtifactsStrength.allCases[max(0, strength.indexOfSelectedItem)]
        settings.output = output.indexOfSelectedItem == 1 ? .newLayer : .layer
        if sender as? NSPopUpButton !== output { requestPreview() }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        renderer.cancel()
        dismiss(self)
    }

    @objc private func applyClicked(_ sender: Any?) {
        let settings = settings
        // Only output Layer writes the source layer, so only it meets a pixel lock (as MCP).
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
            guard let entry = ModelGate.ready(JpegArtifactsRemovalRun.capability)?.entry else {
                throw DoctoredCoreError(message: ModelGate.notInstalled(JpegArtifactsRemovalRun.capability))
            }
            let note = ModelGate.note(JpegArtifactsRemovalRun.menu, entry)
            _ = try JpegArtifactsRemovalRun.start(document: document, layer: layer, settings: settings,
                                                  record: .jpegArtifactsRemoval(settings, note: note),
                                                  showProgress: true)
        } catch {
            document.presentError(error)
        }
    }
}

extension EditorViewController {
    /// Filter ▸ Neural Filters ▸ JPEG Artifacts Removal…: offers Settings ▸ Models when FBCNN is
    /// not installed, else presents the sheet.
    @objc func showJpegArtifactsRemoval(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        guard document.planeEditTarget == .layer, !document.quickMaskActive else { NSSound.beep(); return }
        guard ModelGate.ready(JpegArtifactsRemovalRun.capability) != nil else {
            ModelGate.offerSettings(ModelGate.notInstalled(JpegArtifactsRemovalRun.capability))
            return
        }
        if let reason = NeuralFilterRun.refusal(doc, name: JpegArtifactsRemovalRun.name) {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        let layer = document.activeLayerIndex
        if doc.hasRetainedSource(layer) {
            document.presentError(DoctoredCoreError(message: RetainedImagePolicy.rasterizeFirst))
            return
        }
        do {
            presentAsSheet(try JpegArtifactsRemovalSheetController(document: document, layer: layer))
        } catch {
            document.presentError(error)
        }
    }
}
