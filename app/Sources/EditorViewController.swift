import AppKit

// A feature's MARK block in one of the frozen six (this file, `ImageCanvasView.swift`,
// `AgentServer.swift`, `Sheets.swift`, `DoctoredCore.swift`, `AgentCatalog.swift`) is exactly the
// granularity that becomes an extension file. The pieces the extension files build on are therefore
// internal, not private — the editor's `document`/`canvas`/`setActiveLayer`, the server's
// `target`/`performGroupedEdit`/arg helpers/`jsonResult`, the sheet builders — and the frozen file
// keeps only the wiring (a `coreHandlers` entry for a handler declared in `AgentServer.swift`
// itself (an extension file's handler goes in that file's own table), a closure assignment, a
// `validateUserInterfaceItem` case).
//
// The side panel WIDTH is an app-wide view preference (`ViewToolOptions.panelWidth`,
// `DS.panelWidthRange` 240…560): ONE `panelWidthConstraint` drives all six panels, `loadView`
// seeds it from the store and `applyViewChrome()` is the one change path — so **no panel subview
// may pin a width to `DS.panelWidth`**, and panel content must lay out at every width in the range.

final class EditorViewController: NSViewController {
    // Not private: the per-feature extension files (EditorViewController+…)
    // are handlers of this controller and need the document they act on.
    weak var document: ImageDocument?

    // Not private, like `document` and `canvas`: EditorViewController
    // +Rulers.swift owns the two constraints that move the well's top and
    // leading edges when the rulers appear, and creates them against it.
    let scrollView = NSScrollView()
    // Not private, like `document` above: the extension files need the two
    // things every editor command works against.
    let canvas = ImageCanvasView()
    // The scroll view's documentView since 12D-2: an unrotated box the canvas
    // sits centred inside (ViewRotation.swift, EditorViewController+Views.swift).
    let canvasHost = CanvasHostView()
    /// View-only rotation in degrees, per WINDOW. `setViewRotation` is the
    /// one writer (EditorViewController+Views.swift).
    var viewRotation: Double = 0
    private var didRunInitialZoom = false

    /// Soft proofing (Phase 11C): per-WINDOW view state. Deliberately not in
    /// `EditingState`, never persisted in a `.rz`, never an undo step and
    /// never an Action step — the ruler/grid exemption, for the same reasons.
    /// `EditorViewController+SoftProof.swift` owns every rule.
    var proofDisplay = ProofDisplay()
    let proofRenderer = ProofRenderer()
    /// A float window's view exposure: per-WINDOW view state on the
    /// same terms. `EditorViewController+ViewExposure.swift` owns every rule.
    var viewExposure = ViewExposureState()
    let viewExposureRenderer = PreviewRenderer()
    /// View ▸ Show Clipping's overlay renders.
    let clippingRenderer = PreviewRenderer()

    // The two ruler strips and the corner box they meet in (RulerView.swift;
    // laid out, wired and refreshed by EditorViewController+Rulers.swift),
    // plus the two constraint pairs its visibility toggle swaps — the
    // scrollTrailingToRoot/scrollTrailingToPanel template below.
    let hRuler = RulerView(orientation: .horizontal)
    let vRuler = RulerView(orientation: .vertical)
    let rulerCorner = RulerCornerView()
    var scrollTopToOptions: NSLayoutConstraint!
    var scrollTopToHRuler: NSLayoutConstraint!
    var scrollLeadingToRail: NSLayoutConstraint!
    var scrollLeadingToVRuler: NSLayoutConstraint!

    // Design chrome: fixed 36px options bar under the title bar, 48px left
    // tool rail, floating zoom pill, 26px status bar with mono segments.
    // Not private, like `canvas`: the +ToolOptions extension file builds the
    // bar's descriptor clusters and the rail mirrors its swatches.
    var toolRail: ToolRailView!
    let optionsBar = ToolOptionsBar()
    private let zoomPill = ZoomPillView(frame: .zero)
    private let statusDims = StatusSegment(separator: false)
    private let statusMode = StatusSegment(separator: true)
    private let statusSelection = StatusSegment(separator: true)
    private let statusTool = StatusSegment(separator: false)

    private(set) var currentTool: EditorTool = .select
    /// The tool last selected per bare key, so a shared key returns to it.
    private var lastToolForKey: [String: EditorTool] = [:]

    // Eyedropper readout shown in the options bar: the last sampled pixel.
    // Display only — the sample itself lands in the shared paint color.
    var lastSampleColor: NSColor?
    var lastSampleText = "—"
    // The Info readout's coalescing key: an extension cannot declare stored
    // state, and EditorViewController+Info compares against this before
    // building a PixelReadout — the same pattern lastSampleText follows.
    var lastCursorPixel: (x: Int, y: Int)?

    // True after a rail swatch pointed the shared color panel at this
    // editor; deinit then clears the panel's (unretained) target.
    private var colorPanelTargetsSelf = false

    // Right panel (Layers/Channels/Assistant tabs, toggled by View >
    // Show/Hide Layers). The tab state is internal, like `document` above:
    // the +Feature extension files are handlers of this controller, and
    // EditorViewController+Channels owns the Channels tab's entry points.
    var layersPanel: LayersPanelViewController!
    var channelsPanel: ChannelsPanelViewController!
    // Internal, like the panels above: EditorViewController+Info is a
    // handler of this controller, and an extension cannot see a private
    // member.
    var infoPanel: InfoPanelViewController!
    // Internal, like the panels above: EditorViewController+Navigator owns
    // the tab's entry points, and an extension cannot declare stored state.
    var navigatorPanel: NavigatorPanelViewController!
    // Internal for the same reason: EditorViewController+Paths owns the tab.
    var pathsPanel: PathsPanelViewController!
    private var assistantPanel: AssistantPanelViewController!
    private var panelSeparator: PanelDividerView!
    private var scrollTrailingToRoot: NSLayoutConstraint!
    private var scrollTrailingToPanel: NSLayoutConstraint!
    // The panel's width, seeded from the store here and pushed by
    // applyViewChrome() on every chrome change (EditorViewController+Rulers).
    var panelWidthConstraint: NSLayoutConstraint!
    var layersPanelVisible = true
    /// Tab's tool rail + options bar state, per window and never persisted
    /// (EditorViewController+ScreenChrome.swift).
    var toolChromeVisible = true
    var optionsBarTopConstraint: NSLayoutConstraint!
    var toolRailLeadingConstraint: NSLayoutConstraint!
    /// 0 = Layers, 1 = Channels, 2 = Assistant, 3 = Info, 4 = Navigator,
    /// 5 = Paths (`EditorPanelTab`'s raw values).
    var panelTab = 0

    // Move-tool drag state: the delta ALREADY APPLIED since the press, not
    // any layer's start offset — a group has no offset of its own, so the
    // gesture tracks its own total (MultiLayerEdit.swift.moveDidBegin says
    // why). Internal, like `transformSession` below: the gesture itself lives
    // in MultiLayerEdit.swift, and a stored property cannot.
    var moveAppliedDelta: (x: Int, y: Int)?
    // A Move drag over a selection (EditorViewController+MoveSelected.swift).
    var moveSelectedSession: MoveSelectedSession?
    var moveDuplicate: MoveDuplicateSession?  // ⌥-drag, no selection (+MoveDuplicate)
    var precisionMoveGesture: PrecisionGeometryGesture?
    // …and the union box of the moving set at the press, which is what a
    // snap needs and a pure delta cannot supply: the gesture reports an
    // offset, and nothing during it otherwise knows where the moving set
    // actually is. Written by moveDidBegin and read by moveDidUpdate, both
    // in MultiLayerEdit.swift — it lives here for the same reason
    // moveAppliedDelta does, that a stored property cannot live in an
    // extension.
    var movePressBox: CGRect?

    // The snapping engine's folded layer content boxes, cached against the
    // document HANDLE they were measured from (handles are copy-on-write
    // values, so identity is the right key). Invalidated in imageDidChange
    // on a settled change only: the fold is a per-pixel sweep per leaf, and
    // a Move drag posts that notification on every mouse-moved event.
    // Internal, not private: DragSnapping.swift fills and reads it.
    var snapBoxes: (doc: RasterDocument, boxes: [CGRect?])?

    // The open Free Transform session (see TransformSession, which lives in
    // MultiLayerEdit.swift together with the preview it feeds), and the flag
    // that marks the document change its own commit causes — every OTHER
    // change under an open session ends it. Internal, not private: a stored
    // property cannot live in an extension, so the session STAYS here while
    // everything that reads it moved out.
    var transformSession: TransformSession?
    var isCommittingTransform = false

    // The open typed OPACITY drag (see PrecisionOpacityDrag, driven from
    // EditorViewController+OpacityDrag.swift). A stored property cannot live
    // in an extension, so the session STAYS here while everything that reads
    // it moved out — the `transformSession` arrangement above.
    var precisionOpacityDrag: PrecisionOpacityDrag?

    // Brush/eraser drag state: the document handle when the stroke began.
    // Every stroke tick repaints the whole overlay onto this base, so the
    // live projection always shows the committed result. Mask strokes leave
    // it nil — they never live-edit, they commit once on mouse-up.
    // private(set), not private: EditorViewController+Heal.commitHealOverlay
    // solves against this pre-stroke handle at mouse-up.
    var strokeBase: RasterDocument?
    /// The sampling plate this gesture reads from, latched at mouse-down by
    /// `latchRetouchPlate` and used by BOTH the drag and the commit so the
    /// two cannot sample different pixels (`RetouchPlate`).
    var strokePlate: RetouchPlate?
    var strokeDestination: RasterDocument?
    var retouchSourceOverlay: RetouchSourceOverlay?
    var precisionStroke: PrecisionCanvasStroke?
    // …and which coverage target it commits to. Internal, like the panels
    // above: EditorViewController+Channels.commitCoverageOverlay switches
    // on it at mouse-up.
    var strokeTarget: PaintTarget = .layer
    // The tool the stroke began with, so ticks route to the right op
    // (dodge/burn is a retouch op, everything else paints the overlay).
    // private(set) for the same reason: +Heal dispatches .heal vs .spotHeal
    // on the tool the stroke began with.
    var strokeTool: EditorTool = .brush
    // The Blend option, latched at stroke begin (brush and clone only):
    // non-Normal ticks composite through the core's blend-mode paint op.
    // private(set), like strokeTool above it: the record helper in
    // EditorViewController+Actions.swift writes the latched mode into a
    // recorded brush or clone step.
    var strokeBlendMode = DR_BLEND_NORMAL
    var byteStrokeCapture: BytePaintStrokeCapture?
    var byteLastPublished: RasterDocument?
    var byteStrokeRefused = false

    // The open crop session (logic in EditorViewController+Crop.swift; the
    // canvas draws its overlay and routes the gesture).
    var cropSession: CropSession?
    var isCommittingPrecisionCrop = false
    var shapeEditSession: ShapeEditSession?

    // What brush/eraser edit (see PaintTarget), plus the layer it was chosen
    // for: selecting a different layer drops the choice back to .layer.
    private(set) var paintTarget: PaintTarget = .layer
    private var paintTargetLayer = 0
    /// …and, for a `.channel` target, the STABLE ID of the channel it names
    /// (0 for every other target). `.channel` carries a list position, and a
    /// delete, a duplicate above it or an undo renumbers the list under it —
    /// a range check alone would then leave the target silently pointing at
    /// the neighbour. `channelTargetResolved()` re-finds the index by this
    /// id, exactly as the Channels panel's eye column re-finds its own rows
    /// (`resolveChannelVisibility`); this is that set's single-value twin.
    private(set) var paintTargetChannelID: UInt64 = 0

    // Live copies of the shared colors and text parameters — the canvas and
    // payload builders read these as native types; ToolOptionsStore keeps
    // the persisted form. Internal: the +ToolOptions descriptors bind them.
    var paintColor: NSColor =
        TextLayer.color(fromHex: ToolOptionsStore.shared.sharedState.foreground) ?? .black
    var backgroundColor: NSColor =
        TextLayer.color(fromHex: ToolOptionsStore.shared.sharedState.background) ?? .white
    var fontFamily = ToolOptionsStore.shared.text.family
    var fontSize = CGFloat(min(max(ToolOptionsStore.shared.text.size, 6), 500))
    /// Text-tool line alignment, in the options bar's segment order
    /// (0 left, 1 center, 2 right).
    var textAlignment: NSTextAlignment = EditorViewController.alignmentSegmentValues[
        min(max(ToolOptionsStore.shared.text.alignmentIndex, 0), 2)]
    /// Sampler a Free Transform commit resamples with; remembered between
    /// sessions, like the paint options above.
    var transformSampler = DR_FILTER_CATMULL_ROM {
        didSet {
            if let session = cropSession, session.precisionGesture != nil {
                refreshPrecisionCropPreview(session)
            }
        }
    }

    private static let zoomLadder: [CGFloat] = [
        0.05, 0.1, 0.25, 0.33, 0.5, 0.67, 1.0, 1.5, 2, 3, 4, 6, 8, 12, 16, 24, 32,
    ]

    init(document: ImageDocument) {
        self.document = document
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("EditorViewController does not support NSCoder")
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
        // The shared color panel's target is unretained: if a rail swatch
        // pointed it at this editor, a pick after the window closes would
        // message a freed controller. There is no target getter, so the
        // guard is the flag set when this editor took the panel over.
        if colorPanelTargetsSelf {
            NSColorPanel.shared.setTarget(nil)
            NSColorPanel.shared.setAction(nil)
        }
    }

    // MARK: - View construction

