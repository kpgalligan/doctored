import AppKit

/// Filter ▸ Neural Filters ▸ Depth Blur…: the depth model runs once when the sheet opens
/// (`DepthBlurRun.capture`), then every control previews through the core kernel — the 8-bit
/// `ByteFilterPreview` or the 16/32-bit `PrecisionAdjustmentSession`, as `SliderSheetController`
/// drives them — with one proxy map (`DepthBlurRun.params(…, scale:)`) dividing the frame and the
/// focal point by the proxy factor at both depths. The focal-point picker is Tilt-Shift's centre
/// well (`RadialBlurCenterAccessory`) over the captured frame; picking a point turns Focus Subject
/// off. Apply runs `DepthBlurRun.apply` and records `depth_blur` with the settings only.
final class DepthBlurSheetController: NSViewController {
    private let document: ImageDocument
    private weak var canvas: ImageCanvasView?
    private let layer: Int
    private let input: DepthBlurInput
    private let origin: CGPoint
    private(set) var settings = DepthBlurSettings()
    private let renderer: ByteFilterPreview?
    private let byteCapture: OperationCapture?
    private let precisionSession: PrecisionAdjustmentSession?
    private let previewStatus = NSTextField(labelWithString: "")
    private var sliders: [NSSlider] = []
    private var valueLabels: [NSTextField] = []
    private let focusSubject = NSButton(checkboxWithTitle: "Focus Subject", target: nil, action: nil)
    private let depthOnly = NSButton(checkboxWithTitle: "Output Depth Map Only", target: nil, action: nil)
    private let output = NSPopUpButton(frame: .zero, pullsDown: false)
    private let picker: RadialBlurCenterAccessory
    private var didRequestInitialPreview = false

    init(document: ImageDocument, canvas: ImageCanvasView, layer: Int, input: DepthBlurInput) {
        self.document = document
        self.canvas = canvas
        self.layer = layer
        self.input = input
        let placement = document.doc?.layerPlacement(layer)
        origin = CGPoint(x: placement?.x ?? 0, y: placement?.y ?? 0)
        settings.focusSubject = input.subject != nil
        let size = CGSize(width: document.doc?.width ?? 0, height: document.doc?.height ?? 0)
        picker = RadialBlurCenterAccessory(image: canvas.image, canvas: size, frame: input.frame)
        if document.doc?.isHighDepth == true {
            precisionSession = PrecisionAdjustmentSession(document, canvas: canvas, mode: .destructive)
            precisionSession?.prepareProxy()
            byteCapture = nil
            renderer = nil
        } else {
            precisionSession = nil
            byteCapture = document.doc.flatMap {
                OperationCapture(doc: $0, layerIndex: layer, target: .layer,
                                 selection: document.canvasSelection?.maskBytes())
            }
            renderer = ByteFilterPreview(capture: byteCapture, proxied: true)
        }
        super.init(nibName: nil, bundle: nil)
        renderer?.onRender = { [weak self] image in self?.canvas?.previewImage = image }
        renderer?.onRefusal = { [weak self] message in self?.showRefusal(message) }
        picker.onChange = { [weak self] in self?.pointPicked() }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("DepthBlurSheetController does not support NSCoder")
    }

