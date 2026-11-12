import AppKit

// `EditorViewState` / `get_view` / `set_view` are VIEW state on `set_proof_setup`'s precedent
// (`nonRecordable`; the reply is the READ-BACK state, because the clip view constrains its own
// origin).

/// The Navigator tab's editor half (Phase 12C-2): the window's VIEW state as
/// a value, the two ways it is changed, and the panel's wiring.
///
/// View state is per DOCUMENT WINDOW and is deliberately not part of a saved
/// workspace layout: a layout describes the furniture (the window frame, the
/// panels, the chrome toggles, the tool), while where you are looking inside
/// one picture belongs to that picture. `get_view` / `set_view` are the MCP
/// twins (`AgentServer+Workspace.swift`), `nonRecordable` on the proof-setup
/// precedent — no undo step, no dirty flag, nothing a `save_copy` carries.
struct EditorViewState: Equatable {
    /// The scroll view's magnification: 1 is 100 %.
    let zoom: Double
    /// The centre of the visible rectangle, in canvas pixels.
    let center: CGPoint
    /// What the well is showing, clipped to the canvas, in canvas pixels.
    /// While the view is ROTATED this is the axis-aligned bounding box of the
    /// turned quadrilateral — recorded, not an oversight: the Navigator's
    /// proxy is a rectangle and a document-space answer has to be one too.
    let visible: CGRect
    let canvas: CGSize
    /// View-only rotation in degrees, (−180, 180], positive clockwise
    /// (Phase 12D-2, `ViewRotation.swift`). Per WINDOW.
    let rotation: Double
    /// Which view (window) of the document this is: 1 is the one it opened
    /// with. Per WINDOW, obviously.
    let view: Int
    /// The document's monotonic revision, so two views can be compared.
    /// Per DOCUMENT.
    let revision: UInt64
    /// Window ▸ Sync Pan & Zoom. Per DOCUMENT, not per view: it is the switch
    /// that says "these windows move together".
    let syncPanZoom: Bool
    /// The view exposure in stops (`ViewExposure.swift`): what a 32-bit
    /// float window multiplies the light by before its tone map. 0 at every
    /// other depth. Per WINDOW.
    let exposure: Double
    /// True when the canvas is showing the image rendered for `exposure`.
    /// That render is asynchronous, so it is false just after a change — and
    /// at 0 stops, where there is nothing to render. Read-only.
    let exposureDisplayed: Bool
    /// View ▸ HDR Preview (v1.x, `EditorViewController+HDRPreview.swift`),
    /// per WINDOW, and whether the extended-range image is on screen now.
    let hdrPreview: Bool
    let hdrPreviewDisplayed: Bool
    /// The window's screen's potential EDR headroom (or `DR_EDR_HEADROOM`).
    let edrHeadroom: Double
    /// True when the canvas's backing layer is EDR-configured.
    let edrLayer: Bool
    /// The largest RGB component of the landed extended-range image.
    let edrContentPeak: Float?
    /// The image the canvas draws (`canvasDrawnImage`: a channel display's base,
    /// else the view slot, else the projection).
    /// `json` never hashes it; only MCP `get_view` with `image_hash` true does
    /// (`sha256(of:)`), because the hash reads the whole image.
    let viewImage: CGImage?

    var json: [String: Any] {
        [
            "zoom": zoom,
            "center": ["x": center.x, "y": center.y],
            "visible": ["x": visible.minX, "y": visible.minY,
                        "width": visible.width, "height": visible.height],
            "canvas": ["width": canvas.width, "height": canvas.height],
            "rotation": rotation,
            "view": view,
            "revision": revision,
            "sync_pan_zoom": syncPanZoom,
            "exposure": exposure,
            "exposure_displayed": exposureDisplayed,
            "hdr_preview": hdrPreview,
            "hdr_preview_displayed": hdrPreviewDisplayed,
            "edr_headroom": edrHeadroom,
            "edr_layer": edrLayer,
            "edr_content_peak": edrContentPeak.map { Double($0) } ?? NSNull(),
        ]
    }
}

extension EditorViewState {
    /// SHA-256 of `image`'s bytes (MCP `view_image_sha256`); nil without one.
    static func sha256(of image: CGImage?) -> String? {
        guard let image, let bytes = image.dataProvider?.data as Data? else { return nil }
        return HostSourceError.sha256(bytes)
    }
}

