import AppKit

/// Layer ▸ Duplicate to Document…'s two fields: the Destination — "New
/// Document" first, then every other open document by name (#547 phase 4c)
/// — and, for a new document, its name, prefilled with the topmost selected
/// root's. An empty name (after trimming) leaves NSDocument's default
/// untitled name. `onApply` receives the name and the chosen destination: 0
/// is a new document, `n` the n-th of `destinations`.
final class DuplicateToNewDocumentSheetController: NSViewController {
    private let nameField = NSTextField(string: "")
    private let destinationPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let destinations: [String]
    private let onApply: (String, Int) -> Void

    init(defaultName: String, destinations: [String] = [],
         onApply: @escaping (String, Int) -> Void) {
        self.destinations = destinations
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
        nameField.stringValue = defaultName
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("DuplicateToNewDocumentSheetController does not support NSCoder")
    }

    override func loadView() {
        nameField.widthAnchor.constraint(equalToConstant: 220).isActive = true
        DSField.style(nameField)
        destinationPopup.addItem(withTitle: "New Document")
        for name in destinations { destinationPopup.addItem(withTitle: name) }
        destinationPopup.target = self
        destinationPopup.action = #selector(destinationChanged(_:))
        let grid = NSGridView(views: [[fieldLabel("Destination:"), destinationPopup],
                                      [fieldLabel("Name:"), nameField]])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Duplicate to Document",
            hint: "Copies the selected layers, with their groups' contents, into a new "
                + "document at this one's depth, size, profile and resolution, or into an open "
                + "document at their own positions, converted into its colour mode and profile.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    /// The name names a new document only.
    @objc private func destinationChanged(_ sender: Any?) {
        nameField.isEnabled = destinationPopup.indexOfSelectedItem == 0
    }

    @objc private func applyClicked(_ sender: Any?) {
        let name = nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let destination = max(0, destinationPopup.indexOfSelectedItem)
        dismiss(self)
        onApply(name, destination)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