    override func loadView() {
        let root = FileDropView(frame: NSRect(x: 0, y: 0, width: 1040, height: 600))

        scrollView.translatesAutoresizingMaskIntoConstraints = false
        scrollView.allowsMagnification = true
        scrollView.minMagnification = 0.02
        scrollView.maxMagnification = 32
        scrollView.hasHorizontalScroller = true
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true
        scrollView.backgroundColor = DS.canvasVoid
        scrollView.usesPredominantAxisScrolling = false
        scrollView.contentView = CenteringClipView()
        scrollView.documentView = canvasHost
        canvasHost.addSubview(canvas)
        canvasHost.canvas = canvas
        // Soft ambient shadow beneath the image, over the canvas void.
        canvas.wantsLayer = true
        canvas.layer?.masksToBounds = false
        canvas.shadow = {
            let shadow = NSShadow()
            shadow.shadowColor = NSColor.black.withAlphaComponent(0.5)
            shadow.shadowBlurRadius = 24
            shadow.shadowOffset = NSSize(width: 0, height: -9)
            return shadow
        }()

        if let document = document, let doc = document.doc {
            layoutCanvasHost(canvasSize: doc.canvasSize)
            // The space FIRST: the overlay a stroke paints into is built
            // lazily from it, and a stale one would round-trip the document's
            // pixels through the wrong space.
            canvas.documentColorSpace = doc.drawingSpace
            canvas.image = document.previewImage
            refreshProofImage()
            refreshClippingOverlay()
        }
        canvas.onSelectionChange = { [weak self] _ in self?.updateStatus() }
        canvas.onSelectionCommit = { [weak self] in self?.commitCanvasSelection($0, actionName: $1, record: $2) }
        canvas.onQuickMaskStrokeCommit = { [weak self] in self?.commitQuickMaskStroke($0) }
        canvas.onStrokeBegin = { [weak self] in
            guard let self = self, let document = self.document, let doc = document.doc
            else { return false }
            if doc.precision != nil { return self.beginPrecisionStroke() }
            return self.beginBytePaintStroke()
        }
        canvas.onStrokeUpdate = { [weak self] data, mode, alpha in
            self?.updateBytePaintStroke(data, mode: mode, alpha: alpha)
        }
        canvas.onCommitMaskOverlay = { [weak self] data, _ in self?.commitCoverageOverlay(data) }
        canvas.onCommitStrokeOverlay = { [weak self] d, n in
            guard self?.precisionStroke == nil else { return }
            self?.commitHealOverlay(d, n)
        }
        canvas.onStrokeSourceImage = { [weak self] in self?.strokeSourceImage($0) }
        canvas.onRetouchSourceOverlay = { [weak self] in self?.retouchSourceOverlayImage() }
        canvas.onSelectionGestureCapture = { [weak self] in self?.captureSelectionGesture() }
        canvas.onSelectionGestureCommit = { [weak self] in self?.finishSelectionGesture($0) }
        canvas.onObjectSelectionCommit = { [weak self] in self?.finishObjectSelection($0) }
        canvas.onMagneticLassoCommit = { [weak self] in self?.finishMagneticLasso($0, outline: $1) }
        canvas.onMagneticLassoWidthKey = { [weak self] in self?.magneticLassoWidthKeyChanged($0) }
        canvas.onPatchCapture = { [weak self] in self?.patchCapture() }
        canvas.onPatchCommit = { [weak self] result in self?.patchCommitted(result) }
        canvas.onEscapeCancelsJob = { [weak self] in self?.escapeCancelsJob() ?? false }
        canvas.onRedEyeCommit = { [weak self] rect in self?.redEyeRectDragged(rect) }
        canvas.onRemoveCommit = { [weak self] stroke in self?.removeStrokeCommitted(stroke) }
        canvas.onRemoveApply = { [weak self] in self?.applyPendingRemove() }
        canvas.onRemoveCancel = { [weak self] in self?.cancelPendingRemove() }
        canvas.onStrokeEnd = { [weak self] actionName in
            guard let self else { return }
            if self.precisionStroke != nil { self.finishPrecisionStroke(actionName); return }
            self.endBytePaintStroke(actionName)
        }
        canvas.onStrokeCancel = { [weak self] in
            guard let self else { return }
            if self.precisionStroke != nil { self.cancelPrecisionStroke(); return }
            self.cancelBytePaintStroke()
        }
        canvas.onTextClick = { [weak self] point in self?.textClicked(point) }
        canvas.onCommitText = { [weak self] payload, origin, editingLayer in
            self?.commitTextLayer(payload, origin: origin, editing: editingLayer) ?? true
        }
        canvas.onTextSessionEnd = { [weak self] in
            // Drops the layer-hidden preview a re-edit session put up.
            self?.canvas.previewImage = nil
        }
        canvas.onToolKey = { [weak self] tool in self?.selectToolForKey(tool) }
        installPrecisionToolPolicy()
        canvas.onQuickMaskKey = { [weak self] in self?.toggleQuickMask(nil) }
        canvas.onWandClick = { [weak self] point, mode in self?.wandClicked(point, mode: mode) }
        canvas.onFillClick = { [weak self] point in self?.fillClicked(point) }
        canvas.onEyedropper = { [weak self] point in self?.sampleColor(at: point) }
        canvas.onEyedropperEnd = { [weak self] in self?.eyedropperPressEnded() }
        installColorSamplers()
        installRuler()
        installPerspectiveCrop()
        installPen()
        installMoveControls()
        installPuppetWarp()
        installPerspectiveWarp()
        installLayerDrag()
        canvas.onGradientCommit = { [weak self] a, b in self?.gradientCommitted(a, b) }
        canvas.onBrushSizeKey = { [weak self] newSize in self?.brushSizeKeyChanged(newSize) }
        canvas.onBrushHardnessKey = { [weak self] up in self?.brushHardnessKeyStepped(up: up) }
        canvas.onBrushPresetKey = { [weak self] step in self?.brushPresetKeyStepped(step) }
        canvas.onLockTransparencyKey = { [weak self] in self?.lockTransparencyKeyPressed() }
        canvas.onScreenKey = { [weak self] key in self?.screenKeyPressed(key) }
        canvas.onColorPairKey = { [weak self] key in self?.colorPairKeyPressed(key) }
        canvas.onMoveBegin = { [weak self] point, modifiers in
            self?.moveDidBegin(at: point, modifiers: modifiers)
        }
        canvas.onMoveUpdate = { [weak self] dx, dy, modifiers in
            self?.moveDidUpdate(dx, dy, modifiers)
        }
        canvas.onMoveEnd = { [weak self] in self?.moveDidFinish() }
        canvas.onMoveCancel = { [weak self] in self?.moveDidCancel() }
        canvas.onMoveNudge = { [weak self] in self?.moveNudge($0, $1, duplicate: $2) }
        canvas.onTransformMouseDown = { [weak self] point, modifiers in
            self?.transformMouseDown(point, modifiers)
        }
        canvas.onTransformMouseDragged = { [weak self] point, modifiers in
            self?.transformMouseDragged(point, modifiers)
        }
        canvas.onTransformMouseUp = { [weak self] _, _ in
            self?.transformSession?.drag = nil
            // Same rule as the Move drag: the alignment lines a transform
            // drag earned come off the screen when it ends.
            self?.pushSmartGuides([])
        }
        canvas.onTransformCommit = { [weak self] in self?.commitTransformSession() }
        canvas.onTransformCancel = { [weak self] in self?.endTransformSession() }
        canvas.onTransformNudge = { [weak self] dx, dy in self?.transformNudge(dx, dy) }
        canvas.onCropMouseDown = { [weak self] point in self?.cropMouseDown(point) }
        canvas.onCropMouseDragged = { [weak self] point, modifiers in
            self?.cropMouseDragged(point, modifiers)
        }
        canvas.onCropMouseUp = { [weak self] point in self?.cropMouseUp(point) }
        canvas.onCropCommit = { [weak self] in self?.commitCropSession() }
        canvas.onCropCancel = { [weak self] in self?.resetCropSession() }
        canvas.onShapeCommit = { [weak self] box, flipped in
            self?.commitShapeLayer(box: box, flipped: flipped)
        }
        canvas.onShapeEditMouseDown = { [weak self] point in self?.shapeEditMouseDown(point) }
        canvas.onShapeEditMouseDragged = { [weak self] point, modifiers in
            self?.shapeEditMouseDragged(point, modifiers)
        }
        canvas.onShapeEditMouseUp = { [weak self] in self?.shapeEditSession?.drag = nil }
        canvas.onShapeEditCommit = { [weak self] in self?.commitShapeEditSession() }
        canvas.onShapeEditCancel = { [weak self] in self?.cancelShapeEditSession() }
        canvas.onCursorMove = { [weak self] point in self?.cursorMoved(to: point) }
        // Guides (EditorViewController+Guides.swift): the canvas routes the
        // press, the drag, the release, ⌫ and Escape; the geometry is the
        // extension's.
        canvas.onGuideMouseDown = { [weak self] point, modifiers in
            self?.guideMouseDown(point, modifiers) ?? false
        }
        canvas.onGuideMouseDragged = { [weak self] point, modifiers in
            self?.guideMouseDragged(point, modifiers)
        }
        canvas.onGuideMouseUp = { [weak self] in self?.guideMouseUp() }
        canvas.onGuideDelete = { [weak self] in self?.guideDragDelete() }
        canvas.onGuideCancel = { [weak self] in self?.guideDragCancel() }
        // The engine the canvas's own point drags snap against, built at
        // each of their mouse-downs (DragSnapping.swift).
        canvas.onSnapEngine = { [weak self] in
            self?.makeSnapEngine(for: .canvasPoint) ?? .inactive
        }
        canvas.onViewRotation = { [weak self] in self?.viewRotation ?? 0 }
        canvas.onRotateView = { [weak self] in self?.rotateViewGestureDidChange($0) }
        canvas.onZoomClick = { [weak self] point, out in self?.zoomStep(at: point, out: out) }
        canvas.onZoomTo = { [weak self] target in self?.applyZoom(target) }
        canvas.onZoomRect = { [weak self] rect in self?.zoomToRect(rect) }

        canvas.paintColor = paintColor
        canvas.nativeBrushEnabled = document?.doc?.isHighDepth == true
        canvas.textStyle = currentTextStyle()
        syncCanvasPaintState()

        // Fixed-height options bar under the title bar; the +ToolOptions
        // extension builds its per-tool clusters.
        optionsBar.translatesAutoresizingMaskIntoConstraints = false
        optionsBar.onAnyEdit = { [weak self] in self?.toolOptionsEdited() }

        // Left tool rail: the slots — including which tools share one —
        // come from EditorTool.railGroups; its swatches are the shared
        // foreground/background colors.
        toolRail = ToolRailView(groups: EditorTool.railGroups)
        toolRail.translatesAutoresizingMaskIntoConstraints = false
        toolRail.foregroundSwatchColor = paintColor
        toolRail.backgroundSwatchColor = backgroundColor
        toolRail.onPickForeground = { [weak self] in self?.pickForegroundColor() }
        toolRail.onPickBackground = { [weak self] in self?.pickBackgroundColor() }
        toolRail.onSwapColors = { [weak self] in self?.swapColors() }
        toolRail.onResetColors = { [weak self] in self?.resetColors() }

        let statusBar = BarView(border: .top)
        statusBar.translatesAutoresizingMaskIntoConstraints = false
        statusMode.setAccessibilityIdentifier("status-mode")
        let statusLeft = NSStackView(views: [statusDims, statusMode, statusSelection])
        statusLeft.translatesAutoresizingMaskIntoConstraints = false
        statusLeft.orientation = .horizontal
        statusLeft.spacing = 14
        let statusRight = NSStackView(views: [statusTool])
        statusRight.translatesAutoresizingMaskIntoConstraints = false
        statusRight.orientation = .horizontal
        statusRight.spacing = 14
        statusBar.addSubview(statusLeft)
        statusBar.addSubview(statusRight)

        zoomPill.translatesAutoresizingMaskIntoConstraints = false
        zoomPill.onZoomIn = { [weak self] in self?.zoomIn() }
        zoomPill.onZoomOut = { [weak self] in self?.zoomOut() }

        // Right panel (Layers + Assistant tabs) + its 1px separator line.
        layersPanel = LayersPanelViewController()
        layersPanel.document = document
        installLayerDropTargets(root: root)
        layersPanel.onActiveLayerChange = { [weak self] in
            // Retargeting the panel leaves the transform session: commit it
            // (it carries its own layer index, so it still lands correctly).
            self?.commitPendingTransform()
            self?.syncPaintTarget()
            self?.updateStatus()
            self?.updateActiveLayerRect()
            // The active layer is not a document change and posts no
            // notification, yet the "<layer> Mask" row is computed from it —
            // and so is the canvas's mask base and rubylith.
            self?.channelsPanel?.activeLayerChanged()
            self?.infoPanel?.activeLayerChanged()
            self?.pathsActiveLayerDidChange()
            self?.refreshChannelDisplay()
        }
        layersPanel.onPaintTargetChange = { [weak self] target in
            self?.setPaintTarget(target)
        }
        layersPanel.onPrecisionOpacityScrub = { [weak self] value, ended in
            self?.precisionOpacityScrubbed(value, ended: ended)
        }
        layersPanel.onAdjustmentEdit = { [weak self] idx in
            self?.editAdjustmentLayer(idx)
        }
        layersPanel.onTextEdit = { [weak self] idx in
            self?.editTextLayer(idx)
        }
        layersPanel.onShapeEdit = { [weak self] idx in
            self?.editShapeLayer(idx)
        }
        layersPanel.onFillEdit = { [weak self] idx in self?.editFillLayer(idx) }
        layersPanel.onLayerStyleEdit = { [weak self] idx in
            self?.editLayerStyle(idx)
        }
        layersPanel.onShowChannels = { [weak self] in self?.showChannelsTab() }
        layersPanel.onShowAssistant = { [weak self] in
            self?.panelTab = 2
            self?.updatePanelVisibility()
        }
        layersPanel.onShowInfo = { [weak self] in self?.showInfoTab() }
        layersPanel.onShowNavigator = { [weak self] in self?.showNavigatorTab() }
        layersPanel.onShowPaths = { [weak self] in self?.showPathsTab() }
        layersPanel.onLoadLayerSelection = { [weak self] idx, target, mode in
            self?.loadLayerSelection(layer: idx, target: target, mode: mode)
        }
        addChild(layersPanel)
        let panelView = layersPanel.view
        panelView.translatesAutoresizingMaskIntoConstraints = false
        // The launch-time read of the stored width (the
        // updateRulerVisibility()-inside-installRulers pattern): applyViewChrome()
        // only runs on the change notification, so the first push is here.
        panelWidthConstraint = panelView.widthAnchor.constraint(
            equalToConstant: CGFloat(ToolOptionsStore.shared.view.panelWidth))

        channelsPanel = ChannelsPanelViewController()
        channelsPanel.document = document
        channelsPanel.onShowLayers = { [weak self] in
            self?.panelTab = 0
            self?.updatePanelVisibility()
        }
        channelsPanel.onShowAssistant = { [weak self] in
            self?.panelTab = 2
            self?.updatePanelVisibility()
        }
        channelsPanel.onSelectTarget = { [weak self] target in
            self?.setChannelRowTarget(target)
        }
        channelsPanel.onVisibilityChange = { [weak self] visibility in
            self?.channelViewChanged(visibility)
        }
        channelsPanel.onLoadSelection = { [weak self] source, mode in
            self?.loadRowSelection(source, mode: mode)
        }
        channelsPanel.onRenameChannel = { [weak self] id, name in
            self?.renameChannel(id: id, to: name)
        }
        channelsPanel.onShowInfo = { [weak self] in self?.showInfoTab() }
        channelsPanel.onShowNavigator = { [weak self] in self?.showNavigatorTab() }
        channelsPanel.onShowPaths = { [weak self] in self?.showPathsTab() }
        addChild(channelsPanel)
        let channelsView = channelsPanel.view
        channelsView.translatesAutoresizingMaskIntoConstraints = false
        channelsView.isHidden = true

        assistantPanel = AssistantPanelViewController()
        assistantPanel.document = document
        assistantPanel.onShowLayers = { [weak self] in
            self?.panelTab = 0
            self?.updatePanelVisibility()
        }
        assistantPanel.onShowChannels = { [weak self] in self?.showChannelsTab() }
        assistantPanel.onShowInfo = { [weak self] in self?.showInfoTab() }
        assistantPanel.onShowNavigator = { [weak self] in self?.showNavigatorTab() }
        assistantPanel.onShowPaths = { [weak self] in self?.showPathsTab() }
        addChild(assistantPanel)
        let assistantView = assistantPanel.view
        assistantView.translatesAutoresizingMaskIntoConstraints = false
        assistantView.isHidden = true

        infoPanel = InfoPanelViewController()
        infoPanel.document = document
        infoPanel.onShowLayers = { [weak self] in
            self?.panelTab = 0
            self?.updatePanelVisibility()
        }
        infoPanel.onShowChannels = { [weak self] in self?.showChannelsTab() }
        infoPanel.onShowAssistant = { [weak self] in
            self?.panelTab = 2
            self?.updatePanelVisibility()
        }
        infoPanel.onShowNavigator = { [weak self] in self?.showNavigatorTab() }
        infoPanel.onShowPaths = { [weak self] in self?.showPathsTab() }
        addChild(infoPanel)
        let infoView = infoPanel.view
        infoView.translatesAutoresizingMaskIntoConstraints = false
        infoView.isHidden = true

        navigatorPanel = makeNavigatorPanel()
        let navigatorView = navigatorPanel.view
        pathsPanel = makePathsPanel()

        panelSeparator = PanelDividerView()
        panelSeparator.translatesAutoresizingMaskIntoConstraints = false
        panelSeparator.onDrag = { [weak self] delta in self?.dividerDidDrag(delta) }

        root.addSubview(optionsBar)
        root.addSubview(toolRail)
        root.addSubview(scrollView)
        root.addSubview(zoomPill)
        root.addSubview(panelView)
        root.addSubview(channelsView)
        root.addSubview(assistantView)
        root.addSubview(infoView)
        root.addSubview(navigatorView)
        installPathsPanel(in: root)
        // LAST among the well and the panels, so its 7 pt grab band hit-tests
        // ahead of both neighbours it overlaps.
        root.addSubview(panelSeparator)
        root.addSubview(statusBar)

        optionsBarTopConstraint = optionsBar.topAnchor.constraint(equalTo: root.topAnchor)
        toolRailLeadingConstraint = toolRail.leadingAnchor.constraint(equalTo: root.leadingAnchor)
        NSLayoutConstraint.activate([
            // The bar is fixed-height and always present, so the canvas
            // well's frame never depends on which tool is active.
            optionsBarTopConstraint,
            optionsBar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            optionsBar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            optionsBar.heightAnchor.constraint(equalToConstant: DS.optionsBarHeight),

            toolRailLeadingConstraint,
            toolRail.topAnchor.constraint(equalTo: optionsBar.bottomAnchor),
            toolRail.bottomAnchor.constraint(equalTo: statusBar.topAnchor),
            toolRail.widthAnchor.constraint(equalToConstant: DS.railWidth),

            // The well's top and leading edges are the two the rulers move,
            // so their constraints are created as a swappable pair in
            // installRulers(in:) rather than pinned here.
            scrollView.bottomAnchor.constraint(equalTo: statusBar.topAnchor),

            zoomPill.leadingAnchor.constraint(
                equalTo: scrollView.leadingAnchor, constant: 16),
            zoomPill.bottomAnchor.constraint(
                equalTo: scrollView.bottomAnchor, constant: -16),

            // The four panels and the separator pin to the OPTIONS BAR,
            // not to the well's top: the rulers push the well down, and
            // following it would leave a notch beside the horizontal strip.
            panelView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            panelWidthConstraint,
            panelView.topAnchor.constraint(equalTo: optionsBar.bottomAnchor),
            panelView.bottomAnchor.constraint(equalTo: statusBar.topAnchor),

            channelsView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            channelsView.widthAnchor.constraint(equalTo: panelView.widthAnchor),
            channelsView.topAnchor.constraint(equalTo: optionsBar.bottomAnchor),
            channelsView.bottomAnchor.constraint(equalTo: statusBar.topAnchor),

            assistantView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            assistantView.widthAnchor.constraint(equalTo: panelView.widthAnchor),
            assistantView.topAnchor.constraint(equalTo: optionsBar.bottomAnchor),
            assistantView.bottomAnchor.constraint(equalTo: statusBar.topAnchor),

            infoView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            infoView.widthAnchor.constraint(equalTo: panelView.widthAnchor),
            infoView.topAnchor.constraint(equalTo: optionsBar.bottomAnchor),
            infoView.bottomAnchor.constraint(equalTo: statusBar.topAnchor),

            navigatorView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            navigatorView.widthAnchor.constraint(equalTo: panelView.widthAnchor),
            navigatorView.topAnchor.constraint(equalTo: optionsBar.bottomAnchor),
            navigatorView.bottomAnchor.constraint(equalTo: statusBar.topAnchor),

            // The band overlaps the panel by 3 pt and the well by 3 pt, so the
            // visible line, the panel's edge and the window-size sum are all
            // exactly where they were when this was a 1 px NSBox.
            panelSeparator.trailingAnchor.constraint(
                equalTo: panelView.leadingAnchor, constant: 3),
            panelSeparator.widthAnchor.constraint(equalToConstant: DS.panelDividerWidth),
            panelSeparator.topAnchor.constraint(equalTo: optionsBar.bottomAnchor),
            panelSeparator.bottomAnchor.constraint(equalTo: statusBar.topAnchor),

            statusBar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            statusBar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            statusBar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            statusBar.heightAnchor.constraint(equalToConstant: DS.statusBarHeight),

            statusLeft.leadingAnchor.constraint(equalTo: statusBar.leadingAnchor, constant: 14),
            statusLeft.centerYAnchor.constraint(equalTo: statusBar.centerYAnchor),
            statusLeft.trailingAnchor.constraint(
                lessThanOrEqualTo: statusRight.leadingAnchor, constant: -14),

            statusRight.trailingAnchor.constraint(
                equalTo: statusBar.trailingAnchor, constant: -14),
            statusRight.centerYAnchor.constraint(equalTo: statusBar.centerYAnchor),
        ])
        // The scroll view's trailing swaps between the panel separator
        // (layers visible) and the window edge (layers hidden).
        scrollTrailingToRoot = scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor)
        scrollTrailingToPanel = scrollView.trailingAnchor.constraint(
            equalTo: panelSeparator.leadingAnchor, constant: 3)
        scrollTrailingToPanel.isActive = true
        // The strips, the corner box, their gestures and the top/leading
        // constraint pairs (EditorViewController+Rulers.swift).
        installRulers(in: root)