extension EditorViewController {
    /// The two limits the scroll view itself is configured with, restated
    /// once so the refusal and the slider cannot drift from it.
    static let viewZoomRange: ClosedRange<Double> = 0.02...32

    // MARK: - Reading

    var viewState: EditorViewState {
        let canvasSize = document?.doc?.canvasSize ?? .zero
        // The clip view's bounds are the HOST's coordinates since 12D-2; at
        // 0° the host IS the canvas's old frame and this convert is exactly
        // the identity, so every number below is what it has always been.
        let bounds = canvas.convert(scrollView.contentView.bounds, from: canvasHost)
        let clipped = bounds.intersection(CGRect(origin: .zero, size: canvasSize))
        let visible = clipped.isNull ? CGRect(origin: .zero, size: .zero) : clipped
        return EditorViewState(
            zoom: Double(scrollView.magnification),
            center: CGPoint(x: visible.midX, y: visible.midY),
            visible: visible, canvas: canvasSize,
            rotation: viewRotation, view: viewOrdinal,
            revision: document?.revision ?? 0,
            syncPanZoom: document?.syncPanZoom ?? false,
            exposure: viewExposureStops,
            exposureDisplayed: viewExposureDisplayed,
            hdrPreview: viewExposure.hdrPreview, hdrPreviewDisplayed: hdrPreviewDisplayed,
            edrHeadroom: edrHeadroom, edrLayer: canvas.extendedRangeLayerConfigured,
            edrContentPeak: extendedContentPeak, viewImage: canvasDrawnImage)
    }

    /// The image `ImageCanvasView.draw` draws as the picture: a channel display's base when it
    /// replaces the composite outside a crop (a plane, a channel, the mask, a spot overprint; nil
    /// for every eye off), else the view slot, else the projection.
    var canvasDrawnImage: CGImage? {
        if canvas.cropOverlay == nil, let display = canvas.channelDisplay, display.replacesComposite {
            return display.base
        }
        return canvas.proofImage ?? canvas.image
    }

    // MARK: - Writing

    /// `set_view`'s one path, and the Navigator's own. Both numbers are
    /// refused BY NAME rather than clamped: a caller that asked for 100× wants
    /// to know the request was impossible, not to be silently given 32×.
    ///
    /// The reply is the READ-BACK state, not the request: the clip view
    /// constrains its own origin (`CenteringClipView`), so the honest answer
    /// to "where am I now" is what the scroll view says afterwards.
    @discardableResult
    func applyViewState(
        zoom: Double?, center: CGPoint?, rotation: Double? = nil, fit: Bool = false,
        exposure: Double? = nil, hdrPreview: Bool? = nil
    ) throws -> EditorViewState {
        let canvasSize = document?.doc?.canvasSize ?? .zero
        // The view exposure is checked with the others, before any of
        // them is applied, and by its own writer's rules: outside ±20 stops,
        // or non-zero off a 32-bit float document, is refused by name.
        if let exposure, let refusal = viewExposureRefusal(exposure) {
            throw DoctoredCoreError(message: refusal)
        }
        if let rotation {
            guard rotation.isFinite else {
                throw DoctoredCoreError(
                    message: "rotation must be a finite number of degrees; got \(rotation).")
            }
        }
        if fit, zoom != nil {
            throw DoctoredCoreError(
                message: "fit and zoom cannot both be set — fit chooses the zoom itself.")
        }
        if let zoom {
            guard zoom.isFinite, Self.viewZoomRange.contains(zoom) else {
                throw DoctoredCoreError(
                    message: "zoom must be between \(Self.viewZoomRange.lowerBound) and "
                        + "\(Self.viewZoomRange.upperBound) (1 is 100 %); got \(zoom).")
            }
        }
        if let center {
            guard center.x.isFinite, center.y.isFinite,
                  (0...canvasSize.width).contains(center.x),
                  (0...canvasSize.height).contains(center.y) else {
                throw DoctoredCoreError(
                    message: "center must be inside the canvas — 0 to \(Int(canvasSize.width)) "
                        + "horizontally and 0 to \(Int(canvasSize.height)) vertically.")
            }
        }
        // Rotation FIRST: it changes the host's size, which is what a fit
        // measures and what a centre is scrolled inside.
        if let rotation { setViewRotation(rotation) }
        if fit { zoomToFit() }
        if let zoom { zoomPercent = zoom * 100 }
        if let center { scrollCanvasCenter(to: center) }
        if let exposure { try setViewExposure(exposure) }
        if let hdrPreview { try setHDRPreview(hdrPreview) }
        refreshNavigator()
        // Sync Pan & Zoom: an agent's set_view moves the document's other
        // views exactly as a scroll or a pinch does. Re-entrancy is guarded
        // on the document (EditorViewController+Views.swift).
        propagateViewSync()
        return viewState
    }

