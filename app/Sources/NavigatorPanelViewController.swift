import AppKit

// The Navigator's thumbnail comes from `ImageCanvasView.image` and is NEVER assigned to it (that
// setter clears the overlay, the selection and the Quick Mask), the proxy is the clip view's bounds
// ∩ the canvas, and the cached thumbnail is rebuilt only when the CGImage identity changes AND the
// panel is visible AND the change is not live — a stroke posts on every mouse-moved event.

/// The right panel's fifth tab: the whole composite at thumbnail size with a
/// proxy rectangle showing what the canvas well is looking at, plus the zoom
/// controls.
///
/// **VIEW state only.** Nothing here ever writes a pixel, a document, a
/// history entry or a dirty flag: the thumbnail is READ from
/// `ImageCanvasView.image` (never assigned to it — that setter clears the
/// overlay, the selection and the Quick Mask), the proxy is read from the
/// scroll view, and the two gestures scroll and magnify. It is the ruler /
/// grid / proof-setup exemption, for the same four reasons, which is why
/// `get_view` / `set_view` are `nonRecordable`.
///
/// The thumbnail is CACHED against the identity of the CGImage it was built
/// from and rebuilt only when the panel is visible and the document is not
/// mid-gesture: a live stroke posts its change on every mouse-moved event,
/// and re-scaling a canvas-sized image per tick is exactly the per-event
/// canvas-sized read the Info panel's memo exists to avoid.
final class NavigatorView: NSView {
    /// Where the thumbnail is drawn, in this view's coordinates.
    private(set) var imageRect: CGRect = .zero
    /// The visible rectangle's proxy, mapped into `imageRect`.
    private(set) var proxyRect: CGRect = .zero

    /// A new centre, in CANVAS coordinates (top-left origin, pixels).
    var onCenter: ((CGPoint) -> Void)?

    private var thumbnail: CGImage?
    private var canvasSize: CGSize = .zero
    private var visible: CGRect = .zero
    /// The press offset inside the proxy, so a drag moves the rectangle with
    /// the pointer instead of jumping its centre under it.
    private var grab: CGSize?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("NavigatorView does not support NSCoder")
    }

    override var isFlipped: Bool { true }

    /// Everything the view draws, pushed together so the thumbnail and the
    /// proxy can never describe two different canvases.
    func update(thumbnail: CGImage?, canvasSize: CGSize, visible: CGRect) {
        self.thumbnail = thumbnail
        self.canvasSize = canvasSize
        self.visible = visible
        layoutRects()
        needsDisplay = true
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        layoutRects()
    }

    /// The thumbnail's box (aspect-fit, centred) and the proxy inside it.
    private func layoutRects() {
        guard canvasSize.width > 0, canvasSize.height > 0, bounds.width > 0, bounds.height > 0
        else {
            imageRect = .zero
            proxyRect = .zero
            return
        }
        let scale = min(bounds.width / canvasSize.width, bounds.height / canvasSize.height)
        let width = canvasSize.width * scale
        let height = canvasSize.height * scale
        imageRect = CGRect(x: ((bounds.width - width) / 2).rounded(),
                           y: ((bounds.height - height) / 2).rounded(),
                           width: width, height: height)
        let clipped = visible.intersection(CGRect(origin: .zero, size: canvasSize))
        guard !clipped.isNull, clipped.width > 0, clipped.height > 0 else {
            proxyRect = imageRect
            return
        }
        proxyRect = CGRect(x: imageRect.minX + clipped.minX * scale,
                           y: imageRect.minY + clipped.minY * scale,
                           width: clipped.width * scale, height: clipped.height * scale)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        DS.canvasVoid.setFill()
        bounds.fill()
        guard !imageRect.isEmpty else { return }
        if let thumbnail {
            context.saveGState()
            context.interpolationQuality = .low
            // The view is flipped and CGImage's origin is bottom-left, so the
            // box is flipped once here — the same one-line flip every other
            // CG draw in a flipped view does.
            context.translateBy(x: 0, y: imageRect.maxY + imageRect.minY)
            context.scaleBy(x: 1, y: -1)
            context.draw(thumbnail, in: imageRect)
            context.restoreGState()
        } else {
            DS.chromeBackground.setFill()
            imageRect.fill()
        }
        DS.border.setStroke()
        NSBezierPath(rect: imageRect.insetBy(dx: 0.5, dy: 0.5)).stroke()
        guard !proxyRect.isEmpty else { return }
        DS.marquee.setStroke()
        let outline = NSBezierPath(rect: proxyRect.insetBy(dx: 0.5, dy: 0.5))
        outline.lineWidth = 1
        outline.stroke()
    }

    // MARK: - Gestures

    /// A point in this view, as canvas pixels.
    private func canvasPoint(_ point: CGPoint) -> CGPoint? {
        guard imageRect.width > 0, canvasSize.width > 0 else { return nil }
        let scale = canvasSize.width / imageRect.width
        return CGPoint(x: (point.x - imageRect.minX) * scale,
                       y: (point.y - imageRect.minY) * scale)
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let canvas = canvasPoint(point) else { return }
        if proxyRect.contains(point) {
            // Inside the proxy: remember where it was grabbed and pan by the
            // drag's own delta, so the picture does not jump on the press.
            grab = CGSize(width: point.x - proxyRect.midX, height: point.y - proxyRect.midY)
            return
        }
        // Outside it: centre there, which is the click-to-centre gesture.
        grab = .zero
        onCenter?(canvas)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let grab else { return }
        let point = convert(event.locationInWindow, from: nil)
        let centre = CGPoint(x: point.x - grab.width, y: point.y - grab.height)
        guard let canvas = canvasPoint(centre) else { return }
        onCenter?(canvas)
    }

    override func mouseUp(with event: NSEvent) {
        grab = nil
    }
}