        view = root
        updateOptionsBar()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        scrollView.contentView.postsBoundsChangedNotifications = true
        scrollView.contentView.postsFrameChangedNotifications = true
        let center = NotificationCenter.default
        center.addObserver(
            self, selector: #selector(magnificationDidChange(_:)),
            name: NSScrollView.didEndLiveMagnifyNotification, object: scrollView)
        center.addObserver(
            self, selector: #selector(magnificationDidChange(_:)),
            name: NSView.boundsDidChangeNotification, object: scrollView.contentView)
        center.addObserver(
            self, selector: #selector(magnificationDidChange(_:)),
            name: NSView.frameDidChangeNotification, object: scrollView.contentView)
        center.addObserver(
            self, selector: #selector(imageDidChange(_:)),
            name: .imageDocumentImageDidChange, object: document)
        center.addObserver(self, selector: #selector(selectionHistoryDidChange(_:)),
                           name: .imageDocumentSelectionDidChange, object: document)
        center.addObserver(self, selector: #selector(viewTargetsDidChange(_:)),
                           name: .imageDocumentViewTargetsDidChange, object: document)
        center.addObserver(self, selector: #selector(recoveryDidChange(_:)),
                           name: .imageDocumentRecoveryDidChange, object: document)
        center.addObserver(self, selector: #selector(historyDidChange(_:)),
                           name: .imageDocumentHistoryDidChange, object: document)
        syncSelectionHistory()
        // The view chrome is an APP-WIDE preference, so every open editor
        // answers the change and not only the window that made it
        // (EditorViewController+Rulers.swift).
        center.addObserver(
            self, selector: #selector(viewChromeDidChange(_:)),
            name: .editorViewChromeDidChange, object: nil)
        installHDRPreviewObservers()
        center.addObserver(self, selector: #selector(brushPresetsDidChange(_:)),
                           name: BrushPresetLibrary.didChange, object: nil)
        updateStatus()
        updateActiveLayerRect()
        updateZoomLabel()
        // A document arrives already carrying its guides and its ruler
        // origin (they are .rz state), and the notification that refreshes
        // both caches only fires on a CHANGE — so the first read has to
        // happen here, or a file saved with guides would show none until
        // something edited it.
        refreshCanvasGuides()
        refreshCanvasPath()
        refreshRulers()
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        guard !didRunInitialZoom else { return }
        didRunInitialZoom = true
        guard let doc = document?.doc else { return }
        layoutCanvasHost(canvasSize: doc.canvasSize)
        let visible = scrollView.contentSize
        if doc.canvasSize.width > visible.width || doc.canvasSize.height > visible.height {
            zoomToFit()
        } else {
            applyZoom(1.0)
        }
        updateStatus()
    }

    // MARK: - Tools

    func selectTool(_ tool: EditorTool) {
        guard DocumentPrecision.allows(tool, document: document) else { NSSound.beep(); return }
        // A planned tool has no behavior yet; menu validation already
        // disables its items, this backstops the rail and bare keys.
        guard !tool.planned else {
            NSSound.beep()
            return
        }
        // Switching tools leaves the transform session, which commits it
        // rather than silently dropping the drag; leaving the crop tool
        // quietly drops its (uncommitted) box.
        guard commitPendingTransform() else { return }
        if currentTool == .text, tool != .text {
            guard canvas.commitTextSession() else { return }
        }
        if currentTool == .crop, tool != .crop {
            endCropSession()
        }
        // Leaving a reopened shape commits it (the text session's rule);
        // an untouched session commits nothing.
        if shapeEditSession != nil, tool != currentTool {
            guard commitShapeEditSession() else { return }
        }
        currentTool = tool
        lastToolForKey[tool.keyCharacter] = tool
        canvas.tool = tool
        // Picking a tool that cannot reach the standing target silently
        // points the target back at the layer rather than blocking the tool
        // or painting the wrong thing — one rule
        // (`toolReachableTarget`, EditorViewController+Channels), applied
        // here on the tool edge and inside `setPaintTarget` on the target
        // edge, so the two orders agree.
        let reachable = toolReachableTarget(paintTarget)
        if reachable != paintTarget {
            setPaintTarget(reachable)
        }
        if tool == .crop {
            beginCropSession()
        }
        syncCanvasPaintState()
        updateOptionsBar()
        reflectSelectedTool(tool)
        updateStatus()
    }

    /// A bare tool key. A key shared by several tools cycles them in rail
    /// order on repeated presses, first returning to the one last used; a
    /// key owned by one tool just selects it (`EditorTool.tool(forKey:…)`).
    func selectToolForKey(_ tool: EditorTool) {
        let key = tool.keyCharacter
        guard let next = EditorTool.tool(
            forKey: key, current: currentTool, last: lastToolForKey[key],
            available: { DocumentPrecision.allows($0, document: self.document) })
        else {
            NSSound.beep()
            return
        }
        selectTool(next)
    }

    /// Pushes the options-store state the canvas consumes — the active
    /// paint tool's size and opacity, the shared color, the selection
    /// gesture options, the shape style, scrubby zoom — into the canvas.
    func syncCanvasPaintState() {
        let store = ToolOptionsStore.shared
        if !canvas.isPaintingStroke || canvas.nativeBrushEnabled { canvas.paintColor = paintColor }
        if let paint = store.paintOptions(for: currentTool),
           !canvas.isPaintingStroke || canvas.nativeBrushEnabled {
            canvas.brushSize = CGFloat(min(max(paint.size, 1), currentTool.maximumBrushSize))
            canvas.brushOpacity = CGFloat(min(max(paint.opacity, 0), 100) / 100)
            canvas.brushHardness = CGFloat(min(max(paint.hardness, 0), 100) / 100)
            canvas.brushFlow = CGFloat(min(max(paint.flow, 1), 100) / 100)
            canvas.brushSpacingPercent = CGFloat(min(max(paint.spacing, 1), 200))
            canvas.brushAngle = CGFloat(min(max(paint.angle, -180), 180))
            canvas.brushRoundness = CGFloat(min(max(paint.roundness, 1), 100) / 100)
            canvas.brushSmoothing = CGFloat(min(max(paint.smoothing, 0), 100) / 100)
            canvas.brushPressureSize = paint.pressureSize
            canvas.brushPressureOpacity = paint.pressureOpacity ?? false
            canvas.brushAirbrush = paint.airbrush
            canvas.strokeAligned = currentTool == .heal && paint.aligned // heal-only option
        }
        let modes: [SelectionCombineMode] = [.replace, .add, .subtract, .intersect]
        canvas.selectionCombineBase = modes[min(max(store.select.modeIndex, 0), 3)]
        canvas.selectionFeather = max(store.select.feather, 0)
        canvas.scrubbyZoom = store.view.scrubbyZoom
        // The whole of this phase's view chrome as ONE value (Guides.swift
        // says why it is one), built in ONE place — the grid's spacing is
        // derived from the document as well as from the preferences, so
        // `imageDidChange` refreshes it too. The snap engine is NOT pushed
        // here: the canvas asks for it at each gesture's mouse-down instead
        // (`onSnapEngine`, DragSnapping.swift).
        refreshCanvasChrome()
        syncCanvasShapeStyle()
        refreshMoveControls()
    }

    /// Bare [ and ] on the canvas: steps the active paint tool's size and
    /// mirrors it into the store, the canvas and the options bar.
    func brushSizeKeyChanged(_ newSize: CGFloat) {
        guard var paint = ToolOptionsStore.shared.paintOptions(for: currentTool) else { return }
        paint.size = Double(newSize)
        ToolOptionsStore.shared.setPaintOptions(paint, for: currentTool)
        // Re-read rather than trusting `newSize`: the store floors each tool's
        // size at what that tool can actually do something with, and stepping
        // a healing brush below three must show the size that was stored.
        syncCanvasPaintState()
        optionsBar.refreshValues()
    }

    // MARK: - Paint target (layer, its mask, a colour plane, a channel)

    /// Points every edit at the layer, its mask, one of its colour planes or
    /// one of the document's alpha channels (a mask target falls back to the
    /// layer when there is no mask, a channel target when the channel has
    /// gone), and mirrors the choice into the canvas, both panels and the
    /// document's own filter/adjustment hook.
    ///
    /// An adjustment layer's PIXEL target is never selectable — the
    /// compositor ignores its pixels — so a `.layer` or `.plane` request
    /// lands on the mask while one exists. A `.channel` request is document
    /// state and is deliberately left alone.
    ///
    /// Unsupported tool/target combinations retain the chosen target and
    /// refuse when invoked, so switching tools never redirects an edit.
    func setPaintTarget(_ target: PaintTarget) {
        let idx = document?.activeLayerIndex ?? 0
        var target = toolReachableTarget(target)
        if !target.isChannel,
           document?.doc?.layerIsAdjustment(idx) == true,
           document?.doc?.layerHasMask(idx) == true {
            target = .mask
        }
        if target == .mask, document?.doc?.layerHasMask(idx) != true {
            target = .layer
        }
        target = channelTargetClamped(target)
        let changed = target != paintTarget
        paintTarget = target
        // The identity of the channel just chosen, so a later renumbering can
        // find it again (channelTargetResolved). Recorded here rather than at
        // every call site because this is the ONE writer of paintTarget.
        paintTargetChannelID = channelIdentity(of: target)
        paintTargetLayer = idx
        canvas.paintTarget = target
        layersPanel?.setPaintTarget(target)
        channelsPanel?.setPaintTarget(target)
        document?.planeEditTarget = target
        if changed {
            precisionGeometryDocumentDidChange()
            updateStatus()
            refreshChannelDisplay()
        }
    }

    /// Drops a target that no longer applies — the active layer changed
    /// underneath a mask target, its mask was deleted, applied, or undone
    /// away, or a channel has gone — and forces the mask target whenever the
    /// active layer is an adjustment layer (its pixels are pointless to
    /// paint). A colour plane survives a layer change: planes always exist,
    /// and painting Red on another layer is meaningful.
    func syncPaintTarget() {
        let idx = document?.activeLayerIndex ?? 0
        // A CHANNEL is document state and survives any layer change; it only
        // goes away when the channel itself does — but the list renumbers
        // under it, so the index is re-found by identity, never range-checked
        // in place.
        if case .channel = paintTarget {
            paintTargetLayer = idx
            let resolved = channelTargetResolved()
            if resolved != paintTarget { setPaintTarget(resolved) }
            return
        }
        if document?.doc?.layerIsAdjustment(idx) == true,
           document?.doc?.layerHasMask(idx) == true {
            setPaintTarget(.mask)
            return
        }
        guard paintTarget == .mask else {
            paintTargetLayer = idx
            return
        }
        if idx != paintTargetLayer || document?.doc?.layerHasMask(idx) != true {
            setPaintTarget(.layer)
        }
    }

    /// Mirrors tool selection into the rail (display only). A grouped slot
    /// also starts standing for this tool, so the group remembers what was
    /// last used in it.
    func reflectSelectedTool(_ tool: EditorTool) {
        toolRail?.setSelectedTool(tool)
    }

    @objc func selectSelectTool(_ sender: Any?) { selectTool(.select) }
    @objc func selectEllipseTool(_ sender: Any?) { selectTool(.ellipseSelect) }
    @objc func selectLassoTool(_ sender: Any?) { selectTool(.lasso) }
    @objc func selectWandTool(_ sender: Any?) { selectTool(.wand) }
    @objc func selectSubjectTool(_ sender: Any?) { selectTool(.subject) }
    @objc func selectMoveTool(_ sender: Any?) { selectTool(.move) }
    @objc func selectBrushTool(_ sender: Any?) { selectTool(.brush) }
    @objc func selectEraserTool(_ sender: Any?) { selectTool(.eraser) }
    @objc func selectFillTool(_ sender: Any?) { selectTool(.fill) }
    @objc func selectGradientTool(_ sender: Any?) { selectTool(.gradient) }
    @objc func selectTextTool(_ sender: Any?) { selectTool(.text) }
    @objc func selectEyedropperTool(_ sender: Any?) { selectTool(.eyedropper) }
    @objc func selectCropTool(_ sender: Any?) { selectTool(.crop) }
    @objc func selectCloneTool(_ sender: Any?) { selectTool(.clone) }
    @objc func selectDodgeTool(_ sender: Any?) { selectTool(.dodge) }
    @objc func selectBlurBrushTool(_ sender: Any?) { selectTool(.blurBrush) }
    @objc func selectSharpenBrushTool(_ sender: Any?) { selectTool(.sharpenBrush) }
    @objc func selectSmudgeTool(_ sender: Any?) { selectTool(.smudge) }
    @objc func selectSpongeTool(_ sender: Any?) { selectTool(.sponge) }
    @objc func selectHistoryBrushTool(_ sender: Any?) { selectTool(.historyBrush) }
    @objc func selectArtHistoryBrushTool(_ sender: Any?) { selectTool(.artHistoryBrush) }
    @objc func selectPencilTool(_ sender: Any?) { selectTool(.pencil) }
    @objc func selectHealTool(_ sender: Any?) { selectTool(.heal) }
    @objc func selectSpotHealTool(_ sender: Any?) { selectTool(.spotHeal) }
    @objc func selectPatchTool(_ sender: Any?) { selectTool(.patch) }
    @objc func selectRedEyeTool(_ sender: Any?) { selectTool(.redEye) }
    @objc func selectShapeRectTool(_ sender: Any?) { selectTool(.shapeRect) }
    @objc func selectShapeEllipseTool(_ sender: Any?) { selectTool(.shapeEllipse) }
    @objc func selectShapeLineTool(_ sender: Any?) { selectTool(.shapeLine) }
    @objc func selectZoomTool(_ sender: Any?) { selectTool(.zoom) }
    @objc func selectHandTool(_ sender: Any?) { selectTool(.hand) }

    /// Rebuilds the fixed-height options bar for the current state. The
    /// descriptor lists live in EditorViewController+ToolOptions.swift; a
    /// Free Transform session takes the whole bar over (it is modal on the
    /// canvas, so the active tool's own options cannot be used).
    func updateOptionsBar() {
        presentToolOptions()
    }

    // MARK: - Agent access to the selection

    /// The canvas selection, for the agent's fill/gradient/crop tools.
    var agentSelection: CanvasSelection? { document?.canvasSelection }

    func agentSetSelection(_ selection: CanvasSelection?) {
        guard let document else { return }
        document.setCanvasSelection(selection)
    }

    // MARK: - Wand, fill, gradient actions

    /// The DOCUMENT's bytes of a color (straight alpha): an AUTHORED colour
    /// converts into the document's space exactly once, here, and a colour
    /// SAMPLED from the document is already in that space and passes through
    /// untouched — which is what makes Fill, Gradient and Plane Paint agree
    /// with the Brush, and the eyedropper round trip exact (ColorProfile).
    /// Internal, like the panels above: EditorViewController+PlanePaint
    /// builds a plane fill's gray with it.
    func colorBytes(_ color: NSColor) -> [UInt8] {
        // The conversion itself is `ColorProfile.bytes`, shared with the
        // agent's `colorRGBA`, so the tools and their MCP mirrors cannot
        // drift. Opaque black is the same refusal the old `?? .black` gave.
        ColorProfile.bytes(color, in: document?.nsColorSpace ?? .sRGB) ?? [0, 0, 0, 255]
    }

    private func wandClicked(_ point: CGPoint, mode: SelectionCombineMode) {
        if nativeWandClicked(point, mode: mode) { return }
        let options = ToolOptionsStore.shared.select
        guard let doc = document?.doc,
            let mask = wandMask(doc, at: point, options: options),
            var selection = CanvasSelection(
                shape: .mask(mask), canvasWidth: doc.width, canvasHeight: doc.height)
        else {
            NSSound.beep()
            return
        }
        // The bar's Feather applies here too — the wand commits outside the
        // canvas's commitSelection, so it feathers its own result.
        if options.feather > 0, let feathered = selection.feathered(by: options.feather) {
            selection = feathered
        }
        // An all-zero combination comes back nil and deselects.
        let combined = CanvasSelection.combine(canvas.selection, with: selection, mode: mode)
        let record: [ActionStep] = .selectMagicWand(
            at: point, tolerance: Int(options.tolerance.rounded()),
            contiguous: options.contiguous, mode: mode.agentName, feather: options.feather,
            sampleAllLayers: options.wandSampleAllLayers, sampleSize: options.wandSampleSize)
        commitCanvasSelection(combined, actionName: "Magic Wand", record: record)
    }

    private func fillClicked(_ point: CGPoint) {
        if refuseFillStyleOnCoverage() { return }
        if document?.doc?.isHighDepth == true { fillPrecisionColor(at: point); return }
        guard !refuseOperationScope(.fill) else { return }
        guard let document = document else { return }
        // Every scalar target, including a mask, uses the shared round trip.
        if paintTarget.isCoverage {
            fillPlane(at: point)
            return
        }
        // A canvas click can't be blocked by menu validation: refuse a fill
        // aimed at an adjustment layer's (ignored) pixels — or at a LOCKED
        // layer — with the alert that names the reason.
        guard !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit() else { return }
        guard !refuseLockedEdit(layer: document.activeLayerIndex, kind: DR_EDIT_PIXELS)
        else { return }
        // A Mode or Sample all layers off its default (+BucketFill).
        if fillWithStyle(at: point) { return }
        let options = ToolOptionsStore.shared.fill
        let idx = document.activeLayerIndex
        // The bar's opacity rides in the fill color's own alpha.
        let opacity = min(max(options.opacity, 0), 100) / 100
        let color = fillToolColor
        let rgba = colorBytes(color.withAlphaComponent(color.alphaComponent * CGFloat(opacity)))
        guard let capture = document.captureOperation() else { NSSound.beep(); return }
        document.applyCapturedRewrite(
            capture, "Fill",
            record: fillRecord(at: point, color: color, opacity: opacity, options: options)
        ) {
            capture.doc.bucketFilled(
                idx, x: Int(point.x), y: Int(point.y),
                tolerance: Int(options.tolerance.rounded()),
                rgba: rgba, contiguous: options.contiguous, mask: capture.selection)
        }
    }

    /// Eyedropper sample (tool click/drag tick, or Option borrowing it from
    /// brush/fill/gradient): reads the pixel under the cursor from the
    /// FLATTENED COMPOSITE — the same projection the canvas draws and the
    /// magic wand samples — and routes it into the shared paint color.
    /// Points outside the canvas no-op. Sampling is NOT an edit: no undo
    /// step, no change counting.
    private func sampleColor(at point: CGPoint) {
        if document?.doc?.isHighDepth == true { samplePrecisionColor(at: point); return }
        guard let document = document, let projection = eyedropperComposite() else { return }
        let options = ToolOptionsStore.shared.sample
        // Sample size: a point, or the plain mean of the 3×3 / 5×5 window
        // (out-of-bounds pixels just drop out of the mean).
        let reach = [0, 1, 2][min(max(options.sampleSizeIndex, 0), 2)]
        // The core's sampler (dr_image_sample), which the Info panel and
        // sample_pixel read through too: the plain truncating mean of the
        // block with out-of-bounds pixels dropped, and a CENTRE that may
        // itself be outside — a sample at the canvas edge stays a sample
        // rather than an edge pin, exactly as it has always behaved here.
        guard let sample = projection.sample(
            x: Int(floor(point.x)), y: Int(floor(point.y)), reach: reach)
        else { return }
        // The pixel's numbers are the DOCUMENT's, so the swatch is built in
        // the document's space and converted nowhere: the hex readout reports
        // what the pixel actually holds, and painting it back is a no-op.
        // Into the foreground, the ring and Copy on Pick: `pickSample`
        // (+Eyedropper), shared with the 16-bit and float paths.
        pickSample(
            NSColor(
                colorSpace: document.nsColorSpace,
                components: [
                    CGFloat(sample.r) / 255, CGFloat(sample.g) / 255,
                    CGFloat(sample.b) / 255, CGFloat(sample.a) / 255,
                ],
                count: 4),
            text: RasterImage.hexString(sample), at: point)
    }

    private func gradientCommitted(_ a: CGPoint, _ b: CGPoint) {
        if refuseGradientModeOnCoverage() { return }
        if gradientQuickMask(from: a, to: b) { return }
        if document?.doc?.isHighDepth == true { gradientPrecisionColor(from: a, to: b); return }
        guard !refuseOperationScope(.gradient) else { return }
        // The plane and layer commits: +PlanePaint and +GradientTool.
        if paintTarget.isCoverage { gradientPlane(from: a, to: b) } else { gradientLayer(from: a, to: b) }
    }

    // MARK: - Options bar and rail plumbing

    /// The single write path for the shared paint color (options-bar
    /// swatches, eyedropper samples, the rail's foreground swatch): brush,
    /// fill, gradient start, and text all read `paintColor`, and the rail,
    /// store and canvas mirror it.
    func setPaintColor(_ color: NSColor) {
        paintColor = color
        toolRail?.foregroundSwatchColor = color
        canvas.paintColor = color
        canvas.updateActiveTextSessionStyle()
        var shared = ToolOptionsStore.shared.sharedState
        shared.foreground = TextLayer.hex(color)
        shared.foregroundInk = nil
        ToolOptionsStore.shared.sharedState = shared
    }

    /// The background color's write path (the rail's second swatch): the
    /// gradient tool's end color.
    func setBackgroundColor(_ color: NSColor) {
        backgroundColor = color
        toolRail?.backgroundSwatchColor = color
        var shared = ToolOptionsStore.shared.sharedState
        shared.background = TextLayer.hex(color)
        shared.backgroundInk = nil
        ToolOptionsStore.shared.sharedState = shared
    }

    /// Rail swatch clicks: the Color Picker sheet
    /// (`EditorViewController+ColorPair.swift`).
    func pickForegroundColor() { presentColorPicker(.foreground) }

    func pickBackgroundColor() { presentColorPicker(.background) }

    /// The shared color panel, retargeted at `slot` — the picker's System
    /// Colors… button.
    func openSystemColorPanel(_ slot: ColorPairSlot) {
        openColorPanel(action: slot == .foreground
            ? #selector(colorPanelPickedForeground(_:)) : #selector(colorPanelPickedBackground(_:)))
    }

    private func openColorPanel(action: Selector) {
        let panel = NSColorPanel.shared
        panel.showsAlpha = true
        panel.setTarget(self)
        panel.setAction(action)
        colorPanelTargetsSelf = true
        panel.color = action == #selector(colorPanelPickedForeground(_:))
            ? paintColor : backgroundColor
        panel.makeKeyAndOrderFront(nil)
    }

    @objc private func colorPanelPickedForeground(_ sender: Any?) {
        setPaintColor(NSColorPanel.shared.color)
        optionsBar.refreshValues()
    }

    @objc private func colorPanelPickedBackground(_ sender: Any?) {
        setBackgroundColor(NSColorPanel.shared.color)
        optionsBar.refreshValues()
    }

    /// Fired by the options bar after any control commits a value: mirror
    /// whatever may have changed into the canvas and the status line.
    func toolOptionsEdited() {
        syncCanvasPaintState()
        canvas.textStyle = currentTextStyle()
        canvas.updateActiveTextSessionStyle()
        toolRail?.foregroundSwatchColor = paintColor
        toolRail?.backgroundSwatchColor = backgroundColor
        updateStatus()
    }

    /// Segment order of the alignment control, which is also the payload's
    /// `alignments` order.
    static let alignmentSegmentValues: [NSTextAlignment] = [.left, .center, .right]

    static func alignmentIndex(_ alignment: NSTextAlignment) -> Int {
        alignmentSegmentValues.firstIndex(of: alignment) ?? 0
    }

    // MARK: - Zoom

    private var visibleCenterInCanvas: NSPoint {
        // The clip view's bounds are expressed in the document view's
        // coordinate space, so its midpoint is the visible center.
        let clipBounds = scrollView.contentView.bounds
        return NSPoint(x: clipBounds.midX, y: clipBounds.midY)
    }

    private func applyZoom(_ magnification: CGFloat) {
        let clamped = min(max(magnification, scrollView.minMagnification), scrollView.maxMagnification)
        scrollView.setMagnification(clamped, centeredAt: visibleCenterInCanvas)
        updateZoomLabel()
    }

    func zoomIn() {
        let current = scrollView.magnification
        let next = Self.zoomLadder.first { $0 > current + 0.0001 } ?? Self.zoomLadder.last!
        applyZoom(next)
    }

    func zoomOut() {
        let current = scrollView.magnification
        // Below the ladder floor (fit of a huge image, pinch to minimum) there
        // is no smaller stop; do nothing rather than jump UP to the floor.
        guard let next = Self.zoomLadder.last(where: { $0 < current - 0.0001 }) else { return }
        applyZoom(next)
    }

    func zoomActual() {
        applyZoom(1.0)
    }

    /// The zoom tool's click: one ladder step in (or out, with ⌥), keeping
    /// the clicked point where it is.
    func zoomStep(at point: CGPoint, out: Bool) {
        let current = scrollView.magnification
        let next: CGFloat
        if out {
            guard let below = Self.zoomLadder.last(where: { $0 < current - 0.0001 }) else { return }
            next = below
        } else {
            next = Self.zoomLadder.first { $0 > current + 0.0001 } ?? Self.zoomLadder.last ?? 1
        }
        scrollView.setMagnification(
            min(max(next, scrollView.minMagnification), scrollView.maxMagnification),
            centeredAt: canvasHost.convert(point, from: canvas))
        updateZoomLabel()
    }

    /// The zoom tool's marquee: fill the viewport with the dragged rect.
    func zoomToRect(_ rect: CGRect) {
        guard rect.width > 0, rect.height > 0 else { return }
        scrollView.magnify(toFit: canvasHost.convert(rect, from: canvas))
        updateZoomLabel()
    }

    func zoomToFit() {
        guard document?.doc != nil else { return }
        let size = canvasHost.frame.size
        guard size.width > 0, size.height > 0 else { return }
        let margin: CGFloat = 16
        let available = NSSize(
            width: max(scrollView.contentSize.width - margin * 2, 1),
            height: max(scrollView.contentSize.height - margin * 2, 1))
        let scale = min(available.width / size.width, available.height / size.height)
        applyZoom(min(scale, 8)) // fit may exceed 100% for small images, capped at 8
    }

    // MARK: - Document change

    @objc private func imageDidChange(_ note: Notification) {
        guard let document = document,
              (note.object as? ImageDocument) === document,
              let doc = document.doc
        else { return }
        precisionGeometryDocumentDidChange()
        magneticLassoDocumentDidChange()
        puppetWarpDocumentDidChange()
        perspectiveWarpDocumentDidChange()
        toolOptionsDocumentDidChange(note)
        // The document moved underneath an open transform session — only an
        // agent edit can do this, since every user-driven edit is blocked
        // while the session runs. Its matrix describes the layer rect the
        // session opened on, so replaying it against a changed stack would be
        // wrong: drop the session instead. The session's own commit sets the
        // flag and is exempt.
        if transformSession != nil, transformSession?.precisionGesture == nil,
           !isCommittingTransform {
            endTransformSession()
        }
        // Same for an open shape-edit session: an external edit (agent,
        // undo) may have renumbered or rewritten the layer it describes,
        // so it closes without committing. A successful commit's own
        // publication lands here too and ends the session exactly as the
        // commit does; a refused commit changes no document and keeps it
        // open.
        if shapeEditSession != nil {
            endShapeEditSession()
        }
        let newSize = doc.canvasSize
        // bounds, not frame: a rotated canvas's frame is the turned box.
        let dimensionsChanged = canvas.bounds.size != newSize
        canvas.documentColorSpace = doc.drawingSpace
        canvas.image = document.previewImage
        refreshProofImage()
        refreshClippingOverlay()
        canvas.previewImage = nil
        channelDisplayDidChange(note)
        layoutCanvasHost(canvasSize: newSize)
        if dimensionsChanged {
            zoomToFit()
            // A crop box measured against the old canvas is meaningless:
            // reopen it over the new one.
            if currentTool == .crop,
               cropSession?.precisionGesture == nil || isCommittingPrecisionCrop {
                beginCropSession()
            }
        }
        syncSelectionHistory()
        canvas.needsDisplay = true
        syncPaintTarget()
        // The guides are the document's; the rulers' unit conversion reads
        // its resolution and its ruler origin, and the grid's spacing — in
        // canvas pixels — is converted through that same resolution and the
        // canvas extent, so all three are refreshed on exactly the events
        // that can move them. Each is guarded on its own value, so an edit
        // that touched none of them costs three comparisons.
        refreshCanvasGuides()
        refreshCanvasPath(note)
        refreshRulers()
        refreshCanvasChrome()
        // The snap engine's folded content boxes are a per-pixel sweep per
        // leaf, and a Move drag posts this notification on EVERY mouse-moved
        // event — so only a SETTLED change invalidates them. A live tick
        // differs from the base only in the layers being moved, which the
        // engine excludes anyway, and the engine is frozen for the gesture
        // regardless.
        // "not explicitly live", rather than "explicitly settled": a stale
        // cache is a correctness bug while an extra invalidation only costs
        // one fold, so a post that carried no flag at all must clear it.
        if (note.userInfo?["isLive"] as? Bool) != true { snapBoxes = nil }
        updateStatus()
        updateActiveLayerRect()
        refreshNavigator(isLive: (note.userInfo?["isLive"] as? Bool) == true)
        view.window?.subtitle = "\(doc.width) × \(doc.height) px"
    }

    // MARK: - Status bar

    // The redesign's reduced segment set: dimensions, mode, selection on
    // the left; the active tool and its key on the right. Layer name, blend
    // mode, opacity and the zoom percentage were deliberately dropped —
    // all visible in the Layers panel or the zoom pill.
    func updateStatus() {
        // The Channels panel's Load and Save Selection buttons are nil-target
        // actions, which AppKit never validates: they take their menu twins'
        // own rules from here, where every selection change and every Quick
        // Mask toggle already lands.
        channelsPanel?.setSelectionState(
            hasSelection: canvas.selection != nil, quickMask: canvas.quickMaskActive)
        // The Info panel's Selection rows land here for the same reason;
        // the pixel COUNT is a canvas scan, so whether it is worth taking
        // now is EditorViewController+Info's decision, not this line's.
        infoPanel?.setSelectionState(bounds: canvas.selectionRect, area: selectedPixelArea())
        pathsPanel?.setSelectionState(hasSelection: canvas.selection != nil)
        guard let document = document, let doc = document.doc else {
            statusDims.text = "No document open"
            statusMode.text = ""
            statusSelection.text = ""
            statusTool.text = "Drop a file, or ⌘O"
            return
        }
        // Folded into the two existing segments rather than adding a sixth:
        // the redesign deliberately reduced the set.
        statusDims.text =
            "\(doc.width) × \(doc.height) px · \(PrintSize.resolutionText(doc.resolution))"
        statusMode.text = document.projectionError == nil
            ? "\(doc.modeStatusText) · \(doc.profileName)" : "Preview failed — Save as .doctored"
        // The soft proof's state, or why it turned itself off.
        if let proof = proofStatusText { statusMode.text += " · \(proof)" }
        if let exposure = viewExposureStatusText { statusMode.text += " · \(exposure)" }
        if let recovery = recoveryStatus { statusMode.text += " · \(recovery.text)" }
        if let warning = historyWarningStatus { statusMode.text += " · \(warning.text)" }
        statusMode.toolTip = document.projectionError ?? recoveryStatus?.detail
            ?? historyWarningStatus?.detail
        statusSelection.toolTip = canvas.selection?.outlineNotice
        if canvas.quickMaskActive {
            // The selection segment's slot: the mode holds the selection as
            // its editable buffer, so this is what "selected" currently is.
            statusSelection.text = "Quick Mask"
        } else if let selection = canvas.selectionRect {
            statusSelection.text =
                "Selection: \(Int(selection.width)) × \(Int(selection.height)) px"
            if canvas.selection?.contourIsSimplified == true { statusSelection.text += " · bounds outline" }
        } else {
            statusSelection.text = "Selection: none"
        }
        // Brush and eraser hit the mask, a colour plane or an alpha channel
        // when one is the edit target; say so, alongside the panels' rings.
        let maskSuffix = paintTarget.statusSuffix(in: document.doc)
        statusTool.text = isTransforming
            ? transformStatusText
            : "\(currentTool.displayName) · \(currentTool.keyCharacter.uppercased())\(maskSuffix)"
        updateZoomLabel()
    }

    /// Pushes the active layer's extent (image-pixel coordinates) to the
    /// canvas, which shows the boundary while a paint tool is active and
    /// the layer doesn't cover the whole canvas.
    func updateActiveLayerRect() {
        refreshMoveControls()
        guard let document = document, let doc = document.doc,
              let info = doc.layerInfo(document.activeLayerIndex)
        else {
            canvas.activeLayerRect = nil
            return
        }
        canvas.activeLayerRect = CGRect(
            x: CGFloat(info.offsetX), y: CGFloat(info.offsetY),
            width: CGFloat(info.width), height: CGFloat(info.height))
    }

    private func updateZoomLabel() {
        let percent = Int((scrollView.magnification * 100).rounded())
        zoomPill.setZoomText("\(percent)%")
        // The zoom tool's options show the same number.
        if currentTool == .zoom || currentTool == .hand {
            optionsBar.refreshValues()
        }
    }

    /// The magnification as the options bar's percentage field, applied
    /// through the same clamp the menu actions use.
    var zoomPercent: Double {
        get { Double(scrollView.magnification) * 100 }
        set { applyZoom(CGFloat(newValue / 100)) }
    }

    /// Fill the viewport: the larger of the two fit scales, so the canvas
    /// covers the well with no letterboxing.
    func zoomToFill() {
        guard document?.doc != nil else { return }
        let size = canvasHost.frame.size
        guard size.width > 0, size.height > 0 else { return }
        let available = scrollView.contentSize
        let scale = max(available.width / size.width, available.height / size.height)
        applyZoom(min(scale, 32))
    }

    @objc private func magnificationDidChange(_ note: Notification) {
        updateZoomLabel()
        refreshNavigator()
        propagateViewSync()
        // Scrolling and live pinch both arrive here through the clip view's
        // bounds change, so this one call keeps the ticks under the picture
        // at every zoom and every scroll offset.
        refreshRulers()
        canvas.brushOutlineViewportDidChange()
        canvas.marchingAntsViewportDidChange()
    }

    // MARK: - Edit actions (responder chain)

    private func performEdit(
        _ actionName: String, record: [ActionStep],
        _ transform: (RasterDocument) -> RasterDocument?
    ) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        document.applyEdit(actionName, record: record, transform)
    }

    /// Internal, like the panels above: EditorViewController+AutoAdjust
    /// builds on it (Auto Tone and its two siblings are layer edits).
    func performLayerEdit(
        _ actionName: String, record: [ActionStep], _ op: (RasterImage) -> RasterImage?
    ) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        guard !refuseOperationScope(.filter) else { return }
        document.applyToActiveLayer(actionName, record: record, op)
    }

    // Whole-document geometry goes through applyingDocumentGeometry
    // (DescribedLayerGeometry.swift), which composes the op into every text
    // and shape description so a later re-edit lands in place.
    @objc func rotateCW(_ sender: Any?) {
        performGeometry(.rotate90)
    }

    @objc func rotateCCW(_ sender: Any?) {
        performGeometry(.rotate270)
    }

    @objc func rotate180(_ sender: Any?) {
        performGeometry(.rotate180)
    }

    @objc func flipH(_ sender: Any?) {
        performGeometry(.flipHorizontal)
    }

    @objc func flipV(_ sender: Any?) {
        performGeometry(.flipVertical)
    }

    private func performGeometry(_ op: DocumentGeometry) {
        document?.applyEdit(op.actionName, record: Self.geometryRecord(op),
                            changesCanvasGeometry: true,
                            resultingSelection: document?.selectionCarried(through: op)) {
            try $0.applyingDocumentGeometry(op)
        }
    }

    @objc func cropToSelection(_ sender: Any?) {
        guard let document = document, let selection = canvas.selectionRect else {
            NSSound.beep()
            return
        }
        let (x, y, w, h) = (Int(selection.minX), Int(selection.minY), Int(selection.width), Int(selection.height))
        document.applyEdit("Crop", record: .crop(selection), changesCanvasGeometry: true,
            resultingSelection: document.selectionCarried(croppedToX: x, y: y, width: w, height: h)
        ) { doc in
            try doc.cropped(
                x: Int(selection.minX), y: Int(selection.minY),
                w: Int(selection.width), h: Int(selection.height))
        }
    }

    @objc func resizeImage(_ sender: Any?) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(ImageSizeSheetController(document: document))
    }

    @objc func showCanvasSize(_ sender: Any?) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(CanvasSizeSheetController(document: document))
    }

    // MARK: - Layer actions (Layer menu + panel footer buttons)

    @objc func newLayer(_ sender: Any?) {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let idx = document.activeLayerIndex
        let name = "Layer \(doc.layerCount + 1)"
        let before = document.doc
        // Where the new entry lands is the CORE's answer (`idx + 1` is the
        // wrong index the moment `idx` names a group), taken BEFORE the edit
        // because the handle is replaced by it.
        let position = doc.newEntryPosition(active: idx), landing = doc.newEntryLandingIndex(active: idx)
        document.applyEdit("New Layer", record: .newLayer(name: name, position: position)) {
            try $0.addingLayer(anchor: idx, position: position, name: name)
        }
        guard document.doc !== before else { return }
        // The active layer moved: setActiveLayer carries the whole invariant
        // (paint target, both panels, the canvas's mask base and rubylith).
        setActiveLayer(min(landing, document.doc.layerCount - 1))
    }

    /// Duplicate Layer, over the whole selection: ONE core call, never a
    /// host loop — every structural op renumbers, so a loop would duplicate
    /// the wrong entries from its second iteration on. A group duplicates
    /// with its whole subtree.
    @objc func duplicateLayer(_ sender: Any?) {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let indices = document.selectedLayerIndices
        let idx = document.activeLayerIndex
        let before = document.doc
        // Where the PRIMARY's copy lands, derived from the stack before the
        // edit: each copy sits above its source, and the copies made below
        // push this one up.
        // …and the copies are made over the selection's INDEPENDENT ROOTS, so
        // the primary's landing is looked up by entry rather than by position.
        let landing = doc.layerTree.duplicateLanding(of: idx, in: indices) ?? idx
        document.applyEdit(
            "Duplicate Layer",
            record: .selectedLayersCommand("duplicate_layer", note: "Layer ▸ Duplicate Layer")
        ) { try $0.duplicateLayers(indices) }
        guard document.doc !== before else { return }
        // The active layer moved: setActiveLayer carries the whole invariant.
        setActiveLayer(min(max(landing, 0), document.doc.layerCount - 1))
    }

    /// Delete Layer, over the whole selection: again ONE core call, which is
    /// what makes "delete these three" safe — a host loop deleting
    /// ascending would delete the wrong layers after the first.
    @objc func deleteLayer(_ sender: Any?) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        let indices = document.selectedLayerIndices
        document.applyEdit(
            "Delete Layer",
            record: .selectedLayersCommand("delete_layer", note: "Layer ▸ Delete Layer")
        ) { try $0.removeLayers(indices) }
        // Collapse onto ONE survivor, the lowest slot the deletion left.
        // `applyEdit`'s re-clamp only pulls stale numbers back into range,
        // and after a delete those numbers name layers that were never
        // selected — the panel would highlight them and the next set command
        // (Group, Merge, Align, a Move drag) would act on them. Every other
        // set op in this phase retargets after its edit; so does this one.
        setActiveLayer(min(max(indices.first ?? 0, 0), (document.doc?.layerCount ?? 1) - 1))
        // The active layer moved: any mask paint target goes with it.
        syncPaintTarget()
        layersPanel.reload()
        updateStatus()
        updateActiveLayerRect()
    }

    /// Merge Down on one layer, Merge Layers on a selection (the retitling
    /// the menu item's validation does).
    ///
    /// "The layer below" is now the previous SIBLING, so merging inside a
    /// group never reaches out of it; the core answers where the merged
    /// entry landed by leaving it at the lowest member's slot.
    @objc func mergeDown(_ sender: Any?) {
        guard let document = document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let indices = document.selectedLayerIndices
        let before = document.doc
        if indices.count > 1 {
            // The merged entry lands at the LOWEST member's subtree start,
            // not at its index: those differ the moment that member is a
            // group, and `indices[0]` then names an unrelated layer.
            let landing = doc.layerTree.subtree(of: indices[0]).lowerBound
            // The merged entry replaces the lowest member's picture, so that
            // member's Pixels / Transparency locks refuse the whole merge.
            guard !refuseLockedEdit(layer: indices[0], kind: DR_EDIT_MERGE) else { return }
            document.applyEdit(
                "Merge Layers",
                record: .selectedLayersCommand("merge_down", note: "Layer ▸ Merge Layers")
            ) { try $0.mergeLayers(indices) }
            guard document.doc !== before else { return }
            setActiveLayer(min(landing, document.doc.layerCount - 1))
            return
        }
        let idx = document.activeLayerIndex
        // #1 phase 4c: ⌘E on one group is Merge Group (+MergeStamp.swift).
        if doc.layerIsGroup(idx) { mergeGroup(sender); return }
        let siblings = doc.layerTree.siblings(of: idx)
        guard let below = siblings.last(where: { $0 < idx }) else {
            NSSound.beep()
            return
        }
        // The merge replaces the layer BELOW: its Pixels and Transparency
        // locks refuse it, and the alert names which.
        guard !refuseLockedEdit(layer: below, kind: DR_EDIT_MERGE) else { return }
        // The merged entry lands at the previous sibling's SUBTREE START, not
        // at its index — the same hazard the Merge Layers branch above names,
        // and `below` is an unrelated layer the moment that sibling is a
        // non-empty group.
        let landing = doc.layerTree.subtree(of: below).lowerBound
        document.applyEdit("Merge Down", record: .mergeDown) { try $0.mergingDown(idx) }
        guard document.doc !== before else { return }
        // The active layer moved: setActiveLayer carries the whole invariant.
        setActiveLayer(min(landing, document.doc.layerCount - 1))
    }

    @objc func flattenImage(_ sender: Any?) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        document.applyEdit("Flatten Image", record: .flattenImage) { try $0.flattening() }
        // The active layer moved: any mask paint target goes with it.
        syncPaintTarget()
        layersPanel.reload()
        updateStatus()
        updateActiveLayerRect()
    }

    @objc func pasteAsNewLayer(_ sender: Any?) {
        pastePlanned(placement: .auto)
    }

    // Bound to ⌘V through the responder chain, so a focused field editor
    // (layer rename, sheet fields, canvas text session) claims paste: first
    // and pastes text normally; canvas focus pastes as a new layer.
    @objc func paste(_ sender: Any?) {
        pasteAsNewLayer(sender)
    }

    // MARK: - Layer mask actions (Layer > Mask)

    /// Whether the active layer carries a mask, and whether that mask is
    /// enabled (menu validation, paint-target rules).
    /// Beside a VECTOR mask "the layer mask" is the raster mask kept with it,
    /// which is what Layer ▸ Mask edits (`RasterDocument.maskEditBaseline`).
    private var activeLayerHasMask: Bool {
        guard let document = document, let doc = document.doc else { return false }
        return doc.maskEditBaseline(document.activeLayerIndex) != nil
    }

    private var activeLayerMaskEnabled: Bool {
        guard let document = document, let doc = document.doc else { return false }
        return doc.maskEditBaseline(document.activeLayerIndex)?.enabled ?? false
    }

    @objc func addLayerMaskRevealAll(_ sender: Any?) {
        if document?.doc?.isHighDepth == true { addPrecisionMask(kind: DR_MASK_REVEAL_ALL); return }
        addLayerMask(kind: DR_MASK_REVEAL_ALL, selection: nil)
    }

    @objc func addLayerMaskHideAll(_ sender: Any?) {
        if document?.doc?.isHighDepth == true { addPrecisionMask(kind: DR_MASK_HIDE_ALL); return }
        addLayerMask(kind: DR_MASK_HIDE_ALL, selection: nil)
    }

    @objc func addLayerMaskFromSelection(_ sender: Any?) {
        if document?.doc?.isHighDepth == true { addPrecisionMask(kind: DR_MASK_FROM_SELECTION); return }
        guard let selection = canvas.selection else {
            NSSound.beep()
            return
        }
        // Canvas-sized at (0, 0), never cropped to the layer.
        addLayerMask(kind: DR_MASK_FROM_SELECTION, selection: selection)
    }

    private func addLayerMask(kind: DrMaskKind, selection: CanvasSelection?) {
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        let idx = document.activeLayerIndex
        document.applyEdit(
            "Add Layer Mask", record: .addLayerMask(kind: Self.layerMaskKindName(kind))
        ) {
            if let selection { return $0.addingLayerMask(idx, fromSelection: selection) }
            return $0.addingLayerMask(idx, kind: kind)
        }
        updateStatus()
    }

    @objc func deleteLayerMask(_ sender: Any?) {
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        let idx = document.activeLayerIndex
        document.applyEdit("Delete Layer Mask", record: .removeLayerMask(apply: false)) {
            try $0.removingLayerMask(idx, apply: false)
        }
        updateStatus()
    }

    @objc func applyLayerMask(_ sender: Any?) {
        if document?.doc?.isHighDepth == true { applyNativeLayerMask(); return }
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        let idx = document.activeLayerIndex
        // Applying a mask multiplies its coverage into the layer's ALPHA,
        // which is exactly what Lock Transparency forbids — its own edit
        // kind in the core, so the refusal names the lock instead of beeping.
        guard !refuseLockedEdit(layer: idx, kind: DR_EDIT_MASK_APPLY) else { return }
        document.applyEdit("Apply Layer Mask", record: .removeLayerMask(apply: true)) {
            try $0.removingLayerMask(idx, apply: true)
        }
        updateStatus()
    }

    @objc func toggleLayerMaskEnabled(_ sender: Any?) {
        guard let document = document, let doc = document.doc,
              doc.layerHasMask(document.activeLayerIndex)
        else {
            NSSound.beep()
            return
        }
        let idx = document.activeLayerIndex
        let enabled = !doc.layerMaskEnabled(idx)
        document.applyEdit(
            enabled ? "Enable Layer Mask" : "Disable Layer Mask",
            record: .setLayerMaskEnabled(enabled)
        ) {
            try $0.withLayerMaskEnabled(idx, enabled)
        }
        updateStatus()
    }

    // MARK: - Adjustment layers (Layer > New Adjustment Layer / Adjustment Options)

    /// Whether the ACTIVE layer composites as an adjustment (asked of the
    /// core — the authoritative parse). Gates every pixel-destructive path:
    /// an adjustment layer's pixels are ignored, so editing them is
    /// meaningless.
    private var activeLayerIsAdjustment: Bool {
        guard let document = document, let doc = document.doc else { return false }
        return doc.layerIsAdjustment(document.activeLayerIndex)
    }

    /// Refuses a pixel edit aimed at the active adjustment layer with a
    /// brief app-modal alert (the canvas-click paths — fill, gradient — are
    /// outside menu validation's reach); true when refused. Move and Free
    /// Transform deliberately do NOT come through here: they move the mask
    /// footprint, which is meaningful.
    /// Internal, like `colorBytes` above: EditorViewController+PlanePaint's
    /// fill and gradient guard with the same alert.
    func refuseAdjustmentPixelEdit() -> Bool {
        guard activeLayerIsAdjustment else { return false }
        let alert = NSAlert()
        alert.messageText = "Adjustment layers have no pixels to edit."
        alert.informativeText = "Paint on the layer's mask instead, or target another layer."
        alert.runModal()
        return true
    }

    @objc func newAdjustmentLayerBCS(_ sender: Any?) { newAdjustmentLayer(.bcs) }
    @objc func newAdjustmentLayerCurves(_ sender: Any?) { newAdjustmentLayer(.curves) }
    @objc func newAdjustmentLayerLevels(_ sender: Any?) { newAdjustmentLayer(.levels) }
    @objc func newAdjustmentLayerHueRotate(_ sender: Any?) { newAdjustmentLayer(.hueRotate) }
    @objc func newAdjustmentLayerPosterize(_ sender: Any?) { newAdjustmentLayer(.posterize) }
    @objc func newAdjustmentLayerThreshold(_ sender: Any?) { newAdjustmentLayer(.threshold) }
    @objc func newAdjustmentLayerInvert(_ sender: Any?) { newAdjustmentLayer(.invert) }
    @objc func newAdjustmentLayerGrayscale(_ sender: Any?) { newAdjustmentLayer(.grayscale) }
    @objc func newAdjustmentLayerSepia(_ sender: Any?) { newAdjustmentLayer(.sepia) }

    /// Layer > New Adjustment Layer > <op>. Parameterless ops create
    /// immediately (one undo step); parameterized ops open their live-preview
    /// sheet, and only its Apply commits. Either way the new layer's mask
    /// captures the CURRENT selection (marquee left up, exactly like
    /// Layer > Mask > From Selection) or is reveal-all.
    /// Internal, like the panels above: EditorViewController+Adjustments
    /// builds on it (the twelve phase-5 ops share one tagged selector).
    func newAdjustmentLayer(_ op: AdjustmentLayerOp) {
        if op.isParameterless, applyPrecisionParameterless(op, create: true) { return }
        guard let document = document, document.doc != nil else {
            NSSound.beep()
            return
        }
        let selection = canvas.selection?.maskBytes()
        guard op.isParameterless else {
            guard let sheet = AdjustmentLayerSheetController.make(
                op: op, document: document, canvas: canvas,
                mode: .create(selection: selection),
                onCommitted: { [weak self] idx in self?.didCommitAdjustmentLayer(idx) })
            else {
                NSSound.beep()
                return
            }
            presentAsSheet(sheet)
            return
        }
        guard let meta = AdjustmentLayerPayload(op: op).json() else {
            NSSound.beep()
            return
        }
        let below = document.activeLayerIndex
        let before = document.doc
        let landing = before?.newEntryLandingIndex(active: below) ?? below + 1
        document.applyEdit(
            "New \(op.displayName) Layer",
            record: .addAdjustmentLayer(meta: meta, name: op.displayName,
                                        position: before?.newEntryPosition(active: below) ?? .above)
        ) {
            $0.addingAdjustmentLayer(
                active: below, name: op.displayName, meta: meta, selection: selection)
        }
        guard document.doc !== before else { return }
        didCommitAdjustmentLayer(min(landing, document.doc.layerCount - 1))
    }

    /// Post-commit bookkeeping shared by every adjustment-layer commit (the
    /// steps newLayer takes): select the layer, then refresh. syncPaintTarget
    /// lands brush/eraser on the layer's mask.
    /// Internal, like the panels above: EditorViewController+Adjustments
    /// hands it the index its sheets commit.
    func didCommitAdjustmentLayer(_ idx: Int) {
        guard let document = document, document.doc != nil else { return }
        document.activeLayerIndex = min(max(idx, 0), document.doc.layerCount - 1)
        // Unconditional (not setActiveLayer): re-committing the SAME
        // adjustment layer from its options sheet moves no index but still
        // needs every panel and the canvas refreshed.
        activeLayerDidChange()
    }

    /// Layer > Adjustment Options… — enabled only when the active layer is
    /// an adjustment layer whose op has a dialog.
    @objc func adjustmentOptions(_ sender: Any?) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        editAdjustmentLayer(document.activeLayerIndex)
    }

    /// Makes `idx` the ONLY selected layer and refreshes everything that
    /// follows it — the paint target, the panel, the status line, the
    /// on-canvas layer boundary. Like the panel's own selection this only
    /// retargets future edits: no undo step, no dirty flag.
    func setActiveLayer(_ idx: Int) {
        setSelectedLayers(.single(idx))
    }

    /// The set-aware twin: replaces the whole selection and takes the same
    /// bookkeeping.
    ///
    /// The early-out compares the WHOLE selection, not just its primary, and
    /// it is load-bearing for COST as well as correctness:
    /// `activeLayerDidChange()` reloads the layers panel, and the Move
    /// tool's Auto-Select calls this on every canvas click. Comparing only
    /// the primary would let a set-only change through unnoticed; dropping
    /// the guard entirely would rebuild the panel on every click.
    func setSelectedLayers(_ selection: LayerSelection) {
        guard let document = document, let doc = document.doc, doc.layerCount > 0 else { return }
        let clamped = selection.clamped(to: doc.layerCount)
        guard clamped != document.layerSelection else { return }
        document.setLayerSelection(clamped)
        precisionGeometryDocumentDidChange()
        activeLayerDidChange()
    }

    /// The bookkeeping every path that MOVES the active layer owes, in ONE
    /// place so the invariant is not five copies of four lines.
    ///
    /// The Channels panel's "<layer> Mask" row and the canvas's mask base and
    /// rubylith are computed from the active layer, so they follow it — not
    /// just when the move starts in the layers panel. The paths that assign
    /// `activeLayerIndex` AFTER their edit (New Layer, Duplicate Layer, Merge
    /// Down, an adjustment-layer commit, opening a text layer) reach this
    /// through `setActiveLayer`; the document-change notification has already
    /// been posted by then, so nothing else would refresh the panel and it
    /// would keep listing — and washing the canvas with — the PREVIOUS
    /// layer's mask. (Delete Layer and Flatten rely on `applyEdit`'s own
    /// re-clamp, which happens before that post.)
    func activeLayerDidChange() {
        syncPaintTarget()
        layersPanel.reload()
        channelsPanel?.activeLayerChanged()
        infoPanel?.activeLayerChanged()
        pathsActiveLayerDidChange()
        refreshChannelDisplay()
        // The Move bar's Align and Distribute segments dim by how many
        // entries are selected, so the bar has to re-read the selection here
        // too — not only when the tool changes.
        updateOptionsBar()
        updateStatus()
        updateActiveLayerRect()
    }

    /// Reopens layer `idx`'s dialog pre-filled from its meta (the menu item
    /// and the layers panel's thumbnail double-click both land here). OK
    /// replaces only the meta as one undo step; Cancel leaves the document
    /// untouched.
    func editAdjustmentLayer(_ idx: Int) {
        guard let document = document, let doc = document.doc,
              doc.layerIsAdjustment(idx),
              let payload = doc.adjustmentPayload(idx),
              let op = payload.knownOp
        else {
            NSSound.beep()
            return
        }
        // The panel's double-click bypasses menu validation, so an open
        // shape session commits here — its hidden-layer preview and the
        // sheet's live preview would otherwise fight over previewImage.
        guard commitShapeEditSession() else { return }
        // Editing a layer makes it the active one, like re-opening a text
        // layer does.
        setActiveLayer(idx)
        guard let sheet = AdjustmentLayerSheetController.make(
            op: op, document: document, canvas: canvas,
            mode: .edit(layer: idx, original: payload))
        else {
            NSSound.beep()
            return
        }
        presentAsSheet(sheet)
    }

    @objc func toggleLayersPanel(_ sender: Any?) {
        layersPanelVisible.toggle()
        updatePanelVisibility()
    }

    /// View > Assistant (also the panel's Assistant tab).
    @objc func showAssistant(_ sender: Any?) {
        layersPanelVisible = true
        panelTab = 2
        updatePanelVisibility()
    }

    func updatePanelVisibility() {
        layersPanel.view.isHidden = !layersPanelVisible || panelTab != 0
        channelsPanel.view.isHidden = !layersPanelVisible || panelTab != 1
        channelsPanel.setPanelVisible(!channelsPanel.view.isHidden)
        assistantPanel.view.isHidden = !layersPanelVisible || panelTab != 2
        infoPanel.view.isHidden = !layersPanelVisible || panelTab != 3
        infoPanel.setPanelVisible(!infoPanel.view.isHidden)
        navigatorPanel.view.isHidden = !layersPanelVisible || panelTab != 4
        navigatorPanel.setPanelVisible(!navigatorPanel.view.isHidden)
        pathsPanel.view.isHidden = !layersPanelVisible || panelTab != 5
        pathsPanel.setPanelVisible(!pathsPanel.view.isHidden)
        panelSeparator.isHidden = !layersPanelVisible
        scrollTrailingToRoot.isActive = false
        scrollTrailingToPanel.isActive = false
        (layersPanelVisible ? scrollTrailingToPanel : scrollTrailingToRoot).isActive = true
    }

    // MARK: - Filter sheets

    @objc func showAdjustments(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        guard let sheet = AdjustmentSheets.make(
            op: .bcs, document: document, canvas: canvas, mode: .destructive)
        else { return }
        presentAsSheet(sheet)
    }

    @objc func showBlur(_ sender: Any?) {
        if showQuickMaskBlur() { return }
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(SliderSheetController.gaussianBlur(document: document, canvas: canvas))
    }

    @objc func showHueRotate(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(SliderSheetController.hueRotate(document: document, canvas: canvas))
    }

    @objc func showLevels(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(LevelsAdjustmentEditor(document: document, canvas: canvas, mode: .destructive))
    }

    @objc func showThreshold(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(SliderSheetController.threshold(document: document, canvas: canvas))
    }

    @objc func showPosterize(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(SliderSheetController.posterize(document: document, canvas: canvas))
    }

    @objc func showPixelate(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(SliderSheetController.pixelate(document: document, canvas: canvas))
    }

    @objc func showAddNoise(_ sender: Any?) {
        guard !refuseOperationScope(.filter) else { return }
        guard let document = document else {
            NSSound.beep()
            return
        }
        presentAsSheet(SliderSheetController.addNoise(document: document, canvas: canvas))
    }

    // MARK: - One-shot filters (active layer)

    @objc func applyGrayscale(_ sender: Any?) {
        if applyPrecisionParameterless(.grayscale) { return }
        performLayerEdit("Grayscale", record: filterRecord("grayscale")) { $0.grayscaled() }
    }

    @objc func applyInvert(_ sender: Any?) {
        if applyPrecisionParameterless(.invert) || invertLayerMask() { return }
        performLayerEdit("Invert", record: filterRecord("invert")) { $0.inverted() }
    }

    @objc func applySepia(_ sender: Any?) {
        if applyPrecisionParameterless(.sepia) { return }
        performLayerEdit("Sepia", record: filterRecord("sepia")) { $0.sepia() }
    }

    @objc func applySharpen(_ sender: Any?) {
        if applyPrecisionFilter("sharpen", params: ["amount": 1.5]) { return }
        // 1.5 is written out rather than left to the catalog, whose own
        // default is 1.0: a recorded step never relies on a default, because
        // the UI's and the agent's deliberately differ.
        performLayerEdit(
            "Sharpen", record: filterRecord("sharpen", ["amount": 1.5])
        ) { $0.sharpened(amount: 1.5) }
    }

    @objc func applyEdgeDetect(_ sender: Any?) {
        if applyPrecisionFilter("edge_detect") { return }
        performLayerEdit("Edge Detect", record: filterRecord("edge_detect")) {
            $0.edgeDetected()
        }
    }

    @objc func applyEmboss(_ sender: Any?) {
        if applyPrecisionFilter("emboss") { return }
        performLayerEdit("Emboss", record: filterRecord("emboss")) { $0.embossed() }
    }



    // MARK: - Zoom actions

    @objc func zoomInAction(_ sender: Any?) { zoomIn() }
    @objc func zoomOutAction(_ sender: Any?) { zoomOut() }
    @objc func zoomActualAction(_ sender: Any?) { zoomActual() }
    @objc func zoomFitAction(_ sender: Any?) { zoomToFit() }

    // MARK: - Selection and clipboard

    override func selectAll(_ sender: Any?) {
        let selection = CanvasSelection(shape: .rect(CGRect(origin: .zero, size: canvas.bounds.size)),
                                        canvasWidth: Int(canvas.bounds.width), canvasHeight: Int(canvas.bounds.height))
        // `select_all`, not a literal `select_rect` of this canvas's size:
        // the recorded step has to be canvas-RELATIVE or a batched action
        // would select 4000 × 3000 of a 1600 × 1200 file. Closing that gap
        // is why `select_all` exists as a tool at all.
        commitCanvasSelection(selection, actionName: "Select All", record: .selectAll(mode: "replace"))
    }

    @objc func deselect(_ sender: Any?) {
        commitCanvasSelection(nil, actionName: "Deselect", record: .deselect)
    }

    /// Select > Invert Selection: the complement over the full canvas.
    /// A full-canvas selection inverts to deselect; Undo restores its coverage.
    @objc func invertSelection(_ sender: Any?) {
        guard let selection = canvas.selection else {
            NSSound.beep()
            return
        }
        commitCanvasSelection(selection.inverted(), actionName: "Invert Selection",
                              record: .modifySelection(operation: "invert"))
    }

    /// Select > Feather Selection…: radius sheet, then a Gaussian feather
    /// of the selection's coverage mask.
    @objc func featherSelection(_ sender: Any?) {
        if presentNativeSelectionMorph("feather") { return }
        guard canvas.selection != nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(FeatherSheetController(canvas: canvas))
    }

    // Select > Modify > Expand/Contract/Border/Smooth…: Feather's numeric
    // dialog over the core's morphology ops, committed as transient history.

    /// Select ▸ Modify ▸ Expand… — not the colour-based Select ▸ Grow.
    @objc func growSelection(_ sender: Any?) {
        presentSelectionMorphSheet(
            title: "Expand selection", operation: "grow", label: "Radius:",
            hint: "Expands the selection outward by the radius, rounding "
                + "corners into circular arcs.") { $0.grown(by: $1) }
    }

    /// Select ▸ Modify ▸ Contract… — the pixel-radius inverse of Expand.
    @objc func shrinkSelection(_ sender: Any?) {
        presentSelectionMorphSheet(
            title: "Contract selection", operation: "shrink", label: "Radius:",
            hint: "Contracts the selection inward by the radius; a shrink "
                + "past the middle deselects.") { $0.shrunk(by: $1) }
    }

    @objc func borderSelection(_ sender: Any?) {
        presentSelectionMorphSheet(
            title: "Border selection", operation: "border", label: "Width:",
            hint: "Replaces the selection with a band of this width "
                + "straddling its edge.") { $0.bordered(width: $1) }
    }

    @objc func smoothSelection(_ sender: Any?) {
        presentSelectionMorphSheet(
            title: "Smooth selection", operation: "smooth", label: "Radius:",
            hint: "Rounds corners and evens out jagged edges; long straight "
                + "edges stay put.") { $0.smoothed(by: $1) }
    }

    private func presentSelectionMorphSheet(
        title: String, operation: String, label: String, hint: String,
        transform: @escaping (CanvasSelection, Double) -> CanvasSelection?
    ) {
        guard canvas.selection != nil else {
            NSSound.beep()
            return
        }
        if presentNativeSelectionMorph(operation) { return }
        presentAsSheet(
            SelectionMorphSheetController(
                canvas: canvas, title: title, label: label, hint: hint,
                // Border's number is a WIDTH, everything else's a radius —
                // the same split modify_selection makes.
                step: { value in
                    operation == "border"
                        ? .modifySelection(operation: operation, width: value)
                        : .modifySelection(operation: operation, radius: value)
                },
                transform: transform))
    }

    /// Select > Quick Mask Mode (or the bare Q on the canvas): the
    /// selection becomes an editable coverage buffer under a rubylith tint
    /// — brush adds, eraser removes. Each stroke is transient selection history.
    @objc func toggleQuickMask(_ sender: Any?) {
        guard document?.doc != nil else {
            NSSound.beep()
            return
        }
        // A reopened shape commits before the mode takes the canvas — the
        // same click-away rule the canvas applies.
        if shapeEditSession != nil {
            guard commitShapeEditSession() else { return }
        }
        guard !canvas.isPaintingStroke else { NSSound.beep(); return }
        let active = !(document?.quickMaskActive ?? false)
        document?.setQuickMaskActive(active, record: .step("set_quick_mask", ["active": active],
                                                          note: "Select ▸ Quick Mask Mode"))
    }

    /// Edit > Clear (⌫): erases the selected region out of the ACTIVE layer,
    /// in proportion to the selection's coverage — the pixels lose their
    /// color and become transparent, a feathered or anti-aliased edge fading
    /// out across the fringe. Nothing selected means nothing to clear (the
    /// menu item is disabled, so ⌫ stays free for whoever else wants it).
    @objc func clearSelection(_ sender: Any?) {
        if document?.doc?.isHighDepth == true { clearPrecisionSelection(); return }
        guard let document = document, let selection = canvas.selection else {
            NSSound.beep()
            return
        }
        // Validation disables the menu item on an adjustment layer, and with
        // a colour plane or an alpha channel targeted (Clear has no plane
        // route in this build, and must not hit the layer while every
        // indicator names the channel); these backstops cover any path
        // around it.
        guard !refuseAdjustmentPixelEdit(), !refusePlaneTargetEdit(),
              !refuseGroupPixelEdit()
        else { return }
        let idx = document.activeLayerIndex
        guard !refuseLockedEdit(layer: idx, kind: DR_EDIT_PIXELS) else { return }
        let mask = selection.maskBytes()
        // Rewriting pixels invalidates a text layer's description, so this
        // goes through the rasterize prompt (Cancel abandons the edit).
        let before = document.doc
        document.applyRasterizingEdit(
            "Clear", layer: idx, record: .clearSelection(note: "Edit ▸ Clear")
        ) { doc in
            doc.clearingSelection(idx, mask: mask)
        }
        // A frozen alpha makes a clear a byte-exact no-op, which the core
        // reports as nil and applyRasterizingEdit as a bare beep.
        if document.doc === before { refuseFrozenAlpha(layer: idx) }
    }

    /// Edit > Cut (⌘X): Copy then Clear as one step — the ACTIVE LAYER's
    /// pixels within the selection go to the clipboard exactly as Copy takes
    /// them, then the selected region is cleared out of the layer as one
    /// undoable "Cut" edit. The copy lands first, and only a copy that
    /// succeeded lets the clear proceed, so pixels are never deleted without
    /// having been captured; a Cancel on the rasterize prompt then degrades
    /// to a plain Copy.
    @objc func cut(_ sender: Any?) {
        if document?.doc?.isHighDepth == true { cutPrecision(); return }
        guard let document = document, let doc = document.doc,
              let selection = canvas.selection
        else {
            NSSound.beep()
            return
        }
        // Validation disables the menu item on an adjustment layer, and with
        // a plane or channel targeted (Cut copies the LAYER's pixels, which
        // such a target does not name); these backstops cover any path
        // around it.
        // The group guard comes BEFORE the copy: a group's projection would
        // otherwise land on the clipboard and only then would the clear
        // fail, so ⌘X would silently degrade to Copy.
        guard !refuseAdjustmentPixelEdit(), !refusePlaneTargetEdit(),
              !refuseGroupPixelEdit()
        else { return }
        let idx = document.activeLayerIndex
        guard !refuseLockedEdit(layer: idx, kind: DR_EDIT_PIXELS) else { return }
        guard copyToPasteboard(doc.layerCanvasImage(idx)) else { return }
        let mask = selection.maskBytes()
        let before = document.doc
        // Cut's DOCUMENT half is exactly Clear; since 12G-1 the step is the
        // `cut` tool itself, so a replayed Cut copies too.
        document.applyRasterizingEdit(
            "Cut", layer: idx, record: .cut(note: "Edit ▸ Cut")
        ) { doc in
            doc.clearingSelection(idx, mask: mask)
        }
        if document.doc === before { refuseFrozenAlpha(layer: idx) }
    }

    /// Edit > Copy: the ACTIVE LAYER's pixels within the selection — raw
    /// layer pixels, so opacity, blend mode and the layer mask stay out of it
    /// and the copy round-trips through Paste as New Layer unchanged.
    /// Everything the layer does not reach inside the selection comes out
    /// transparent.
    @objc func copy(_ sender: Any?) {
        // A Copy that wrote the pasteboard is a recorded step.
        func recorded(_ copied: Bool) {
            if copied { ActionRecorder.shared.record(.copy(note: "Edit ▸ Copy"), on: document) }
        }
        if document?.doc != nil, document?.canvasSelection == nil {
            recorded(copyLayers())
            return
        }
        if document?.doc?.isHighDepth == true { recorded(copyPrecision()); return }
        guard let doc = document?.doc else {
            NSSound.beep()
            return
        }
        let idx = document?.activeLayerIndex ?? 0
        recorded(copyToPasteboard(doc.byteCopySource(layer: idx, whole: canvas.selection == nil),
                                  layer: idx))
    }

    /// Edit > Copy Merged: the same region of the flattened composite —
    /// every visible layer, with opacity, blend modes, masks, clipping and
    /// adjustment layers all applied, as one image.
    @objc func copyMerged(_ sender: Any?) {
        let copied: Bool
        if document?.doc?.isHighDepth == true {
            copied = copyMergedPrecision()
        } else {
            guard let source = mergedCopySource() else { return }
            copied = copyToPasteboard(source)
        }
        guard copied else { return }
        ActionRecorder.shared.record(.copyMerged(note: "Edit ▸ Copy Merged"), on: document)
    }

    /// Shared tail of Cut and both copies: multiply the canvas-sized
    /// source's alpha by the selection's coverage — only the SELECTED pixels
    /// come along, so anything inside the bounding box but outside the shape
    /// goes transparent, and a feathered or anti-aliased edge fades out
    /// across the fringe, exactly as Clear removes it — then crop to the
    /// selection's BOUNDS (the whole canvas without a selection) and write
    /// TIFF + PNG. True when the clipboard was written — Cut clears only on
    /// success.
    @discardableResult
    private func copyToPasteboard(_ source: RasterImage?, layer: Int? = nil) -> Bool {
        var image = source
        if let selection = canvas.selection {
            let bounds = selection.bounds
            image = image?.masked(by: selection.maskBytes())
            image = image?.cropped(
                x: Int(bounds.minX), y: Int(bounds.minY),
                w: Int(bounds.width), h: Int(bounds.height))
        }
        // TAGGED with the document's profile and NOT converted: every
        // pasteboard consumer on this platform is colour-managed, and
        // converting would gamut-clip a P3 copy on its way to a P3 app.
        guard let image,
              let cgImage = image.makeCGImage(in: document?.colorSpace ?? ColorProfile.sRGB)
        else {
            NSSound.beep()
            return false
        }
        let origin = canvas.selection.map { (x: Int($0.bounds.minX), y: Int($0.bounds.minY)) }
            ?? (x: 0, y: 0)
        let typed = typedBytes(forByteCopy: image, offset: origin,
                               layer: canvas.selection == nil ? layer : nil)
        do { try Clipboard.writeItem(cgImage, typed: typed, source: clipboardSource) } catch {
            NSSound.beep()
            return false
        }
        return true
    }
}

