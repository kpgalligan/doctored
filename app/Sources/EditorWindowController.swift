import AppKit

final class EditorWindowController: NSWindowController {
    /// Which VIEW of its document this window is (Phase 12D-2): 1 for the one
    /// the document opened with, 2 for the first New View. Minted once, by
    /// `ImageDocument.makeViewWindowController`, and never re-used inside a
    /// session — it is the id `list_views` reports and `get_view`/`set_view`
    /// address a window by.
    let viewOrdinal: Int

    init(document: ImageDocument, ordinal: Int) {
        self.viewOrdinal = ordinal
        let window = EditorWindow(
            contentRect: NSRect(x: 0, y: 0, width: 480, height: 320),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false)
        let editor = EditorViewController(document: document)
        window.contentViewController = editor
        window.initialFirstResponder = editor.canvas
        window.backgroundColor = DS.chromeBackground
        window.titlebarAppearsTransparent = false

        // Initial content size: canvas size plus the editor chrome (the right
        // panel's STORED width + separator, tool rail 48, options bar 36,
        // status bar 26), clamped to 80% of the placement screen's visible
        // frame (the key window's screen, so a new document opens on the
        // display the user is working on), minimum 560x360. The store rather
        // than DS.panelWidth since 12C-2: the panel is draggable, and the
        // 1:1-vs-fit decision in viewDidAppear has to see the well this
        // window will actually have.
        var contentSize = document.doc?.canvasSize ?? NSSize(width: 480, height: 320)
        contentSize.width += CGFloat(ToolOptionsStore.shared.view.panelWidth) + 1 + DS.railWidth
        contentSize.height += DS.optionsBarHeight + DS.statusBarHeight
        // …and the two ruler strips when they are on, or a document that
        // exactly fits at 100 % opens zoomed-to-fit instead:
        // EditorViewController.viewDidAppear decides 1:1-vs-fit from
        // scrollView.contentSize, which the strips take a bite out of.
        if ToolOptionsStore.shared.view.rulers {
            contentSize.width += DS.rulerThickness
            contentSize.height += DS.rulerThickness
        }
        let screen = Self.placementScreen()
        if let screen {
            let limit = screen.visibleFrame
            contentSize.width = min(contentSize.width, limit.width * 0.8)
            contentSize.height = min(contentSize.height, limit.height * 0.8)
        }
        contentSize.width = max(contentSize.width, 560)
        contentSize.height = max(contentSize.height, 360)
        window.setContentSize(contentSize)
        // Settle the width before centring: the editor chrome's minimum-width
        // constraints can widen the window past the clamped size, and a later
        // widening keeps the origin, which would leave it right of centre.
        window.layoutIfNeeded()
        // `center()` centres on the window's current screen, so move it onto
        // the placement screen first; on one display this is today's centring.
        if let screen {
            window.setFrameOrigin(screen.visibleFrame.origin)
        }
        window.center()
        // A remembered frame and panel layout win over the canvas-sized
        // default, cascaded off any editor window already at that origin.
        if EditorWindowMemory.isEnabled, let memory = EditorWindowMemory.load() {
            let occupied = NSApp.windows.filter { $0 is EditorWindow && $0.isVisible }.map(\.frame)
            window.setFrame(EditorWindowMemory.placement(
                memory.frame, occupied: occupied,
                visibleFrames: NSScreen.screens.map(\.visibleFrame),
                fallback: screen?.visibleFrame ?? memory.frame), display: false)
            editor.applyPanelLayout(visible: memory.panelVisible,
                                    tab: EditorPanelTab(name: memory.panelTab) ?? .layers)
        }

        super.init(window: window)

        NotificationCenter.default.addObserver(
            self, selector: #selector(editorWindowWillClose(_:)),
            name: NSWindow.willCloseNotification, object: window)
        for name in [NSWindow.didMoveNotification, NSWindow.didEndLiveResizeNotification,
                     NSWindow.didResignKeyNotification] {
            NotificationCenter.default.addObserver(
                self, selector: #selector(rememberWindow(_:)), name: name, object: window)
        }

        // Window title tracks the document name via the standard
        // synchronizeWindowTitleWithDocumentName behavior; only the subtitle
        // is managed here (and by the editor after edits). The design's
        // 38px two-line title bar is the native title + subtitle stack.
        if let doc = document.doc {
            window.subtitle = "\(doc.width) × \(doc.height) px"
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("EditorWindowController does not support NSCoder")
    }

    deinit { NotificationCenter.default.removeObserver(self) }

    /// The screen a new editor window opens on: the key window's, then the
    /// main window's, then `NSScreen.main`.
    private static func placementScreen() -> NSScreen? {
        NSApp.keyWindow?.screen ?? NSApp.mainWindow?.screen ?? NSScreen.main
    }

    override func showWindow(_ sender: Any?) {
        if BackgroundLaunch.keepsWindowsBehind {
            window?.orderBack(sender)
        } else {
            super.showWindow(sender)
        }
    }

    /// “<name>:2” while the document has more than one view, and the plain
    /// name again the moment it is back to one —
    /// `ImageDocument.removeWindowController` re-synchronizes the survivors.
    override func windowTitle(forDocumentDisplayName displayName: String) -> String {
        guard let document = document as? ImageDocument, document.windowControllers.count > 1
        else { return displayName }
        return "\(displayName):\(viewOrdinal)"
    }

    /// Actual close, after the document's unsaved-change validation. This also
    /// covers the close button when AppKit removes the controller before closing
    /// the document; cancelling a close never clears a visible photograph.
    @objc private func editorWindowWillClose(_ notification: Notification) {
        rememberWindow(notification)
        (contentViewController as? EditorViewController)?.releaseDisplayForWindowClose()
    }

    /// Records this window's frame and panel layout as the one the next new
    /// window opens with (`EditorWindowMemory`); never while full screen
    /// or under automation.
    @objc private func rememberWindow(_ notification: Notification) {
        guard EditorWindowMemory.isEnabled, let window,
              !window.styleMask.contains(.fullScreen),
              let editor = contentViewController as? EditorViewController else { return }
        let layout = editor.captureWorkspace(name: "")
        EditorWindowMemory(frame: layout.windowFrame, panelVisible: layout.panelVisible,
                           panelTab: layout.panelTab.name).save()
    }

    /// Mirrors the editor's current tool into the tool rail. Display
    /// only: setting the selection does not re-dispatch the slot action.
    func reflectSelectedTool(_ tool: EditorTool) {
        (contentViewController as? EditorViewController)?.reflectSelectedTool(tool)
    }
}

/// The editor's window. Two additions to AppKit's key handling:
///
/// - Keys reach the canvas when the window itself holds focus, as it
///   does after an options-bar edit or preset pick: the canvas is made first
///   responder again and handed the key, so tool keys, Esc's deselect and
///   every other canvas key keep working (`handKeyToCanvas`, which the
///   Layers and Channels tables use too). An open canvas text session
///   takes the key instead, so typing resumes in it. The canvas's own
///   unhandled `super.keyDown` comes back here with the canvas as first
///   responder, so this cannot loop.
/// - Esc that nothing else in the window consumed: the canvas's
///   `keyDown` and every field editor take Esc first, and a panel table
///   passes it up as `cancelOperation` or `keyDown`. It cancels the
///   document's running UI job through `JobEscape` (the canvas's own rule,
///   which swallows Esc while the job drains). A text view keeps its Esc;
///   everything else goes on as before.
final class EditorWindow: NSWindow {
    override func keyDown(with event: NSEvent) {
        let editor = contentViewController as? EditorViewController
        if firstResponder === self, handKeyToCanvas(event) { return }
        if JobEscape.isBareEscape(event), JobEscape.cancelsJob(in: self, document: editor?.document) {
            return
        }
        super.keyDown(with: event)
    }

    /// Hands `event` to the canvas's open text session when there is one,
    /// else to the canvas: that view is made first responder,
    /// then sent the key. false when there is no editor or it refused focus.
    func handKeyToCanvas(_ event: NSEvent) -> Bool {
        guard let editor = contentViewController as? EditorViewController else { return false }
        let target: NSView = editor.canvas.activeTextView ?? editor.canvas
        guard makeFirstResponder(target) else { return false }
        target.keyDown(with: event)
        return true
    }

    override func cancelOperation(_ sender: Any?) {
        let editor = contentViewController as? EditorViewController
        if JobEscape.cancelsJob(in: self, document: editor?.document) { return }
        if NSWindow.instancesRespond(to: #selector(NSResponder.cancelOperation(_:))) {
            super.cancelOperation(sender)
        } else {
            JobEscape.forwardCancel(sender, from: self)
        }
    }
}
