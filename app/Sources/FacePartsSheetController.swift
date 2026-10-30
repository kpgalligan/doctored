import AppKit

/// Select ▸ Face Parts…: the faces Vision found, listed and clickable in the
/// captured image, and the parts to select on them. The preview is the
/// core's own coverage of the choice; Apply hands it to `applyFaceParts`.
final class FacePartsSheetController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    private weak var editor: EditorViewController?
    let capture: SelectionCapture
    let analysis: FaceAnalysis
    private let sourceView = SelectionPreviewView(frame: .zero)
    private let maskView = SelectionPreviewView(frame: .zero)
    private let table = NSTableView()
    private var partBoxes: [FacePart: NSButton] = [:]
    private var checkedParts: Set<FacePart> = [.face]
    private let modePopup = NSPopUpButton()
    private let featherField = NSTextField(string: "0")
    private let status = NSTextField(labelWithString: "")
    private var applyButton: NSButton!
    private let renderer = PreviewRenderer()
    private var previewToken: RasterJobControl?

    init(editor: EditorViewController, document: ImageDocument) throws {
        self.editor = editor
        capture = try SelectionCapture(document: document)
        analysis = try SubjectTargeting.faces(capture, in: document)
        super.init(nibName: nil, bundle: nil)
        sourceView.image = capture.source?.makeCGImage(in: capture.doc.colorSpace)
        sourceView.onPoint = { [weak self] point in self?.toggleFace(at: point) }
        renderer.onRender = { [weak self] image in self?.maskView.image = image }
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("FacePartsSheetController does not support NSCoder") }

    override func loadView() {
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("face"))
        column.title = "Faces (bounds in pixels)"
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
        scroll.heightAnchor.constraint(equalToConstant: 90).isActive = true
        for (index, part) in FacePart.allCases.enumerated() {
            let box = NSButton(checkboxWithTitle: part.title, target: self, action: #selector(partClicked(_:)))
            box.tag = index
            box.font = DS.sans(13)
            box.state = checkedParts.contains(part) ? .on : .off
            partBoxes[part] = box
        }
        let parts = NSStackView(views: FacePart.allCases.compactMap { partBoxes[$0] })
        parts.orientation = .horizontal
        parts.spacing = 10
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
        modePopup.addItems(withTitles: ["Replace", "Add", "Subtract", "Intersect"])
        let options = NSStackView(views: [fieldLabel("Feather:"), featherField, unit, modePopup])
        options.orientation = .horizontal
        options.spacing = 8
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        let content = NSStackView(views: [selectionPreviewRow(sourceView, maskView), scroll, parts, options, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(title: "Face Parts",
            hint: "Click faces in the image to toggle them, or select rows with Command/Shift. Each part is a smooth outline through Vision's face landmarks; the face's forehead is estimated above the brows.",
            content: content, buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: applyButton, leading: [sheetFootnote("feather 0 – 250 px")]), width: 556)
        table.selectRowIndexes(IndexSet(integersIn: 0..<analysis.faces.count), byExtendingSelection: false)
        requestPreview()
    }

    var selectedFaces: [Int] { table.selectedRowIndexes.map { $0 + 1 } }
    var selectedParts: [FacePart] { FacePart.allCases.filter { checkedParts.contains($0) } }

    func numberOfRows(in tableView: NSTableView) -> Int { analysis.faces.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard analysis.faces.indices.contains(row) else { return nil }
        let box = analysis.faces[row].bounds
        let label = NSTextField(labelWithString: "Face \(row + 1)    \(Int(box.minX)), \(Int(box.minY)) · \(Int(box.width)) × \(Int(box.height))")
        label.font = DS.mono(12)
        return label
    }
    func tableViewSelectionDidChange(_ notification: Notification) {
        guard isViewLoaded, applyButton != nil else { return }
        requestPreview()
    }

    func toggleFace(at point: CGPoint) {
        guard let face = analysis.face(at: point) else { NSSound.beep(); return }
        var rows = table.selectedRowIndexes
        if rows.contains(face - 1) { rows.remove(face - 1) } else { rows.insert(face - 1) }
        table.selectRowIndexes(rows, byExtendingSelection: false)
    }

    func setPart(_ part: FacePart, on: Bool) {
        if on { checkedParts.insert(part) } else { checkedParts.remove(part) }
        partBoxes[part]?.state = on ? .on : .off
        guard isViewLoaded, applyButton != nil else { return }
        requestPreview()
    }

    @objc private func partClicked(_ sender: NSButton) {
        guard FacePart.allCases.indices.contains(sender.tag) else { return }
        setPart(FacePart.allCases[sender.tag], on: sender.state == .on)
    }

    private func requestPreview() {
        previewToken?.cancel()
        let faces = selectedFaces, parts = selectedParts
        applyButton.isEnabled = false
        guard !faces.isEmpty, !parts.isEmpty else {
            maskView.image = nil
            status.stringValue = "Choose at least one face and one part."
            return
        }
        do {
            let json = try analysis.regionsJSON(parts: parts, faces: faces)
            let width = analysis.width, height = analysis.height
            // Measured before the preview's own reservation is registered.
            let context = try PrecisionWorkContext()
            let reservation = try SelectionWorkBudget.reservePreview(
                bytes: FaceParts.coverageScratch(width: width, height: height),
                outputBytes: UInt64(width * height) * 2)
            let token = reservation.control
            previewToken = token
            renderer.cancelCompute = { token.cancel() }
            renderer.request { reservation.run { token in
                guard !token.isCancelled,
                      let mask = try? RasterLandmarkCore.coverage(
                        json, width: width, height: height, limit: context.limit,
                        other: context.otherRetained, control: token),
                      !token.isCancelled else { return nil }
                // `CanvasSelection.maskBytes()`'s u16 → byte rounding.
                let bytes = mask.map { UInt8((UInt32($0) + 128) / 257) }
                return CanvasSelection.grayImage(bytes, width, height)
            } }
            applyButton.isEnabled = true
            status.stringValue = "\(parts.map(\.title).joined(separator: ", ")) on \(faces.count) of \(analysis.faces.count) faces."
        } catch {
            maskView.image = nil
            status.stringValue = error.localizedDescription
        }
    }

    @objc func applyClicked(_ sender: Any?) {
        let faces = selectedFaces, parts = selectedParts
        // An empty field reads as 0, which is the field's own default.
        let feather = featherField.doubleValue
        guard !faces.isEmpty, !parts.isEmpty, feather.isFinite, (0...250).contains(feather) else {
            NSSound.beep()
            return
        }
        let mode: SelectionCombineMode = [.replace, .add, .subtract, .intersect][max(0, modePopup.indexOfSelectedItem)]
        renderer.cancel()
        dismiss(self)
        editor?.applyFaceParts(capture, analysis: analysis, parts: parts, faces: faces, mode: mode, feather: feather)
    }
    @objc func cancelClicked(_ sender: Any?) { renderer.cancel(); dismiss(self) }
}
