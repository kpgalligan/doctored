import AppKit

// `JobEscape` is the bare-Esc test, the text-view exemption over
// `DocumentJobs.escapeCancelsUserJob`, and the pre-override `cancelOperation` forward.
//
// `EditorWindow` uses the same rule as `JobEscape`.

/// Esc cancels the document's running UI job from a window that is not the
/// canvas: the editor window itself, and the document's
/// floating History, Snapshots and Layer Comps panels and the Actions window.
///
/// - A bare Esc (no ⌘⌥⌃) that reaches the window's `keyDown` asks the
///   rule; so does `cancelOperation`, which is how a key-interpreting view
///   (a table) passes Esc up the chain.
/// - The rule is `DocumentJobs.escapeCancelsUserJob`: it cancels on the
///   first press and swallows every later one while the job drains.
/// - A text view keeps its Esc. With no running UI job the key goes where it
///   went before: `super`, or, for `cancelOperation`, on up the chain.
enum JobEscape {
    static func isBareEscape(_ event: NSEvent) -> Bool {
        event.keyCode == 53 && event.modifierFlags.intersection([.command, .option, .control]).isEmpty
    }

    static func cancelsJob(in window: NSWindow, document: ImageDocument?) -> Bool {
        if window.firstResponder is NSText { return false }
        return DocumentJobs.escapeCancelsUserJob(on: document)
    }

    /// An unconsumed `cancelOperation` when the window's superclass does not
    /// implement it: what the responder chain did before the override.
    static func forwardCancel(_ sender: Any?, from window: NSWindow) {
        if let next = window.nextResponder {
            next.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        } else {
            NSSound.beep()
        }
    }
}

/// The document's History, Snapshots and Layer Comps panels. ⌘Z/⇧⌘Z there
/// are the document's main editor's Undo/Redo — a step, or "Cancel
/// <title>" over its running UI job — validated by the editor too; a field
/// editor in the panel keeps its own typing undo.
final class JobEscapePanel: NSPanel {
    var jobDocument: () -> ImageDocument? = { nil }

    /// The editor ⌘Z/⇧⌘Z forward to; nil while a field editor here has focus.
    private var undoEditor: EditorViewController? {
        firstResponder is NSText ? nil : jobDocument()?.editors.first
    }

    @objc func undo(_ sender: Any?) {
        if let editor = undoEditor { editor.undo(sender) } else { firstResponder?.undoManager?.undo() }
    }

    @objc func redo(_ sender: Any?) {
        if let editor = undoEditor { editor.redo(sender) } else { firstResponder?.undoManager?.redo() }
    }

    override func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        guard item.action == #selector(undo(_:)) || item.action == #selector(redo(_:)) else {
            return super.validateUserInterfaceItem(item)
        }
        if let editor = undoEditor { return editor.validateUserInterfaceItem(item) }
        guard let manager = firstResponder?.undoManager else { return false }
        let undo = item.action == #selector(undo(_:))
        if let menuItem = item as? NSMenuItem {
            menuItem.title = undo ? manager.undoMenuItemTitle : manager.redoMenuItemTitle
        }
        return undo ? manager.canUndo : manager.canRedo
    }

    override func keyDown(with event: NSEvent) {
        if JobEscape.isBareEscape(event), JobEscape.cancelsJob(in: self, document: jobDocument()) { return }
        super.keyDown(with: event)
    }

    override func cancelOperation(_ sender: Any?) {
        if JobEscape.cancelsJob(in: self, document: jobDocument()) { return }
        if NSPanel.instancesRespond(to: #selector(NSResponder.cancelOperation(_:))) {
            super.cancelOperation(sender)
        } else {
            JobEscape.forwardCancel(sender, from: self)
        }
    }
}

/// The Actions window: its document is Play's target.
final class JobEscapeWindow: NSWindow {
    var jobDocument: () -> ImageDocument? = { nil }

    override func keyDown(with event: NSEvent) {
        if JobEscape.isBareEscape(event), JobEscape.cancelsJob(in: self, document: jobDocument()) { return }
        super.keyDown(with: event)
    }

    override func cancelOperation(_ sender: Any?) {
        if JobEscape.cancelsJob(in: self, document: jobDocument()) { return }
        if NSWindow.instancesRespond(to: #selector(NSResponder.cancelOperation(_:))) {
            super.cancelOperation(sender)
        } else {
            JobEscape.forwardCancel(sender, from: self)
        }
    }
}
