import AppKit

// The options bar's `More` panel is a square borderless child panel that is key while open and
// forwards ⌘W/⌃⌘M/Zoom/full screen to its host window.

// MARK: - OverflowPanel

/// The options bar's More overflow: a square, borderless child panel hung
/// under its anchor with trailing edges aligned. It replaces `NSPopover`,
/// whose OS-drawn corners and arrow the sharp chrome cannot square.
///
/// The panel is KEY while open, because the fields inside need the field
/// editor; the editor window stays main (`NSPanel` cannot become main). It
/// closes on Escape, on losing key status, on any mouse-down in another
/// window, and when the window-level menu commands (⌘W, ⌃⌘M, Zoom, full
/// screen) reach it — those are forwarded to the host window after the
/// panel closes, because `NSWindow` implements them and a nil-target action
/// would otherwise stop, and beep, here. `onClose` fires exactly once per
/// show, whichever path closed it.
final class OverflowPanel: NSPanel {
    var onClose: (() -> Void)?

    private var closed = false
    private var mouseMonitor: Any?
    private var resignObserver: NSObjectProtocol?

    init(content: NSView) {
        let size = content.frame.size
        super.init(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered, defer: true)
        isReleasedWhenClosed = false
        isOpaque = false
        backgroundColor = .clear
        hasShadow = true
        let surface = SurfaceView(frame: NSRect(origin: .zero, size: size))
        content.setFrameOrigin(.zero)
        surface.addSubview(content)
        contentView = surface
    }

    override var canBecomeKey: Bool { true }

    /// Hangs the panel 4pt below `anchor`, trailing edges aligned, as a
    /// child of the anchor's window, and makes it key.
    func show(below anchor: NSView) {
        guard let host = anchor.window else {
            close()
            return
        }
        closed = false
        host.addChildWindow(self, ordered: .above)
        let inWindow = anchor.convert(anchor.bounds, to: nil)
        let onScreen = host.convertToScreen(inWindow)
        setFrameTopLeftPoint(NSPoint(x: onScreen.maxX - frame.width, y: onScreen.minY - 4))
        makeKey()
        guard !closed else { return }

        resignObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.didResignKeyNotification, object: self, queue: .main
        ) { [weak self] _ in
            self?.close()
        }
        mouseMonitor = NSEvent.addLocalMonitorForEvents(
            matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]
        ) { [weak self] event in
            if let self, event.window !== self { self.close() }
            return event
        }
    }

    override func cancelOperation(_ sender: Any?) {
        close()
    }

    override func close() {
        guard !closed else { return }
        closed = true
        if let monitor = mouseMonitor {
            NSEvent.removeMonitor(monitor)
            mouseMonitor = nil
        }
        if let observer = resignObserver {
            NotificationCenter.default.removeObserver(observer)
            resignObserver = nil
        }
        parent?.removeChildWindow(self)
        super.close()
        onClose?()
    }

    // MARK: Window commands forwarded to the host

    override func performClose(_ sender: Any?) {
        let host = parent
        close()
        host?.performClose(sender)
    }

    override func performMiniaturize(_ sender: Any?) {
        let host = parent
        close()
        host?.performMiniaturize(sender)
    }

    override func performZoom(_ sender: Any?) {
        let host = parent
        close()
        host?.performZoom(sender)
    }

    override func toggleFullScreen(_ sender: Any?) {
        let host = parent
        close()
        host?.toggleFullScreen(sender)
    }
}

// MARK: - Surface

/// The panel's square surface: the chrome background with a 1pt strong
/// border drawn just inside its edge.
private final class SurfaceView: NSView {
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SurfaceView does not support NSCoder")
    }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        DS.chromeBackground.setFill()
        bounds.fill()
        let border = NSBezierPath(
            roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5),
            xRadius: DS.popoverRadius, yRadius: DS.popoverRadius)
        border.lineWidth = 1
        DS.borderStrong.setStroke()
        border.stroke()
    }
}