// MARK: - Undo plumbing

extension EditorViewController {
    // Intercept the nil-target undo:/redo: menu actions ahead of NSWindow so
    // the text-session and preview-sheet guards in validateUserInterfaceItem
    // apply to them; otherwise Cmd-Z reaches the document's undo manager
    // directly and mutates the image underneath an open session or sheet.
    @objc func undo(_ sender: Any?) {
        if undoKeyCancelsJob() { return }
        activeUndoManager?.undo()
    }
    @objc func redo(_ sender: Any?) {
        if undoKeyCancelsJob() { return }
        activeUndoManager?.redo()
    }

    /// Mirrors NSWindow's resolution: a focused field editor (options-bar and
    /// sheet text fields) keeps its own typing undo; everything else gets the
    /// document's manager.
    private var activeUndoManager: UndoManager? {
        view.window?.firstResponder?.undoManager ?? document?.undoManager
    }

    /// Called by ImageDocument on save/close/export so an in-progress canvas
    /// session — text entry, a reopened shape, or a Free Transform — is
    /// never silently dropped from the written file. False when a refused
    /// commit left a session open; the caller stops.
    func commitPendingSessions() -> Bool {
        guard canvas.commitTextSession(), commitShapeEditSession() else { return false }
        return commitPendingTransform()
    }
}

