import AppKit
import QuartzCore

// `MarchingAntsView` is a layer-backed canvas subview (bottom-most, so a text view stays above;
// `hitTest` nil) whose backing layer is a `CAShapeLayer` stroked in `DS.marquee`, 2 / magnification
// wide with a `[5, 4]` / magnification dash, and marched by a discrete `lineDashPhase` keyframe
// animation (9 steps, 0.6 s, no `Timer`). `updateMarchingAnts()` is wired into `setSelection` and
// the `transformPreview` / `moveSelectionOffset` didSets (hidden during Free Transform and Move
// Selected, which keep their own marquee), `marchingAntsViewportDidChange()` into
// `magnificationDidChange`. The march pauses unless the app is active and the window key, visible
// and not miniaturized, and under Reduce Motion, Low Power, the `SelectionMarchingAnts` defaults
// kill switch or above `maxAnimatedElements` (20,000) path elements. `drawSelection` keeps only the
// dim wash; `strokeMarquee` stays for the lasso preview and the moving selection. `cacheDisplay`
// never captures the layer, so screenshots lose the committed marquee.

/// The committed selection's marching ants: the marquee
/// is a `CAShapeLayer` over the canvas whose dash phase steps round a
/// discrete keyframe animation, so the render server marches it and the
/// canvas never redraws for it. Drag and lasso previews and a moving
/// selection keep `strokeMarquee`'s static dash.
///
/// View feedback only: no undo, dirty count, Action or MCP twin.
enum MarchingAnts {
    /// 2px dashed coral marquee — one of the design's two sanctioned coral
    /// elements. Screen points; divided by the magnification so it stays
    /// 5 on, 4 off on screen at every zoom. Mask selections get the marquee
    /// on their traced contour.
    static let dash: [CGFloat] = [5, 4]
    /// One discrete phase step per screen point of the dash period.
    static let steps = 9
    static let cycle: CFTimeInterval = 0.6
    /// Above this many path elements the ants stand still: the largest
    /// contour the review measured.
    static let maxAnimatedElements = 20_000
    static let animationKey = "marchingAnts"
    static let enabledDefaultsKey = "SelectionMarchingAnts"

    /// A kill switch only: absent means on.
    static var enabled: Bool {
        UserDefaults.standard.object(forKey: enabledDefaultsKey) as? Bool ?? true
    }

    /// Test seams.
    static var reduceMotion: () -> Bool = { DS.reduceMotion }
    static var lowPower: () -> Bool = { ProcessInfo.processInfo.isLowPowerModeEnabled }
}

