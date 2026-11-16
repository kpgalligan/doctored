import AppKit

/// Layer ▸ Warp Text… (designs/warp-text.md, WT-5): a Style popup (None and the nine envelope
/// styles) and Bend, Horizontal and Vertical sliders from −100 to 100. When a control commits
/// (a slider on release, not per tick) the layer is re-rendered under the chosen warp into
/// `canvas.previewImage`; Cancel clears that preview, and OK clears it and runs
/// `edit_text_layer` through the agent dispatch as one recorded step
/// (`EditorViewController.commitWarpText`). The sheet captures the document, revision and
/// editing state at creation, as Replace Missing Fonts does, and an OK after any of them changed
/// beeps and commits nothing.
final class WarpTextSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?
    private let source: RasterDocument
    private let revision: UInt64
    private let state: EditingState?
    private let layer: Int
    /// The layer's stored description, in the document's ink.
    private let stored: TextLayerPayload
    private let anchor: CGPoint
    private let name: String

    private let stylePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let sliders = (0..<3).map { _ in
        NSSlider(value: 0, minValue: -100, maxValue: 100, target: nil, action: nil)
    }
    private let readouts = (0..<3).map { _ in NSTextField(labelWithString: "0") }
    private let notice = NSTextField(wrappingLabelWithString: "")
    /// The controls as the sheet opens (`controlState`). The sliders hold whole hundredths, so a
    /// stored warp off that grid (an MCP bend of 0.125 shows as 13) reads back rounded; leaving
    /// the controls as they opened keeps the stored warp rather than rewriting it rounded. The
    /// raw controls, not the warp they describe: one that rounds to 0 (a bend of 0.004 shows as
    /// Arch 0/0/0) describes no warp, and None must still remove it.
    private var opened: [Int] = []

    /// Throws unless `layer` is a text layer off a path whose position can be recovered.
    init(editor: EditorViewController, document: ImageDocument, layer: Int) throws {
        guard let source = document.doc, let stored = source.textPayload(layer),
              stored.onPath == nil, let anchor = source.describedAnchor(layer)
        else {
            throw NativeArtworkEvaluation.failure(
                "Warp Text needs a text layer that is not on a path.")
        }
        self.editor = editor
        self.document = document
        self.source = source
        self.revision = document.revision
        self.state = document.captureEditingState()
        self.layer = layer
        self.stored = stored
        self.anchor = anchor
        self.name = source.layerInfo(layer)?.name ?? "Text"
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("WarpTextSheetController does not support NSCoder")
    }

    override func loadView() {
        stylePopup.addItems(withTitles: ["None"] + TextWarp.titles)
        stylePopup.identifier = NSUserInterfaceItemIdentifier("warpText.style")
        stylePopup.target = self
        stylePopup.action = #selector(controlChanged(_:))
        let warp = stored.warp
        stylePopup.selectItem(at: warp.flatMap { TextWarp.styles.firstIndex(of: $0.style) }
            .map { $0 + 1 } ?? 0)
        let values: [Double] = [warp?.bend ?? 0, warp?.horizontal ?? 0, warp?.vertical ?? 0]
        var rows: [[NSView]] = [[fieldLabel("Style:"), stylePopup]]
        for (index, title) in ["Bend:", "Horizontal:", "Vertical:"].enumerated() {
            let slider = sliders[index]
            slider.identifier = NSUserInterfaceItemIdentifier(
                "warpText." + ["bend", "horizontal", "vertical"][index])
            slider.isContinuous = false
            slider.doubleValue = (values[index] * 100).rounded()
            slider.target = self
            slider.action = #selector(controlChanged(_:))
            slider.widthAnchor.constraint(equalToConstant: 200).isActive = true
            readouts[index].font = DS.mono(11)
            readouts[index].alignment = .right
            readouts[index].widthAnchor.constraint(equalToConstant: 40).isActive = true
            let row = NSStackView(views: [slider, readouts[index]])
            row.orientation = .horizontal
            row.spacing = 8
            rows.append([fieldLabel(title), row])
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        notice.font = DS.sans(11)
        notice.preferredMaxLayoutWidth = 376
        notice.widthAnchor.constraint(equalToConstant: 376).isActive = true
        let stack = NSStackView(views: [grid, notice])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        cancel.identifier = NSUserInterfaceItemIdentifier("warpText.cancel")
        let ok = sheetApplyButton(target: self, action: #selector(ok(_:)))
        ok.identifier = NSUserInterfaceItemIdentifier("warpText.ok")
        syncControls()
        opened = controlState
        view = makeSheetView(
            title: "Warp Text",
            hint: "Bends the text layer's glyphs and underlines through an envelope over its "
                + "text block. The layer stays editable text; None removes the warp.",
            content: stack,
            buttonRow: makeButtonRow(cancel: cancel, apply: ok,
                                     leading: [sheetFootnote("one undo step")]))
    }

    // MARK: - State

    /// The warp the controls describe: nil for None or all three numbers at 0.
    var chosen: TextWarp? {
        let index = stylePopup.indexOfSelectedItem - 1
        guard TextWarp.styles.indices.contains(index) else { return nil }
        let numbers = sliders.map { $0.doubleValue.rounded() / 100 }
        let warp = TextWarp(style: TextWarp.styles[index], bend: numbers[0],
                            horizontal: numbers[1], vertical: numbers[2])
        return warp.isIdentity ? nil : warp
    }

    /// The popup's index and each slider's whole value.
    private var controlState: [Int] {
        [stylePopup.indexOfSelectedItem] + sliders.map { Int($0.doubleValue.rounded()) }
    }

    /// Whether the controls leave the stored warp as it is: they describe it, or they are
    /// still as the sheet opened.
    private var keepsStored: Bool { chosen == stored.warp || controlState == opened }

    /// The sliders are off at None; each readout shows its slider's value.
    private func syncControls() {
        let styled = stylePopup.indexOfSelectedItem > 0
        for (slider, readout) in zip(sliders, readouts) {
            slider.isEnabled = styled
            readout.stringValue = "\(Int(slider.doubleValue.rounded()))"
        }
    }

    // MARK: - Preview

    /// The layer re-rendered under the chosen warp, into the canvas's preview: on a worker
    /// for a high-depth document (`ImageDocument.refreshProjection`'s rule), skipped while
    /// other background work runs. The stored warp shows the document itself; an error clears
    /// the preview and is shown.
    func refreshPreview() {
        syncControls()
        guard let editor else { return }
        let chosen = self.chosen
        guard !keepsStored else {
            editor.canvas.previewImage = nil
            notice.stringValue = ""
            return
        }
        let source = self.source
        if source.precision != nil, BackgroundWork.occupied { return }
        do {
            // A layer saved before box widths were stored re-wraps as edit_text_layer does.
            let payload = stored.resolvingLegacyBox(width: max(Double(source.width) - anchor.x, 10))
                .withWarp(chosen)
            guard let candidate = try source.nativeArtwork(
                .text(payload), anchor: anchor, layer: layer, replacing: true, name: name)
            else { throw NativeArtworkEvaluation.failure("The layer refused the warp.") }
            let prepared: PreparedDocumentEdit
            if source.precision != nil {
                prepared = try BackgroundWork.wait(
                    try WorkerTask { PreparedDocumentEdit(candidate) },
                    title: "Preparing Warp Preview")
            } else {
                prepared = PreparedDocumentEdit(candidate)
            }
            if let error = prepared.error { throw NativeArtworkEvaluation.failure(error) }
            editor.canvas.previewImage = prepared.preview
            notice.stringValue = ""
        } catch {
            editor.canvas.previewImage = nil
            notice.stringValue = error.localizedDescription
        }
    }

    // MARK: - Actions

    @objc private func controlChanged(_ sender: Any?) { refreshPreview() }

    @objc private func ok(_ sender: Any?) {
        let chosen = self.chosen
        editor?.canvas.previewImage = nil
        dismiss(self)
        guard !keepsStored else { return }
        guard let editor, let document, document.doc === source, document.revision == revision,
              document.isCurrentEditingState(state)
        else {
            NSSound.beep()
            return
        }
        editor.commitWarpText(chosen, layer: layer)
    }

    @objc private func cancel(_ sender: Any?) {
        editor?.canvas.previewImage = nil
        dismiss(self)
    }
}
