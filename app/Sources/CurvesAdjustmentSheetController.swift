import AppKit

// The Curves controller (moved out of the frozen `Sheets.swift`) runs on `AdjustmentSheetMode`:
// `.create` / `.edit` are the adjustment-layer dialog, `.destructive` is `LevelsAdjustmentEditor`'s
// arm verbatim (plane capture, layer histogram, `capture.adjustmentImage(op: "curves")` preview and
// commit, recorded as `apply_filter` curves with the dialog's `params`; 16 bits through
// `PrecisionAdjustmentSession`). The Points | Pencil control (`curves-edit-mode`) switches every
// channel between control points and a drawn map (the map form,
// `CurvesMeta.maps`): Pencil converts each channel's curve to its map (`CurvePencil.map(fromPoints:)`),
// Points converts each map back (`CurvesMeta.points(fromMap:)`: 16 samples). In MAP STATE
// `CurveEditorView.map` draws the channel's 256 entries and the mouse draws into them, the fine
// controls hide, Smooth (`curves-smooth`) and the note show, Reset resets the channel's map, and
// Apply commits the maps in every arm, on RGB and on a CMYK document's ink.
//
// Levels… is ⌘L; Curves… is ⌘M.

/// The Curves dialog, in all three of `AdjustmentSheetMode`'s modes: the
/// `.curves` branch of the adjustment-layer dialog seam (create and edit)
/// and Image ▸ Adjustments ▸ Curves… (`.destructive`). Identical
/// lifecycle to AdjustmentLayerSheetController — captured-handle live
/// preview of the exact op Apply will commit, Apply = one undo step, Cancel
/// = drop the preview — but the content is a channel popup +
/// CurveEditorView instead of slider rows. Per-channel point lists live
/// here; the view only ever shows (and edits) the current channel's.
///
/// The destructive arm is `LevelsAdjustmentEditor`'s, verbatim: the plane
/// capture taken at init, the histogram of the tones Apply rewrites, the
/// captured preview and a commit recorded as the `apply_filter` curves step
/// (`params` holding the point lists) — at 16 bits through the precision
/// session, whose `.destructive` arm records the same step.
///
/// MAP STATE: a layer whose curve was drawn with the
/// pencil (`rgb_map`… or `cmyk_map`… params, `CurvesMeta.maps`) opens in it,
/// and Pencil enters it; each channel's map shows as a polyline the mouse
/// draws into, with the fine controls hidden and Smooth and a note in their
/// place. Apply commits the maps; Points converts every channel to control
/// points (`CurvesMeta.points(fromMap:)`) and the dialog continues as a
/// point curve. Auto and the eyedroppers leave map state (their answers are
/// points).
final class CurvesAdjustmentSheetController: NSViewController {
    private let document: ImageDocument
    private weak var canvas: ImageCanvasView?
    private let mode: AdjustmentSheetMode
    // In-context preview base: the chained ops are applied to this captured
    // handle and the flattened result is shown, so masks, blend modes and
    // the other layers stay visible while editing.
    private let baseDoc: RasterDocument?
    /// The destructive arm's plane capture (nil in `.create` / `.edit`).
    private let capture: OperationCapture?
    private let aboveIndex: Int
    private let renderer = PreviewRenderer()
    private let precisionSession: PrecisionAdjustmentSession?
    /// The ⌥-drag end-point display and the After plot.
    private let feedback: ToneSheetFeedback
    var onCommitted: ((Int) -> Void)?

    private var channelPoints: [CurveChannel: [CurvePoint]] = [:]
    /// Non-nil in map state: every channel's 256-entry map, as prefilled.
    private var channelMaps: [CurveChannel: [Int]]?
    private let mapNoteLabel = NSTextField(
        wrappingLabelWithString: "Drag to draw the curve; Points converts it to control points.")
    /// Points | Pencil.
    private let editModeControl = NSSegmentedControl(
        labels: ["Points", "Pencil"], trackingMode: .selectOne, target: nil, action: nil)
    private var smoothButton: StickerButton!
    private var resetButton: StickerButton!
    private var channel: CurveChannel = .rgb
    private let channelPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    /// A destructive edit of a mask, plane or channel writes one scalar, so
    /// only the RGB curve applies there: the Channel popup is hidden (as in
    /// Levels) and MCP refuses r/g/b on such a target.
    private let scalarTarget: Bool
    /// #547: a CMYK document — the CMYK, Cyan, Magenta, Yellow and Black
    /// curves on ink, axes in ink %, with the histogram on the ink axes, Auto
    /// and Auto Options, the eyedroppers and CMYK presets (phase 5,
    /// `CmykToneTools.swift`), and drawn curves on ink; Targeted
    /// Adjustment is not there yet.
    private let cmyk: Bool
    private let channels: [CurveChannel]
    /// #547 phase 5: a CMYK document's histogram and its tool row.
    private var cmykHistogram: CmykHistogram?
    private var cmykTools: CmykToneControls?
    private let curveView = CurveEditorView()
    private let readoutLabel = NSTextField(labelWithString: "")
    private var didRequestInitialPreview = false
    /// The saved-preset row.
    private var presetControl: AdjustmentPresetControl?
    /// Targeted Adjustment: the canvas monitor while the box is on, and
    /// the point a drag moves with its output when the press landed (`y` is
    /// the press in window points, `windowY`).
    private var monitor: CanvasPointerMonitor?
    private var dragStart: (y: CGFloat, index: Int, output: Double)?

