import AppKit

/// Select > Select and Mask. Every preview starts from the captured coverage,
/// so changing a control never compounds an earlier preview. The Edge
/// Detection, Refine Edge brush and Decontaminate Colors rows and their
/// actions are in `RefineSelectionSheetController+Edge.swift`.
final class RefineSelectionSheetController: NSViewController {
    private weak var document: ImageDocument?
    let capture: SelectionRefinementCapture
    let inspection = RefinementCanvasView(frame: .zero)
    private let scroll = NSScrollView()
    private let background = NSPopUpButton()
    let output = NSPopUpButton()
    private let before = NSButton(checkboxWithTitle: "Before", target: nil, action: nil)
    let status = NSTextField(wrappingLabelWithString: "")
    private let zoomLabel = NSTextField(labelWithString: "")
    private var fields: [NSTextField] = []
    private var sliders: [NSSlider] = []
    private var applyButton: NSButton!
    private let renderer = PreviewRenderer()
    private let stamp = RefinementPreviewStamp()
    private var serial = 0
    private var token: RasterJobControl?

    // Edge Detection, Refine Edge brush and Decontaminate Colors controls
    // (built and driven in the +Edge file).
    var edgeRadiusField: NSTextField?
    var edgeRadiusSlider: NSSlider?
    let smartRadius = NSButton(checkboxWithTitle: "Smart Radius", target: nil, action: nil)
    let brushMode = NSPopUpButton()
    var brushSizeField: NSTextField?
    var undoStrokeButton: NSButton?
    var clearStrokesButton: NSButton?
    let decontaminate = NSButton(checkboxWithTitle: "Decontaminate Colors", target: nil, action: nil)
    var decontaminateAmountField: NSTextField?
    /// The Refine Edge brush's strokes and settings.
    var brush: RefineEdgeBrushSession

    /// The Output popup's items, in order.
    static let outputs: [SelectionRefinementOutput] = [.selection, .mask, .newLayer, .newLayerWithMask]