    /// Scrolls so `point` (canvas pixels) is the middle of the well. The clip
    /// view's bounds are already in the canvas's own coordinates, so the
    /// arithmetic is one subtraction; the constraint on how far it may go is
    /// the clip view's, not this method's.
    func scrollCanvasCenter(to point: CGPoint) {
        let clip = scrollView.contentView
        let size = clip.bounds.size
        // Canvas -> host: the clip view scrolls the HOST, and at 0° that
        // conversion is the identity (ViewRotation.swift).
        let inHost = canvasHost.convert(point, from: canvas)
        clip.scroll(to: CGPoint(x: inHost.x - size.width / 2, y: inHost.y - size.height / 2))
        scrollView.reflectScrolledClipView(clip)
    }

    // MARK: - The panel

    /// Built in `loadView` beside the other four panels.
    func makeNavigatorPanel() -> NavigatorPanelViewController {
        let panel = NavigatorPanelViewController()
        panel.onShowLayers = { [weak self] in
            self?.panelTab = 0
            self?.updatePanelVisibility()
        }
        panel.onShowChannels = { [weak self] in self?.showChannelsTab() }
        panel.onShowAssistant = { [weak self] in
            self?.panelTab = 2
            self?.updatePanelVisibility()
        }
        panel.onShowInfo = { [weak self] in self?.showInfoTab() }
        panel.onShowPaths = { [weak self] in self?.showPathsTab() }
        panel.onCenter = { [weak self] point in self?.navigatorDidCenter(point) }
        panel.onZoom = { [weak self] zoom in self?.navigatorDidZoom(zoom) }
        panel.onZoomFit = { [weak self] in self?.navigatorDidFit() }
        addChild(panel)
        panel.view.translatesAutoresizingMaskIntoConstraints = false
        panel.view.isHidden = true
        return panel
    }

    /// Pushes the current picture, canvas size, visible rectangle and zoom at
    /// the panel. Called from the scroll/magnify notification, from the
    /// document-change notification and whenever the tab becomes visible.
    func refreshNavigator(isLive: Bool = false) {
        guard let panel = navigatorPanel else { return }
        let state = viewState
        // `state.visible` rather than the raw clip bounds: it is the well
        // mapped back into CANVAS pixels, which is what the proxy is drawn
        // in. The panel clips to the canvas itself, so at 0° — where the two
        // are the identity — the proxy is exactly the rectangle it was.
        panel.update(image: canvas.image, canvasSize: state.canvas,
                     visible: state.visible,
                     zoom: CGFloat(state.zoom), isLive: isLive)
    }

    func showNavigatorTab() {
        panelTab = 4
        updatePanelVisibility()
        refreshNavigator()
    }

    /// View ▸ Navigator (⌃⌘N), and the four panels' Navigator tab.
    @objc func showNavigator(_ sender: Any?) {
        layersPanelVisible = true
        showNavigatorTab()
    }

    // MARK: - Gestures

    private func navigatorDidCenter(_ point: CGPoint) {
        scrollCanvasCenter(to: point)
        refreshNavigator(isLive: true)
    }

    private func navigatorDidZoom(_ zoom: CGFloat) {
        let clamped = min(max(Double(zoom), Self.viewZoomRange.lowerBound),
                          Self.viewZoomRange.upperBound)
        zoomPercent = clamped * 100
        refreshNavigator()
    }

    private func navigatorDidFit() {
        zoomToFit()
        refreshNavigator()
    }
}
