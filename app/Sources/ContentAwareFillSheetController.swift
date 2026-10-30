import AppKit

/// Edit > Content-Aware Fill: editing eligibility invalidates the exact
/// candidate; Prepare evaluates once, and Apply publishes that reviewed owner.
final class ContentAwareFillSheetController: NSViewController, NSTextFieldDelegate {
    private weak var editor: EditorViewController?
    private var review: ContentAwareFillReview?
    private var sourceMask = ContentAwareSourceMask()
    private let preview = ContentAwareFillCanvasView(frame: .zero)
    private let ring = NSTextField(string: "48")
    private let seed = NSTextField(string: "0")
    private let radius = NSTextField(string: "20")
    private let sample = NSPopUpButton(frame: .zero, pullsDown: false)
    private let output = NSPopUpButton(frame: .zero, pullsDown: false)
    private let sourceMode = NSPopUpButton(frame: .zero, pullsDown: false)
    private let compare = NSSegmentedControl(labels: ["Source Area", "Before", "After"], trackingMode: .selectOne, target: nil, action: nil)
    private let zoom = NSSegmentedControl(labels: ["Fit", "100%"], trackingMode: .selectOne, target: nil, action: nil)
    private let ignore = NSButton(checkboxWithTitle: "Ignore adjustment layers", target: nil, action: nil)
    private let status = NSTextField(wrappingLabelWithString: "")
    private var applyButton: NSButton?
    private var ownerClose: NSObjectProtocol?
    private var initialImage: CGImage?
    private var initialError: String?
    private var closing = false

