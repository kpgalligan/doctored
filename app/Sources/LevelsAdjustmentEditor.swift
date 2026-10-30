import AppKit

// `LevelsAdjustmentEditor` is the Levels dialog in all three `AdjustmentSheetMode`s — Image ▸
// Adjustments ▸ Levels… (⌘L, `showLevels`) and the adjustment layer's create/edit
// (`AdjustmentLayerSheetController.make` routes `.levels` there; `opHasDialog` names it) — on
// `CurvesAdjustmentSheetController`'s lifecycle and per-mode commit: a Channel popup (hidden for a
// destructive scalar target), the histogram whose display follows the channel, input Black/White
// (`ClipTrackingSlider`, ⌥-drag clipping) and log-feel Gamma, Output Black/White,
// `FineToneControls(id: "levels")` over the current stage, the Auto row (create/edit only: Auto
// Tone / Auto Color, ids `levels-auto-tone`, `levels-auto-color`, `levels-auto-status`, disabled at
// float; on a Lab Color layer Auto Tone stretches Lightness alone and Auto Color is disabled,
// #547 Lab phase 2f — `LevelsAuto` over the scoped-analysis `auto`, whose scope the editor captures
// at init beside `Histogram.sheetPlan`) and Reset; the 8-bit destructive commit is
// `capture.adjustmentImage(op: "levels")` recorded as the flat `apply_filter` levels step, 16 bits
// the precision session.
//
// The editor's members `plot`, `precisionSession`, `renderer`, `blackLabel`, `whiteLabel`,
// `gammaLabel` and the ids `levels-black`, `levels-white`, `levels-after-histogram`,
// `tone.levels.*` are what `scripts/selected-tone-probe` and `native-selection-probe` read.

/// The Levels dialog, in all three of `AdjustmentSheetMode`'s modes: Image ▸
/// Adjustments ▸ Levels… (`.destructive`) and the adjustment layer's create
/// and edit dialogs. A Channel popup picks the stage — RGB (the
/// composite) or Red, Green, Blue — and the input Black/White/Gamma and the
/// Output Black/White sliders edit that stage; the core applies each
/// channel's stage before the composite's (`core/src/adjust_levels.rs`).
///
/// The lifecycle and the per-mode commit are `CurvesAdjustmentSheetController`'s
/// verbatim: the plane capture taken at init (destructive), the histogram of
/// the tones Apply rewrites (the layer, or the backdrop below an adjustment
/// layer), the captured in-context preview of the exact op Apply commits,
/// one undo step, Cancel = drop the preview — and at 16 bits the precision
/// session. The destructive commit is recorded as the flat `apply_filter`
/// levels step (`r`/`g`/`b` as objects beside the scalars).
///
/// A scalar target (a mask, a colour plane, an alpha channel) has one gray
/// value per pixel, so the popup is hidden there and only the composite
/// stage is edited.
///
/// The create and edit modes also carry Auto Tone / Auto Color
/// (`LevelsAuto`): measured once per click over the backdrop the plot shows,
/// they replace all four stages; Apply commits them like any slider edit.
final class LevelsAdjustmentEditor: NSViewController {
    private let document: ImageDocument
    private weak var canvas: ImageCanvasView?
    private let mode: AdjustmentSheetMode
    private let baseDoc: RasterDocument?
    /// The destructive arm's plane capture (nil in `.create` / `.edit`).
    private let capture: OperationCapture?
    private let aboveIndex: Int
    private let renderer = PreviewRenderer()
    private let precisionSession: PrecisionAdjustmentSession?
    /// The ⌥-drag Black/White display and the After plot.
    private let feedback: ToneSheetFeedback
    var onCommitted: ((Int) -> Void)?

