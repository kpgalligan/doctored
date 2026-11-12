import AppKit

/// Edit ▸ Define Pattern…: the composite inside a hard-edged rectangular selection — or the whole
/// canvas with nothing selected — saved under a name in the user's pattern library
/// (`PatternLibrary`; the tile's form per depth and mode is `DefinePattern`'s). The tile is read
/// when the item is chosen, so the sheet names its size; a library write is a preference: no
/// undo step, no dirty flag, no Action step. MCP mirror: `define_pattern`
/// (`AgentServer+Patterns.swift`).
extension EditorViewController {
    @objc func definePattern(_ sender: Any?) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let tile: DefinePattern.Tile
        do {
            tile = try DefinePattern.tile(
                of: doc, in: document, selection: document.canvasSelection)
        } catch {
            showFillAlert(error.localizedDescription)
            return
        }
        let sheet = DefinePatternSheetController(tile: tile) { name in
            Self.savePattern(name: name, tile: tile)
        }
        presentAsSheet(sheet)
    }

    /// The sheet's commit: nil when the library saved the tile, else the library's refusal.
    static func savePattern(name: String, tile: DefinePattern.Tile) -> String? {
        do {
            try PatternLibrary.save(
                name: name, width: tile.width, height: tile.height, rgba: tile.rgba, replace: false)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    /// Define Pattern… and the Edit ▸ Patterns items; nil for any other item. Define Pattern… is
    /// off in Quick Mask, whose selection is coverage being edited, and while a stroke is being
    /// painted; the library items read no document, so nothing here turns them off
    /// (`EditorViewController+PatternLibrary.swift`).
    func validateDefinePatternItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        if let action = item.action, [#selector(deleteLibraryPattern(_:)),
                                      #selector(importPatternLibrary(_:)),
                                      #selector(exportPatternLibrary(_:))].contains(action) {
            return true
        }
        guard item.action == #selector(definePattern(_:)) else { return nil }
        guard let document, document.doc != nil else { return false }
        return !document.quickMaskActive && !canvas.isPaintingStroke
    }
}

/// Define Pattern's sheet: the tile's size and where it came from, and the name to save it
/// under. Apply asks `onSave`; a refusal (a name in use, a built-in's name) stays on the sheet.
final class DefinePatternSheetController: NSViewController {
    private let tile: DefinePattern.Tile
    private let onSave: (String) -> String?
    private let nameField = NSTextField(string: "Pattern")
    private let status = NSTextField(labelWithString: "")

    init(tile: DefinePattern.Tile, onSave: @escaping (String) -> String?) {
        self.tile = tile
        self.onSave = onSave
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("DefinePatternSheetController does not support NSCoder")
    }

    override func loadView() {
        DSField.style(nameField)
        nameField.widthAnchor.constraint(equalToConstant: 220).isActive = true
        nameField.setAccessibilityIdentifier("define-pattern-name")
        let swatch = NSImageView(image: LibraryPattern(
            name: "Pattern", width: tile.width, height: tile.height, rgba: tile.rgba).swatch)
        swatch.imageScaling = .scaleProportionallyUpOrDown
        swatch.widthAnchor.constraint(equalToConstant: 32).isActive = true
        swatch.heightAnchor.constraint(equalToConstant: 32).isActive = true
        let size = NSTextField(labelWithString: "\(tile.width) × \(tile.height) px, from the "
            + (tile.source == "canvas" ? "whole canvas" : "selection"))
        size.font = DS.sans(13)
        size.textColor = DS.textMuted
        status.font = DS.sans(12)
        status.textColor = DS.textMuted
        let grid = NSGridView(views: [
            [fieldLabel("Name:"), nameField],
            [fieldLabel("Tile:"), NSStackView(views: [swatch, size])],
            [NSGridCell.emptyContentView, status],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Define Pattern",
            hint: "Saves the composite inside the rectangular selection — or the whole canvas "
                + "with nothing selected — as a pattern for Edit ▸ Fill…, Pattern Overlay, "
                + "pattern strokes, bevel textures and pattern fill layers.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("a preference: no undo step")]))
    }

    @objc private func applyClicked(_ sender: Any?) {
        if let refusal = onSave(nameField.stringValue) {
            NSSound.beep()
            status.stringValue = refusal
            return
        }
        dismiss(self)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
