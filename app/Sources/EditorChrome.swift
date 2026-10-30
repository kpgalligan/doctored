import AppKit

// `PanelDividerView` is 7 pt of grab band drawing the panel divider's 1 px line, added to the root
// LAST so it hit-tests ahead of the well and the panel it overlaps by 3 pt each.

// MARK: - BarView

/// A chrome bar: card surface with an optional 1px subtle border on the
/// top or bottom edge. Used for the toolbar, options bar, and status bar.
final class BarView: NSView {
    enum Border {
        case top
        case bottom
    }

    private let border: Border

    init(border: Border) {
        self.border = border
        super.init(frame: .zero)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("BarView does not support NSCoder")
    }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        DS.chromeBackground.setFill()
        bounds.fill()
        DS.border.setFill()
        let line: NSRect
        switch border {
        case .top:
            line = NSRect(x: 0, y: 0, width: bounds.width, height: 1)
        case .bottom:
            line = NSRect(x: 0, y: bounds.height - 1, width: bounds.width, height: 1)
        }
        line.fill()
    }
}

// MARK: - PanelDividerView

/// The right panel's draggable edge. It is `DS.panelDividerWidth` (7 pt) wide
/// and draws the SAME 1 px separator line the old `NSBox` drew, at x = 3, so
/// the picture is unchanged: the extra six points are grab band, overlapping
/// the canvas well and the panel by three each. A 1 pt target is not a target
/// — the view is added to the root last so it hit-tests ahead of both
/// neighbours, and it paints nothing but the line so neither neighbour is
/// covered.
///
/// `onDrag` reports a horizontal delta in POINTS; +x means a wider panel,
/// which is the direction the panel's leading edge moves AWAY from.
final class PanelDividerView: NSView {
    var onDrag: ((CGFloat) -> Void)?

    private var dragOrigin: CGFloat?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PanelDividerView does not support NSCoder")
    }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        DS.border.setFill()
        NSRect(x: 3, y: 0, width: 1, height: bounds.height).fill()
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .resizeLeftRight)
    }

    override func mouseDown(with event: NSEvent) {
        dragOrigin = event.locationInWindow.x
    }

    override func mouseDragged(with event: NSEvent) {
        guard let origin = dragOrigin else { return }
        // WINDOW coordinates, and an INCREMENTAL delta: this view moves left
        // as the panel grows, so a delta measured in its own space would
        // depend on whether layout had caught up with the previous event.
        let x = event.locationInWindow.x
        guard x != origin else { return }
        dragOrigin = x
        onDrag?(origin - x)
    }

    override func mouseUp(with event: NSEvent) {
        dragOrigin = nil
    }
}

// MARK: - CenteringClipView

/// Centers the document view inside the scroll area when it is smaller than
/// the visible rect, so the canvas floats in the middle of the well.
///
/// It also forwards a selection DRAG that begins on the pasteboard to the hosted
/// canvas. The press is held until the first drag tick, so a click on
/// the void stays inert for every tool.
final class CenteringClipView: NSClipView {
    /// The canvas owning the current margin press. Weak: the window may close mid-drag.
    private weak var marginPressCanvas: ImageCanvasView?
    /// The press, held until the first drag tick proves it is a drag.
    private var pendingMarginPress: NSEvent?

    /// Hover tracking for an open Magnetic Lasso outline over the margin.
    private var marginTracking: NSTrackingArea?

    private var hostedCanvas: ImageCanvasView? { (documentView as? CanvasHostView)?.canvas }

    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var rect = super.constrainBoundsRect(proposedBounds)
        guard let doc = documentView else { return rect }
        let docFrame = doc.frame
        if docFrame.width < rect.width {
            rect.origin.x = docFrame.minX - (rect.width - docFrame.width) / 2
        }
        if docFrame.height < rect.height {
            rect.origin.y = docFrame.minY - (rect.height - docFrame.height) / 2
        }
        return rect
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let marginTracking { removeTrackingArea(marginTracking) }
        let area = NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect],
                                  owner: self, userInfo: nil)
        addTrackingArea(area)
        marginTracking = area
    }

    /// An open Magnetic Lasso outline keeps tracing while the pointer is over
    /// the margin; the canvas clamps the point onto the image edge.
    /// Inside the canvas its own tracking area already delivers the move.
    override func mouseMoved(with event: NSEvent) {
        guard let canvas = hostedCanvas, canvas.hasMagneticLassoSession,
              !canvas.bounds.contains(canvas.convert(event.locationInWindow, from: nil)) else {
            super.mouseMoved(with: event)
            return
        }
        canvas.mouseMoved(with: event)
    }

    override func mouseDown(with event: NSEvent) {
        // A click on the margin with a Magnetic Lasso outline open is one of
        // its clicks (fasten, close), delivered at once — never held as a drag.
        if let canvas = hostedCanvas, canvas.hasMagneticLassoSession {
            canvas.mouseDown(with: event)
            return
        }
        guard let canvas = hostedCanvas, canvas.acceptsMarginPress else {
            super.mouseDown(with: event)
            return
        }
        marginPressCanvas = canvas
        pendingMarginPress = event
    }

    override func mouseDragged(with event: NSEvent) {
        guard let canvas = marginPressCanvas else {
            super.mouseDragged(with: event)
            return
        }
        if let pending = pendingMarginPress {
            pendingMarginPress = nil
            guard canvas.acceptsMarginPress else {
                marginPressCanvas = nil
                return
            }
            canvas.mouseDown(with: pending)
        }
        canvas.mouseDragged(with: event)
    }

    override func mouseUp(with event: NSEvent) {
        let canvas = marginPressCanvas
        let wasLatched = canvas != nil || pendingMarginPress != nil
        let stillPending = pendingMarginPress != nil
        marginPressCanvas = nil
        pendingMarginPress = nil
        if let canvas, !stillPending {
            canvas.mouseUp(with: event)
        } else if !wasLatched {
            super.mouseUp(with: event)
        }
    }
}

