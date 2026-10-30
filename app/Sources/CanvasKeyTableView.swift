import AppKit

/// The Layers and Channels panels' table: a bare key (no ⌘⌥⌃) the
/// table has no use for goes to the canvas instead of type-to-select, so a
/// tool key, X/D, Esc's deselect (or a running job's cancel) and every other
/// canvas key keep working after a click in the panel. The table keeps its
/// navigation and editing keys: the arrows, Home/End/Page Up/Page Down,
/// Return/Enter, Tab, Space and Delete/Forward Delete. Because every other
/// bare key leaves here, type-to-select never starts.
///
/// Both panels build their table with `refusesFirstResponder = true`, so a
/// click selects a row and leaves focus where it was: in the app as built a
/// key reaches this override only if something else puts the table in focus
/// or sends it the event. The override is the guard for that case, so the
/// rule does not depend on the refusal staying.
final class CanvasKeyTableView: NSTableView {
    /// Arrows, Home, End, Page Up, Page Down, Return, Enter, Tab, Space,
    /// Delete and Forward Delete.
    private static let tableKeys: Set<UInt16> = [
        123, 124, 125, 126, 115, 119, 116, 121, 36, 76, 48, 49, 51, 117,
    ]

    override func keyDown(with event: NSEvent) {
        let bare = event.modifierFlags.intersection([.command, .option, .control]).isEmpty
        if bare, !Self.tableKeys.contains(event.keyCode),
           let window = window as? EditorWindow, window.handKeyToCanvas(event) {
            return
        }
        super.keyDown(with: event)
    }
}