/// A layer-backed subview of the canvas whose backing layer is the ants'
/// `CAShapeLayer`, so AppKit keeps its flipped geometry, its rotation and its
/// frame in step with the canvas. It never takes a click.
final class MarchingAntsView: NSView {
    private(set) var eligible = false
    private(set) var configuredMagnification: CGFloat = 1
    /// The selection the path was built from, so a transform tick that only
    /// hides the ants does not rebuild a large contour.
    fileprivate var configuredSelection: (shape: CanvasSelection.Shape, width: Int, height: Int)?
    private var observers: [(center: NotificationCenter, token: NSObjectProtocol)] = []

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("MarchingAntsView does not support NSCoder")
    }

    deinit {
        removeObservers()
    }

    var shapeLayer: CAShapeLayer? { layer as? CAShapeLayer }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }
    override func updateLayer() {}

    override func makeBackingLayer() -> CALayer {
        let layer = CAShapeLayer()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer.fillColor = nil
        layer.strokeColor = DS.marquee.cgColor
        layer.lineCap = .butt
        layer.masksToBounds = true
        CATransaction.commit()
        return layer
    }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    var shouldAnimate: Bool {
        guard MarchingAnts.enabled, !MarchingAnts.reduceMotion(), !MarchingAnts.lowPower(),
              NSApp.isActive, let window, window.isKeyWindow,
              window.occlusionState.contains(.visible), !window.isMiniaturized,
              !isHidden, eligible else { return false }
        return true
    }

    var isAnimating: Bool {
        shapeLayer?.animation(forKey: MarchingAnts.animationKey) != nil
    }

    func configure(path: CGPath?, elementCount: Int, magnification: CGFloat) {
        eligible = elementCount <= MarchingAnts.maxAnimatedElements
        configuredMagnification = magnification
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        shapeLayer?.path = path
        applyStroke()
        CATransaction.commit()
        restartAnimation()
    }

    func setMagnification(_ magnification: CGFloat) {
        guard magnification != configuredMagnification else { return }
        configuredMagnification = magnification
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        applyStroke()
        CATransaction.commit()
        restartAnimation()
    }

    /// Removes the march and adds it back only while every condition in
    /// `shouldAnimate` holds. The phase steps are in screen points too.
    func restartAnimation() {
        guard let shapeLayer else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        shapeLayer.removeAnimation(forKey: MarchingAnts.animationKey)
        if shouldAnimate {
            let scale = configuredMagnification
            let march = CAKeyframeAnimation(keyPath: "lineDashPhase")
            march.calculationMode = .discrete
            march.values = (0..<MarchingAnts.steps).map { NSNumber(value: Double(CGFloat($0) / scale)) }
            march.duration = MarchingAnts.cycle
            march.repeatCount = .infinity
            march.isRemovedOnCompletion = false
            shapeLayer.add(march, forKey: MarchingAnts.animationKey)
        }
        CATransaction.commit()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        removeObservers()
        let restart: (Notification) -> Void = { [weak self] _ in self?.restartAnimation() }
        let center = NotificationCenter.default
        if let window {
            for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification,
                         NSWindow.didChangeOcclusionStateNotification,
                         NSWindow.didMiniaturizeNotification,
                         NSWindow.didDeminiaturizeNotification] {
                observers.append((center, center.addObserver(
                    forName: name, object: window, queue: .main, using: restart)))
            }
        }
        for name in [NSApplication.didBecomeActiveNotification,
                     NSApplication.didResignActiveNotification] {
            observers.append((center, center.addObserver(
                forName: name, object: nil, queue: .main, using: restart)))
        }
        let workspace = NSWorkspace.shared.notificationCenter
        observers.append((workspace, workspace.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification, object: nil,
            queue: .main, using: restart)))
        observers.append((center, center.addObserver(
            forName: .NSProcessInfoPowerStateDidChange, object: nil, queue: .main,
            using: restart)))
        restartAnimation()
    }

    private func applyStroke() {
        let scale = configuredMagnification
        shapeLayer?.lineWidth = 2 / scale
        shapeLayer?.lineDashPattern = MarchingAnts.dash.map { NSNumber(value: Double($0 / scale)) }
    }

    private func removeObservers() {
        for observer in observers {
            observer.center.removeObserver(observer.token)
        }
        observers = []
    }
}

extension ImageCanvasView {
    /// The ants follow the committed selection. Hidden with no selection,
    /// during Free Transform (which ignores the selection) and while the
    /// selection moves (`drawMovingSelection` keeps its static marquee).
    func updateMarchingAnts() {
        guard let selection else {
            if let view = marchingAntsView, !view.isHidden {
                view.isHidden = true
                view.configuredSelection = nil
                view.configure(path: nil, elementCount: 0, magnification: magnification)
            }
            return
        }
        let view: MarchingAntsView
        if let existing = marchingAntsView {
            view = existing
        } else {
            view = MarchingAntsView(frame: bounds)
            view.autoresizingMask = [.width, .height]
            // Below every other subview, so a text session stays on top.
            addSubview(view, positioned: .below, relativeTo: nil)
            marchingAntsView = view
        }
        let hidden = isTransforming || moveSelectionOffset != nil
        let hiddenChanged = view.isHidden != hidden
        view.isHidden = hidden
        if let configured = view.configuredSelection, configured.shape == selection.shape,
           configured.width == selection.canvasWidth,
           configured.height == selection.canvasHeight {
            view.setMagnification(magnification)
            if hiddenChanged { view.restartAnimation() }
            return
        }
        view.configuredSelection = (selection.shape, selection.canvasWidth, selection.canvasHeight)
        let path = selection.marqueePath ?? NSBezierPath(rect: selection.bounds)
        view.configure(path: path.cgPath, elementCount: path.elementCount,
                       magnification: magnification)
    }

    /// Zoom changed: the dash and the line width are in screen points.
    func marchingAntsViewportDidChange() {
        marchingAntsView?.setMagnification(magnification)
    }
}
