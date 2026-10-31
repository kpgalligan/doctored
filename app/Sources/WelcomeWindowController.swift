import AppKit

/// The no-document window (design proposal 4): shown when the app is
/// frontmost with nothing open, replacing the bare open panel. Accepts
/// dropped files via FileDropView and closes as soon as a document opens.
final class WelcomeWindowController: NSWindowController {
    static let shared = WelcomeWindowController()

    private init() {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 720, height: 480),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered,
            defer: false)
        window.title = "Doctored"
        window.center()
        super.init(window: window)

        let root = FileDropView(frame: NSRect(x: 0, y: 0, width: 720, height: 480))
        root.wantsLayer = true

        // The app icon itself, so the window can never show a stale mark.
        let icon = NSImageView(image: NSApp.applicationIconImage)
        icon.translatesAutoresizingMaskIntoConstraints = false
        icon.imageScaling = .scaleProportionallyUpOrDown

        let headline = NSTextField(labelWithString: "Fixing Pixels.")
        headline.translatesAutoresizingMaskIntoConstraints = false
        headline.font = DS.display(42)
        headline.textColor = DS.textStrong
        headline.alignment = .center

        let openButton = StickerButton(
            title: "Open…", style: .primary, target: nil,
            action: #selector(NSDocumentController.openDocument(_:)))
        let clipboardButton = StickerButton(
            title: "New from Clipboard", style: .secondary, target: nil,
            action: #selector(AppDelegate.newFromClipboard(_:)))
        // File ▸ New… reaches the same nil-target action, so the welcome
        // window and the menu share one handler.
        let newButton = StickerButton(
            title: "New Document…", style: .secondary, target: nil,
            action: #selector(AppDelegate.newDocument(_:)))
        let buttons = NSStackView(views: [openButton, newButton, clipboardButton])
        buttons.translatesAutoresizingMaskIntoConstraints = false
        buttons.orientation = .horizontal
        buttons.spacing = 12

        let stack = NSStackView(views: [icon, headline, buttons])
        stack.translatesAutoresizingMaskIntoConstraints = false
        stack.orientation = .vertical
        stack.alignment = .centerX
        stack.spacing = 18
        stack.setCustomSpacing(20, after: icon)
        stack.setCustomSpacing(30, after: headline)

        root.addSubview(stack)
        NSLayoutConstraint.activate([
            icon.widthAnchor.constraint(equalToConstant: 128),
            icon.heightAnchor.constraint(equalToConstant: 128),
            stack.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: root.centerYAnchor, constant: -8),
            stack.widthAnchor.constraint(lessThanOrEqualToConstant: 520),
        ])
        window.contentView = root
        applyBackground()

        // Close as soon as any document window becomes main; the welcome
        // window returns via applicationShouldOpenUntitledFile when the app
        // reactivates with nothing open.
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowBecameMain(_:)),
            name: NSWindow.didBecomeMainNotification, object: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("WelcomeWindowController does not support NSCoder")
    }

    private func applyBackground() {
        window?.backgroundColor = DS.chromeBackground
        window?.contentView?.layer?.backgroundColor = DS.chromeBackground.cgColor
    }

    @objc private func windowBecameMain(_ note: Notification) {
        guard let main = note.object as? NSWindow, main !== window,
              main.windowController?.document != nil
        else { return }
        window?.close()
    }

    func show() {
        applyBackground()
        window?.center()
        BackgroundLaunch.show(self)
    }
}