    override func loadView() {
        var rows: [[NSView]] = []
        for slider in DepthBlurSettings.sliders {
            let control = NSSlider(value: slider.initial, minValue: slider.range.lowerBound,
                                   maxValue: slider.range.upperBound, target: self,
                                   action: #selector(sliderChanged(_:)))
            control.isContinuous = true
            control.widthAnchor.constraint(equalToConstant: 180).isActive = true
            control.setAccessibilityIdentifier("depth-blur-\(slider.key)")
            let label = NSTextField(labelWithString: String(format: "%.0f", slider.initial))
            label.font = DS.mono(12)
            label.textColor = DS.textMuted
            label.alignment = .right
            label.widthAnchor.constraint(equalToConstant: 52).isActive = true
            sliders.append(control)
            valueLabels.append(label)
            rows.append([fieldLabel(slider.label), control, label])
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        focusSubject.target = self
        focusSubject.action = #selector(focusSubjectChanged(_:))
        focusSubject.state = settings.focusSubject ? .on : .off
        focusSubject.isEnabled = input.subject != nil
        focusSubject.setAccessibilityIdentifier("depth-blur-focus-subject")
        depthOnly.target = self
        depthOnly.action = #selector(depthOnlyChanged(_:))
        depthOnly.setAccessibilityIdentifier("depth-blur-depth-only")
        output.addItems(withTitles: ["Current Layer", "New Layer"])
        output.target = self
        output.action = #selector(outputChanged(_:))
        output.setAccessibilityIdentifier("depth-blur-output")
        let outputRow = NSStackView(views: [fieldLabel("Output:"), output])
        outputRow.orientation = .horizontal
        outputRow.spacing = 12

        let content = NSStackView(views: [focusSubject, picker.view, grid, depthOnly, outputRow])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Depth Blur",
            hint: "Pick a focal point on the image, or let Focus Subject keep the subject sharp.",
            content: precisionSession?.content(content) ?? withPreviewStatus(content),
            buttonRow: makeButtonRow(cancel: cancel, apply: apply, leading: [sheetFootnote("one undo step")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        guard !didRequestInitialPreview else { return }
        didRequestInitialPreview = true
        requestPreview()
    }

    private func withPreviewStatus(_ content: NSView) -> NSView {
        previewStatus.textColor = .systemRed
        previewStatus.font = DS.sans(11)
        previewStatus.lineBreakMode = .byWordWrapping
        previewStatus.maximumNumberOfLines = 3
        previewStatus.setAccessibilityIdentifier("tone.preview.status")
        previewStatus.widthAnchor.constraint(lessThanOrEqualToConstant: 360).isActive = true
        previewStatus.isHidden = true
        let stack = NSStackView(views: [content, previewStatus])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        return stack
    }

    private func showRefusal(_ message: String?) {
        previewStatus.stringValue = message ?? ""
        previewStatus.isHidden = message == nil
    }

    /// One preview of the current settings at both depths (type comment).
    private func requestPreview() {
        let settings = settings, input = input, origin = origin
        let params = { (scale: Double) in DepthBlurRun.params(settings, input, layerOrigin: origin, scale: scale) }
        if let precisionSession {
            precisionSession.request(filter: "depth_blur", params: params(1), proxyParams: { params($0) })
            return
        }
        guard let renderer else { return }
        let exact = params(1)
        renderer.request(
            op: { image, control in (try? image.filterOp("depth_blur", params: exact, control: control)) ?? image },
            proxyOp: { image, factor in
                (try? image.filterOp("depth_blur", params: Self.fitted(params(factor), image))) ?? image
            },
            reserve: { try ByteFilterAdmission.reservePreview(ByteFilterRequest(op: "depth_blur", params: exact), capture: $0) })
    }

    /// A proxy request's frame and focal point kept inside the proxy plane `image`, whose
    /// size rounds differently from the scaled frame.
    private static func fitted(_ params: [String: Any], _ image: RasterImage) -> [String: Any] {
        guard var depth = params["depth"] as? [String: Any], let frame = depth["frame"] as? [Int],
              frame.count == 4 else { return params }
        let x = min(frame[0], max(0, image.width - 1)), y = min(frame[1], max(0, image.height - 1))
        let w = max(1, min(frame[2], image.width - x)), h = max(1, min(frame[3], image.height - y))
        depth["frame"] = [x, y, w, h]
        var fitted = params
        fitted["depth"] = depth
        if let point = params["focal_point"] as? [Double], point.count == 2 {
            fitted["focal_point"] = [min(Double(x + w), max(Double(x), point[0])),
                                     min(Double(y + h), max(Double(y), point[1]))]
        }
        return fitted
    }

    private func pointPicked() {
        let values = picker.values
        guard values.count == 2 else { return }
        settings.focalPoint = CGPoint(x: input.frame.minX + CGFloat(values[0]) * input.frame.width,
                                      y: input.frame.minY + CGFloat(values[1]) * input.frame.height)
        settings.focusSubject = false
        focusSubject.state = .off
        requestPreview()
    }

    @objc private func sliderChanged(_ sender: Any?) {
        guard let slider = sender as? NSSlider, let index = sliders.firstIndex(where: { $0 === slider }) else { return }
        slider.doubleValue = slider.doubleValue.rounded()
        settings.set(DepthBlurSettings.sliders[index].key, slider.doubleValue)
        valueLabels[index].stringValue = String(format: "%.0f", slider.doubleValue)
        requestPreview()
    }

    @objc private func focusSubjectChanged(_ sender: Any?) {
        settings.focusSubject = focusSubject.state == .on
        requestPreview()
    }

    @objc private func depthOnlyChanged(_ sender: Any?) {
        settings.depthMapOnly = depthOnly.state == .on
        requestPreview()
    }

    @objc private func outputChanged(_ sender: Any?) {
        settings.output = output.indexOfSelectedItem == 1 ? .newLayer : .layer
    }

    @objc private func applyClicked(_ sender: Any?) {
        let settings = settings
        if settings.output == .layer, !document.confirmRasterize(layer: layer) { return }
        guard BackgroundWork.awaitSlotForSheetApply(status: precisionSession?.previewStatus ?? previewStatus) else {
            return
        }
        renderer?.drain()
        precisionSession?.drainForCommit()
        canvas?.previewImage = nil
        dismiss(self)
        do {
            _ = try DepthBlurRun.apply(document: document, layer: layer, settings: settings, input: input,
                                       record: .depthBlur(settings))
        } catch is JobCancelled {
        } catch {
            document.presentError(error)
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        precisionSession?.cancel()
        renderer?.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }

    /// The sheet's capture: the depth and Vision's subject; when Vision finds no subject, the
    /// depth alone from the same job, so a focal point can still be picked.
    static func capture(document: ImageDocument, layer: Int) throws -> DepthBlurInput {
        try DepthBlurRun.capture(document: document, layer: layer, needsSubject: true,
                                 subjectOptional: true)
    }
}

extension EditorViewController {
    /// Filter ▸ Neural Filters ▸ Depth Blur…: offers Settings ▸ Models when the model is not
    /// installed, else runs the model once and presents the sheet.
    @objc func showDepthBlur(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document else { NSSound.beep(); return }
        guard document.planeEditTarget == .layer, !document.quickMaskActive else { NSSound.beep(); return }
        guard DepthBlurRun.readyModel != nil else {
            ModelGate.offerSettings(DepthBlurRun.notInstalled)
            return
        }
        let layer = document.activeLayerIndex
        let input: DepthBlurInput
        do { input = try DepthBlurSheetController.capture(document: document, layer: layer) } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
            return
        }
        presentAsSheet(DepthBlurSheetController(document: document, canvas: canvas, layer: layer, input: input))
    }
}