    /// The params Apply merges into: an edited layer's own (so keys this
    /// build does not read survive), a preset's, or none.
    private let original: [String: Any]
    private var stages: [LevelsChannel: LevelsStage]
    private var channel: LevelsChannel = .rgb
    /// A mask, colour-plane or channel target: the composite stage only.
    private let scalarTarget: Bool
    /// #547: a CMYK document — the CMYK, Cyan, Magenta, Yellow and Black
    /// stages on ink, with the histogram of L per ink, Auto and Auto Options,
    /// the three eyedroppers and CMYK presets (phase 5, `CmykToneTools.swift`).
    private let cmyk: Bool
    private let channels: [LevelsChannel]
    /// #547 phase 5: a CMYK document's histogram and its tool row.
    private var cmykHistogram: CmykHistogram?
    private var cmykTools: CmykToneControls?

    /// What the Auto buttons measure, captured at init beside the
    /// plot's own plan (`Histogram.sheetPlan`); nil in `.destructive`.
    private struct AutoScope {
        let source: RasterDocument
        let below: Int
        let insert: (anchor: Int, position: LayerInsertPosition)?
        let selection: CanvasSelection?
        let coverage: [UInt8]?
    }
    private let autoScope: AutoScope?

    private let channelPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let plot = HistogramView(frame: .zero)
    private let blackSlider = ClipTrackingSlider(value: 0, minValue: 0, maxValue: 0.99,
                                                 target: nil, action: nil)
    private let whiteSlider = ClipTrackingSlider(value: 1, minValue: 0.01, maxValue: 1,
                                                 target: nil, action: nil)
    // Log-feel gamma: the slider runs -1…1 and maps to 10^x, so 0.1, 1 and
    // 10 sit at the left edge, the centre and the right edge.
    private let gammaSlider = NSSlider(value: 0, minValue: -1, maxValue: 1, target: nil, action: nil)
    private let outputBlackSlider = NSSlider(value: 0, minValue: 0, maxValue: 1, target: nil, action: nil)
    private let outputWhiteSlider = NSSlider(value: 1, minValue: 0, maxValue: 1, target: nil, action: nil)
    private let blackLabel = NSTextField(labelWithString: "0.00")
    private let whiteLabel = NSTextField(labelWithString: "1.00")
    private let gammaLabel = NSTextField(labelWithString: "1.00")
    private let outputBlackLabel = NSTextField(labelWithString: "0.00")
    private let outputWhiteLabel = NSTextField(labelWithString: "1.00")
    private var fineControls: FineToneControls?
    private var didRequestInitialPreview = false
    /// The saved-preset row.
    private var presetControl: AdjustmentPresetControl?
    private lazy var autoToneButton = StickerButton(
        title: LevelsAutoMode.tone.displayName, style: .secondary, target: self,
        action: #selector(autoClicked(_:)))
    private lazy var autoColorButton = StickerButton(
        title: LevelsAutoMode.color.displayName, style: .secondary, target: self,
        action: #selector(autoClicked(_:)))
    private let autoStatus = NSTextField(labelWithString: "")

    /// `preset` prefills a `.create` dialog (Layer ▸ New Adjustment Layer ▸
    /// From Photoshop Preset…); it is ignored in every other mode and when it
    /// is not a levels payload.
    init(
        document: ImageDocument, canvas: ImageCanvasView,
        mode: AdjustmentSheetMode, preset: AdjustmentLayerPayload? = nil
    ) {
        self.document = document
        self.canvas = canvas
        self.mode = mode
        self.baseDoc = document.doc
        self.aboveIndex = document.activeLayerIndex
        let destructive: Bool
        switch mode {
        case .destructive:
            destructive = true
            capture = document.captureOperation(previewBase: canvas.channelDisplay?.baseKind)
            original = [:]
        case .create:
            destructive = false
            capture = nil
            original = preset?.op == AdjustmentLayerOp.levels.rawValue ? preset?.params ?? [:] : [:]
        case .edit(_, let payload):
            destructive = false
            capture = nil
            original = payload.params
        }
        stages = LevelsMeta.stages(in: original)
        precisionSession = PrecisionAdjustmentSession(document, canvas: canvas, mode: mode)
        let target = capture?.target ?? precisionSession?.histogramCapture.target
        scalarTarget = destructive && target.map { $0 != .layer } == true
        // A mask is gray coverage at every mode, so it keeps the gray channel, histogram and Auto.
        cmyk = document.doc?.isCmyk == true && !scalarTarget
        // A Lab layer lists Lightness, a and b (#547 Lab phase 2c).
        channels = LevelsChannel.cases(cmyk: cmyk,
                                       lab: document.doc?.isLab == true && !scalarTarget)
        channel = channels[0]
        let composite = canvas.channelDisplay?.replacesComposite != true
        let byteComposite = !destructive || (capture?.previewBase == .composite && capture?.target == .layer)
        feedback = ToneSheetFeedback(active: composite
            && (precisionSession.map(\.supportsCompositeFeedback) ?? byteComposite))
        autoScope = Self.autoScope(document, mode: mode, above: aboveIndex,
            source: precisionSession?.histogramCapture.facade ?? baseDoc,
            precisionSelection: precisionSession?.histogramCapture.selection,
            typed: precisionSession != nil)
        super.init(nibName: nil, bundle: nil)
        precisionSession?.feedback = feedback
        // The tones the sliders move, captured once and OFF the main thread
        // (`Histogram.loadForSheet`): the layer Apply rewrites, or the
        // backdrop below an adjustment layer rather than the composite that
        // already carries its correction. A CMYK document's is the core's
        // histogram of L per ink (`CmykToneControls`), read in `loadView`.
        if cmyk {
            let scope = CmykTone.scope(document, mode: mode, activeLayer: aboveIndex)
            cmykTools = CmykToneControls(document: document, canvas: canvas, scope: scope.request,
                                         coverage: scope.coverage)
        } else { Histogram.loadForSheet(
            document, mode: mode, destructiveLayer: destructive ? document.activeLayerIndex : nil,
            destructiveSource: capture?.source, precisionCapture: precisionSession?.histogramCapture
        ) { [weak self] bins in
            self?.plot.bins = bins
        } onFailure: { [weak self] reason in
            self?.plot.unavailableReason = reason
        } }
        renderer.onRender = { [weak self] cgImage in
            self?.feedback.present(cgImage, on: self?.canvas)
        }
    }

    /// The backdrop the plot counts (create: where the new layer lands, gated
    /// by the selection it captured — the typed capture's at 16 bits, the
    /// mode's bytes at 8; edit: below the layer, which brings its own mask).
    private static func autoScope(
        _ document: ImageDocument, mode: AdjustmentSheetMode, above: Int,
        source: RasterDocument?, precisionSelection: CanvasSelection?, typed: Bool
    ) -> AutoScope? {
        if case .destructive = mode { return nil }
        let plan = Histogram.sheetPlan(document, mode: mode, destructiveLayer: nil)
        guard let source,
              let below = LevelsAuto.backdrop(plan.source, layerCount: source.layerCount)
        else { return nil }
        guard case .create(let captured) = mode else {
            return AutoScope(source: source, below: below, insert: nil, selection: nil, coverage: nil)
        }
        return AutoScope(source: source, below: below, insert: (above, .above),
            selection: typed ? precisionSelection : nil, coverage: typed ? nil : captured)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("LevelsAdjustmentEditor does not support NSCoder")
    }

    override func loadView() {
        channelPopup.addItems(withTitles: channels.map { $0.title(lab: labLayer) })
        channelPopup.font = DS.sans(13)
        channelPopup.target = self
        channelPopup.action = #selector(channelChanged(_:))
        channelPopup.setAccessibilityIdentifier("levels-channel")
        let channelRow = NSStackView(views: [fieldLabel("Channel:"), channelPopup])
        channelRow.orientation = .horizontal
        channelRow.alignment = .centerY
        channelRow.spacing = 8
        channelRow.isHidden = scalarTarget || document.doc?.isGray == true

        plot.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            plot.widthAnchor.constraint(equalToConstant: 256),
            plot.heightAnchor.constraint(equalToConstant: 72),
        ])

        let input = sliderGrid([
            ("Black:", blackSlider, blackLabel),
            ("White:", whiteSlider, whiteLabel),
            ("Gamma:", gammaSlider, gammaLabel),
        ])
        let output = sliderGrid([
            ("Output black:", outputBlackSlider, outputBlackLabel),
            ("Output white:", outputWhiteSlider, outputWhiteLabel),
        ])
        for slider in [blackSlider, whiteSlider] {
            slider.onTrackingEnded = { [weak self] in self?.clipTrackingEnded() }
            if feedback.isActive { slider.toolTip = "⌥-drag to see clipped pixels" }
        }
        blackSlider.setAccessibilityIdentifier("levels-black")
        whiteSlider.setAccessibilityIdentifier("levels-white")
        outputBlackSlider.setAccessibilityIdentifier("levels-output-black")
        outputWhiteSlider.setAccessibilityIdentifier("levels-output-white")
        feedback.afterPlot.setAccessibilityIdentifier("levels-after-histogram")

        let fine = FineToneControls(
            id: "levels", native: document.doc.isHighDepth,
            parameters: [
                .init(key: "black", title: "Black", range: 0...1),
                .init(key: "white", title: "White", range: 0...1),
                .init(key: "gamma", title: "Gamma", range: 0.1...10, isCode: false),
                .init(key: "output_black", title: "Output black", range: 0...1),
                .init(key: "output_white", title: "Output white", range: 0...1),
            ],
            read: { [weak self] in self?.fineValues() ?? [] },
            write: { [weak self] in self?.fineEntered($0) ?? false })
        fineControls = fine

        let resetButton = StickerButton(
            title: "Reset", style: .secondary, target: self, action: #selector(resetClicked(_:)))

        autoToneButton.setAccessibilityIdentifier("levels-auto-tone")
        autoColorButton.setAccessibilityIdentifier("levels-auto-color")
        autoStatus.setAccessibilityIdentifier("levels-auto-status")
        autoStatus.font = DS.sans(12)
        autoStatus.textColor = DS.textMuted
        let canAuto = document.doc?.isFloat != true && !cmyk
        // A Lab Color layer (#547 Lab phase 2f, `scoped_auto`): Auto Tone stretches Lightness
        // alone; Auto Color refuses.
        let lab = document.doc?.isLab == true
        autoToneButton.isEnabled = canAuto
        autoColorButton.isEnabled = canAuto && !lab
        let what = autoScope?.insert != nil
            ? "the backdrop below the new layer, within the selection that becomes its mask"
            : "the backdrop below this layer, within its mask"
        let points = lab ? "Lightness" : "Per-channel"
        autoToneButton.toolTip = canAuto ? "\(points) black and white points from \(what)"
            : "Auto is unavailable in a 32-bit document"
        autoColorButton.toolTip = lab
            ? "Auto Color is unavailable in a Lab Color document."
            : canAuto ? "Auto Tone plus neutral midtones, from \(what)" : autoToneButton.toolTip
        let autoRow = NSStackView(views: [autoToneButton, autoColorButton, autoStatus])
        autoRow.orientation = .horizontal
        autoRow.alignment = .centerY
        autoRow.spacing = 8
        autoRow.isHidden = autoScope == nil || cmyk

        let presets = AdjustmentPresetControl(
            op: .levels, cmyk: cmyk, current: { [weak self] in self?.params ?? [:] },
            load: { [weak self] in self?.loadPreset($0) })
        presetControl = presets
        // #547 phase 5: a CMYK document's Auto, Options… and eyedroppers, every mode.
        cmykTools?.onAuto = { [weak self] in self?.cmykAuto($0) }
        cmykTools?.onEyedropper = { [weak self] in self?.cmykEyedropper($0, sample: $1, target: $2) }
        let cmykRow = cmykTools?.row ?? NSStackView()
        let cmykStatus = cmykTools?.statusLabel ?? NSTextField(labelWithString: "")
        cmykRow.isHidden = !cmyk
        cmykStatus.isHidden = !cmyk
        // No After plot on a CMYK document: its ⌥ display is the ink's own clipping.
        let after = feedback.afterView()
        if cmyk { after.isHidden = true }
        let content = NSStackView(views: [
            presets.row, channelRow, plot, after, input, output, fine, autoRow,
            cmykRow, cmykStatus, resetButton,
        ])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        if cmyk {
            cmykHistogram = cmykTools?.loadHistogram()
            if cmykHistogram == nil { plot.unavailableReason = cmykTools?.statusLabel.stringValue }
        }
        showStage()

        let title: String
        switch mode {
        case .destructive: title = AdjustmentLayerOp.levels.displayName
        case .create: title = "New \(AdjustmentLayerOp.levels.displayName) layer"
        case .edit: title = "\(AdjustmentLayerOp.levels.displayName) options"
        }
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: title, content: precisionSession?.content(content) ?? content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote(feedback.isActive
                    ? "gamma > 1 brightens · ⌥-drag Black/White shows clipping · one undo"
                    : "gamma > 1 brightens · one undo")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        // The defaults are the identity, but the preview still runs once so a
        // plane target shows its plane and an edited layer its own result.
        guard !didRequestInitialPreview else { return }
        didRequestInitialPreview = true
        requestPreview()
    }

    private func sliderGrid(_ rows: [(String, NSSlider, NSTextField)]) -> NSView {
        AdjustmentSheet.grid(rows.map { label, slider, readout in
            slider.isContinuous = true
            slider.target = self
            slider.action = #selector(sliderChanged(_:))
            slider.widthAnchor.constraint(equalToConstant: 180).isActive = true
            readout.font = DS.mono(12)
            readout.textColor = DS.textMuted
            readout.alignment = .right
            readout.widthAnchor.constraint(equalToConstant: 52).isActive = true
            return [fieldLabel(label), slider, readout]
        })
    }

    private var stage: LevelsStage {
        get { stages[channel.slot] ?? .identity }
        set { stages[channel.slot] = newValue }
    }

    /// What the preview runs and Apply commits.
    private var params: [String: Any] { LevelsMeta.merged(stages, into: original) }

    /// A saved preset (or Default) replaces all four stages.
    private func loadPreset(_ payload: AdjustmentLayerPayload) {
        stages = LevelsMeta.stages(in: payload.params)
        showStage()
        requestPreview()
    }

    /// A Lab Color layer's dialog (#547 Lab phase 2c): Lightness, a and b; a mask keeps its gray.
    private var labLayer: Bool { document.doc?.isLab == true && !scalarTarget }

    /// The current stage onto the sliders, readouts, fine fields and plot.
    private func showStage() {
        let stage = self.stage
        blackSlider.doubleValue = stage.black
        whiteSlider.doubleValue = stage.white
        gammaSlider.doubleValue = log10(stage.gamma)
        outputBlackSlider.doubleValue = stage.outputBlack
        outputWhiteSlider.doubleValue = stage.outputWhite
        // A Lab layer plots the plane of the codes that holds the stage (Lightness red, a green,
        // b blue); a mask keeps its gray plot.
        plot.display = labLayer && channel == .rgb ? .red : channel.histogramDisplay
        if let cmykHistogram { plot.bins = cmykHistogram.bins(for: channel.rawValue, inkAxis: false) }
        updateValueLabels()
        fineControls?.refresh()
    }

    private func updateValueLabels() {
        let stage = self.stage
        blackLabel.stringValue = String(format: "%.2f", stage.black)
        whiteLabel.stringValue = String(format: "%.2f", stage.white)
        gammaLabel.stringValue = String(format: "%.2f", stage.gamma)
        outputBlackLabel.stringValue = String(format: "%.2f", stage.outputBlack)
        outputWhiteLabel.stringValue = String(format: "%.2f", stage.outputWhite)
    }

    private func fineValues() -> [Double] {
        let stage = self.stage
        return [stage.black, stage.white, stage.gamma, stage.outputBlack, stage.outputWhite]
    }

    /// A fine entry, in the stage's own units; the input points must stay
    /// ordered (the FineToneControls message says so).
    private func fineEntered(_ values: [Double]) -> Bool {
        guard values.count == 5, values[0] < values[1] else { return false }
        var entered = LevelsStage()
        entered.black = values[0]
        entered.white = values[1]
        entered.gamma = values[2]
        entered.outputBlack = values[3]
        entered.outputWhite = values[4]
        stage = entered
        showStage()
        presetControl?.markCustom()
        requestPreview()
        return true
    }

    private func requestPreview() {
        let params = self.params
        if let precisionSession { precisionSession.request(op: .levels, params: params); return }
        guard let baseDoc else { return }
        let mode = mode
        let above = aboveIndex
        let name = AdjustmentLayerOp.levels.displayName
        let clip = feedback.clip, box = feedback.isActive ? feedback.box : nil
        if case .destructive = mode {
            guard let capture else { return }
            renderer.request {
                capture.preview({ capture.adjustmentImage($0, op: "levels", params: params) },
                                clip: clip, box: box)
            }
            return
        }
        guard let meta = AdjustmentLayerPayload(op: .levels, params: params).json() else { return }
        renderer.request {
            let previewDoc: RasterDocument?
            switch mode {
            case .create(let selection):
                previewDoc = baseDoc.addingAdjustmentLayer(
                    active: above, name: name, meta: meta, selection: selection)
            case .edit(let idx, _):
                previewDoc = baseDoc.withLayerMeta(idx, meta)
            case .destructive:
                previewDoc = nil
            }
            return ToneSheetFeedback.byteFrame(previewDoc?.flattened(), colorSpace: baseDoc.colorSpace,
                                               clip: clip, box: box)
        }
    }

    @objc private func sliderChanged(_ sender: Any?) {
        let source = sender as AnyObject
        var stage = self.stage
        if source === blackSlider {
            stage.black = blackSlider.doubleValue
            // Keep black < white by pushing the OTHER point along.
            if stage.white <= stage.black { stage.white = min(stage.black + 0.01, 1) }
        } else if source === whiteSlider {
            stage.white = whiteSlider.doubleValue
            if stage.black >= stage.white { stage.black = max(stage.white - 0.01, 0) }
        } else if source === gammaSlider {
            stage.gamma = pow(10, gammaSlider.doubleValue)
        } else if source === outputBlackSlider {
            stage.outputBlack = outputBlackSlider.doubleValue
        } else if source === outputWhiteSlider {
            stage.outputWhite = outputWhiteSlider.doubleValue
        }
        self.stage = stage
        blackSlider.doubleValue = stage.black
        whiteSlider.doubleValue = stage.white
        // White ⌥-drag shows highlights, Black shadows; the rest none.
        let event = NSApp.currentEvent
        feedback.setClip(source === whiteSlider ? ClippingDrag.active(.highlights, event: event)
            : source === blackSlider ? ClippingDrag.active(.shadows, event: event) : nil)
        updateValueLabels()
        fineControls?.refresh()
        presetControl?.markCustom()
        requestPreview()
    }

    private func clipTrackingEnded() {
        if feedback.setClip(nil) { requestPreview() }
    }

    @objc private func channelChanged(_ sender: Any?) {
        let index = channelPopup.indexOfSelectedItem
        guard channels.indices.contains(index) else { return }
        channel = channels[index]
        // Switching stages changes nothing in the params — no new preview.
        showStage()
    }

    /// Auto Tone / Auto Color — the derived stages replace all four,
    /// the shown channel refreshes and the preview reruns. Dialog state only;
    /// Apply records the numbers.
    @objc private func autoClicked(_ sender: Any?) {
        guard let scope = autoScope else { return }
        let mode: LevelsAutoMode = (sender as AnyObject) === autoColorButton ? .color : .tone
        do {
            guard let derived = try LevelsAuto.derive(
                source: scope.source, below: scope.below, insert: scope.insert,
                selection: scope.selection, coverage: scope.coverage, mode: mode)
            else {
                NSSound.beep()
                autoStatus.stringValue = LevelsAuto.nothingToMeasure
                return
            }
            autoStatus.stringValue = ""
            stages = derived
            showStage()
            presetControl?.markCustom()
            requestPreview()
        } catch is JobCancelled {
            return
        } catch {
            autoStatus.stringValue = error.localizedDescription
        }
    }

    /// #547 phase 5: Auto on a CMYK document — the derived stages replace every stage.
    private func cmykAuto(_ auto: [String: Any]) {
        guard let derived = CmykToneTools.levelsStages(fromAuto: auto) else { NSSound.beep(); return }
        stages = derived
        showStage()
        presetControl?.markCustom()
        requestPreview()
    }

    /// #547 phase 5: an eyedropper click on a CMYK document — the per-ink stages move.
    private func cmykEyedropper(_ kind: CmykEyedropper, sample: [Double], target: [Double]) {
        stages = CmykToneTools.levelsStages(kind, sample: sample, target: target, onto: stages)
        showStage()
        presetControl?.markCustom()
        requestPreview()
    }

    override func viewWillDisappear() {
        super.viewWillDisappear()
        cmykTools?.stop()
    }

    /// Resets the CURRENT stage to identity; the others keep their values.
    @objc private func resetClicked(_ sender: Any?) {
        stage = .identity
        showStage()
        presetControl?.markCustom()
        requestPreview()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard fineControls?.accept() != false else { return }
        let params = self.params
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
        // Closing the dialog with an unchanged adjustment must not register
        // an undo step or dirty the file, at either depth.
        if case .edit(_, let payload) = mode, LevelsMeta.sameEffect(params, payload.params) {
            precisionSession?.cancel()
            return
        }
        if let precisionSession {
            precisionSession.commit(op: .levels, params: params, onCommitted: onCommitted)
            return
        }
        let name = AdjustmentLayerOp.levels.displayName
        switch mode {
        case .destructive:
            // The DESTRUCTIVE half is `apply_filter` "levels" with its
            // arguments flat — the shape `levelsFilterParams` reads back on
            // replay.
            let capture = capture
            document.applyCapturedOperation(capture, name,
                record: .filter("levels", params, target: capture?.agentTarget ?? "layer")
            ) {
                capture?.adjustmentImage($0, op: "levels", params: params)
            }
        case .create(let selection):
            guard let meta = AdjustmentLayerPayload(op: .levels, params: params).json() else {
                NSSound.beep()
                return
            }
            // Compose on the document's CURRENT doc and active layer, not the
            // captured preview base, so any edit that slipped in while the
            // dialog was open (an agent's) survives.
            let below = document.activeLayerIndex
            let before = document.doc
            // An active GROUP takes the new entry as its top child,
            // so the core answers where it went (§4.5).
            let landing = before?.newEntryLandingIndex(active: below) ?? below + 1
            document.applyEdit(
                "New \(name) Layer", record: .addAdjustmentLayer(
                    meta: meta, name: name,
                    position: before?.newEntryPosition(active: below) ?? .above)
            ) {
                $0.addingAdjustmentLayer(
                    active: below, name: name, meta: meta, selection: selection)
            }
            guard document.doc !== before, let doc = document.doc else { return }
            onCommitted?(min(landing, doc.layerCount - 1))
        case .edit(let idx, _):
            guard let meta = AdjustmentLayerPayload(op: .levels, params: params).json() else {
                NSSound.beep()
                return
            }
            document.applyEdit(
                "Edit \(name) Layer",
                record: .editAdjustmentLayer(meta: meta, layer: document.doc?.layerRef(idx))
            ) { doc in
                // The layer must still BE an adjustment layer (only an agent
                // edit can move the stack under an open dialog).
                guard doc.layerIsAdjustment(idx) else { return nil }
                return doc.withLayerMeta(idx, meta)
            }
            onCommitted?(idx)
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        precisionSession?.cancel()
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}