// MARK: - Validation

extension EditorViewController: NSUserInterfaceValidations {
    private static let toolActions: [Selector: EditorTool] = [
        #selector(selectSelectTool(_:)): .select,
        #selector(selectEllipseTool(_:)): .ellipseSelect,
        #selector(selectLassoTool(_:)): .lasso,
        #selector(selectFreehandLassoTool(_:)): .freehandLasso,
        #selector(selectQuickSelectTool(_:)): .quickSelect,
        #selector(selectObjectSelectTool(_:)): .objectSelect,
        #selector(selectMagneticLassoTool(_:)): .magneticLasso,
        #selector(selectWandTool(_:)): .wand,
        #selector(selectSubjectTool(_:)): .subject,
        #selector(selectMoveTool(_:)): .move,
        #selector(selectBrushTool(_:)): .brush,
        #selector(selectEraserTool(_:)): .eraser,
        #selector(selectFillTool(_:)): .fill,
        #selector(selectGradientTool(_:)): .gradient,
        #selector(selectTextTool(_:)): .text,
        #selector(selectEyedropperTool(_:)): .eyedropper,
        #selector(selectColorSamplerTool(_:)): .colorSampler,
        #selector(selectRulerTool(_:)): .ruler,
        #selector(selectCropTool(_:)): .crop,
        #selector(selectPerspectiveCropTool(_:)): .perspectiveCrop,
        #selector(selectCloneTool(_:)): .clone,
        #selector(selectDodgeTool(_:)): .dodge,
        #selector(selectBlurBrushTool(_:)): .blurBrush,
        #selector(selectSharpenBrushTool(_:)): .sharpenBrush,
        #selector(selectSmudgeTool(_:)): .smudge,
        #selector(selectSpongeTool(_:)): .sponge,
        #selector(selectHistoryBrushTool(_:)): .historyBrush,
        #selector(selectArtHistoryBrushTool(_:)): .artHistoryBrush,
        #selector(selectPencilTool(_:)): .pencil,
        #selector(selectHealTool(_:)): .heal,
        #selector(selectSpotHealTool(_:)): .spotHeal,
        #selector(selectPatchTool(_:)): .patch,
        #selector(selectRedEyeTool(_:)): .redEye,
        #selector(selectRemoveTool(_:)): .remove,
        #selector(selectContentAwareMoveTool(_:)): .contentAwareMove,
        #selector(selectShapeRectTool(_:)): .shapeRect,
        #selector(selectShapeEllipseTool(_:)): .shapeEllipse,
        #selector(selectShapeLineTool(_:)): .shapeLine,
        #selector(selectPenTool(_:)): .pen,
        #selector(selectZoomTool(_:)): .zoom,
        #selector(selectHandTool(_:)): .hand,
    ]