    init(document: ImageDocument) throws {
        self.document = document
        capture = try SelectionRefinementCapture(document)
        brush = try RefineEdgeBrushSession(width: capture.selection.doc.width,
                                           height: capture.selection.doc.height)
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] image in self?.showPreview(image) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("RefineSelectionSheetController does not support NSCoder") }

    override func loadView() {
        inspection.frame = NSRect(x: 0, y: 0, width: capture.selection.doc.width,
                                  height: capture.selection.doc.height)
        inspection.onBrushBegin = { [weak self] in self?.brushBegan(at: $0) ?? false }
        inspection.onBrushMove = { [weak self] in self?.brushMoved(to: $0) }
        inspection.onBrushEnd = { [weak self] in self?.brushEnded() }
        scroll.documentView = inspection
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = true
        scroll.allowsMagnification = true
        scroll.minMagnification = 0.01
        scroll.maxMagnification = 8
        scroll.widthAnchor.constraint(equalToConstant: 560).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 380).isActive = true
        background.addItems(withTitles: RefinementCanvasView.Background.allCases.map(\.title))
        background.identifier = NSUserInterfaceItemIdentifier("refine.background")
        background.target = self; background.action = #selector(viewChanged(_:))
        before.identifier = NSUserInterfaceItemIdentifier("refine.before")
        before.target = self; before.action = #selector(viewChanged(_:))
        let fit = StickerButton(title: "Fit", style: .secondary, target: self, action: #selector(fitClicked(_:)))
        fit.identifier = NSUserInterfaceItemIdentifier("refine.fit")
        let actual = StickerButton(title: "100%", style: .secondary, target: self, action: #selector(actualClicked(_:)))
        actual.identifier = NSUserInterfaceItemIdentifier("refine.actual")
        zoomLabel.font = DS.mono(11)
        let toolbar = NSStackView(views: [background, before, fit, actual, zoomLabel])
        toolbar.spacing = 8
        let preview = NSStackView(views: [scroll, toolbar])
        preview.orientation = .vertical; preview.alignment = .leading; preview.spacing = 10

        var controls: [NSView] = makeEdgeDetectionRows()
        let names = ["Smooth", "Feather", "Shift", "Contrast"]
        let ranges: [(Double, Double)] = [(0, 64), (0, 64), (-64, 64), (0, 100)]
        for (index, name) in names.enumerated() {
            let field = selectionSheetField(0, identifier: "refine.\(name.lowercased())",
                target: self, action: #selector(fieldChanged(_:)))
            field.tag = index
            let slider = NSSlider(value: 0, minValue: ranges[index].0, maxValue: ranges[index].1,
                                  target: self, action: #selector(sliderChanged(_:)))
            slider.tag = index; slider.isContinuous = true
            slider.widthAnchor.constraint(equalToConstant: 210).isActive = true
            fields.append(field); sliders.append(slider)
            let row = NSStackView(views: [fieldLabel("\(name) (\(index == 3 ? "%" : "px"))"), field])
            row.distribution = .fill; row.spacing = 8
            controls.append(row); controls.append(slider)
        }
        controls += makeBrushRows()
        controls += makeDecontaminateRows()
        output.addItems(withTitles: ["Selection", "\(capture.hadMask ? "Replace" : "New") Layer Mask",
                                     "New Layer", "New Layer with Layer Mask"])
        output.identifier = NSUserInterfaceItemIdentifier("refine.output")
        output.target = self; output.action = #selector(outputChanged(_:))
        let target = NSTextField(wrappingLabelWithString: "Mask output: \(capture.layerName)")
        target.font = DS.sans(11); target.textColor = DS.textMuted
        controls += [fieldLabel("Output"), output, target]
        let options = NSStackView(views: controls)
        options.orientation = .vertical; options.alignment = .leading; options.spacing = 6
        options.widthAnchor.constraint(equalToConstant: 220).isActive = true
        let row = NSStackView(views: [preview, options])
        row.orientation = .horizontal; row.alignment = .top; row.spacing = 24
        status.font = DS.sans(11); status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 800
        let content = NSStackView(views: [row, status])
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 12
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("refine.apply")
        applyButton.isEnabled = false
        view = makeSheetView(title: "Select and Mask",
            hint: "Inspect the edge against different backgrounds. Controls refine the original coverage; Apply creates one reversible edit.",
            content: content, buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: applyButton), width: 856)
        syncBrushControls()
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        fitClicked(nil)
        requestPreview()
    }

    var parameters: SelectionRefinementParameters {
        guard fields.count == 4 else { return SelectionRefinementParameters() }
        return SelectionRefinementParameters(
            smooth: Double(fields[0].stringValue) ?? .nan,
            feather: Double(fields[1].stringValue) ?? .nan,
            shift: Double(fields[2].stringValue) ?? .nan,
            contrast: (Double(fields[3].stringValue) ?? .nan) / 100,
            edgeRadius: edgeRadiusField.map { Double($0.stringValue) ?? .nan } ?? 0,
            smartRadius: smartRadius.state == .on,
            brush: brush.committed,
            decontaminate: decontaminate.state == .on,
            decontaminateAmount: decontaminateAmountField.map { (Double($0.stringValue) ?? .nan) / 100 } ?? 1)
    }
    var selectedOutput: SelectionRefinementOutput {
        let index = output.indexOfSelectedItem
        return Self.outputs.indices.contains(index) ? Self.outputs[index] : .selection
    }

    @objc private func sliderChanged(_ sender: NSSlider) {
        fields[sender.tag].stringValue = String(format: "%.2f", sender.doubleValue)
        requestPreview()
    }
    @objc private func fieldChanged(_ sender: Any?) {
        for (index, field) in fields.enumerated() {
            if let value = Double(field.stringValue), value.isFinite { sliders[index].doubleValue = value }
        }
        requestPreview()
    }
    @objc private func viewChanged(_ sender: Any?) { requestPreview() }
    @objc func fitClicked(_ sender: Any?) {
        let size = scroll.contentSize
        let scale = min(size.width / inspection.bounds.width, size.height / inspection.bounds.height, 1)
        scroll.magnification = max(scroll.minMagnification, scale)
        zoomLabel.stringValue = "\(Int(scroll.magnification * 100))%"
    }
    @objc func actualClicked(_ sender: Any?) {
        scroll.magnification = 1
        zoomLabel.stringValue = "100%"
    }

    func requestPreview() {
        renderer.onRender = { [weak self] image in self?.showPreview(image) }
        serial += 1
        token?.cancel()
        applyButton.isEnabled = false
        guard let document else { return }
        do {
            let output = selectedOutput
            let parameters = try parameters.validated(for: output)
            try capture.validate(in: document, output: output)
            let background = RefinementCanvasView.Background(rawValue: background.indexOfSelectedItem) ?? .checker
            inspection.background = background
            let captured = capture, original = before.state == .on
            // The brush's area plane, rasterized once per change of strokes.
            let area = try brush.areaPlane()
            if captured.isNative {
                // The typed plans are built on main, then computed inside the
                // reservation that already counts their scratch. The coverage
                // stages are planned over the captured coverage, a stand-in of
                // the same length for the edge stage's result; the two run one
                // after the other, so the reservation is the larger of them
                // plus the edge result held between them.
                let edge = try captured.nativeEdgeWork(parameters, area: area)
                let work = try captured.nativeWork(parameters)
                let pixels = UInt64(captured.coverage.count)
                let stages = edge.map { max($0.scratchEstimate, work.scratchEstimate) + pixels * 2 }
                    ?? work.scratchEstimate
                let reservedBytes = stages + pixels * 2 + pixels * 5
                    + CanvasSelection.contourWorkingBytes
                let reservation = try SelectionWorkBudget.reservePreview(bytes: reservedBytes,
                                                                        outputBytes: pixels * 2)
                token = reservation.control
                let token = reservation.control
                renderer.cancelCompute = { token.cancel() }
                let version = serial, stamp = stamp, coverage16 = captured.coverage16
                let planes = parameters.runsPlaneStage
                renderer.request { reservation.run { token in
                    defer { stamp.set(version) }
                    guard !token.isCancelled else { return nil }
                    let samples: [UInt16]?
                    if original {
                        samples = coverage16
                    } else if let edge {
                        samples = try? Self.nativeStages(edge: edge, planes: planes ? work : nil,
                                                         control: token, ownReservation: reservedBytes)
                    } else {
                        samples = try? work.compute(control: token, ownReservation: reservedBytes)
                    }
                    guard let samples, !token.isCancelled else { return nil }
                    return captured.nativeDisplay(samples, background: background)
                } }
                status.stringValue = "Updating preview…"
                return
            }
            let projection = output == .mask ? try captured.selection.doc.projectionScratchEstimate() : 0
            let maskStorage = output == .mask ? captured.maskOutputBytes * 2 : 0
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: captured.scratchEstimate(for: parameters) + projection + maskStorage
                    + UInt64(captured.coverage.count) * 12,
                outputBytes: UInt64(captured.coverage.count))
            token = reservation.control
            let token = reservation.control
            renderer.cancelCompute = { token.cancel() }
            let version = serial, stamp = stamp
            let core = parameters.coreParameters
            renderer.request { reservation.run { token in
                defer { stamp.set(version) }
                guard !token.isCancelled, let source = captured.selection.source else { return nil }
                let mask = original ? captured.coverage
                    : try? source.refiningSelection(captured.coverage, brush: area, parameters: core,
                                                    control: token)
                guard let mask, !token.isCancelled else { return nil }
                if background == .mask {
                    return CanvasSelection.grayImage(mask, captured.selection.doc.width, captured.selection.doc.height)
                }
                return captured.preview(mask, output: output)
            } }
            status.stringValue = "Updating preview…"
        } catch {
            status.stringValue = error.localizedDescription
            inspection.image = nil
        }
    }

    /// The typed preview's two stages under one reservation: the edge work,
    /// then (when any coverage control is set) the coverage work over its
    /// result. Each core call takes its own child of `control`.
    private static func nativeStages(edge: NativeSelectionWork, planes: NativeSelectionWork?,
                                     control: RasterJobControl,
                                     ownReservation: UInt64) throws -> [UInt16] {
        try control.beginCompoundOperation()
        let refined = try control.withChildOperation(from: 0, to: planes == nil ? 1 : 0.7) {
            try edge.compute(control: $0, ownReservation: ownReservation)
        }
        guard let planes else { return refined }
        return try control.withChildOperation(from: 0.7, to: 1) {
            try planes.compute(control: $0, ownReservation: ownReservation, over: refined)
        }
    }

    private func showPreview(_ image: CGImage?) {
        guard stamp.value == serial else { return }
        inspection.image = image
        applyButton.isEnabled = image != nil
        status.stringValue = image == nil ? "The preview could not be rendered. Reduce the settings and try again."
            : before.state == .on ? "Before: original captured selection."
            : capture.isNative ? "After: refined coverage at 65,536 levels; preview is a byte display."
            : "After: refined coverage. Use 100% and scroll to inspect individual edges."
    }

    @objc func applyClicked(_ sender: Any?) {
        guard let document else { dismiss(self); return }
        do {
            let output = selectedOutput
            let parameters = try parameters.validated(for: output)
            try capture.validate(in: document, output: output)
            renderer.cancel()
            var arguments = parameters.arguments
            arguments["output"] = output.rawValue
            if output == .mask { arguments["layer"] = capture.layer }
            try capture.apply(parameters, output: output, in: document,
                record: .step("refine_selection", arguments, note: "Select ▸ Select and Mask"))
            dismiss(self)
        } catch { status.stringValue = error.localizedDescription; document.presentError(error) }
    }

    @objc func cancelClicked(_ sender: Any?) { renderer.cancel(); dismiss(self) }
}
