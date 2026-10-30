import AppKit

/// Select > Color Range. Preview is candidate coverage before the requested
/// combination; the captured document/selection is reused and checked on Apply.
/// The Select popup lists, in order, Sampled Colors, the six hue presets,
/// Highlights, Midtones, Shadows, Skin Tones, Out Of Gamut, then Custom
/// Lightness; the dialog's values open as the last Apply left them
/// (`ColorRangeDialogOptions`).
final class ColorRangeSheetController: NSViewController {
    private weak var editor: EditorViewController?
    let capture: SelectionCapture
    private let sourceView = SelectionPreviewView(frame: .zero)
    private let maskView = SelectionPreviewView(frame: .zero)
    private let ranges = ["sampled"] + ColorRangeParameters.hueRanges
        + ["highlights", "midtones", "shadows", ColorRangeParameters.skinTones,
           ColorRangeGamut.range, "tonal"]
    private let rangePopup = NSPopUpButton()
    private let modePopup = NSPopUpButton()
    private var samples: [[Int]] = []
    private var samples16: [[Int]] = []
    /// The canvas pixel each sample was taken at, parallel to the samples
    /// (Localized Color Clusters' points).
    private var points: [[Int]] = []
    private let invertBox = NSButton(checkboxWithTitle: "Invert", target: nil, action: nil)
    private let localizedBox = NSButton(
        checkboxWithTitle: "Localized Color Clusters", target: nil, action: nil)
    private var clusterRangeField: NSTextField!
    private let sampleLabel = NSTextField(labelWithString: "Click the captured image to sample a color.")
    private let status = NSTextField(wrappingLabelWithString: "")
    private var toleranceField: NSTextField!
    private var softnessField: NSTextField!
    private var lowerField: NSTextField!
    private var upperField: NSTextField!
    /// Skin Tones' Fuzziness (0–200), enabled only for Skin Tones.
    private var fuzzinessField: NSTextField!
    private var applyButton: NSButton!
    private let renderer = PreviewRenderer()
    private var previewToken: RasterJobControl?
    private var previewGeneration = 0
    private let previewStamp = RefinementPreviewStamp()

    init(editor: EditorViewController, document: ImageDocument) throws {
        self.editor = editor
        capture = try SelectionCapture(document: document)
        super.init(nibName: nil, bundle: nil)
        sourceView.image = capture.source?.makeCGImage(in: capture.doc.colorSpace)
        sourceView.onPoint = { [weak self] point in self?.sample(point) }
        renderer.onRender = { [weak self] image in
            guard let self, self.previewStamp.value == self.previewGeneration else { return }
            guard let document = self.editor?.document, self.capture.isCurrent(in: document) else {
                self.maskView.image = nil; self.applyButton?.isEnabled = false
                self.status.stringValue = "The image, target or selection changed. Reopen Color Range."
                return
            }
            self.maskView.image = image
        }
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("ColorRangeSheetController does not support NSCoder") }

