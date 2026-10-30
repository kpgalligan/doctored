import AppKit

/// Subject targeting with a reusable immutable Vision analysis, for foreground
/// objects (Select ▸ Select Objects…) or individual people (Select ▸ Select
/// People…). The picker can combine subjects by list, image hit test or the
/// captured selection region. For people, All People commits the analysis'
/// aggregate coverage, as Select ▸ All People does.
final class SubjectTargetsSheetController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private weak var editor: EditorViewController?
    let capture: SelectionCapture
    let analysis: SubjectAnalysis
    let model: SubjectModel
    private let boxes: [CGRect]
    private let sourceView = SelectionPreviewView(frame: .zero)
    private let maskView = SelectionPreviewView(frame: .zero)
    private let table = NSTableView()
    private let modePopup = NSPopUpButton()
    private let featherField = NSTextField(string: "0")
    private let status = NSTextField(labelWithString: "")
    private var applyButton: NSButton!
    private let renderer = PreviewRenderer()
    private var previewToken: RasterJobControl?

    init(editor: EditorViewController, document: ImageDocument, model: SubjectModel = .objects) throws {
        self.editor = editor
        self.model = model
        capture = try SelectionCapture(document: document)
        analysis = try SubjectTargeting.analyze(capture, model: model, in: document)
        boxes = analysis.instanceBounds
        super.init(nibName: nil, bundle: nil)
        sourceView.image = capture.source?.makeCGImage(in: capture.doc.colorSpace)
        sourceView.onPoint = { [weak self] point in self?.toggleObject(at: point) }
        renderer.onRender = { [weak self] image in self?.maskView.image = image }
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("SubjectTargetsSheetController does not support NSCoder") }

    override func loadView() {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("object"))
        column.title = model.columnTitle
        column.width = 500
        table.addTableColumn(column)
        table.dataSource = self
        table.delegate = self
        table.allowsMultipleSelection = true
        table.allowsEmptySelection = true
        table.rowHeight = 23
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        scroll.widthAnchor.constraint(equalToConstant: 512).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 110).isActive = true
        modePopup.addItems(withTitles: ["Replace", "Add", "Subtract", "Intersect"])
        let region = StickerButton(title: "Use Selection Region", style: .secondary,
                                   target: self, action: #selector(useSelectionRegion(_:)))
        region.isEnabled = capture.selection.selection != nil
        let all = StickerButton(title: model.allButtonTitle, style: .secondary,
                                target: self, action: #selector(selectAllObjects(_:)))
        let buttons = NSStackView(views: [all, region, modePopup])
        buttons.orientation = .horizontal
        buttons.spacing = 8
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.minimumFractionDigits = 0
        formatter.maximumFractionDigits = 1
        featherField.formatter = formatter
        featherField.doubleValue = 0
        featherField.widthAnchor.constraint(equalToConstant: 80).isActive = true
        DSField.style(featherField)
        let unit = NSTextField(labelWithString: "px")
        unit.font = DS.mono(13)
        unit.textColor = DS.textMuted
        let options = NSStackView(views: [fieldLabel("Feather:"), featherField, unit])
        options.orientation = .horizontal
        options.spacing = 8
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        let content = NSStackView(views: [selectionPreviewRow(sourceView, maskView), scroll, buttons, options, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(title: model.title,
            hint: model.hint,
            content: content, buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: applyButton, leading: [sheetFootnote("feather 0 – 250 px")]), width: 556)
        table.selectRowIndexes(IndexSet(integersIn: 0..<analysis.instanceCount), byExtendingSelection: false)
        requestPreview()
    }

    var selectedInstances: [Int] { table.selectedRowIndexes.map { $0 + 1 } }
    /// The feather field's value; an empty field reads as 0, its own default.
    var feather: Double { featherField.doubleValue }
    func numberOfRows(in tableView: NSTableView) -> Int { analysis.instanceCount }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard boxes.indices.contains(row) else { return nil }
        let box = boxes[row]
        let label = NSTextField(labelWithString: "\(model.rowLabel(row + 1))    \(Int(box.minX)), \(Int(box.minY)) · \(Int(box.width)) × \(Int(box.height))")
        label.font = DS.mono(12)
        return label
    }
    func tableViewSelectionDidChange(_ notification: Notification) {
        guard isViewLoaded, applyButton != nil else { return }
        requestPreview()
    }

    func toggleObject(at point: CGPoint) {
        guard let instance = analysis.subject(at: point) else { NSSound.beep(); return }
        var rows = table.selectedRowIndexes
        if rows.contains(instance - 1) { rows.remove(instance - 1) } else { rows.insert(instance - 1) }
        table.selectRowIndexes(rows, byExtendingSelection: false)
    }

    func requestPreview() {
        previewToken?.cancel()
        let instances = selectedInstances
        applyButton.isEnabled = !instances.isEmpty
        guard !instances.isEmpty else { maskView.image = nil; status.stringValue = model.emptyStatus; return }
        do {
            let reservation = try SelectionWorkBudget.reservePreview(bytes: SubjectTargeting.scratchBytes(capture),
                outputBytes: UInt64(capture.doc.width * capture.doc.height))
            let token = reservation.control
            previewToken = token
            renderer.cancelCompute = { token.cancel() }
            let analysis = self.analysis
            renderer.request { reservation.run { token in
                guard !token.isCancelled, let mask = analysis.mask(subjects: instances),
                      !token.isCancelled else { return nil }
                return CanvasSelection.grayImage(mask, analysis.width, analysis.height)
            } }
            status.stringValue = model.status(selected: instances.count, of: analysis.instanceCount)
        } catch { maskView.image = nil; status.stringValue = error.localizedDescription }
    }

    /// Objects: chooses every row. People: commits All People at once.
    @objc func selectAllObjects(_ sender: Any?) {
        guard model == .people else {
            table.selectRowIndexes(IndexSet(integersIn: 0..<analysis.instanceCount), byExtendingSelection: false)
            return
        }
        guard let mode = chosenMode() else { NSSound.beep(); return }
        let feather = self.feather
        renderer.cancel()
        dismiss(self)
        editor?.applyAllPeople(capture, analysis: analysis, mode: mode, feather: feather)
    }
    @objc private func useSelectionRegion(_ sender: Any?) {
        guard let region = capture.selection.selection?.bounds else { return }
        table.selectRowIndexes(IndexSet(analysis.subjects(intersecting: region).map { $0 - 1 }), byExtendingSelection: false)
    }
    /// The combine mode, or nil when the feather field is out of range.
    private func chosenMode() -> SelectionCombineMode? {
        guard feather.isFinite, (0...250).contains(feather) else { return nil }
        return [.replace, .add, .subtract, .intersect][max(0, modePopup.indexOfSelectedItem)]
    }
    @objc func applyClicked(_ sender: Any?) {
        guard !selectedInstances.isEmpty, let mode = chosenMode() else { NSSound.beep(); return }
        let instances = selectedInstances, feather = self.feather
        renderer.cancel()
        dismiss(self)
        editor?.applySubjectTargets(capture, analysis: analysis, instances: instances, mode: mode, feather: feather)
    }
    @objc func cancelClicked(_ sender: Any?) { renderer.cancel(); dismiss(self) }
}
