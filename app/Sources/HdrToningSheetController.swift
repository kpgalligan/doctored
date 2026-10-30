import AppKit

/// Image ▸ Adjustments ▸ HDR Toning…: the dialog — Preset
/// (`HdrToningPresetControl`), Method (all four), Exposure and Gamma's two sliders, Local
/// Adaptation's pane (`HdrToningLocalAdaptationPane`, with the Toning Curve over its
/// histogram), and Mode (8 or 16 Bits/Channel) for a 32-bit source — on the shared
/// `makeSheetView` / `makeButtonRow` builders, with a live preview of the flattened, toned
/// result through `PreviewRenderer`
/// (`ConvertProfileSheetController.requestPreview`'s lease).
///
/// The histogram is read ONCE, on open, through a `HistogramLoader` under a preview reservation of
/// the Local Adaptation plan. The histogram's reservation and a preview's are never held at once:
/// the first preview is issued when the histogram is delivered (or fails), after its lease ended,
/// and a control change before then only waits for it (`histogramPending`).
///
/// It decides nothing: OK hands the settings to `EditorViewController.commitHdrToning`, which
/// re-plans and runs the job on the document's current handle.
final class HdrToningSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var canvas: ImageCanvasView?
    private weak var document: ImageDocument?
    /// The handle the preview runs on, captured when the sheet opened: the renderer's closure
    /// runs on a background queue and must never reach into live document state.
    private let baseDoc: RasterDocument
    private var settings: HdrToningSettings
    private let renderer = PreviewRenderer()
    private var presetControl: HdrToningPresetControl!
    private var methodRow: CardControlRow!
    private var exposureRow: CardControlRow!
    private var gammaRow: CardControlRow!
    private var localPane: HdrToningLocalAdaptationPane!
    private let note = NSTextField(wrappingLabelWithString: "")
    private let histogramLoader = HistogramLoader()
    /// Cancels the histogram read's control when the sheet closes first.
    private var cancelHistogram: (() -> Void)?
    /// True until the histogram is delivered or fails; no preview is issued before then.
    private var histogramPending = true

    init(editor: EditorViewController, document: ImageDocument, source: RasterDocument,
         settings: HdrToningSettings) {
        self.editor = editor
        canvas = editor.canvas
        self.document = document
        baseDoc = source
        self.settings = settings
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] image in self?.showPreview(image) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("HdrToningSheetController does not support NSCoder")
    }

    override func loadView() {
        let methods = HdrToningMethod.allCases
        let opening = settings
        presetControl = HdrToningPresetControl(
            current: { [weak self] in self?.settings ?? opening },
            load: { [weak self] preset in self?.presetChosen(preset) })
        methodRow = cardPopupRow(
            label: "Method:",
            titles: methods.map(\.title),
            selected: methods.firstIndex(of: settings.method) ?? 0
        ) { [weak self] index in self?.methodChanged(index) }
        methodRow.popUp?.setAccessibilityIdentifier("hdr-toning-method")
        exposureRow = cardSliderRow(
            label: "Exposure:", min: -20, max: 20, value: settings.exposure,
            format: { String(format: "%+.2f", $0) }
        ) { [weak self] value in self?.exposureChanged(value) }
        gammaRow = cardSliderRow(
            label: "Gamma:", min: 0.01, max: 9.99, value: settings.gamma,
            format: { String(format: "%.2f", $0) }
        ) { [weak self] value in self?.gammaChanged(value) }
        localPane = HdrToningLocalAdaptationPane(values: settings.local) { [weak self] values in
            self?.localChanged(values)
        }
        var rows: [NSView] = [presetControl.row, methodRow, exposureRow, gammaRow, localPane.view]
        if baseDoc.isFloat {
            let modeRow = cardPopupRow(
                label: "Mode:", titles: ["8 Bits/Channel", "16 Bits/Channel"],
                selected: settings.bitDepth == 8 ? 0 : 1
            ) { [weak self] index in self?.modeChanged(index) }
            modeRow.popUp?.setAccessibilityIdentifier("hdr-toning-mode")
            rows.append(modeRow)
        }
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = 380
        rows.append(note)
        let content = NSStackView(views: rows)
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = StickerButton(title: "OK", style: .primary, target: self,
                                  action: #selector(okClicked(_:)))
        syncControls()
        view = makeSheetView(
            title: HdrToning.title,
            hint: "Tone maps the document's composite into an 8- or 16-bit image and flattens it. "
                + "Local Adaptation compresses the tones region by region and keeps their detail, "
                + "Exposure and Gamma sets the brightness "
                + "and contrast by hand, Highlight Compression rolls the brightest value down to "
                + "white, and Equalize Histogram spreads the image's tones over the whole range.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancel, apply: apply,
                                     leading: [sheetFootnote("flattens · one undo step")]))
        loadHistogram()
    }

    // MARK: - State

    private func syncControls() {
        let manual = settings.method == .exposureGamma
        exposureRow.isHidden = !manual
        gammaRow.isHidden = !manual
        localPane.view.isHidden = settings.method != .localAdaptation
        let depth = settings.bitDepth == 8 ? "8" : "16"
        note.stringValue = baseDoc.isFloat
            ? "The 32-bit document becomes \(depth) bits per channel."
            : "The document keeps \(depth) bits per channel."
    }

    private func methodChanged(_ index: Int) {
        let methods = HdrToningMethod.allCases
        guard methods.indices.contains(index) else { NSSound.beep(); return }
        settings.method = methods[index]
        presetControl.markCustom()
        syncControls()
        settingsChanged()
    }

    private func exposureChanged(_ value: Double) {
        settings.exposure = value
        presetControl.markCustom()
        settingsChanged()
    }

    private func gammaChanged(_ value: Double) {
        settings.gamma = value
        presetControl.markCustom()
        settingsChanged()
    }

    private func localChanged(_ values: HdrLocalAdaptation) {
        settings.local = values
        presetControl.markCustom()
        settingsChanged()
    }

    /// A chosen preset: every control set silently to its values (the depth kept), then one
    /// preview.
    private func presetChosen(_ preset: HdrToningPreset) {
        settings = preset.applied(to: settings)
        methodRow.setSelected(HdrToningMethod.allCases.firstIndex(of: settings.method) ?? 0)
        exposureRow.setValue(settings.exposure)
        gammaRow.setValue(settings.gamma)
        localPane.load(settings.local)
        syncControls()
        settingsChanged()
    }

    private func modeChanged(_ index: Int) {
        settings.bitDepth = index == 0 ? 8 : 16
        syncControls()
        settingsChanged()
    }

    /// A new preview, unless the histogram is still loading: its delivery issues the first one.
    private func settingsChanged() {
        guard !histogramPending else { return }
        requestPreview()
    }

    // MARK: - Histogram

    /// The Toning Curve's histogram, read once under a preview reservation of the default Local
    /// Adaptation plan. The read's closure drops the reservation before it returns, so the
    /// delivery on main (which issues the first preview) runs after the lease ended.
    private func loadHistogram() {
        let source = baseDoc
        histogramLoader.onBins = { [weak self] bins in self?.histogramLoaded(bins) }
        histogramLoader.onFailure = { [weak self] reason in self?.histogramFailed(reason) }
        do {
            var planned = settings
            planned.method = .localAdaptation
            planned.local = HdrLocalAdaptation()
            let scratch = try Self.planBytes(source, settings: planned)
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: scratch + source.projectionScratchEstimate(), outputBytes: 0)
            let control = reservation.control
            cancelHistogram = { control.cancel() }
            var lease: SelectionWorkBudget.Reservation? = reservation
            histogramLoader.request {
                defer { lease = nil }
                guard let held = lease else { return nil }
                return try held.runAsOwnLease { token in
                    Result {
                        try HistogramLoader.reading(under: token) {
                            guard token.setScratchLimit(scratch) else {
                                throw DoctoredCoreError(message: "Could not bound the histogram's memory.")
                            }
                            return try HdrToningRun.histogram(source, control: token)
                        }
                    }
                }.get()
            }
        } catch {
            histogramFailed(error.localizedDescription)
        }
    }

    private func histogramLoaded(_ bins: HistogramBins?) {
        localPane.histogram = bins
        if bins == nil { localPane.histogramUnavailableReason = "The histogram was not read." }
        histogramDelivered()
    }

    private func histogramFailed(_ reason: String) {
        localPane.histogram = nil
        localPane.histogramUnavailableReason = reason
        histogramDelivered()
    }

    private func histogramDelivered() {
        cancelHistogram = nil
        guard histogramPending else { return }
        histogramPending = false
        requestPreview()
    }

    /// Stops a histogram read still in flight; returns once it has (`HistogramLoader.cancel`).
    /// A delivery already queued on main finds no callback and nothing pending, so it never
    /// asks a closed sheet for a preview.
    private func stopHistogram() {
        cancelHistogram?()
        cancelHistogram = nil
        histogramLoader.onBins = nil
        histogramLoader.onFailure = nil
        histogramPending = false
        histogramLoader.cancel()
    }

    /// `HdrToningRun`'s plan for `settings` on `source`.
    private static func planBytes(_ source: RasterDocument, settings: HdrToningSettings) throws -> UInt64 {
        let request = try settings.coreRequest()
        let plan: [String: Any]
        if let typed = source.precision {
            plan = try typed.hdrToningPlan(request, bitDepth: settings.bitDepth)
        } else {
            plan = try source.hdrToningPlan(request)
        }
        return try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
    }

    // MARK: - Preview

    /// A render that lands after the document moved on is dropped, so a stale picture never
    /// sits over the canvas.
    private func showPreview(_ image: CGImage?) {
        guard editor?.document?.doc === baseDoc else { canvas?.previewImage = nil; return }
        canvas?.previewImage = image
    }

    /// The flattened, toned document as OK would leave it, under a preview lease that charges the
    /// toning's own plan and the display preparation.
    private func requestPreview() {
        let source = baseDoc, settings = settings
        do {
            let request = try settings.coreRequest()
            let plan: [String: Any]
            if let typed = source.precision {
                plan = try typed.hdrToningPlan(request, bitDepth: settings.bitDepth)
            } else {
                plan = try source.hdrToningPlan(request)
            }
            let scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: scratch + source.projectionScratchEstimate(), outputBytes: 0)
            renderer.cancelCompute?()
            renderer.cancelCompute = { reservation.control.cancel() }
            renderer.request { reservation.runAsOwnLease { token in
                do {
                    guard token.setScratchLimit(scratch) else { return nil }
                    let toned = try HdrToningRun.toned(source, request: request,
                                                       bitDepth: settings.bitDepth, control: token)
                    guard !token.isCancelled else { return nil }
                    let preview = PreparedDocumentEdit(toned).preview
                    return token.isCancelled ? nil : preview
                } catch { return nil }
            } }
        } catch {
            renderer.request { nil }
        }
    }

    // MARK: - Actions

    @objc private func okClicked(_ sender: Any?) {
        let settings = settings
        stopHistogram()
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
        guard let document else { return }
        editor?.commitHdrToning(settings, document: document)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        stopHistogram()
        renderer.cancel()
        canvas?.previewImage = nil
        dismiss(self)
    }
}
