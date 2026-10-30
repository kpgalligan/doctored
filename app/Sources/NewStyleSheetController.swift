import AppKit

/// Window ▸ Styles ▸ New Style…: the New Style dialog
/// over the active layer — a name, an optional group, Include Layer Effects
/// and Include Layer Blending Options — saved into the style-preset library
/// (`StylePreset.fromLayer`, `StylePresetLibrary.save`). A library write is
/// a preference: no undo step, no Action step. MCP twin:
/// `save_style_preset`.
final class NewStyleSheetController: NSViewController {
    private let document: ImageDocument
    private let layer: Int
    private let nameField = NSTextField(string: "Style 1")
    private let groupField = NSTextField(string: "")
    private let effectsBox = NSButton(checkboxWithTitle: "Include Layer Effects", target: nil,
                                      action: nil)
    private let blendingBox = NSButton(checkboxWithTitle: "Include Layer Blending Options",
                                       target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")

    init(document: ImageDocument, layer: Int) {
        self.document = document
        self.layer = layer
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("NewStyleSheetController does not support NSCoder")
    }

    override func loadView() {
        for field in [nameField, groupField] {
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 240).isActive = true
        }
        groupField.placeholderString = "No group"
        effectsBox.state = .on
        blendingBox.state = .off
        message.font = DS.sans(12)
        message.textColor = DS.warningText
        message.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        let grid = NSGridView(views: [
            [fieldLabel("Name:"), nameField],
            [fieldLabel("Group:"), groupField],
            [NSView(), effectsBox],
            [NSView(), blendingBox],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let content = NSStackView(views: [grid, message])
        content.orientation = .vertical
        content.alignment = .leading
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let save = StickerButton(title: "Save", style: .primary, target: self,
                                 action: #selector(saveClicked(_:)))
        view = makeSheetView(
            title: "New Style",
            hint: "Saves the active layer's style as a preset in the Styles panel, for every "
                + "document. Saving changes no picture and adds no undo step.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancel, apply: save))
    }

    @objc private func saveClicked(_ sender: Any?) {
        guard let doc = document.doc else {
            NSSound.beep()
            return
        }
        // "/" separates a group from a name in the library's key; the field
        // turns it into "-".
        let name = nameField.stringValue.replacingOccurrences(of: "/", with: "-")
        do {
            let preset = try StylePreset.fromLayer(
                doc, layer: layer, name: name, group: groupField.stringValue,
                includeEffects: effectsBox.state == .on, includeBlending: blendingBox.state == .on)
            try StylePresetLibrary.save(preset, replace: false)
            dismiss(self)
            StylesPanelController.shared.show()
        } catch {
            NSSound.beep()
            message.stringValue = error.localizedDescription
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
