import AppKit

extension AppDelegate {
    func windowMenu() -> NSMenu {
        let menu = NSMenu(title: "Window")
        // ⌃⌘M: ⌘M is Image ▸ Adjustments ▸ Curves….
        // Not `performMiniaturize:` itself: on macOS 26 AppKit forces any item
        // with that action to a bare ⌘ mask (measured: a ⌃⌘M set on one reads
        // back ⌘M), so the item would take ⌘M from Curves….
        menu.addItem(
            item("Minimize", #selector(minimizeWindow(_:)), "m", [.command, .control]))
        menu.addItem(item("Zoom", #selector(NSWindow.performZoom(_:))))
        // A second view of the same document (Phase 12D-2). The title is
        // rewritten with the document's name in validateUserInterfaceItem;
        // neither item takes a chord — they are deliberate, occasional
        // commands, and every free ⌘ chord is worth more elsewhere.
        menu.addItem(item("New View", #selector(EditorViewController.newView(_:))))
        menu.addItem(
            item("Sync Pan & Zoom", #selector(EditorViewController.toggleSyncPanZoom(_:))))
        menu.addItem(item("Snapshots…", #selector(EditorViewController.showSnapshots(_:))))
        menu.addItem(item("Layer Comps…", #selector(EditorViewController.showLayerComps(_:))))
        menu.addItem(item("History…", #selector(EditorViewController.showHistory(_:))))
        menu.addItem(item("Styles", #selector(showStylesPanel(_:))))
        // Saved workspace layouts; the list is rebuilt on every menu update
        // (AppDelegate+Workspaces.swift).
        menu.addItem(submenuItem(workspacesMenu()))
        menu.addItem(.separator())
        menu.addItem(item("Bring All to Front", #selector(NSApplication.arrangeInFront(_:))))
        return menu
    }

    /// Window ▸ Styles: the app-wide Styles panel
    /// (`StylesPanelController`), with or without a document open.
    @objc func showStylesPanel(_ sender: Any?) {
        StylesPanelController.shared.show()
    }

    /// Window ▸ Minimize (⌃⌘M): `performMiniaturize:` on the key window, as
    /// the nil-targeted item reached it before — the Options bar's More panel
    /// forwards it to its host, and a window with no minimize button beeps.
    @objc func minimizeWindow(_ sender: Any?) {
        guard let window = NSApp.keyWindow ?? NSApp.mainWindow else {
            NSSound.beep()
            return
        }
        window.performMiniaturize(sender)
    }
}