    /// True while a text-editing responder owns the keyboard: a field
    /// editor (every NSTextField in the window edits through one) or the
    /// canvas text session's own text view — both are NSText subclasses.
    private var isEditingText: Bool {
        view.window?.firstResponder is NSText
    }

    private static let zoomActions: Set<Selector> = [
        #selector(zoomInAction(_:)),
        #selector(zoomOutAction(_:)),
        #selector(zoomActualAction(_:)),
        #selector(zoomFitAction(_:)),
    ]

    func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        if let document, let selector = item.action,
           let allowed = RetainedImagePolicy.validate(selector, document: document) { return allowed }
        guard document?.doc != nil else { return false }
        guard document?.isPublishingEditingState != true else { return false }
        // While a preview sheet is up, its captured base and the document must
        // not diverge: block every edit action delivered via key equivalents.
        guard view.window?.attachedSheet == nil else { return false }

        if let action = item.action, let tool = Self.toolActions[action] {
            if let menuItem = item as? NSMenuItem {
                menuItem.state = currentTool == tool ? .on : .off
            }
            // A planned tool stays visible with its "Soon" affordance but
            // never enabled — in menus, the rail's dropdowns, anywhere.
            return !tool.planned && DocumentPrecision.allows(tool, document: document)
        }

        if let selector = item.action, let allowed = validateRawPresets(selector) { return allowed }
        if let allowed = validatePrecisionItem(item) { return allowed }
        if let selector = item.action, let allowed = validateLinkedImageItem(selector) { return allowed }