/// The Navigator tab: the thumbnail above a zoom slider, a percentage field
/// and Fit.
final class NavigatorPanelViewController: NSViewController {
    /// Tab switches, mirroring the other four panels.
    var onShowLayers: (() -> Void)?
    var onShowChannels: (() -> Void)?
    var onShowAssistant: (() -> Void)?
    var onShowInfo: (() -> Void)?
    var onShowPaths: (() -> Void)?

    /// A new canvas centre, from a click or a proxy drag.
    var onCenter: ((CGPoint) -> Void)?
    /// A new magnification, from the slider or the percentage field.
    var onZoom: ((CGFloat) -> Void)?
    var onZoomFit: (() -> Void)?

    let navigatorView = NavigatorView()
    private let slider = NSSlider()
    private let percentField = NSTextField()
    private let sizeLabel = NSTextField(labelWithString: "")

    /// The cached thumbnail and the CGImage identity it was built from, so a
    /// redraw costs nothing and a document change costs one scale.
    private var thumbnail: CGImage?
    private var thumbnailSource: ObjectIdentifier?
    private var isPanelVisible = false

    /// The slider is LOG2-scaled over the scroll view's own magnification
    /// limits: linear, 2 % to 3200 % puts 100 % at three tenths of a percent
    /// of the travel and makes every useful zoom the same pixel.
    private static let minimumZoom: CGFloat = 0.02
    private static let maximumZoom: CGFloat = 32

    init() {
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("NavigatorPanelViewController does not support NSCoder")
    }

    // MARK: - View construction

