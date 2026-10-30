import AppKit

/// File ▸ Contact Sheet… — compose this document's items into a grid, as a new
/// document or straight to a file through an 11A preset.
///
/// The sheet chooses the layout and the destination; `ImageDocument
/// .contactSheet` does the work, so the sheet, MCP and a replayed Action share
/// one path and one set of refusals.
final class ContactSheetSheetController: NSViewController {
    private weak var document: ImageDocument?
    private let itemsPopup = NSPopUpButton()
    private let columnsField = NSTextField()
    private let cellWidthField = NSTextField()
    private let cellHeightField = NSTextField()
    private let gutterField = NSTextField()
    private let captionField = NSTextField()
    private let backgroundField = NSTextField()
    private let captionBox = NSButton(
        checkboxWithTitle: "Caption each cell with its layer name", target: nil, action: nil)
    private let outputPopup = NSPopUpButton()
    private let presetPopup = NSPopUpButton()
    private let summary = NSTextField(wrappingLabelWithString: "")
    private let message = NSTextField(wrappingLabelWithString: "")
    private var presets: [ExportPreset] = []

    init(document: ImageDocument) throws {
        guard document.doc != nil else {
            throw DoctoredCoreError(message: "No image loaded.")
        }
        self.document = document
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ContactSheetSheetController does not support NSCoder")
    }

    override func loadView() {
        itemsPopup.addItems(withTitles: LayerExportItems.allCases.map(\.displayName))
        itemsPopup.selectItem(at: LayerExportItems.allCases.firstIndex(of: .all) ?? 0)
        outputPopup.addItems(withTitles: ["New document", "Export through a preset"])
        for popup in [itemsPopup, outputPopup, presetPopup] {
            popup.widthAnchor.constraint(equalToConstant: 200).isActive = true
            popup.target = self
            popup.action = #selector(controlChanged(_:))
        }
        itemsPopup.setAccessibilityIdentifier("contact-sheet.items")
        outputPopup.setAccessibilityIdentifier("contact-sheet.output")
        presetPopup.setAccessibilityIdentifier("contact-sheet.preset")

        let defaults = ContactSheetLayout.defaults
        for (field, value, identifier) in [
            (columnsField, "\(defaults.columns)", "columns"),
            (cellWidthField, "\(defaults.cellWidth)", "cell-width"),
            (cellHeightField, "\(defaults.cellHeight)", "cell-height"),
            (gutterField, "\(defaults.gutter)", "gutter"),
            (captionField, "\(defaults.captionHeight)", "caption-height"),
        ] {
            DSField.style(field)
            field.stringValue = value
            field.widthAnchor.constraint(equalToConstant: 72).isActive = true
            field.target = self
            field.action = #selector(controlChanged(_:))
            field.setAccessibilityIdentifier("contact-sheet.\(identifier)")
        }
        DSField.style(backgroundField)
        backgroundField.stringValue = defaults.background
        backgroundField.widthAnchor.constraint(equalToConstant: 110).isActive = true
        backgroundField.target = self
        backgroundField.action = #selector(controlChanged(_:))
        backgroundField.setAccessibilityIdentifier("contact-sheet.background")
        captionBox.font = DS.sans(12)
        captionBox.state = .on
        captionBox.target = self
        captionBox.action = #selector(controlChanged(_:))

        for label in [summary, message] {
            label.font = DS.sans(11)
            label.textColor = DS.textMuted
            label.preferredMaxLayoutWidth = 380
        }

        let grid = NSStackView(views: [row("Columns", columnsField),
                                       row("Cell width", cellWidthField),
                                       row("Cell height", cellHeightField)])
        grid.spacing = 12
        let spacing = NSStackView(views: [row("Gutter", gutterField),
                                          row("Caption strip", captionField),
                                          row("Background", backgroundField)])
        spacing.spacing = 12

        let content = NSStackView(views: [
            row("Items", itemsPopup), grid, spacing, captionBox,
            row("Output", outputPopup), row("Preset", presetPopup), summary, message,
        ])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let build = StickerButton(title: "Build Sheet", style: .primary,
                                  target: self, action: #selector(buildClicked(_:)))
        build.setAccessibilityIdentifier("contact-sheet.run")
        view = makeSheetView(
            title: "Contact Sheet",
            hint: "Cells are 8-bit sRGB and are never upscaled; a transparent item shows the "
                + "background colour. The sources are not changed.",
            content: content,
            buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: build),
            width: 540)
        reloadPresets()
        controlChanged(nil)
    }

    private func row(_ label: String, _ control: NSView) -> NSStackView {
        let row = NSStackView(views: [fieldLabel(label), control])
        row.orientation = .horizontal
        row.spacing = 8
        return row
    }

    private var mode: LayerExportItems {
        let index = itemsPopup.indexOfSelectedItem
        return LayerExportItems.allCases.indices.contains(index)
            ? LayerExportItems.allCases[index] : .all
    }

    private var layout: ContactSheetLayout {
        var layout = ContactSheetLayout.defaults
        layout.columns = Int(columnsField.stringValue) ?? layout.columns
        layout.cellWidth = Int(cellWidthField.stringValue) ?? layout.cellWidth
        layout.cellHeight = Int(cellHeightField.stringValue) ?? layout.cellHeight
        layout.gutter = Int(gutterField.stringValue) ?? layout.gutter
        layout.captionHeight = Int(captionField.stringValue) ?? layout.captionHeight
        layout.captions = captionBox.state == .on
        layout.background = backgroundField.stringValue
        return layout
    }

    private func reloadPresets() {
        presetPopup.removeAllItems()
        do {
            presets = try ExportPresetLibrary.entries()
            presetPopup.addItems(withTitles: presets.map(\.name))
        } catch {
            presets = []
            message.stringValue = error.localizedDescription
        }
    }

    @objc private func controlChanged(_ sender: Any?) {
        presetPopup.isEnabled = outputPopup.indexOfSelectedItem == 1 && !presets.isEmpty
        guard let document else { return }
        let count = LayerExport.items(document, mode).count
        // The CORE owns the geometry; ask it so the readout and the sheet
        // cannot disagree, and show its refusal when the layout is impossible.
        do {
            let (w, h, rows) = try RasterExport.contactSheetSize(request: layout.requestJSON,
                                                                 count: count)
            summary.stringValue = "\(count) cells · \(rows) rows · \(w)×\(h) px"
            message.stringValue = ""
        } catch {
            summary.stringValue = "\(count) cells"
            message.stringValue = error.localizedDescription
        }
    }

    @objc private func buildClicked(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        var output = ContactSheetOutput.document
        if outputPopup.indexOfSelectedItem == 1 {
            let index = presetPopup.indexOfSelectedItem
            guard presets.indices.contains(index) else {
                message.stringValue = "Choose an export preset, or output a new document."
                NSSound.beep()
                return
            }
            let preset = presets[index]
            let settings: ExportSettings
            do { settings = try preset.resolvedSettings() } catch {
                message.stringValue = error.localizedDescription
                NSSound.beep()
                return
            }
            output = .export(settings: settings, destination: preset.destination,
                             pattern: preset.pattern, preset: preset.name)
        }
        do {
            let outcome = try document.contactSheet(items: mode, layout: layout, output: output)
            dismiss(self)
            if let url = outcome.url {
                document.presentExportNotice(
                    "Wrote \(url.path)\n\(outcome.cells) cells · \(outcome.width)×\(outcome.height) px")
            }
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }

    @objc private func cancelClicked(_ sender: Any?) { dismiss(self) }
}