    /// `preset` prefills a `.create` sheet (Layer ▸ New Adjustment Layer ▸
    /// From Photoshop Preset…); it is ignored in every other mode and when
    /// it is not a curves payload.
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
        if case .destructive = mode {
            destructive = true
            self.capture = document.captureOperation(previewBase: canvas.channelDisplay?.baseKind)
        } else {
            destructive = false
            self.capture = nil
        }
        // Edit mode prefills every channel from the layer's meta (missing
        // channels show identity); create mode starts from the preset when
        // one was opened, and everything else starts all-identity.
        var prefill: AdjustmentLayerPayload?
        switch mode {
        case .edit(_, let original): prefill = original
        case .create: prefill = preset?.op == AdjustmentLayerOp.curves.rawValue ? preset : nil
        case .destructive: prefill = nil
        }
        for ch in CurveChannel.allCases {
            if let prefill {
                channelPoints[ch] = CurvesMeta.points(of: ch, in: prefill)
            } else {
                channelPoints[ch] = CurvePoint.identity
            }
        }
        precisionSession = PrecisionAdjustmentSession(document, canvas: canvas, mode: mode)
        let target = capture?.target ?? precisionSession?.histogramCapture.target
        scalarTarget = destructive && target.map { $0 != .layer } == true
        // A mask is gray coverage at every mode, so its one curve is `rgb`.
        cmyk = document.doc?.isCmyk == true && !scalarTarget
        // A Lab layer lists Lightness, a and b (#547 Lab phase 2c).
        channels = CurveChannel.cases(cmyk: cmyk,
                                      lab: document.doc?.isLab == true && !scalarTarget)
        channel = channels[0]
        let isCmyk = cmyk
        channelMaps = prefill.flatMap { CurvesMeta.maps(in: $0, cmyk: isCmyk) }
        let composite = canvas.channelDisplay?.replacesComposite != true
        let byteComposite = !destructive || (capture?.previewBase == .composite && capture?.target == .layer)
        feedback = ToneSheetFeedback(active: composite
            && (precisionSession.map(\.supportsCompositeFeedback) ?? byteComposite))
        super.init(nibName: nil, bundle: nil)
        precisionSession?.feedback = feedback
        // The tones the curve moves, captured once and OFF the main thread:
        // the BACKDROP BELOW the layer for an adjustment layer, so the plot
        // shows what the curve acts on rather than the already-corrected
        // composite, and the LAYER being rewritten for the destructive arm.
        if cmyk {
            let scope = CmykTone.scope(document, mode: mode, activeLayer: aboveIndex)
            cmykTools = CmykToneControls(document: document, canvas: canvas, scope: scope.request,
                                         coverage: scope.coverage)
        } else {
            Histogram.loadForSheet(
                document, mode: mode, destructiveLayer: destructive ? document.activeLayerIndex : nil,
                destructiveSource: capture?.source, precisionCapture: precisionSession?.histogramCapture
            ) { [weak self] in self?.curveView.histogram = $0 }
                onFailure: { [weak self] in self?.curveView.histogramUnavailableReason = $0 }
        }
        renderer.onRender = { [weak self] cgImage in
            self?.feedback.present(cgImage, on: self?.canvas)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("CurvesAdjustmentSheetController does not support NSCoder")
    }

