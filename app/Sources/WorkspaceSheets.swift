import AppKit

/// Window ▸ Workspaces ▸ Save Workspace… — one field and two buttons, on the
/// shared `makeSheetView` / `makeButtonRow` builders so it looks exactly like
/// every other dialog in the app.
///
/// It only NAMES the layout; capturing and storing it is
/// `EditorViewController.saveWorkspace(_:)`, so there is one writer.
final class WorkspaceNameSheetController: NSObject {
    private let field = NSTextField()
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var name: String?

    private override init() {
        super.init()
    }

    /// The trimmed name, or nil when the sheet was cancelled.
    static func run() -> String? {
        WorkspaceNameSheetController().present()
    }

    private func present() -> String? {
        DSField.style(field)
        field.placeholderString = "Workspace name"
        field.widthAnchor.constraint(equalToConstant: 330).isActive = true
        field.setAccessibilityIdentifier("workspace-name")
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340
        let content = NSStackView(views: [field, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let save = StickerButton(title: "Save Workspace", style: .primary, target: self,
                                 action: #selector(saveClicked(_:)))
        save.setAccessibilityIdentifier("workspace-save")
        modal = ModalCardWindowController(title: "Save Workspace", card: makeSheetView(
            title: "Save Workspace",
            hint: "The window frame, the right panel's tab, width and visibility, the rulers, "
                + "grid and snap toggles, and the current tool. The document's zoom and scroll "
                + "position are not saved: those belong to the picture, not to the layout.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: save)))
        _ = modal?.runModal()
        return name
    }

    @objc private func cancelClicked(_ sender: Any?) {
        modal?.end(.cancel)
    }

    @objc private func saveClicked(_ sender: Any?) {
        do {
            name = try WorkspaceLibrary.normalizedName(field.stringValue)
            modal?.end(.OK)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }
}