// MARK: - ZoomPillView

/// The floating zoom control at the canvas well's bottom-left: a dark
/// translucent square HUD with a 1pt border and a mono percentage between
/// two square steppers. One of the design's two sanctioned uses of blur.
final class ZoomPillView: NSView {
    private let label = NSTextField(labelWithString: "100%")
    private let effect = NSVisualEffectView()

    var onZoomIn: (() -> Void)?
    var onZoomOut: (() -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true

        effect.translatesAutoresizingMaskIntoConstraints = false
        effect.material = .hudWindow
        effect.blendingMode = .withinWindow
        effect.state = .active
        effect.appearance = NSAppearance(named: .darkAqua)
        effect.wantsLayer = true
        effect.layer?.cornerRadius = DS.popoverRadius
        effect.layer?.masksToBounds = true
        effect.layer?.borderWidth = 1
        effect.layer?.borderColor = NSColor(white: 1, alpha: 0.08).cgColor
        effect.layer?.backgroundColor =
            NSColor(srgbRed: 11 / 255, green: 18 / 255, blue: 15 / 255, alpha: 0.4).cgColor
        addSubview(effect)

        let minus = stepper(glyph: "−", action: #selector(minusClicked(_:)))
        let plus = stepper(glyph: "+", action: #selector(plusClicked(_:)))

        label.font = DS.mono(11)
        label.textColor = NSColor(srgbRed: 0.98, green: 0.97, blue: 0.94, alpha: 1)
        label.alignment = .center

        let stack = NSStackView(views: [minus, label, plus])
        stack.translatesAutoresizingMaskIntoConstraints = false
        stack.orientation = .horizontal
        stack.spacing = 6
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 6, bottom: 4, right: 6)
        addSubview(stack)

        NSLayoutConstraint.activate([
            effect.topAnchor.constraint(equalTo: topAnchor),
            effect.bottomAnchor.constraint(equalTo: bottomAnchor),
            effect.leadingAnchor.constraint(equalTo: leadingAnchor),
            effect.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            label.widthAnchor.constraint(greaterThanOrEqualToConstant: 38),
            heightAnchor.constraint(equalToConstant: DS.zoomHUDHeight),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ZoomPillView does not support NSCoder")
    }

    func setZoomText(_ text: String) {
        label.stringValue = text
    }

    private func stepper(glyph: String, action: Selector) -> NSButton {
        let button = HUDStepper(title: glyph, target: self, action: action)
        button.isBordered = false
        button.font = DS.mono(13, weight: .medium)
        button.contentTintColor = NSColor(srgbRed: 0.98, green: 0.97, blue: 0.94, alpha: 1)
        button.setButtonType(.momentaryChange)
        button.widthAnchor.constraint(equalToConstant: 22).isActive = true
        button.heightAnchor.constraint(equalToConstant: 22).isActive = true
        button.wantsLayer = true
        button.layer?.cornerRadius = DS.popoverRadius
        return button
    }

    @objc private func minusClicked(_ sender: Any?) { onZoomOut?() }
    @objc private func plusClicked(_ sender: Any?) { onZoomIn?() }
}

/// One of the zoom HUD's square steppers: a borderless glyph button that
/// fills faintly white while the pointer is over it.
private final class HUDStepper: NSButton {
    private var hovered = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("HUDStepper does not support NSCoder")
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        // Owner-filtered — see the pitfall in `StickerButton.updateTrackingAreas` (Controls.swift).
        for area in trackingAreas where area.owner === self { removeTrackingArea(area) }
        addTrackingArea(
            NSTrackingArea(
                rect: bounds, options: [.mouseEnteredAndExited, .activeInKeyWindow],
                owner: self, userInfo: nil))
    }

    override func mouseEntered(with event: NSEvent) {
        hovered = true
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        hovered = false
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        if hovered && isEnabled {
            NSColor(white: 1, alpha: 0.10).setFill()
            bounds.fill()
        }
        super.draw(dirtyRect)
    }
}

// MARK: - Status segments

/// One status-bar segment: 10px mono faint text with an optional 1px
/// leading separator, per the design's border-separated segments.
final class StatusSegment: NSView {
    private let label = NSTextField(labelWithString: "")
    private let showsSeparator: Bool

    init(separator: Bool) {
        self.showsSeparator = separator
        super.init(frame: .zero)
        label.translatesAutoresizingMaskIntoConstraints = false
        label.font = DS.mono(10)
        label.textColor = DS.textFaint
        label.lineBreakMode = .byTruncatingTail
        addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(
                equalTo: leadingAnchor, constant: separator ? 14 : 0),
            label.trailingAnchor.constraint(equalTo: trailingAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
        setContentHuggingPriority(.defaultHigh, for: .horizontal)
        setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("StatusSegment does not support NSCoder")
    }

    var text: String {
        get { label.stringValue }
        set {
            label.stringValue = newValue
            isHidden = newValue.isEmpty
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        guard showsSeparator, !isHidden else { return }
        DS.border.setFill()
        NSRect(x: 0, y: (bounds.height - 14) / 2, width: 1, height: 14).fill()
    }
}