    override func loadView() {
        let root = NSView(frame: NSRect(x: 0, y: 0, width: DS.panelWidth, height: 400))
        root.wantsLayer = true
        root.layer?.backgroundColor = DS.chromeBackground.cgColor

        let tabs = PanelTabsView(titles: EditorPanelTab.titles, activeIndex: 4) { [weak self] index in
            if index == 0 { self?.onShowLayers?() }
            if index == 1 { self?.onShowChannels?() }
            if index == 2 { self?.onShowAssistant?() }
            if index == 3 { self?.onShowInfo?() }
            if index == 5 { self?.onShowPaths?() }
        }
        tabs.translatesAutoresizingMaskIntoConstraints = false

        navigatorView.translatesAutoresizingMaskIntoConstraints = false
        navigatorView.setAccessibilityIdentifier("navigator.view")
        navigatorView.onCenter = { [weak self] point in self?.onCenter?(point) }

        slider.translatesAutoresizingMaskIntoConstraints = false
        slider.minValue = Double(log2(Self.minimumZoom))
        slider.maxValue = Double(log2(Self.maximumZoom))
        slider.doubleValue = 0
        slider.controlSize = .small
        slider.target = self
        slider.action = #selector(sliderChanged(_:))
        slider.setAccessibilityIdentifier("navigator.zoom.slider")

        DSField.style(percentField)
        percentField.translatesAutoresizingMaskIntoConstraints = false
        percentField.alignment = .right
        percentField.font = DS.mono(11)
        percentField.target = self
        percentField.action = #selector(percentChanged(_:))
        percentField.setAccessibilityIdentifier("navigator.zoom.percent")

        let fit = StickerButton(title: "Fit", style: .secondary, target: self,
                                action: #selector(fitClicked(_:)))
        fit.translatesAutoresizingMaskIntoConstraints = false
        fit.setAccessibilityIdentifier("navigator.zoom.fit")

        sizeLabel.font = DS.mono(10)
        sizeLabel.textColor = DS.textFaint
        sizeLabel.lineBreakMode = .byTruncatingTail

        let controls = NSStackView(views: [slider, percentField, fit])
        controls.translatesAutoresizingMaskIntoConstraints = false
        controls.orientation = .horizontal
        controls.alignment = .centerY
        controls.spacing = 8

        root.addSubview(tabs)
        root.addSubview(navigatorView)
        root.addSubview(controls)
        root.addSubview(sizeLabel)
        sizeLabel.translatesAutoresizingMaskIntoConstraints = false

        NSLayoutConstraint.activate([
            tabs.topAnchor.constraint(equalTo: root.topAnchor),
            tabs.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            tabs.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            tabs.heightAnchor.constraint(equalToConstant: DS.tabHeight),

            navigatorView.topAnchor.constraint(equalTo: tabs.bottomAnchor, constant: 10),
            navigatorView.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            navigatorView.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            navigatorView.bottomAnchor.constraint(equalTo: controls.topAnchor, constant: -10),

            controls.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            controls.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            controls.bottomAnchor.constraint(equalTo: sizeLabel.topAnchor, constant: -8),
            percentField.widthAnchor.constraint(equalToConstant: 62),
            slider.heightAnchor.constraint(equalToConstant: DS.controlHeight),

            sizeLabel.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            sizeLabel.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            sizeLabel.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -12),
        ])
        view = root
    }

    // MARK: - Refresh

    /// The one entry point the editor pushes through. `isLive` is true during
    /// a gesture: the proxy still follows, but the thumbnail is left alone.
    func update(image: CGImage?, canvasSize: CGSize, visible: CGRect, zoom: CGFloat,
                isLive: Bool) {
        _ = view
        if let image, isPanelVisible, !isLive,
           thumbnailSource != ObjectIdentifier(image) {
            thumbnail = Self.scaled(image)
            thumbnailSource = ObjectIdentifier(image)
        }
        if image == nil {
            thumbnail = nil
            thumbnailSource = nil
        }
        navigatorView.update(thumbnail: thumbnail, canvasSize: canvasSize, visible: visible)
        let percent = Int((Double(zoom) * 100).rounded())
        if percentField.currentEditor() == nil {
            percentField.stringValue = "\(percent)%"
        }
        let clamped = min(max(zoom, Self.minimumZoom), Self.maximumZoom)
        slider.doubleValue = Double(log2(clamped))
        sizeLabel.stringValue = canvasSize.width > 0
            ? "\(Int(canvasSize.width)) × \(Int(canvasSize.height)) px"
            : "No document open"
    }

    /// Shown or hidden by `updatePanelVisibility`. A hidden panel builds no
    /// thumbnail; becoming visible is what asks for the first one.
    func setPanelVisible(_ visible: Bool) {
        isPanelVisible = visible
    }

    /// The thumbnail: at most 512 px on the long side, which is plenty for a
    /// panel that is never wider than `DS.panelWidthRange.upperBound`.
    private static func scaled(_ image: CGImage) -> CGImage? {
        let longest = max(image.width, image.height)
        guard longest > 0 else { return nil }
        if longest <= 512 { return image }
        let scale = 512.0 / Double(longest)
        let width = max(Int((Double(image.width) * scale).rounded()), 1)
        let height = max(Int((Double(image.height) * scale).rounded()), 1)
        guard let space = image.colorSpace,
              let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                space: space, bitmapInfo: image.bitmapInfo.rawValue)
        else { return image }
        context.interpolationQuality = .low
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return context.makeImage() ?? image
    }

    // MARK: - Controls

    @objc private func sliderChanged(_ sender: Any?) {
        onZoom?(CGFloat(pow(2, slider.doubleValue)))
    }

    @objc private func percentChanged(_ sender: Any?) {
        let text = percentField.stringValue.replacingOccurrences(of: "%", with: "")
            .trimmingCharacters(in: .whitespaces)
        guard let value = Double(text), value.isFinite, value > 0 else {
            NSSound.beep()
            return
        }
        onZoom?(CGFloat(value / 100))
    }

    @objc private func fitClicked(_ sender: Any?) {
        onZoomFit?()
    }
}
