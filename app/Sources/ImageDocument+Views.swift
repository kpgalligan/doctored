import AppKit

// **The per-VIEW vs per-DOCUMENT rule:** zoom, scroll, rotation, the panels and every canvas
// SESSION (text, transform, crop box, Quick Mask buffer, selection stroke) are per WINDOW; the
// picture, the history, the undo stack, the selection, the layer selection, the paint target, the
// channel eyes and Sync Pan & Zoom are per DOCUMENT. Target mirroring rides
// `.imageDocumentViewTargetsDidChange` — a channel of its own, posted by
// `notifyViewTargetsChanged()` only on a real change, only with more than one window controller and
// never inside a publication or history-navigation window — and NOTHING new may post
// `.imageDocumentImageDidChange`, which fires on every live drag tick and would change what
// undo/revert deliver to a one-window document.

extension Notification.Name {
    /// Posted (object: the document) when a value the OTHER views of the same
    /// document must mirror actually changed — the layer selection, the paint
    /// target, the channel eyes.
    ///
    /// Deliberately a channel of its own rather than `.imageDocumentImageDidChange`:
    /// that one fires on every live drag tick and inside every publication
    /// window, so hanging cross-view mirroring on it would have changed what
    /// undo, redo, revert and a depth conversion deliver to a single-window
    /// document. This one is posted ONLY when a value changed, ONLY when the
    /// document has more than one window, and NEVER while a state is being
    /// published or history is being navigated.
    static let imageDocumentViewTargetsDidChange =
        Notification.Name("ImageDocumentViewTargetsDidChange")
}

/// Phase 12D-2's document half: the second view.
///
/// A second view is a second `EditorWindowController` on the SAME `NSDocument`
/// — AppKit's own multiple-view model — so there is one `doc`, one history and
/// one undo stack by construction. NSDocument's default
/// `shouldCloseWindowController` already keeps the document open while another
/// view exists and asks to save only on the last one, and `close()` closes them
/// all with one prompt.
extension ImageDocument {
    /// Every new window controller comes from here, so an ordinal is minted
    /// exactly once and never re-used inside a session.
    func makeViewWindowController() -> EditorWindowController {
        let controller = EditorWindowController(document: self, ordinal: nextViewOrdinal)
        nextViewOrdinal += 1
        return controller
    }

    /// Window ▸ New View for “<document>” and MCP `open_second_view`.
    /// Returns the new editor, or nil for a document with no image.
    @discardableResult
    func openSecondView() -> EditorViewController? {
        guard doc != nil else { return nil }
        let controller = makeViewWindowController()
        addWindowController(controller)
        showWindows()
        // Both windows' titles change the moment there are two of them.
        for controller in windowControllers {
            controller.synchronizeWindowTitleWithDocumentName()
        }
        return controller.contentViewController as? EditorViewController
    }

    /// This document's editors, in window-controller order — which puts view 1
    /// first, the one rule every MCP handler resolves an unqualified call with.
    var editors: [EditorViewController] {
        windowControllers.compactMap { $0.contentViewController as? EditorViewController }
    }

    func editor(ordinal: Int) -> EditorViewController? {
        for controller in windowControllers {
            if let editor = controller as? EditorWindowController, editor.viewOrdinal == ordinal {
                return editor.contentViewController as? EditorViewController
            }
        }
        return nil
    }

    /// The one poster of `.imageDocumentViewTargetsDidChange`, called from the
    /// `oldValue !=`-guarded `didSet`s on `layerSelection`, `planeEditTarget`
    /// and `channelVisibility`.
    ///
    /// The three guards are what keep a single-window document's behaviour
    /// bit-identical to before this phase: no second window, nothing is posted.
    func notifyViewTargetsChanged() {
        guard windowControllers.count > 1, !isPublishingEditingState, !history.navigating else {
            return
        }
        NotificationCenter.default.post(name: .imageDocumentViewTargetsDidChange, object: self)
    }
}
