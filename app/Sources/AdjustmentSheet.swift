import AppKit

/// Which surface opened an adjustment dialog, and therefore what Apply
/// commits. The three share ONE sheet class per op because the destructive
/// twin runs the same core op (`dr_image_adjust_op`) the layer composites —
/// so the controls, the ranges and the preview are the same picture in all
/// three, and only the commit target differs.
enum AdjustmentSheetMode {
    /// Image ▸ Adjustments ▸ … — rewrites the active layer's pixels.
    case destructive
    /// Layer ▸ New Adjustment Layer ▸ … — inserts a layer above the active
    /// one, its mask built from `selection` (the marquee, left up) or
    /// reveal-all.
    case create(selection: [UInt8]?)
    /// Adjustment Options… — replaces layer `layer`'s meta; `original`
    /// prefills the controls, and an unchanged ADJUSTMENT commits nothing
    /// (`AdjustmentSchema.sameEffect`: the same pixels, however differently
    /// the two params objects spell them).
    case edit(layer: Int, original: AdjustmentLayerPayload)

    /// The adjustment-layer seam's own mode, widened. The legacy sheet's
    /// `Mode` has no destructive case (its ops' destructive twins predate
    /// the shared dialog), so the mapping is total in this direction only.
    static func from(_ mode: AdjustmentLayerSheetController.Mode) -> AdjustmentSheetMode {
        switch mode {
        case .create(let selection): return .create(selection: selection)
        case .edit(let layer, let original): return .edit(layer: layer, original: original)
        }
    }
}

/// The lifecycle every phase-5 adjustment dialog shares: handles captured at
/// init, a debounced in-context preview of the EXACT op Apply will run, one
/// undo step on Apply, nothing at all on Cancel. Subclasses supply controls
/// and params; they never touch the document, the renderer or the canvas.
///
/// The five-step contract is `SliderSheetController`'s and
/// `AdjustmentLayerSheetController`'s, verbatim: handles captured in `init`
/// (so an agent edit mid-sheet cannot move the ground under the preview), a
/// 60 ms-debounced `PreviewRenderer` whose closure captures VALUES only,
/// first paint in `viewDidAppear` behind `didRequestInitialPreview`, and the
/// fixed teardown `renderer.cancel()` → `canvas?.previewImage = nil` →
/// `dismiss(self)` → commit.
class AdjustmentSheet: NSViewController {
    let op: AdjustmentLayerOp
    let document: ImageDocument
    let mode: AdjustmentSheetMode
    /// The payload the sheet opened on: the layer's params in `.edit`, a
    /// Photoshop preset's in `.create` when one was opened, the op's
    /// defaults otherwise (`AdjustmentSchema.defaults`). A saved preset
    /// chosen in the sheet (`load(_:)`) replaces it, keeping the
    /// sheet's own evaluation; subclasses read it only in `loadState()`.
    private(set) var initial: AdjustmentLayerPayload
    /// The Photoshop preset a `.create` sheet was opened on (Layer ▸ New
    /// Adjustment Layer ▸ From Photoshop Preset…); nil in every other mode
    /// and for a preset of another op.
    let preset: AdjustmentLayerPayload?
    /// Fires after a successful Apply with the created/edited layer's index
    /// (the editor selects it and refreshes). Never fires in `.destructive`,
    /// which creates no layer.
    var onCommitted: ((Int) -> Void)?

    /// The canvas the preview lands on — also what Targeted Adjustment
    /// samples (`CanvasPointerMonitor`).
    private(set) weak var canvas: ImageCanvasView?
    /// In-context preview base: the chained pure ops run on these captured
    /// handles and the flattened result is shown, so masks, blend modes and
    /// the other layers stay visible while scrubbing.
    private let baseDoc: RasterDocument?
    private let capture: OperationCapture?
    /// The active layer when the sheet opened: the insertion anchor in
    /// `.create`, the rewritten layer in `.destructive`.
    let aboveIndex: Int
    private let renderer = PreviewRenderer()
    private let precisionSession: PrecisionAdjustmentSession?
    private var didRequestInitialPreview = false
    /// Retained so a row's closure outlives the call that built it —
    /// AppKit targets are unowned.
    private var actions: [ControlAction] = []
    /// The controls' host: `load(_:)` swaps its one arranged view for a
    /// fresh `makeContent()`.
    private let contentHost = NSStackView()
    /// The saved-preset row, for the ops the library serves.
    private var presetControl: AdjustmentPresetControl?

