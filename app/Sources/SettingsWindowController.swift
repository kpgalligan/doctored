import AppKit

/// Doctored ▸ Settings… (⌘,): one app-modal card titled "Settings" holding an `NSTabView`
/// (identifier `settings-tabs`) with Shortcuts (`ShortcutsSheetController.makeContent()`),
/// Models (`ModelsSettingsPane`), MCP (`MCPSettingsPane`: the helper's path and Copy MCP Command)
/// and Cloud (`CloudSettingsPane`: provider keys, consents, the agent switch).
///
/// A `ModalCardWindowController` rather than a document-modal sheet, for the New Document sheet's
/// reason: it has to work at the Welcome window, before any document exists. Every tab applies and
/// stores every edit immediately, so the card's two buttons both close it.
final class SettingsWindowController: NSObject {
    private let shortcuts: ShortcutsSheetController
    let models: ModelsSettingsPane
    let mcp: MCPSettingsPane
    let cloud: CloudSettingsPane
    let tabs = NSTabView()
    private var modal: ModalCardWindowController?
    private var size: [NSLayoutConstraint] = []
    private var contents: [NSView] = []

    init(menu: NSMenu, helperURL: URL = AgentServer.mcpHelperURL,
         pasteboard: NSPasteboard = .general) {
        shortcuts = ShortcutsSheetController(menu: menu)
        models = ModelsSettingsPane()
        mcp = MCPSettingsPane(helperURL: helperURL, pasteboard: pasteboard)
        cloud = CloudSettingsPane()
        super.init()
    }

    /// Opens the card on the live main menu.
    static func run() {
        guard let menu = NSApp.mainMenu else {
            NSSound.beep()
            return
        }
        let controller = SettingsWindowController(menu: menu)
        _ = controller.makeWindow().runModal()
    }

    /// The card's window, built once; `run()` runs it modally and probes drive it.
    func makeWindow() -> ModalCardWindowController {
        if let modal { return modal }
        tabs.setAccessibilityIdentifier("settings-tabs")
        tabs.identifier = NSUserInterfaceItemIdentifier("settings-tabs")
        tabs.translatesAutoresizingMaskIntoConstraints = false
        contents = [shortcuts.makeContent(), models.view, mcp, cloud.view]
        for (title, content) in zip(["Shortcuts", "Models", MCPSettingsPane.paneTitle, "Cloud"], contents) {
            let item = NSTabViewItem(identifier: title)
            item.label = title
            let host = NSView()
            content.translatesAutoresizingMaskIntoConstraints = false
            host.addSubview(content)
            NSLayoutConstraint.activate([
                content.topAnchor.constraint(equalTo: host.topAnchor, constant: 12),
                content.leadingAnchor.constraint(equalTo: host.leadingAnchor, constant: 12),
            ])
            item.view = host
            tabs.addTabViewItem(item)
        }
        resizeTabs()
        shortcuts.onResize = { [weak self] in self?.refit() }
        models.onResize = { [weak self] in self?.refit() }
        cloud.onResize = { [weak self] in self?.refit() }
        let done = StickerButton(title: "Done", style: .primary, target: self, action: #selector(closeClicked(_:)))
        let close = StickerButton(title: "Close", style: .secondary, target: self, action: #selector(closeClicked(_:)))
        let card = makeSheetView(title: "Settings", content: tabs,
                                 buttonRow: makeButtonRow(cancel: close, apply: done), width: 520)
        let window = ModalCardWindowController(title: "Settings", card: card)
        modal = window
        return window
    }

    /// The tab view takes the larger tab's fitting size plus its own chrome, since an
    /// `NSTabView` does not size itself from its items.
    private func resizeTabs() {
        NSLayoutConstraint.deactivate(size)
        var width: CGFloat = 0, height: CGFloat = 0
        for content in contents {
            content.layoutSubtreeIfNeeded()
            width = max(width, content.fittingSize.width)
            height = max(height, content.fittingSize.height)
        }
        size = [tabs.widthAnchor.constraint(equalToConstant: min(476, width + 40)),
                tabs.heightAnchor.constraint(equalToConstant: height + 64)]
        NSLayoutConstraint.activate(size)
    }

    private func refit() {
        resizeTabs()
        modal?.refit()
    }

    @objc private func closeClicked(_ sender: Any?) {
        modal?.end(.OK)
    }
}