    override func loadView() {
        let lab = document.doc?.isLab == true && !scalarTarget
        channelPopup.addItems(withTitles: channels.map { $0.title(lab: lab) })
        curveView.inkAxes = cmyk
        channelPopup.font = DS.sans(13)
        channelPopup.target = self
        channelPopup.action = #selector(channelChanged(_:))
        channelPopup.setAccessibilityIdentifier("curves-channel")

        let channelRow = NSStackView(views: [fieldLabel("Channel:"), channelPopup])
        channelRow.orientation = .horizontal
        channelRow.alignment = .centerY
        channelRow.spacing = 8
        channelRow.isHidden = scalarTarget || document.doc?.isGray == true

        curveView.points = channelPoints[channel] ?? CurvePoint.identity
        curveView.map = channelMaps?[channel]
        curveView.curveColor = channel.strokeColor
        curveView.onPointsChanged = { [weak self] points in
            guard let self = self else { return }
            self.channelPoints[self.channel] = points
            self.presetControl?.markCustom()
            self.requestPreview()
        }
        curveView.onDragReadout = { [weak self] readout in
            self?.readoutLabel.stringValue = readout ?? ""
        }
        curveView.onClipDrag = { [weak self] in self?.clipDragged($0) }
        curveView.onMapChanged = { [weak self] in self?.mapDrawn($0) }
        feedback.afterPlot.setAccessibilityIdentifier("curves-after-histogram")
        // Pin the square explicitly so stack stretching can never skew the
        // plot's value mapping.
        let side = curveView.intrinsicContentSize.width
        NSLayoutConstraint.activate([
            curveView.widthAnchor.constraint(equalToConstant: side),
            curveView.heightAnchor.constraint(equalToConstant: side),
        ])

        // Live "in → out" readout during drags, in the options bar's numeric
        // style (monospaced digits, small).
        readoutLabel.font = NSFont.monospacedDigitSystemFont(
            ofSize: NSFont.smallSystemFontSize, weight: .regular)
        readoutLabel.textColor = DS.textMuted

        resetButton = StickerButton(
            title: "Reset", style: .secondary, target: self, action: #selector(resetClicked(_:)))
        smoothButton = StickerButton(
            title: "Smooth", style: .secondary, target: self, action: #selector(smoothClicked(_:)))
        smoothButton.setAccessibilityIdentifier("curves-smooth")
        editModeControl.target = self
        editModeControl.action = #selector(editModeChanged(_:))
        editModeControl.setAccessibilityIdentifier("curves-edit-mode")
        editModeControl.font = DS.sans(12)
        let modeRow = NSStackView(views: [fieldLabel("Edit:"), editModeControl])
        modeRow.orientation = .horizontal
        modeRow.alignment = .centerY
        modeRow.spacing = 8
        mapNoteLabel.font = DS.sans(NSFont.smallSystemFontSize)
        mapNoteLabel.textColor = DS.textMuted
        // The under-row is pinned to the plot's width; the note wraps in
        // what Smooth and Reset leave of it.
        mapNoteLabel.preferredMaxLayoutWidth = 150
        let spacer = NSView()
        spacer.setContentHuggingPriority(.defaultLow - 1, for: .horizontal)
        let underRow = NSStackView(
            views: [readoutLabel, mapNoteLabel, spacer, smoothButton, resetButton])
        underRow.orientation = .horizontal
        underRow.alignment = .centerY
        underRow.spacing = 8
        let fineControls = curveView.fineControls(native: document.doc.isHighDepth)
        showMapState(channelMaps != nil)

        let presets = AdjustmentPresetControl(
            op: .curves, cmyk: cmyk, current: { [weak self] in self?.currentParams() ?? [:] },
            load: { [weak self] in self?.loadPreset($0) })
        presetControl = presets
        let targeted = NSButton(
            checkboxWithTitle: "Targeted Adjustment", target: self,
            action: #selector(targetedChanged(_:)))
        targeted.font = DS.sans(13)
        // A float document has no 0…255 sample to place a point at; a CMYK
        // one reads ink, which the sampler does not give yet.
        targeted.isEnabled = document.doc?.isFloat != true && !cmyk
        // #547 phase 5: a CMYK document's Auto, Options… and eyedroppers, every mode.
        cmykTools?.onAuto = { [weak self] in self?.cmykAuto($0) }
        cmykTools?.onEyedropper = { [weak self] in self?.cmykEyedropper(sample: $1, target: $2) }
        let cmykRow = cmykTools?.row ?? NSStackView()
        let cmykStatus = cmykTools?.statusLabel ?? NSTextField(labelWithString: "")
        cmykRow.isHidden = !cmyk
        cmykStatus.isHidden = !cmyk
        // No After plot on a CMYK document: its ⌥ display is the ink's own clipping.
        let after = feedback.afterView()
        if cmyk { after.isHidden = true }
        let content = NSStackView(views: [
            presets.row, channelRow, modeRow, curveView, after, fineControls, underRow,
            cmykRow, cmykStatus, targeted,
        ])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        underRow.widthAnchor.constraint(equalTo: curveView.widthAnchor).isActive = true
        if cmyk {
            cmykHistogram = cmykTools?.loadHistogram()
            if cmykHistogram == nil {
                curveView.histogramUnavailableReason = cmykTools?.statusLabel.stringValue
            }
            showCmykHistogram()
        }

        let title: String
        switch mode {
        case .destructive: title = AdjustmentLayerOp.curves.displayName
        case .create: title = "New \(AdjustmentLayerOp.curves.displayName) layer"
        case .edit: title = "\(AdjustmentLayerOp.curves.displayName) options"
        }
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: title,
            hint: "Click the curve to add a point (Reset clears the current "
                + "channel); drag a point off the plot to remove it. Pencil draws the curve."
                + (feedback.isActive ? " ⌥-drag an end point to see clipping." : ""),
            content: precisionSession?.content(content) ?? content,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        // Same pattern as the slider sheets: preview immediately so what the
        // user sees is what Apply produces.
        guard !didRequestInitialPreview else { return }
        didRequestInitialPreview = true
        requestPreview()
    }

    /// Meta from the current point lists: only non-identity channels are
    /// included (missing channel = identity in the schema); all-identity is
    /// a valid curves op with empty params. In map state, the maps as
    /// prefilled (`CurvesMeta.params(maps:)`).
    private func metaJSON() -> String? {
        AdjustmentLayerPayload(op: .curves, params: currentParams()).json()
    }

    /// The params the dialog describes: the maps in map state, else the
    /// point lists.
    private func currentParams() -> [String: Any] {
        if let channelMaps { return CurvesMeta.params(maps: channelMaps, cmyk: cmyk) }
        return CurvesMeta.params(channelPoints)
    }

    /// A saved preset (or Default) into every channel — map state when
    /// it is a drawn curve — then a fresh preview.
    private func loadPreset(_ payload: AdjustmentLayerPayload) {
        for ch in CurveChannel.allCases {
            channelPoints[ch] = CurvesMeta.points(of: ch, in: payload)
        }
        channelMaps = CurvesMeta.maps(in: payload, cmyk: cmyk)
        curveView.points = channelPoints[channel] ?? CurvePoint.identity
        curveView.map = channelMaps?[channel]
        readoutLabel.stringValue = ""
        showMapState(channelMaps != nil)
        requestPreview()
    }

    /// Map state's controls: Pencil selected, the note and Smooth shown and
    /// the fine controls hidden — or the reverse. Reset shows in both.
    private func showMapState(_ mapState: Bool) {
        editModeControl.selectedSegment = mapState ? 1 : 0
        mapNoteLabel.isHidden = !mapState
        smoothButton.isHidden = !mapState
        curveView.fineControls(native: document.doc.isHighDepth).isHidden = mapState
    }

    /// Points | Pencil: every channel converts to the other form (type comment).
    @objc private func editModeChanged(_ sender: Any?) {
        let pencil = editModeControl.selectedSegment == 1
        guard pencil != (channelMaps != nil) else { return }
        if pencil {
            var maps: [CurveChannel: [Int]] = [:]
            for ch in channels {
                maps[ch] = CurvePencil.map(fromPoints: channelPoints[ch] ?? CurvePoint.identity)
            }
            channelMaps = maps
            curveView.map = maps[channel]
            showMapState(true)
            presetControl?.markCustom()
            requestPreview()
        } else {
            leaveMapState()
        }
    }

    /// The pencil drew into the current channel's map.
    private func mapDrawn(_ map: [Int]) {
        guard channelMaps != nil else { return }
        channelMaps?[channel] = map
        presetControl?.markCustom()
        requestPreview()
    }

    /// Smooth: the current channel's map through `CurvePencil.smoothed`.
    @objc private func smoothClicked(_ sender: Any?) {
        guard let map = channelMaps?[channel] else { return }
        let smoothed = CurvePencil.smoothed(map)
        guard smoothed != map else { return }
        channelMaps?[channel] = smoothed
        curveView.map = smoothed
        presetControl?.markCustom()
        requestPreview()
    }

    private func requestPreview() {
        if let precisionSession { precisionSession.request(meta: metaJSON()); return }
        guard let baseDoc = baseDoc else { return }
        let mode = mode
        let above = aboveIndex
        let name = AdjustmentLayerOp.curves.displayName
        let clip = feedback.clip, box = feedback.isActive ? feedback.box : nil
        if case .destructive = mode {
            guard let capture = capture else { return }
            let params = currentParams()
            renderer.request {
                capture.preview({ capture.adjustmentImage($0, op: "curves", params: params) },
                                clip: clip, box: box)
            }
            return
        }
        guard let meta = metaJSON() else { return }
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

    /// An end point ⌥-dragged on the curve (CurveEditorView.onClipDrag).
    private func clipDragged(_ mode: ClipDisplayMode?) {
        var mode = mode
        // On the ink axes (#547) the left end point is bare paper — the highlights.
        if cmyk { mode = mode == .shadows ? .highlights : mode == .highlights ? .shadows : mode }
        if feedback.setClip(mode) { requestPreview() }
    }

    @objc private func channelChanged(_ sender: Any?) {
        let index = channelPopup.indexOfSelectedItem
        guard channels.indices.contains(index) else { return }
        channel = channels[index]
        curveView.points = channelPoints[channel] ?? CurvePoint.identity
        curveView.map = channelMaps?[channel]
        curveView.curveColor = channel.strokeColor
        readoutLabel.stringValue = ""
        showCmykHistogram()
        // Switching channels changes nothing in the meta — no new preview.
    }

    /// #547 phase 5: the current ink's histogram on the ink axes.
    private func showCmykHistogram() {
        guard let cmykHistogram else { return }
        curveView.histogram = cmykHistogram.bins(for: channel.rawValue, inkAxis: true)
    }

    /// #547 phase 5: Auto on a CMYK document — the derived curves replace every channel.
    private func cmykAuto(_ auto: [String: Any]) {
        guard let derived = CmykToneTools.curvePoints(fromAuto: auto) else { NSSound.beep(); return }
        dropMaps()
        for ch in channels { channelPoints[ch] = derived[ch] ?? CurvePoint.identity }
        curveView.points = channelPoints[channel] ?? CurvePoint.identity
        presetControl?.markCustom()
        requestPreview()
    }

    /// #547 phase 5: an eyedropper click on a CMYK document — a point per ink.
    private func cmykEyedropper(sample: [Double], target: [Double]) {
        if channelMaps != nil { pointsFromMaps() }
        channelPoints = CmykToneTools.curvePoints(sample: sample, target: target, onto: channelPoints)
        curveView.points = channelPoints[channel] ?? CurvePoint.identity
        presetControl?.markCustom()
        requestPreview()
    }

    /// Resets the CURRENT channel to identity — its map in map state; the
    /// others keep theirs.
    @objc private func resetClicked(_ sender: Any?) {
        if channelMaps != nil {
            channelMaps?[channel] = CurvesMeta.identityMap
            curveView.map = CurvesMeta.identityMap
            presetControl?.markCustom()
            requestPreview()
            return
        }
        channelPoints[channel] = CurvePoint.identity
        curveView.points = CurvePoint.identity
        presetControl?.markCustom()
        requestPreview()
    }

    // MARK: - Targeted Adjustment

    @objc private func targetedChanged(_ sender: Any?) {
        monitor?.stop()
        monitor = nil
        dragStart = nil
        guard (sender as? NSButton)?.state == .on, let canvas else { return }
        monitor = CanvasPointerMonitor(canvas: canvas) { [weak self] in self?.pointer($0) }
    }

    /// A press reads the adjustment's input under the pointer — its Rec. 709
    /// luma on the RGB curve, the channel's own value otherwise — and grabs
    /// the point within ±4 of it or adds one on the curve there; dragging up
    /// raises its output 1 per point. Map state and a full curve beep.
    private func pointer(_ event: CanvasPointerEvent) {
        switch event.phase {
        case .down:
            dragStart = nil
            guard channelMaps == nil else { NSSound.beep(); return }
            var layer: Int?
            if case .destructive = mode { layer = aboveIndex }
            guard let rgb = CanvasSample.rgb(
                document, mode: mode, destructiveLayer: layer, at: event.point)
            else { NSSound.beep(); return }
            let x: Double
            switch channel {
            case .rgb: x = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
            case .red: x = rgb[0]
            case .green: x = rgb[1]
            case .blue: x = rgb[2]
            case .cmyk, .cyan, .magenta, .yellow, .black: NSSound.beep(); return
            }
            var points = channelPoints[channel] ?? CurvePoint.identity
            let index: Int
            if let near = points.indices.first(where: { abs(points[$0].x - x) <= 4 }) {
                index = near
            } else {
                guard points.count < CurveEditorView.maxPoints else { NSSound.beep(); return }
                let column = min(max(x.rounded(), 0), 255)
                let output = NaturalCurve(points: points).value(at: column).rounded()
                points.append(CurvePoint(x: column, y: output))
                points.sort { $0.x < $1.x }
                index = points.firstIndex { $0.x == column } ?? 0
                setPoints(points)
            }
            dragStart = (windowY(event.point), index, points[index].y)
        case .dragged, .up:
            guard let start = dragStart else { return }
            var points = channelPoints[channel] ?? CurvePoint.identity
            guard points.indices.contains(start.index) else { return }
            let output = (start.output + Double(windowY(event.point) - start.y)).rounded()
            points[start.index].y = min(max(output, 0), 255)
            setPoints(points)
            if event.phase == .up { dragStart = nil }
        }
    }

    /// A canvas point's height in the WINDOW — screen points, up is a larger
    /// y. The drag is measured there, not in canvas units (document pixels),
    /// so one point is one level at every zoom: at a 25 % view a canvas unit
    /// is a quarter of a point.
    private func windowY(_ point: CGPoint) -> CGFloat {
        canvas?.convert(point, to: nil).y ?? -point.y
    }

    private func setPoints(_ points: [CurvePoint]) {
        channelPoints[channel] = points
        curveView.points = points
        presetControl?.markCustom()
        requestPreview()
    }

    override func viewWillDisappear() {
        super.viewWillDisappear()
        monitor?.stop()
        monitor = nil
        cmykTools?.stop()
    }

    /// Points: every channel's map becomes control points (identity
    /// channels stay `CurvePoint.identity`), and the dialog continues as a
    /// point curve — Apply then commits the point form.
    private func leaveMapState() {
        guard channelMaps != nil else { return }
        pointsFromMaps()
        presetControl?.markCustom()
        requestPreview()
    }

    /// The maps as control points, map state left (no preview).
    private func pointsFromMaps() {
        guard let maps = channelMaps else { return }
        for ch in CurveChannel.allCases {
            channelPoints[ch] = CurvesMeta.points(fromMap: maps[ch] ?? CurvesMeta.identityMap)
        }
        dropMaps()
    }

    /// Leaves map state without converting (Auto replaces every channel).
    private func dropMaps() {
        channelMaps = nil
        curveView.map = nil
        curveView.points = channelPoints[channel] ?? CurvePoint.identity
        showMapState(false)
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard channelMaps != nil || curveView.acceptFineEntry() else { return }
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
        if let precisionSession { precisionSession.commit(meta: metaJSON(), onCommitted: onCommitted); return }
        if case .destructive = mode {
            // The DESTRUCTIVE half is `apply_filter` with filter "curves"
            // and the point lists or maps in `params` — the shape
            // `adjustmentFilterParams` reads back on replay.
            let params = currentParams()
            let capture = capture
            document.applyCapturedOperation(capture,
                AdjustmentLayerOp.curves.displayName,
                record: .filter(
                    "curves", ["params": params],
                    target: capture?.agentTarget ?? "layer")
            ) {
                capture?.adjustmentImage($0, op: "curves", params: params)
            }
            return
        }
        guard let meta = metaJSON() else {
            NSSound.beep()
            return
        }
        let name = AdjustmentLayerOp.curves.displayName
        switch mode {
        case .create(let selection):
            // Compose on the document's CURRENT doc and active layer, not
            // the captured preview base, so any edit that slipped in while
            // the sheet was open (an agent's) survives.
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
        case .edit(let idx, let original):
            // Closing the dialog with unchanged values must not register an
            // undo step or dirty the file (sorted-key encoding makes the
            // comparison byte-stable for metas this app wrote).
            guard meta != original.json() else { return }
            document.applyEdit(
                "Edit \(name) Layer",
                record: .editAdjustmentLayer(
                    meta: meta, layer: document.doc?.layerRef(idx))
            ) { doc in
                // The layer must still BE an adjustment layer (only an agent
                // edit can move the stack under an open sheet).
                guard doc.layerIsAdjustment(idx) else { return nil }
                return doc.withLayerMeta(idx, meta)
            }
            onCommitted?(idx)
        case .destructive:
            return
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        precisionSession?.cancel()
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}