    init(editor: EditorViewController) {
        self.editor = editor
        initialImage = editor.document?.previewImage
        super.init(nibName: nil, bundle: nil)
        do {
            guard let document = editor.document, let mask = editor.canvas.selection?.maskBytes() else {
                throw DoctoredCoreError(message: "Make a selection before opening Content-Aware Fill.")
            }
            review = try ContentAwareFillReview(document: document, layer: document.activeLayerIndex, coverage: mask)
            review?.onChange = { [weak self] in self?.refresh() }
            ownerClose = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
                object: document, queue: nil) { [weak self] _ in self?.close() }
        } catch { initialError = error.localizedDescription }
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("ContentAwareFillSheetController does not support NSCoder") }
    deinit {
        review?.cancel()
        if let ownerClose { NotificationCenter.default.removeObserver(ownerClose) }
    }

    override func loadView() {
        let options = ToolOptionsStore.shared.contentAwareFill
        ring.stringValue = String(Int(options.ring)); seed.stringValue = String(options.seed)
        for field in [ring, seed, radius] {
            field.widthAnchor.constraint(equalToConstant: 74).isActive = true
            DSField.style(field); field.delegate = self
            field.target = self; field.action = #selector(changed(_:))
        }
        radius.identifier = .init("caf.radius"); ring.identifier = .init("caf.ring"); seed.identifier = .init("caf.seed")
        sample.addItems(withTitles: RetouchSampling.allCases.map(\.displayName))
        sample.selectItem(at: RetouchSampling.allCases.firstIndex(of: options.sample) ?? 0)
        output.addItems(withTitles: RetouchOutput.allCases.map(\.displayName))
        sourceMode.addItems(withTitles: ["Pan", "Include Brush", "Exclude Brush", "Include Rectangle", "Exclude Rectangle"])
        for popup in [sample, output, sourceMode] { popup.target = self; popup.action = #selector(changed(_:)); popup.font = DS.sans(12) }
        sample.identifier = .init("caf.sample"); output.identifier = .init("caf.output"); sourceMode.identifier = .init("caf.sourceMode")
        sourceMode.selectItem(at: 2)
        ignore.state = options.ignoreAdjustments ? .on : .off
        ignore.target = self; ignore.action = #selector(changed(_:))
        for control in [compare, zoom] { control.target = self; control.action = #selector(displayChanged(_:)); control.selectedSegment = 0 }
        compare.identifier = .init("caf.compare"); zoom.identifier = .init("caf.zoom")
        let all = NSButton(title: "Include All", target: self, action: #selector(resetAll(_:)))
        let none = NSButton(title: "Exclude All", target: self, action: #selector(resetNone(_:)))
        all.identifier = .init("caf.reset"); none.identifier = .init("caf.resetNone")
        let prepare = NSButton(title: "Prepare Final Result", target: self, action: #selector(prepareClicked(_:)))
        prepare.identifier = .init("caf.prepare")
        preview.identifier = .init("caf.source")
        preview.widthAnchor.constraint(equalToConstant: 970 - sheetInset * 2).isActive = true
        preview.heightAnchor.constraint(equalToConstant: 430).isActive = true
        preview.image = initialImage
        preview.selectionPath = editor?.canvas.selection?.marqueePath?.cgPath
        preview.onOperation = { [weak self] operation in self?.append(operation) }
        status.font = DS.sans(12); status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 920
        let controls = stack([
            row([fieldLabel("Sample:"), sample, ignore, fieldLabel("Output:"), output]),
            row([fieldLabel("Ring (px):"), ring, fieldLabel("Seed:"), seed, prepare]),
            row([sourceMode, fieldLabel("Radius (px):"), radius, all, none, NSView(), compare, zoom]),
            preview, status])
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        cancel.identifier = .init("caf.cancel"); apply.identifier = .init("caf.apply"); applyButton = apply
        view = makeSheetView(title: "Content-aware fill", hint:
            "Paint or draw rectangles to include (green) or exclude (red) source areas. The selection remains the fill destination. Prepare the final result, compare Before and After at Fit or 100%, then Apply. A narrow sampling ring is widened automatically. New Layer preserves the original; translucent backdrops may composite differently. A selection over 1 MP is searched at reduced scale: large structure is matched, and fine texture is copied without being matched to the edge. Limits: selection plus ring 4 MP (16 MP for a selection over 1 MP, 64 MP over 4 MP), one part's box 32 MP, 80 MP of component boxes.",
            content: controls, buttonRow: makeButtonRow(cancel: cancel, apply: apply), width: 970)
        refresh()
        preview.mode = .excludeBrush
    }
    override func viewDidAppear() {
        super.viewDidAppear()
        if review?.state == "idle" { prepareClicked(nil) }
    }
    override func viewDidDisappear() {
        super.viewDidDisappear()
        review?.cancel(); review = nil; preview.image = nil; initialImage = nil
    }
    private func row(_ views: [NSView]) -> NSStackView {
        let row = NSStackView(views: views); row.orientation = .horizontal; row.spacing = 8
        row.alignment = .centerY; return row
    }
    private func stack(_ views: [NSView]) -> NSStackView {
        let stack = NSStackView(views: views); stack.orientation = .vertical
        stack.alignment = .leading; stack.spacing = 10; return stack
    }
    private func settings() throws -> ContentAwareFillSettings {
        guard let ringValue = Int(ring.stringValue), (0...512).contains(ringValue),
              let seedValue = UInt64(seed.stringValue), seedValue <= UInt64(Int.max) else {
            throw DoctoredCoreError(message: "Ring must be an integer from 0 to 512; seed must be a non-negative integer.")
        }
        var settings = ContentAwareFillSettings()
        settings.ring = ringValue; settings.seed = seedValue
        settings.choice = RetouchSamplingChoice(mode: RetouchSampling.allCases[max(0, sample.indexOfSelectedItem)],
            ignoreAdjustments: ignore.state == .on)
        settings.output = RetouchOutput.allCases[max(0, output.indexOfSelectedItem)]
        settings.sourceMask = sourceMask.fields
        return settings
    }
    private func refresh() {
        guard isViewLoaded else { return }
        status.stringValue = initialError ?? review?.message ?? "No fill source."
        applyButton?.isEnabled = review?.state == "ready"
        compare.setEnabled(review?.candidate != nil, forSegment: 2)
        if compare.selectedSegment == 2, review?.candidate == nil { compare.selectedSegment = 0 }
        preview.image = compare.selectedSegment == 2 ? review?.candidate?.prepared.preview
            : review?.candidate?.before ?? initialImage
        preview.sourceMask = sourceMask; preview.showSource = compare.selectedSegment == 0
        preview.nativeDetail = zoom.selectedSegment == 1
    }
    private func append(_ operation: [String: Any]) {
        do { try sourceMask.append(operation); review?.invalidate(); refresh() }
        catch { status.stringValue = error.localizedDescription }
    }
    func controlTextDidChange(_ obj: Notification) { review?.invalidate(); refresh() }
    @objc private func changed(_ sender: Any?) {
        if let value = Double(radius.stringValue), value.isFinite, value > 0, value <= 4096 { preview.radius = value }
        preview.mode = ContentAwareFillCanvasView.Mode(rawValue: sourceMode.indexOfSelectedItem) ?? .pan
        ignore.isEnabled = sample.indexOfSelectedItem == RetouchSampling.allCases.firstIndex(of: .all)
        if let sender = sender as? NSPopUpButton, sender === sourceMode { return }
        if let sender = sender as? NSTextField, sender === radius { return }
        review?.invalidate(); refresh()
    }
    @objc private func displayChanged(_ sender: Any?) { refresh() }
    @objc private func resetAll(_ sender: Any?) { sourceMask = ContentAwareSourceMask(); review?.invalidate(); refresh() }
    @objc private func resetNone(_ sender: Any?) { sourceMask = ContentAwareSourceMask(initial: "none"); review?.invalidate(); refresh() }
    @objc private func prepareClicked(_ sender: Any?) {
        do {
            let settings = try settings()
            guard let review, let document = editor?.document else { return }
            try review.capture.requireCurrent(document)
            if let reason = RetainedImagePolicy.refusal(tool: "content_aware_fill", arguments: settings.arguments(layer: review.layer), document: document) {
                throw DoctoredCoreError(message: reason)
            }
            review.request(settings)
        } catch { status.stringValue = error.localizedDescription; applyButton?.isEnabled = false }
    }
    @objc private func applyClicked(_ sender: Any?) {
        do {
            guard let review, let document = editor?.document else { return }
            let candidate = try review.requireCandidate()
            if review.settings.output == .layer {
                guard document.confirmRasterize(layer: review.layer) else { return }
            }
            try ContentAwareFillPublication.apply(candidate, capture: review.capture, settings: review.settings,
                layer: review.layer, to: document, record: review.settings.record(layer: review.layer))
            ToolOptionsStore.shared.contentAwareFill = ContentAwareFillOptions(ring: Double(review.settings.ring),
                sample: review.settings.choice.mode, ignoreAdjustments: review.settings.choice.ignoreAdjustments,
                seed: Int(review.settings.seed))
            close()
        } catch { status.stringValue = error.localizedDescription; applyButton?.isEnabled = false }
    }
    @objc private func cancelClicked(_ sender: Any?) { close() }
    private func close() {
        guard !closing else { return }
        closing = true
        review?.cancel(); review = nil; preview.image = nil; preview.selectionPath = nil
        initialImage = nil
        // Document close may already have detached the sheet. Dismissing an
        // unpresented controller raises an Objective-C exception in AppKit.
        if let presenter = presentingViewController,
           presenter.presentedViewControllers?.contains(where: { $0 === self }) == true {
            presenter.dismiss(self)
        }
    }
}