        // Rulers, guides, the grid and the snap toggles answer through ONE
        // early-out, and it sits ABOVE the session guard below rather than
        // beside validateStructureItem: a view-only toggle changes no pixels
        // and is perfectly safe inside a text, shape-edit or transform
        // session, while the two items that EDIT the document (New Guide…,
        // Clear Guides) return false there themselves
        // (EditorViewController+Rulers.swift). nil means "not one of mine".
        if let handled = validateViewChromeItem(item) { return handled }
        if let handled = validateColorSamplerItem(item) { return handled }

        // View ▸ Rotate View… / Reset View Rotation and Window ▸ New View /
        // Sync Pan & Zoom. View-only like the chrome above
        // (EditorViewController+Views.swift).
        if let allowed = validateViewsItem(item) { return allowed }
        if let allowed = validateViewExposureItem(item) { return allowed }
        if let allowed = validateHDRPreviewItem(item) { return allowed }
        if let allowed = validateScreenChromeItem(item) { return allowed }

        // View ▸ Proof Setup… / Proof Colors / Gamut Warning. View-only like
        // the chrome above, so they answer here rather than inside the
        // session guard below (EditorViewController+SoftProof.swift).
        switch item.action {
        case #selector(showProofSetup(_:)), #selector(toggleProofColors(_:)),
             #selector(toggleGamutWarning(_:)), #selector(applyProofPresetFromMenu(_:)),
             #selector(deleteProofPresetFile(_:)), #selector(applyWorkingProof(_:)):
            return validateProofItem(item)
        default:
            break
        }