    // No histogram seam here on purpose. None of the fourteen ops this class
    // serves plots one — a plot belongs behind controls that are read
    // AGAINST the tones (Levels' black/white/gamma, Curves' control points),
    // and these fourteen are not shaped that way. The two dialogs that do
    // plot (`LevelsAdjustmentEditor`, `CurvesAdjustmentSheetController`)
    // call `Histogram.loadForSheet` directly, which is three lines and needs no
    // hook; a fifteenth op that wanted one would do the same rather than
    // inherit an ordering guarantee no subclass had ever exercised.

    init(
        op: AdjustmentLayerOp, document: ImageDocument, canvas: ImageCanvasView,
        mode: AdjustmentSheetMode, preset: AdjustmentLayerPayload? = nil,
        evaluation: String? = nil
    ) {
        self.op = op
        self.document = document
        self.canvas = canvas
        self.mode = mode
        self.baseDoc = document.doc
        self.aboveIndex = document.activeLayerIndex
        if case .destructive = mode {
            self.capture = document.captureOperation(previewBase: canvas.channelDisplay?.baseKind)
        } else {
            self.capture = nil
        }
        if case .create = mode, let preset, preset.op == op.rawValue {
            self.preset = preset
        } else {
            self.preset = nil
        }
        if case .edit(_, let original) = mode {
            // An explicit evaluation (the refusal's legacy switch)
            // re-edits in that evaluation; `original` keeps the stored one,
            // so `sameEffect` sees the switch as a change.
            if let evaluation, CalibratedAdjustment.supports(op.rawValue) {
                var switched = original
                switched.params["evaluation"] = evaluation
                self.initial = switched
            } else {
                self.initial = original
            }
        } else if let preset = self.preset {
            // A preset carries no evaluation (the core strips it), so a
            // calibrated op takes the one a new layer would get.
            var params = preset.params
            if CalibratedAdjustment.supports(op.rawValue) {
                params["evaluation"] = CalibratedAdjustment.initialParams(
                    op: op, mode: mode, target: document.planeEditTarget)["evaluation"]
            }
            self.initial = AdjustmentLayerPayload(op: op, params: params)
        } else {
            self.initial = AdjustmentLayerPayload(
                op: op, params: CalibratedAdjustment.initialParams(
                    op: op, mode: mode, target: document.planeEditTarget))
        }
        precisionSession = PrecisionAdjustmentSession(document, canvas: canvas, mode: mode)
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] cgImage in
            self?.canvas?.previewImage = cgImage
        }
        loadState()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AdjustmentSheet does not support NSCoder")
    }

    // MARK: - Subclass hooks

    /// Reads every control value from `initial`. The base calls it once at
    /// the end of `init` and again from `load(_:)`, so it is the ONE place a
    /// subclass reads `initial` into its state.
    func loadState() {}

    /// The 8-bit destructive op Apply runs and the preview shows. The
    /// default is the ONE twin (`dr_image_adjust_op`); Brightness/Contrast's
    /// legacy form keeps its own frozen export. The closure must capture
    /// values only, never the sheet: it runs on the render queue.
    func destructiveOperation(
        capture: OperationCapture, params: [String: Any]
    ) -> (RasterImage) -> RasterImage? {
        let name = op.rawValue
        return { capture.adjustmentImage($0, op: name, params: params) }
    }

    /// The Action step a destructive Apply records: `apply_filter` with the
    /// op's own name and its params inside `params`.
    func destructiveRecord(params: [String: Any], target: String) -> [ActionStep] {
        .filter(op.rawValue, ["params": params], target: target)
    }

    /// The op's controls, built once from `initial`. Every action of a
    /// control that sets a parameter must end in `valuesChanged()` — the
    /// house rows below already do.
    func makeContent() -> NSView {
        NSView()
    }

    /// The current control values as meta params — already inside
    /// `AdjustmentSchema`'s ranges, because the controls' own ranges are
    /// the schema's.
    func currentParams() -> [String: Any] {
        AdjustmentSchema.defaults(for: op)
    }

    /// Extra text under the buttons. The default names the undo cost; an op
    /// with a convention worth stating (Exposure's gamma, White Balance's
    /// direction) says it here.
    var footnote: String { "one undo step" }

    var colorEvaluationCaption: String {
        let evaluation = initial.string("evaluation", default: CalibratedAdjustment.legacy)
        if case .edit(_, let original) = mode,
           original.string("evaluation", default: CalibratedAdjustment.legacy) != evaluation {
            return CalibratedAdjustment.legacySwitchCaption
        }
        return evaluation == CalibratedAdjustment.document
            ? "Processing uses the document's colour profile."
            : "Legacy sRGB processing preserves this adjustment's existing appearance."
    }


    // MARK: - For subclasses to call

    /// Re-previews, debounced. Every control change ends here; a change
    /// after a preset was chosen shows the popup's Custom row.
    func valuesChanged() {
        presetControl?.markCustom()
        requestPreview()
    }

    /// Opens `payload`'s values in the controls — a saved preset or Default:
    /// the sheet keeps its own evaluation, re-reads its state,
    /// rebuilds its controls and re-previews.
    func load(_ payload: AdjustmentLayerPayload) {
        var next = AdjustmentLayerPayload(op: op, params: payload.params)
        next.params.removeValue(forKey: "evaluation")
        if CalibratedAdjustment.supports(op.rawValue), let kept = initial.params["evaluation"] {
            next.params["evaluation"] = kept
        }
        initial = next
        loadState()
        installContent()
        valuesChanged()
    }

    /// Puts a fresh `makeContent()` into the host. The bound actions of the
    /// controls it replaces go with them.
    private func installContent() {
        for view in contentHost.arrangedSubviews {
            contentHost.removeArrangedSubview(view)
            view.removeFromSuperview()
        }
        actions.removeAll()
        contentHost.addArrangedSubview(makeContent())
    }

    /// The space an adjustment's colours live in — the DOCUMENT's, because
    /// an adjustment is a function of the pixels it transforms
    /// (`AdjustmentColor`). On a Lab Color document (#1 phase 2d) sRGB: its
    /// Photo Filter colour and Gradient Map stops are sRGB numbers the core
    /// converts for the kernel (`lab_adjust::kernel_params`), not Lab codes,
    /// though the twin's own space is a LUT-mode destination.
    var documentColorSpace: NSColorSpace {
        document.doc?.isLab == true ? .sRGB : document.nsColorSpace
    }

    // MARK: - House rows

    /// The house slider row: a 180 pt continuous slider between the 106 pt
    /// trailing label column and a 52 pt mono readout. `value`, the range
    /// and what `onChange` receives are all in the SAME space, so a mapped
    /// slider (a log-feel gamma) maps inside its own closures.
    func sliderRow(
        _ label: String, range: ClosedRange<Double>, value: Double,
        format: @escaping (Double) -> String,
        onChange: @escaping (Double) -> Void
    ) -> [NSView] {
        let readout = NSTextField(labelWithString: format(value))
        readout.font = DS.mono(12)
        readout.textColor = DS.textMuted
        readout.alignment = .right
        readout.widthAnchor.constraint(equalToConstant: 52).isActive = true

        let slider = NSSlider(
            value: value, minValue: range.lowerBound, maxValue: range.upperBound,
            target: nil, action: nil)
        slider.isContinuous = true
        slider.widthAnchor.constraint(equalToConstant: 180).isActive = true
        bind(slider) { [weak self] sender in
            guard let slider = sender as? NSSlider else { return }
            readout.stringValue = format(slider.doubleValue)
            onChange(slider.doubleValue)
            self?.valuesChanged()
        }
        return [fieldLabel(label), slider, readout]
    }

    /// A checkbox under the control column (the label column stays empty —
    /// the title is the checkbox's own). `changesValues` is false for a box
    /// that switches a mode of the DIALOG (sampling from the canvas) and no
    /// parameter: ticking it neither re-previews nor moves the Preset popup
    /// to Custom.
    func checkboxRow(
        _ title: String, value: Bool, changesValues: Bool = true,
        onChange: @escaping (Bool) -> Void
    ) -> [NSView] {
        let box = NSButton(checkboxWithTitle: title, target: nil, action: nil)
        box.font = DS.sans(13)
        box.state = value ? .on : .off
        bind(box) { [weak self] sender in
            guard let box = sender as? NSButton else { return }
            onChange(box.state == .on)
            if changesValues { self?.valuesChanged() }
        }
        return [NSGridCell.emptyContentView, box]
    }

    /// A labelled popup; `onChange` receives the selected index.
    func popupRow(
        _ label: String, titles: [String], selected: Int,
        onChange: @escaping (Int) -> Void
    ) -> [NSView] {
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.addItems(withTitles: titles)
        popup.font = DS.sans(13)
        if selected >= 0, selected < titles.count { popup.selectItem(at: selected) }
        bind(popup) { [weak self] sender in
            guard let popup = sender as? NSPopUpButton, popup.indexOfSelectedItem >= 0
            else { return }
            onChange(popup.indexOfSelectedItem)
            self?.valuesChanged()
        }
        return [fieldLabel(label), popup]
    }

    /// A labelled colour well. `hex` and what `onChange` receives are the
    /// DOCUMENT's numbers: the well is built in the document's space and
    /// read back through it, so a picked colour converts exactly once and a
    /// re-opened sheet shows the colour it stored.
    func colorRow(
        _ label: String, hex: String, onChange: @escaping (String) -> Void
    ) -> [NSView] {
        let space = documentColorSpace
        let well = NSColorWell(frame: .zero)
        well.color = AdjustmentColor.color(fromHex: hex, in: space) ?? .black
        well.widthAnchor.constraint(equalToConstant: 54).isActive = true
        well.heightAnchor.constraint(equalToConstant: 22).isActive = true
        bind(well) { [weak self] sender in
            guard let well = sender as? NSColorWell else { return }
            onChange(AdjustmentColor.hex(well.color, in: space))
            self?.valuesChanged()
        }
        return [fieldLabel(label), well]
    }

    /// The house grid every sheet's rows go into: a 106 pt trailing label
    /// column, 12 pt between columns, 10 pt between rows.
    static func grid(_ rows: [[NSView]]) -> NSGridView {
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        return grid
    }

    /// Retains a closure as `control`'s target/action.
    private func bind(_ control: NSControl, _ run: @escaping (Any?) -> Void) {
        let action = ControlAction(run)
        actions.append(action)
        control.target = action
        control.action = #selector(ControlAction.fire(_:))
    }

    // MARK: - Lifecycle

    override func loadView() {
        let title: String
        switch mode {
        case .destructive: title = op.displayName
        case .create: title = "New \(op.displayName) layer"
        case .edit: title = "\(op.displayName) options"
        }
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        contentHost.orientation = .vertical
        contentHost.alignment = .leading
        installContent()
        var body: NSView = contentHost
        if AdjustmentPresetLibrary.supportedOps.contains(op) {
            let control = AdjustmentPresetControl(
                op: op, current: { [weak self] in self?.currentParams() ?? [:] },
                load: { [weak self] in self?.load($0) })
            presetControl = control
            let stack = NSStackView(views: [control.row, contentHost])
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 12
            body = stack
        }
        view = makeSheetView(
            title: title, content: precisionSession.map { $0.content(body) } ?? body,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote(footnote)]))
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        // Several of these ops have non-identity defaults (Shadows/
        // Highlights lifts by 35 %, Photo Filter warms by 25 %), so the
        // first paint must happen before anything is touched: what the user
        // sees is what Apply produces.
        guard !didRequestInitialPreview else { return }
        didRequestInitialPreview = true
        requestPreview()
    }

    /// Controls edit numbers; the captured evaluation version survives re-edit
    /// and is recorded beside them, including legacy descriptions with no key.
    private func evaluatedParams() -> [String: Any] {
        var params = currentParams()
        if CalibratedAdjustment.supports(op.rawValue) {
            params["evaluation"] = initial.string("evaluation", default: CalibratedAdjustment.legacy)
        }
        return params
    }

    private func metaJSON(_ params: [String: Any]) -> String? {
        AdjustmentLayerPayload(op: op, params: params).json()
    }

    /// Previews the EXACT op Apply will run, on the captured handles (self
    /// never crosses onto the render queue; only values do).
    private func requestPreview() {
        if let precisionSession { precisionSession.request(op: op, params: evaluatedParams()); return }
        guard let baseDoc = baseDoc else { return }
        let params = evaluatedParams()
        let name = op.rawValue
        switch mode {
        case .destructive:
            guard let capture = capture else { return }
            let operation = destructiveOperation(capture: capture, params: params)
            renderer.request {
                capture.preview(operation)
            }
        case .create(let selection):
            guard let meta = metaJSON(params) else { return }
            let above = aboveIndex
            let layerName = op.displayName
            renderer.request {
                baseDoc.addingAdjustmentLayer(
                    active: above, name: layerName, meta: meta, selection: selection)?
                    .flattened()?.makeCGImage(in: baseDoc.colorSpace)
            }
        case .edit(let idx, _):
            guard let meta = metaJSON(params) else { return }
            renderer.request {
                baseDoc.withLayerMeta(idx, meta)?.flattened()?
                    .makeCGImage(in: baseDoc.colorSpace)
            }
        }
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard document.doc === baseDoc else {
            renderer.cancel()
            canvas?.previewImage = nil
            dismiss(self)
            let alert = NSAlert()
            alert.messageText = "The document changed while the adjustment was open."
            alert.informativeText = "Reopen the adjustment to preview the current image and colour profile."
            alert.runModal()
            return
        }
        let params = evaluatedParams()
        let operation = capture.map { destructiveOperation(capture: $0, params: params) }
        let record = destructiveRecord(params: params, target: capture?.agentTarget ?? "layer")
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
        if let precisionSession { precisionSession.commit(op: op, params: params, onCommitted: onCommitted); return }
        switch mode {
        case .destructive:
            // The DESTRUCTIVE half of an adjustment is `apply_filter` with
            // the op's own name as the filter and its parameters in `params`
            // — the same shape `applyingAdjustment` takes.
            document.applyCapturedOperation(capture, op.displayName, record: record) {
                operation?($0)
            }
        case .create(let selection):
            guard let meta = metaJSON(params) else {
                NSSound.beep()
                return
            }
            // The revision guard above keeps the preview's document and
            // colour profile current; use its captured insertion point too.
            let below = aboveIndex
            let layerName = op.displayName
            let before = document.doc
            // An active GROUP takes the new entry as its top child,
            // so the core answers where it went (§4.5).
            let landing = before?.newEntryLandingIndex(active: below) ?? below + 1
            document.applyEdit(
                "New \(layerName) Layer",
                record: .addAdjustmentLayer(
                    meta: meta, name: layerName,
                    position: before?.newEntryPosition(active: below) ?? .above)
            ) {
                $0.addingAdjustmentLayer(
                    active: below, name: layerName, meta: meta, selection: selection)
            }
            guard document.doc !== before, let doc = document.doc else { return }
            onCommitted?(min(landing, doc.layerCount - 1))
        case .edit(let idx, let original):
            guard let meta = metaJSON(params) else {
                NSSound.beep()
                return
            }
            // Closing with unchanged values must register no undo step and
            // no dirty flag. The test is SEMANTIC, not byte-wise: this
            // sheet writes every key it has a control for, while the layer
            // may have been authored over MCP, by an older build or read
            // from a `.rz` with only the keys its caller passed, so two
            // spellings of one picture are the norm rather than the
            // exception (`AdjustmentSchema.sameEffect`).
            guard !AdjustmentSchema.sameEffect(params, original.params, for: op) else { return }
            document.applyEdit(
                "Edit \(op.displayName) Layer",
                record: .editAdjustmentLayer(
                    meta: meta, layer: document.doc?.layerRef(idx))
            ) { doc in
                // The layer must still BE an adjustment layer (only an
                // agent edit can move the stack under an open sheet).
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

/// A closure as an AppKit target. AppKit holds a target unowned, so the
/// sheet keeps these alive for as long as its controls exist.
private final class ControlAction: NSObject {
    private let run: (Any?) -> Void

    init(_ run: @escaping (Any?) -> Void) {
        self.run = run
    }

    @objc func fire(_ sender: Any?) {
        run(sender)
    }
}

/// A colour inside an ADJUSTMENT's params is the DOCUMENT's numbers — an
/// adjustment is a function of the pixels it transforms, so its colour lives
/// in their space and converts nowhere (the eyedropper's rule,
/// ColorProfile.swift). This is NOT `LayerStyleColor`, which is authored
/// sRGB and converts in the core at composite time; the two spell the same
/// hex and mean different colours on a Display P3 document, which is exactly
/// why they are separate types.
enum AdjustmentColor {
    /// The DOCUMENT's spelling of a colour the user just picked or dropped.
    /// Falls back to the sRGB spelling, and then to black, only when
    /// ColorSync refuses the conversion outright — where the alternative is
    /// storing a number that names no colour at all.
    static func hex(_ color: NSColor, in space: NSColorSpace) -> String {
        let bytes = ColorProfile.bytes(color, in: space)
            ?? ColorProfile.bytes(color, in: .sRGB)
        guard let bytes = bytes, bytes.count >= 3 else { return "#000000" }
        return String(format: "#%02x%02x%02x", bytes[0], bytes[1], bytes[2])
    }

    /// The colour a stored `#rrggbb` names, built IN the document's space
    /// because those bytes already are the document's numbers. nil for a
    /// spelling that is not six hex digits.
    static func color(fromHex hex: String, in space: NSColorSpace) -> NSColor? {
        var digits = Substring(hex)
        if digits.hasPrefix("#") { digits = digits.dropFirst() }
        guard digits.count == 6 || digits.count == 8,
              let value = UInt32(digits.prefix(6), radix: 16)
        else { return nil }
        return NSColor(
            colorSpace: space,
            components: [
                CGFloat((value >> 16) & 0xff) / 255,
                CGFloat((value >> 8) & 0xff) / 255,
                CGFloat(value & 0xff) / 255,
                1,
            ],
            count: 4)
    }

    /// A NAMED PRESET (Warming 85, the Black & White tint, a gradient
    /// preset) is an AUTHORED sRGB colour and converts ONCE, here, at the
    /// moment the user picks it — after which it is stored as the document's
    /// numbers like any other adjustment colour. This is the one place the
    /// adjustment rule above and the app's authored-colour rule meet:
    /// `#ec8a00` really is Warming 85 *in sRGB*, and on a Display P3
    /// document those same numbers are a different colour, so a preset that
    /// stored its literal hex would quietly stop being the filter it names.
    static func documentHex(forSRGBHex hex: String, in space: NSColorSpace) -> String {
        guard let authored = color(fromHex: hex, in: .sRGB) else { return hex }
        return self.hex(authored, in: space)
    }
}

/// The registry the editor's dialog seam consults: the phase-5 ops have
/// their own sheets (one class each, all `AdjustmentSheet` subclasses),
/// while the legacy nine keep `AdjustmentLayerSheetController`'s slider
/// table and Curves keeps its editor.
enum AdjustmentSheets {
    /// Whether `op` is one of the ops this registry builds.
    static func hasDialog(_ op: AdjustmentLayerOp) -> Bool {
        op == .bcs || !AdjustmentSchema.params(for: op).isEmpty
    }

    static func make(
        op: AdjustmentLayerOp, document: ImageDocument, canvas: ImageCanvasView,
        mode: AdjustmentSheetMode, preset: AdjustmentLayerPayload? = nil,
        onCommitted: ((Int) -> Void)? = nil
    ) -> AdjustmentSheet? {
        var evaluation: String?
        if let reason = CalibratedAdjustment.sheetRefusal(op: op, mode: mode, imageDocument: document) {
            guard CalibratedAdjustment.presentRefusal(reason, mode: mode) else { return nil }
            evaluation = CalibratedAdjustment.legacy
        }
        let sheet: AdjustmentSheet?
        switch op {
        case .bcs:
            sheet = BrightnessContrastSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .replaceColor:
            sheet = ReplaceColorSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .exposure:
            sheet = ExposureSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset,
                evaluation: evaluation)
        case .vibrance:
            sheet = VibranceSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .hueSaturation:
            sheet = HueSaturationSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .colorBalance:
            sheet = ColorBalanceSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .blackAndWhite:
            sheet = BlackAndWhiteSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .photoFilter:
            sheet = PhotoFilterSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .channelMixer:
            sheet = ChannelMixerSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .selectiveColor:
            sheet = SelectiveColorSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .shadowsHighlights:
            sheet = ShadowsHighlightsSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .whiteBalance:
            sheet = WhiteBalanceSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset,
                evaluation: evaluation)
        case .gradientMap:
            sheet = GradientMapSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .colorLookup:
            sheet = ColorLookupSheet(
                op: op, document: document, canvas: canvas, mode: mode, preset: preset)
        case .curves, .levels, .hueRotate, .posterize, .threshold,
            .invert, .grayscale, .sepia:
            sheet = nil
        }
        sheet?.onCommitted = onCommitted
        return sheet
    }
}