    override func loadView() {
        let stored = ToolOptionsStore.shared.colorRangeDialog
        rangePopup.addItems(withTitles: ["Sampled Colors", "Reds", "Yellows", "Greens", "Cyans", "Blues",
                                         "Magentas", "Highlights", "Midtones", "Shadows", "Skin Tones",
                                         "Out Of Gamut", "Custom Lightness"])
        rangePopup.identifier = NSUserInterfaceItemIdentifier("colorRange.range")
        rangePopup.target = self
        rangePopup.action = #selector(rangeChanged(_:))
        rangePopup.selectItem(at: ranges.firstIndex(of: stored.range) ?? 0)
        modePopup.addItems(withTitles: ["Replace", "Add", "Subtract", "Intersect"])
        modePopup.identifier = NSUserInterfaceItemIdentifier("colorRange.mode")
        modePopup.selectItem(at: (0..<4).contains(stored.modeIndex) ? stored.modeIndex : 0)
        toleranceField = selectionSheetField(stored.tolerance, identifier: "colorRange.tolerance", target: self, action: #selector(valueChanged(_:)))
        softnessField = selectionSheetField(stored.softness, identifier: "colorRange.softness", target: self, action: #selector(valueChanged(_:)))
        lowerField = selectionSheetField(stored.lower, identifier: "colorRange.lower", target: self, action: #selector(valueChanged(_:)))
        upperField = selectionSheetField(stored.upper, identifier: "colorRange.upper", target: self, action: #selector(valueChanged(_:)))
        clusterRangeField = selectionSheetField(stored.clusterRange, identifier: "colorRange.clusterRange", target: self, action: #selector(valueChanged(_:)))
        fuzzinessField = selectionSheetField(stored.fuzziness, identifier: "colorRange.fuzziness", target: self, action: #selector(valueChanged(_:)))
        for (box, id, on) in [(invertBox, "colorRange.invert", stored.invert),
                              (localizedBox, "colorRange.localized", stored.localized)] {
            box.identifier = NSUserInterfaceItemIdentifier(id)
            box.target = self
            box.action = #selector(valueChanged(_:))
            box.state = on ? .on : .off
        }
        let grid = NSGridView(views: [
            [fieldLabel("Select:"), rangePopup, fieldLabel("Combine:"), modePopup],
            [fieldLabel("Tolerance ΔE:"), toleranceField, fieldLabel("Softness:"), softnessField],
            [fieldLabel("Lower L*:"), lowerField, fieldLabel("Upper L*:"), upperField],
            [invertBox, localizedBox, fieldLabel("Range %:"), clusterRangeField],
            [fieldLabel("Fuzziness:"), fuzzinessField, NSGridCell.emptyContentView,
             NSGridCell.emptyContentView],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 10
        let clear = StickerButton(title: "Clear Samples", style: .secondary,
                                  target: self, action: #selector(clearSamples(_:)))
        let samplesRow = NSStackView(views: [sampleLabel, clear])
        samplesRow.orientation = .horizontal
        sampleLabel.font = DS.mono(11)
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 510
        let content = NSStackView(views: [selectionPreviewRow(sourceView, maskView), samplesRow, grid, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(title: "Color Range",
            hint: "Sample up to 32 colors from the image, or choose a hue preset, a lightness range, Skin Tones (Lab ΔE76 within 18 of a skin colour, then a soft edge 0.25 × Fuzziness wide) or Out Of Gamut (what View ▸ Gamut Warning flags for the window's proof profile). Tolerance uses Lab ΔE76 in the document profile. Localized Color Clusters weights each sample by distance from where it was taken, within Range % of the longer side. The preview shows candidate coverage before combining with the existing selection.",
            content: content, buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: applyButton), width: 556)
        updateControls()
    }

    var parameters: ColorRangeParameters {
        let range = ranges[max(0, rangePopup.indexOfSelectedItem)]
        return ColorRangeParameters(range: range, samples: samples,
            samples16: capture.doc.isHighDepth ? samples16 : nil,
            tolerance: Double(toleranceField.stringValue) ?? .nan,
            softness: Double(softnessField.stringValue) ?? .nan,
            lower: Double(lowerField.stringValue) ?? .nan, upper: Double(upperField.stringValue) ?? .nan,
            invert: invertBox.state == .on,
            localized: range == "sampled" && localizedBox.state == .on,
            clusterRange: Double(clusterRangeField.stringValue) ?? .nan, points: points,
            fuzziness: Double(fuzzinessField.stringValue) ?? .nan)
    }

    /// Appends one (colour, point) pair; an exact repeat of both is skipped.
    private func appendSample(_ value: [Int], at point: CGPoint, to list: inout [[Int]]) {
        let pixel = [Int(point.x), Int(point.y)]
        guard !zip(list, points).contains(where: { $0 == value && $1 == pixel }) else { return }
        list.append(value)
        points.append(pixel)
    }

    func sample(_ point: CGPoint) {
        if capture.doc.isHighDepth {
            do {
                guard samples16.count < 32 else { return }
                // The retained projection of the captured document, never
                // another document's. A float document reads its own
                // render: the sample reply's `center_display_codes` is the R1
                // display copy Color Range compares.
                let float = capture.doc.isFloat
                let read = try ScopedAnalysisRead(source: capture.doc, scope: .composite, selection: nil,
                    point: (Int(point.x), Int(point.y)),
                    projection: float ? nil : editor?.document.flatMap {
                        capture.isCurrent(in: $0) ? $0.retainedPrecisionProjection : nil
                    })
                let report = try read.run()
                let key = float ? "center_display_codes" : "center_rgba_codes"
                guard let rgba = report[key] as? [NSNumber], rgba.count == 4, rgba[3].intValue > 0 else { NSSound.beep(); return }
                let value = rgba.prefix(3).map(\.intValue)
                appendSample(value, at: point, to: &samples16)
                rangePopup.selectItem(at: 0)
                let codes = capture.doc.isLab ? "L*a*b* codes" : "native RGB"
                sampleLabel.stringValue = "\(samples16.count) color(s); \(codes) \(value[0]), \(value[1]), \(value[2])"
                requestPreview()
            } catch { status.stringValue = error.localizedDescription }
            return
        }
        guard samples.count < 32, let rgba = capture.source?.pixelRGBA(x: Int(point.x), y: Int(point.y)),
              rgba.a > 0 else { NSSound.beep(); return }
        let value = [Int(rgba.r), Int(rgba.g), Int(rgba.b)]
        appendSample(value, at: point, to: &samples)
        rangePopup.selectItem(at: 0)
        sampleLabel.stringValue = "\(samples.count) color(s); last RGB \(value[0]), \(value[1]), \(value[2])"
        requestPreview()
    }

    private func updateControls() {
        let range = ranges[max(0, rangePopup.indexOfSelectedItem)]
        toleranceField.isEnabled = range == "sampled"
        softnessField.isEnabled = !ColorRangeParameters.hueRanges.contains(range)
            && range != ColorRangeGamut.range && range != ColorRangeParameters.skinTones
        fuzzinessField.isEnabled = range == ColorRangeParameters.skinTones
        lowerField.isEnabled = range == "tonal"
        upperField.isEnabled = range == "tonal"
        localizedBox.isEnabled = range == "sampled"
        clusterRangeField.isEnabled = range == "sampled" && localizedBox.state == .on
        applyButton.isEnabled = (try? parameters.validated()) != nil
    }

    func requestPreview() {
        previewGeneration += 1
        let generation = previewGeneration, stamp = previewStamp
        maskView.image = nil
        updateControls()
        previewToken?.cancel()
        guard let parameters = try? parameters.validated(), let source = capture.source else {
            maskView.image = nil
            status.stringValue = "Choose valid values and sample at least one color for Sampled Colors."
            return
        }
        do {
            if parameters.isOutOfGamut {
                try requestOutOfGamutPreview(parameters, generation: generation, stamp: stamp)
                return
            }
            if let source = capture.doc.precision {
                let work = try NativeSelectionWork(source: source, width: capture.doc.width, height: capture.doc.height,
                    operation: ["kind": "color_range", "params": parameters.nativeCoreParameters])
                let reservation = try SelectionWorkBudget.reservePreview(bytes: work.scratchEstimate + UInt64(work.width * work.height) * 4,
                    outputBytes: UInt64(work.width * work.height) * 4)
                let token = reservation.control; previewToken = token; renderer.cancelCompute = { token.cancel() }
                let own = work.scratchEstimate + UInt64(work.width * work.height) * 4
                renderer.request { reservation.run { token in
                    guard let mask = try? work.compute(control: token, ownReservation: own), !token.isCancelled else { return nil }
                    stamp.set(generation)
                    return CanvasSelection.grayImage(mask.map { UInt8((UInt32($0) + 128) / 257) }, work.width, work.height)
                } }
                status.stringValue = "Native samples and 65,536-level coverage; this mask image is a display preview."
                return
            }
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: UInt64(capture.doc.width * capture.doc.height) * 6 + 262_144,
                outputBytes: UInt64(capture.doc.width * capture.doc.height))
            let token = reservation.control
            previewToken = token
            renderer.cancelCompute = { token.cancel() }
            let doc = capture.doc, core = parameters.coreParameters
            renderer.request { reservation.run { token in
                guard let mask = try? doc.colorRange(source: source, parameters: core, control: token),
                      !token.isCancelled else { return nil }
                stamp.set(generation)
                return CanvasSelection.grayImage(mask, doc.width, doc.height)
            } }
            status.stringValue = "White selects fully; gray selects partially. Transparent pixels remain outside."
        } catch { maskView.image = nil; applyButton.isEnabled = false; status.stringValue = error.localizedDescription }
    }

    /// Out Of Gamut's preview: the final's own input and core call, reserved through
    /// `SelectionWorkBudget` and run as its own lease, since the export charges its scratch.
    private func requestOutOfGamutPreview(_ parameters: ColorRangeParameters, generation: Int,
                                          stamp: RefinementPreviewStamp) throws {
        guard let document = editor?.document else { return }
        try capture.requireCurrent(in: document)
        let setup = try ColorRangeGamut.setup(window: editor?.proofDisplay.setup)
        let input = try ColorRangeGamutInput(capture: capture, document: document, setup: setup)
        let width = capture.doc.width, height = capture.doc.height, invert = parameters.invert
        let reservation = try SelectionWorkBudget.reservePreview(
            bytes: input.reservationBytes, outputBytes: UInt64(width * height))
        let token = reservation.control
        previewToken = token
        renderer.cancelCompute = { token.cancel() }
        renderer.request { reservation.runAsOwnLease { token in
            guard case .mask(let mask)? = try? input.coverage(invert: invert, control: token),
                  !token.isCancelled else { return nil }
            stamp.set(generation)
            return CanvasSelection.grayImage(mask, width, height)
        } }
        status.stringValue = input.isCmyk
            ? "Proof profile: \(setup.profileName). A CMYK document is all in gamut, so nothing is selected."
            : "Proof profile: \(setup.profileName). White is out of gamut, as View ▸ Gamut Warning flags it."
    }

    @objc private func rangeChanged(_ sender: Any?) { requestPreview() }
    @objc private func valueChanged(_ sender: Any?) { requestPreview() }
    @objc private func clearSamples(_ sender: Any?) {
        samples = []; samples16 = []; points = []
        sampleLabel.stringValue = "Click the captured image to sample a color."
        requestPreview()
    }
    @objc func applyClicked(_ sender: Any?) {
        guard let parameters = try? parameters.validated() else { NSSound.beep(); return }
        let mode: SelectionCombineMode = [.replace, .add, .subtract, .intersect][max(0, modePopup.indexOfSelectedItem)]
        saveDialogOptions()
        renderer.cancel()
        dismiss(self)
        editor?.applyColorRange(capture, parameters: parameters, mode: mode)
    }
    @objc func cancelClicked(_ sender: Any?) { renderer.cancel(); dismiss(self) }

    /// The dialog's values for the next open; a disabled field left
    /// unparsable keeps its stored value.
    private func saveDialogOptions() {
        var options = ToolOptionsStore.shared.colorRangeDialog
        options.range = ranges[max(0, rangePopup.indexOfSelectedItem)]
        options.modeIndex = max(0, modePopup.indexOfSelectedItem)
        options.invert = invertBox.state == .on
        options.localized = localizedBox.state == .on
        let value = { (field: NSTextField, kept: Double) in Double(field.stringValue).flatMap { $0.isFinite ? $0 : nil } ?? kept }
        options.tolerance = value(toleranceField, options.tolerance)
        options.softness = value(softnessField, options.softness)
        options.lower = value(lowerField, options.lower)
        options.upper = value(upperField, options.upper)
        options.clusterRange = value(clusterRangeField, options.clusterRange)
        options.fuzziness = value(fuzzinessField, options.fuzziness)
        ToolOptionsStore.shared.colorRangeDialog = options
    }
}
