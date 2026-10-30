import AppKit

// Window ▸ Workspaces ▸ Delete Workspace adds a "Delete Broken File" group of
// `deleteWorkspaceFile:` rows carrying the file.
//
// `applyWorkspace` restores the tab through the EXISTING entry points — `showInfoTab()` is the only
// path that installs `infoPanel.selectionProvider`, `showNavigatorTab()` the only one that pushes
// the first thumbnail — and a tool the depth refuses is SKIPPED, not beeped.

/// Saved workspace layouts (Phase 12C-2): capturing a window's furniture,
/// putting it back, and the one path the panel's width is written by.
///
/// A library write here is a PREFERENCE — outside document Undo, no dirty
/// flag, never an Action step — exactly like the export, filter and document
/// preset libraries beside it.
extension EditorViewController {
    /// The frame a capture taken with no window stands in. `.zero` would be
    /// WRITTEN and then refused for ever by `WorkspaceLayout.decode`, whose
    /// frame must have width — a file nothing can read back is worse than a
    /// default size.
    static let placeholderWindowFrame = CGRect(x: 0, y: 0, width: 1200, height: 800)

    /// What this window looks like right now, under `name`.
    func captureWorkspace(name: String) -> WorkspaceLayout {
        let chrome = ToolOptionsStore.shared.view
        return WorkspaceLayout(
            name: name,
            windowFrame: view.window?.frame ?? Self.placeholderWindowFrame,
            panelVisible: layersPanelVisible,
            panelTab: EditorPanelTab(rawValue: panelTab) ?? .layers,
            panelWidth: chrome.panelWidth,
            rulers: chrome.rulers,
            grid: chrome.showGrid,
            snap: chrome.snapEnabled,
            tool: currentTool)
    }

    /// Puts a layout back.
    ///
    /// The TAB is restored through the SAME entry points the tabs themselves
    /// use, never a bare `panelTab = …`: `showInfoTab()` is the only path that
    /// installs `infoPanel.selectionProvider` and seeds the readout
    /// (`EditorViewController+Info.swift`), and `showNavigatorTab()` is the
    /// only one that pushes the first thumbnail. A restored tab that looked
    /// right and measured nothing would be the bug this note prevents.
    func applyWorkspace(_ layout: WorkspaceLayout) {
        if let window = view.window, layout.windowFrame.width > 0,
           !window.styleMask.contains(.fullScreen) {
            // A frame that lies inside the usable area (the union of the
            // screens' visible frames) is kept exactly. Otherwise it is moved,
            // and shrunk if needed, wholly onto one display. A full-screen
            // window keeps its frame.
            window.setFrame(WindowFrameFit.fitted(
                layout.windowFrame, visibleFrames: NSScreen.screens.map(\.visibleFrame),
                fallback: (window.screen ?? NSScreen.main)?.visibleFrame ?? layout.windowFrame),
                display: true)
        }
        applyPanelLayout(visible: layout.panelVisible, tab: layout.panelTab)
        setToolChromeVisible(true)
        var chrome = ToolOptionsStore.shared.view
        chrome.panelWidth = min(max(layout.panelWidth, Double(DS.panelWidthRange.lowerBound)),
                                Double(DS.panelWidthRange.upperBound))
        chrome.rulers = layout.rulers
        chrome.showGrid = layout.grid
        chrome.snapEnabled = layout.snap
        ToolOptionsStore.shared.view = chrome
        viewChromeChanged()
        // A tool the document's depth refuses is SKIPPED rather than beeped:
        // restoring a layout on a 16-bit document is not the moment to tell
        // somebody their Brush is unavailable.
        if DocumentPrecision.allows(layout.tool, document: document) {
            selectTool(layout.tool)
        }
    }

    /// The right panel's visibility and tab — a workspace's panel half, and
    /// what a new window restores from `EditorWindowMemory`. The tab
    /// goes through the tabs' own entry points (see `applyWorkspace`).
    func applyPanelLayout(visible: Bool, tab: EditorPanelTab) {
        layersPanelVisible = visible
        switch tab {
        case .layers:
            panelTab = 0
            updatePanelVisibility()
        case .channels:
            showChannelsTab()
        case .assistant:
            panelTab = 2
            updatePanelVisibility()
        case .info:
            showInfoTab()
        case .navigator:
            showNavigatorTab()
        case .paths:
            showPathsTab()
        }
    }

    /// The right panel's width — the ONE writer, shared by the divider drag
    /// and by a restored layout. App-wide like the rest of the view chrome, so
    /// it broadcasts and every open window follows.
    func setPanelWidth(_ width: CGFloat) {
        let clamped = min(max(width, DS.panelWidthRange.lowerBound),
                          DS.panelWidthRange.upperBound)
        guard Double(clamped) != ToolOptionsStore.shared.view.panelWidth else { return }
        ToolOptionsStore.shared.view.panelWidth = Double(clamped)
        viewChromeChanged()
    }

    /// The divider's drag, forwarded from `loadView`. The delta is
    /// incremental, so this is one addition.
    func dividerDidDrag(_ delta: CGFloat) {
        setPanelWidth(CGFloat(ToolOptionsStore.shared.view.panelWidth) + delta)
    }

    // MARK: - Menu actions (Window ▸ Workspaces)

    /// Window ▸ Workspaces ▸ Save Workspace… — names the layout and stores it.
    /// The UI's save REPLACES an existing name (the sheet asked, and the user
    /// typed the name); MCP's `save_workspace` defaults to `replace: false`,
    /// which is every preset tool's convention.
    @objc func saveWorkspace(_ sender: Any?) {
        guard let name = WorkspaceNameSheetController.run() else { return }
        do {
            try WorkspaceLibrary.save(captureWorkspace(name: name), replace: true)
        } catch {
            NSSound.beep()
            let alert = NSAlert()
            alert.messageText = "Could Not Save the Workspace"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    /// One of the named rows: its `representedObject` is the layout's name.
    @objc func restoreWorkspace(_ sender: Any?) {
        guard let name = (sender as? NSMenuItem)?.representedObject as? String else {
            NSSound.beep()
            return
        }
        do {
            applyWorkspace(try WorkspaceLibrary.named(name))
        } catch {
            NSSound.beep()
            let alert = NSAlert()
            alert.messageText = "Could Not Restore the Workspace"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    /// Window ▸ Workspaces ▸ Delete Workspace ▸ <name>.
    @objc func deleteWorkspace(_ sender: Any?) {
        guard let name = (sender as? NSMenuItem)?.representedObject as? String else {
            NSSound.beep()
            return
        }
        do {
            try WorkspaceLibrary.delete(name: name)
        } catch {
            NSSound.beep()
            let alert = NSAlert()
            alert.messageText = "Could Not Delete the Workspace"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    /// Window ▸ Workspaces ▸ Delete Workspace ▸ Delete Broken File ▸ <file>:
    /// its `representedObject` is the file's basename. The UI twin of
    /// `delete_workspace {file}` — the only way to clear a file that has no
    /// layout name to ask for.
    @objc func deleteWorkspaceFile(_ sender: Any?) {
        guard let file = (sender as? NSMenuItem)?.representedObject as? String else {
            NSSound.beep()
            return
        }
        do {
            try WorkspaceLibrary.delete(file: file)
        } catch {
            NSSound.beep()
            let alert = NSAlert()
            alert.messageText = "Could Not Delete the File"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }
}