        // Filters ▸ Repeat Last Filter — its own early-out, because it flips
        // its own title and carries the ⌃F text-editing guard
        // (EditorViewController+Actions.swift).
        if let handled = validateActionsItem(item) { return handled }

        // While a text session, a shape-edit session or a Free Transform is
        // active, only tool switching (handled above — it commits the
        // session) and zooming are safe; edit/filter/clipboard actions must
        // not mutate the image underneath the session, and Free Transform
        // must not re-enter.
        if canvas.hasActiveTextSession || isTransforming || shapeEditSession != nil
            || canvas.hasMagneticLassoSession || canvas.puppetWarpSession != nil
            || canvas.perspectiveWarpSession != nil {
            if let action = item.action, Self.zoomActions.contains(action) {
                return true
            }
            return false
        }

        // Edit ▸ Fill… and its ⌫ chords (EditorViewController+FillStroke.swift).
        if let handled = validateFillItem(item) { return handled }
        // New Fill Layer (+FillLayers.swift) and Blur in Quick Mask (+QuickMaskEdits.swift).
        if let handled = validateFillLayerItem(item) ?? validateQuickMaskEditItem(item)
            ?? validatePathsItem(item) { return handled }

        // Every item this phase added — group, ungroup, the locks, align,
        // distribute, link, arrange, via copy/cut, merge/stamp visible —
        // answers through ONE early-out rather than two dozen cases in a
        // switch that already carries thirty-two (EditorViewController
        // +Locks.swift). nil means "not one of mine", and the switch below
        // decides as before.
        if let handled = validateStructureItem(item) { return handled }

        // Layer ▸ Label (EditorViewController+LayerLabels.swift): one line
        // here, the radio mark and the answer there.
        if item.action == #selector(setLayerLabel(_:)) {
            return validateLayerLabelItem(item)
        }

        if let handled = validateJobKeyItem(item) { return handled }

        switch item.action {
        // The export workspace and Quick Export (EditorViewController
        // +Export.swift) read the document and write a file; they need
        // nothing but an image.
        case #selector(showExportWorkspace(_:)):
            return document?.doc != nil
        case #selector(quickExport(_:)):
            return document?.doc != nil
        case #selector(exportLayers(_:)):
            return document?.doc != nil
        case #selector(showContactSheet(_:)):
            return document?.doc != nil
        case #selector(exportAsPDF(_:)):
            return document?.doc != nil
        case #selector(duplicateToNewDocument(_:)):
            return document?.doc != nil

        // Image ▸ Evaluation… (EditorViewController+Evaluation.swift).
        case #selector(showEvaluation(_:)):
            return validateEvaluationItem()
        case #selector(showFileInfo(_:)): return validateFileInfoItem()

        // Image ▸ Tone Map… (EditorViewController+FloatDocument.swift).
        case #selector(showToneMap(_:)):
            return validateToneMapItem()
        case #selector(showPersistedWarp(_:)):
            return validatePersistedWarpItem()

        case #selector(editArtworkColors(_:)):
            guard let doc = document?.doc else { return false }
            return doc.artworkIndices.contains(document?.activeLayerIndex ?? -1)
        case #selector(replaceMissingFonts(_:)): return validateReplaceFontsItem()
        case #selector(rasterizeArtwork(_:)):
            guard let doc = document?.doc else { return false }
            let active = document?.activeLayerIndex ?? -1
            return doc.artworkIndices.contains(active) || doc.isGeneratedLayer(active)

        case #selector(freeTransformCopy(_:)): return validateFreeTransformCopyItem()
        case #selector(transformAgain(_:)), #selector(transformAgainWithCopy(_:)):
            return validateTransformAgainItem()
        case #selector(freeTransform(_:)):
            if document?.canvasSelection != nil { return true }
            guard supportsOperationScope(.layerGeometry) else { return false }
            // Needs a layer with pixels to transform.
            let active = document?.activeLayerIndex ?? 0
            guard let info = document?.doc?.layerInfo(active) else { return false }
            return info.width > 0 && info.height > 0
        case #selector(reselect(_:)):
            return !canvas.quickMaskActive && (document?.canReselect ?? false)
        case #selector(toggleQuickMask(_:)):
            // The one Select-menu item that stays live inside the mode (it
            // is the way out); checked while active.
            if let menuItem = item as? NSMenuItem {
                menuItem.state = canvas.quickMaskActive ? .on : .off
            }
            return true
        case #selector(selectAll(_:)):
            // Every other Select-menu item is disabled inside Quick Mask
            // mode: the selection lives in the mode's buffer until exit.
            return !canvas.quickMaskActive
        case #selector(selectSubject(_:)):
            // Segmentation reads the composite, so an image is all it needs
            // — unlike the modify-the-selection items, it makes a selection
            // from nothing.
            return !canvas.quickMaskActive && document?.doc != nil
        case #selector(deselect(_:)),
            #selector(invertSelection(_:)), #selector(featherSelection(_:)),
            #selector(growSelection(_:)), #selector(shrinkSelection(_:)),
            #selector(borderSelection(_:)), #selector(smoothSelection(_:)),
            #selector(selectGrow(_:)), #selector(selectSimilar(_:)):
            return !canvas.quickMaskActive && canvas.selectionRect != nil
        case #selector(cropToSelection(_:)):
            // Crop needs a selection that would actually shrink the canvas.
            // Selection BOUNDS are what it crops to, so a selection already
            // spanning the image — Select All, or a lasso whose extent
            // covers it — leaves nothing to do; the core refuses that crop
            // as a no-op, so the item would be dead anyway.
            guard !canvas.quickMaskActive, let rect = canvas.selectionRect,
                  let doc = document?.doc
            else { return false }
            return rect.integral
                != CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        case #selector(clearSelection(_:)):
            // Clear's key equivalent is a BARE ⌫, resolved ahead of the first
            // responder, so it stands down wherever ⌫ belongs to someone else
            // (`deleteChordAllowed`, EditorViewController+FillStroke.swift).
            // It needs something to clear: a selection and an active layer
            // with pixels. Nor does it take a colour plane or an alpha
            // channel: Clear rewrites the LAYER's pixels and has no plane
            // route (Edit ▸ Fill… is how a plane or channel is filled).
            guard deleteChordAllowed, canvas.selection != nil,
                  !activeLayerIsAdjustment,
                  !activeLayerIsGroup, !paintTarget.isCoverage
            else { return false }
            return document?.doc?.layerInfo(document?.activeLayerIndex ?? 0) != nil
        case #selector(cut(_:)):
            // Cut is Copy + Clear in one step, so it needs what Clear needs:
            // a selection and an active layer with pixels, and no plane or
            // channel target — its copy half takes the LAYER's pixels, so on
            // such a target it would cut one thing and copy another. No
            // text-editing guard — ⌘X reaches a field editor first, which
            // claims cut: itself, exactly as ⌘C does for copy.
            guard canvas.selection != nil, !activeLayerIsAdjustment, !activeLayerIsGroup,
                  !paintTarget.isCoverage
            else { return false }
            return document?.doc?.layerInfo(document?.activeLayerIndex ?? 0) != nil
        case #selector(showAdjustments(_:)), #selector(showBlur(_:)),
            #selector(showHueRotate(_:)), #selector(showLevels(_:)), #selector(showCurves(_:)),
            #selector(showThreshold(_:)), #selector(showPosterize(_:)),
            #selector(showPixelate(_:)), #selector(showAddNoise(_:)),
            #selector(applyGrayscale(_:)), #selector(applyInvert(_:)), #selector(equalize(_:)),
            #selector(applyDesaturate(_:)),
            #selector(applySepia(_:)), #selector(applySharpen(_:)),
            #selector(showUnsharpMask(_:)), #selector(showHighPass(_:)),
            #selector(showReduceNoise(_:)), #selector(showDustAndScratches(_:)),
            #selector(showBoxBlur(_:)), #selector(showSurfaceBlur(_:)), #selector(showAddGrain(_:)),
            #selector(showRadialBlur(_:)), #selector(showOffset(_:)),
            #selector(showMotionBlur(_:)), #selector(showLensBlur(_:)), #selector(showTiltShift(_:)),
            #selector(showLensCorrection(_:)), #selector(showPresence(_:)),
            #selector(showDevelopFilter(_:)), #selector(showSmartSharpen(_:)),
            #selector(showDepthBlur(_:)), #selector(showSuperZoom(_:)), #selector(showJpegArtifactsRemoval(_:)), #selector(showColorize(_:)),
            #selector(applyClouds(_:)), #selector(applyDifferenceClouds(_:)),
            #selector(applyEdgeDetect(_:)), #selector(applyEmboss(_:)),
            #selector(showAdjustmentSheet(_:)), #selector(autoTone(_:)),
            #selector(autoContrast(_:)), #selector(autoColor(_:)):
            // Destructive filters rewrite the active layer's PIXELS, which
            // an adjustment layer doesn't meaningfully have; its parameters
            // re-open through Adjustment Options… instead. With a CHANNEL
            // targeted they rewrite that channel instead of any layer, so
            // the active layer's kind is irrelevant (§0.4).
            return supportsOperationScope(.filter)
        case #selector(showLiquify(_:)): return supportsOperationScope(.pixelOnly)
        case #selector(showPuppetWarp(_:)): return supportsOperationScope(.pixelOnly)
        case #selector(showPerspectiveWarp(_:)): return supportsOperationScope(.pixelOnly)
        case #selector(showSkyReplacement(_:)): return validateSkyReplacementItem()
        case #selector(showGenerativeFill(_:)): return validateGenerativeFillItem()
        case #selector(showGenerativeExpand(_:)): return validateGenerativeExpandItem()
        case #selector(showGenerativeVariations(_:)): return validateGenerativeVariationsItem()
        case #selector(contentAwareFill(_:)):
            // Fills the SELECTION on the active layer's own pixels, so it
            // needs one — and, like Clear, must not stand on an adjustment
            // layer, a colour plane or a channel.
            return !canvas.quickMaskActive && canvas.selection != nil
                && !activeLayerIsAdjustment && !paintTarget.isCoverage
        case #selector(removeRedEye(_:)):
            // Vision's automatic pass rewrites the active layer's pixels.
            return !activeLayerIsAdjustment && !paintTarget.isCoverage
        case #selector(layerStyle(_:)), #selector(layerStyleEffect(_:)):
            return canEditLayerStyle
        case #selector(placeEmbeddedImage(_:)):
            return document?.doc != nil
        case #selector(rasterizeEmbeddedImage(_:)):
            guard let document else { return false }
            return document.doc?.hasRetainedSource(document.activeLayerIndex) == true
        case #selector(pasteLayerStyle(_:)):
            return canPasteLayerStyle
        case #selector(copyLayerStyle(_:)), #selector(clearLayerStyle(_:)):
            return activeLayerHasStyle
        case #selector(adjustmentOptions(_:)):
            guard let document = document, let doc = document.doc else { return false }
            let idx = document.activeLayerIndex
            guard doc.layerIsAdjustment(idx),
                  let op = doc.adjustmentPayload(idx)?.knownOp
            else { return false }
            return AdjustmentLayerSheetController.opHasDialog(op)
        case #selector(deleteLayer(_:)):
            // What a removal TAKES is not the number of selected rows — a
            // group takes its whole subtree (+Groups.swift).
            return canDeleteLayer
        case #selector(mergeDown(_:)):
            // Retitled for a multi-selection (Merge Layers), and
            // "the layer below" is the previous SIBLING — merging inside a
            // group never reaches out of it. The core refuses to merge into
            // a hidden layer; mirror that here (and match the panel's merge
            // button).
            let multiple = (document?.layerSelection.isMultiple ?? false)
            let group = !multiple && document.map { $0.doc?.layerIsGroup($0.activeLayerIndex) == true } == true
            if let menuItem = item as? NSMenuItem {
                menuItem.title = multiple ? "Merge Layers" : group ? "Merge Group" : "Merge Down"
            }
            if multiple { return canMergeSelectedLayers }
            if group { return canMergeGroup }
            guard let below = clippingBaseBelowActiveLayer else { return false }
            return document?.doc?.layerInfo(below)?.visible == true
        case #selector(flattenImage(_:)):
            return (document?.doc?.layerCount ?? 1) > 1
        case #selector(stampSelected(_:)):
            return canStampSelected
        case #selector(addLayerMaskRevealAll(_:)), #selector(addLayerMaskHideAll(_:)):
            return !activeLayerHasMask
        case #selector(addLayerMaskFromSelection(_:)):
            return !activeLayerHasMask && canvas.selection != nil
        case #selector(deleteLayerMask(_:)):
            return activeLayerHasMask
        case #selector(applyLayerMask(_:)):
            // Applying takes the whole baked mask, a vector mask's shape too.
            return document?.doc?.layerHasMask(document?.activeLayerIndex ?? -1) == true
        case #selector(toggleLayerMaskEnabled(_:)):
            // Checkmark state, fixed title: a mask is enabled or it isn't.
            if let menuItem = item as? NSMenuItem {
                menuItem.state = activeLayerMaskEnabled ? .on : .off
            }
            return activeLayerHasMask
        case #selector(toggleClippingMask(_:)):
            // One item, retitled. The bottom layer has
            // no layer below to clip to, so it stays disabled there; the
            // no-document case is the guard at the top.
            if let menuItem = item as? NSMenuItem {
                menuItem.title =
                    activeLayerClipped ? "Release Clipping Mask" : "Create Clipping Mask"
            }
            return clippingBaseBelowActiveLayer != nil
        case #selector(copy(_:)):
            // Copy takes the ACTIVE LAYER's pixels, and an adjustment layer
            // has none worth copying — its effect lives in the composite, so
            // Copy Merged is the one that captures it and stays enabled.
            return !activeLayerIsAdjustment || canvas.selection == nil
        case #selector(pasteAsNewLayer(_:)), #selector(paste(_:)):
            return validatePaste()
        case #selector(pasteInPlace(_:)):
            return validatePasteInPlace()
        case #selector(pasteInto(_:)), #selector(pasteOutside(_:)):
            return !isEditingText && validatePasteInto()
        case #selector(showInfo(_:)), #selector(showSnapshots(_:)), #selector(showHistory(_:)),
             #selector(showLayerComps(_:)):
            // Like the other panel tabs: a document is all it needs, and
            // the guard at the top has already established one.
            return true
        case #selector(showMaskProperties(_:)), #selector(transformMask(_:)), #selector(toggleMaskContentLink(_:)):
            return validateIndependentMaskItem(item)
        case #selector(showColorRange(_:)), #selector(showSubjectTargets(_:)), #selector(selectPeople(_:)),
             #selector(showPeopleTargets(_:)), #selector(showFaceParts(_:)), #selector(selectSky(_:)),
             #selector(showPersonParts(_:)):
            return !isTransforming && document?.quickMaskActive == false
        case #selector(transformSelection(_:)), #selector(refineSelection(_:)):
            return !isTransforming && document?.quickMaskActive == false && document?.canvasSelection != nil
        case #selector(assignProfile(_:)), #selector(convertToProfile(_:)):
            return validateColorItem(item)
        case #selector(showChannels(_:)), #selector(newChannel(_:)),
             #selector(duplicateChannel(_:)), #selector(deleteChannel(_:)),
             #selector(channelOptions(_:)), #selector(invertChannel(_:)), #selector(modifyNativePlane(_:)),
             #selector(loadChannelAsSelection(_:)), #selector(saveSelectionSheet(_:)),
             #selector(loadSelectionSheet(_:)), #selector(addLuminosityMasks(_:)),
             #selector(applyImageSheet(_:)), #selector(calculationsSheet(_:)),
             #selector(newSpotChannel(_:)), #selector(spotChannelOptions(_:)), #selector(mergeSpotChannel(_:)):
            return validateChannelItem(item)
        case #selector(loadPortraitMatte(_:)):
            return validatePortraitMatteItem(item)
        case #selector(undo(_:)):
            if let menuItem = item as? NSMenuItem, let manager = activeUndoManager {
                menuItem.title = manager.undoMenuItemTitle
            }
            return activeUndoManager?.canUndo ?? false
        case #selector(redo(_:)):
            if let menuItem = item as? NSMenuItem, let manager = activeUndoManager {
                menuItem.title = manager.redoMenuItemTitle
            }
            return activeUndoManager?.canRedo ?? false
        default:
            return true
        }
    }
}
