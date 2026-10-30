import AppKit

// The ruler strips HIDE while a view is turned (`rulersVisible`; the preference is untouched) while
// the grid, guides, marquee and handles rotate for free inside `ImageCanvasView.draw`.

/// Phase 12D-2's editor half: view-only rotation, the canvas host's layout,
/// the second view's menu commands and the cross-view target mirroring.
///
/// **Per VIEW vs per DOCUMENT.** Zoom, scroll and rotation belong to the
/// WINDOW — two views of one picture are exactly a 100 % detail beside a fit
/// overview — while the layer selection, the paint target and the channel
/// eyes are DOCUMENT state and are mirrored into every open view through
/// `.imageDocumentViewTargetsDidChange`. Canvas SESSIONS (a text entry, a Free
/// Transform, a crop box, the Quick Mask buffer, a selection stroke) stay per
/// window; `ImageDocument.commitPendingCanvasSessions` already walks them all.
///
/// Nothing here writes a document: no `applyEdit`, no `updateChangeCount`, no
/// history entry and no Action step — the ruler/grid/proof-setup exemption,
/// for the same four reasons.
extension EditorViewController {
    // MARK: - Which view this is

    /// 1 for the window the document opened with, 2 for the first New View,
    /// and 0 for an editor that no longer belongs to a document.
    var viewOrdinal: Int {
        for controller in document?.windowControllers ?? [] {
            if let editor = controller as? EditorWindowController,
               editor.contentViewController === self {
                return editor.viewOrdinal
            }
        }
        return 0
    }

    // MARK: - Layout

    /// The ONE place the host's and the canvas's geometry is written.
    ///
    /// The order matters and was measured: `frameCenterRotation` is applied
    /// ABOUT the frame's centre, so the frame has to be sized and placed with
    /// the rotation off or the centre moves under it. Assigning it only when
    /// it actually differs keeps the 0° transform exactly the identity — a
    /// rotate-and-return would otherwise leave a 1e-14 residue in every
    /// number `get_view` and the Navigator report.
    func layoutCanvasHost(canvasSize: CGSize) {
        let host = ViewRotation.hostSize(canvas: canvasSize, degrees: viewRotation)
        canvasHost.setFrameSize(host)
        if canvas.frameCenterRotation != 0 { canvas.frameCenterRotation = 0 }
        canvas.setFrameSize(canvasSize)
        canvas.setFrameOrigin(ViewRotation.canvasOrigin(canvas: canvasSize, host: host))
        if viewRotation != 0 { canvas.frameCenterRotation = CGFloat(viewRotation) }
        canvas.layer?.shadowPath = CGPath(rect: canvas.bounds, transform: nil)
        // loadView calls this before the ruler constraints and the panels
        // exist; everything below is chrome that follows the geometry.
        guard isViewLoaded else { return }
        updateRulerVisibility()
        view.window?.invalidateCursorRects(for: canvas)
        view.window?.invalidateCursorRects(for: canvasHost)
        refreshNavigator()
    }

    /// Rotate the VIEW to `degrees`, keeping the canvas point under the well's
    /// centre where it is — which is what makes the picture turn about what
    /// the user is looking at instead of sliding away.
    func setViewRotation(_ degrees: Double) {
        let normalized = ViewRotation.normalize(degrees)
        let canvasSize = document?.doc?.canvasSize ?? .zero
        let clip = scrollView.contentView
        let anchor = canvas.convert(CGPoint(x: clip.bounds.midX, y: clip.bounds.midY),
                                    from: canvasHost)
        viewRotation = normalized
        layoutCanvasHost(canvasSize: canvasSize)
        scrollCanvasCenter(to: anchor)
        refreshNavigator()
        // A rotation need not scroll, and the host-resize notification reads the old angle.
        canvas.brushOutlineViewportDidChange()
    }

    /// The canvas's ⌥-drag and trackpad rotate, forwarded from the closure.
    func rotateViewGestureDidChange(_ degrees: Double) {
        setViewRotation(degrees)
    }

    // MARK: - Menu commands

    /// View ▸ Rotate View… (no chord). The sheet previews live and Cancel puts the
    /// angle the sheet opened on back.
    @objc func rotateView(_ sender: Any?) {
        guard document?.doc != nil else { NSSound.beep(); return }
        let start = viewRotation
        let committed = RotateViewSheetController.run(
            current: start, onChange: { [weak self] degrees in self?.setViewRotation(degrees) })
        setViewRotation(committed ?? start)
    }

    /// View ▸ Reset View Rotation (⌃⌥⌘R).
    @objc func resetViewRotation(_ sender: Any?) {
        setViewRotation(0)
    }

    /// Window ▸ New View for “<document>”.
    @objc func newView(_ sender: Any?) {
        if document?.openSecondView() == nil { NSSound.beep() }
    }

    /// Window ▸ Sync Pan & Zoom. DOCUMENT-wide, not per view: it is the one
    /// switch that says "these windows move together".
    @objc func toggleSyncPanZoom(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        document.syncPanZoom.toggle()
        propagateViewSync()
    }

    /// The four items this file owns. nil means "not one of mine".
    func validateViewsItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        switch item.action {
        case #selector(rotateView(_:)):
            return document?.doc != nil
        case #selector(resetViewRotation(_:)):
            return document?.doc != nil && viewRotation != 0
        case #selector(newView(_:)):
            if let menuItem = item as? NSMenuItem {
                menuItem.title = "New View for “\(document?.displayName ?? "Untitled")”"
            }
            return document?.doc != nil
        case #selector(toggleSyncPanZoom(_:)):
            if let menuItem = item as? NSMenuItem {
                menuItem.state = document?.syncPanZoom == true ? .on : .off
            }
            return document?.doc != nil
        default:
            return nil
        }
    }

    // MARK: - Sync Pan & Zoom

    /// Pushes this view's zoom and centre — never its rotation, which is the
    /// one thing each window is entitled to keep — at the document's other
    /// views.
    ///
    /// The re-entrancy guard is load-bearing: `applyViewState` on the other
    /// editor moves its clip view, which posts its own bounds change and would
    /// push straight back.
    func propagateViewSync() {
        guard let document, document.syncPanZoom, !document.isPropagatingViewSync else { return }
        let others = document.editors.filter { $0 !== self }
        guard !others.isEmpty else { return }
        let state = viewState
        document.isPropagatingViewSync = true
        defer { document.isPropagatingViewSync = false }
        for other in others {
            _ = try? other.applyViewState(zoom: state.zoom, center: state.center)
        }
    }

    // MARK: - Cross-view target mirroring

    /// The document's per-layer TARGETS changed in one of its views: mirror
    /// them into this one.
    ///
    /// Deliberately NOT `.imageDocumentImageDidChange`: that one fires on
    /// every live drag tick and inside every publication window, and reusing
    /// it would have made a second window change what undo, revert and a depth
    /// conversion deliver. `ImageDocument.notifyViewTargetsChanged` posts this
    /// one only when a value actually changed, only with more than one window
    /// open, and never while a state is being published or history navigated.
    ///
    /// It writes no document property that differs from the posted value, so
    /// there is nothing here to post again.
    @objc func viewTargetsDidChange(_ note: Notification) {
        guard let document, (note.object as? ImageDocument) === document else { return }
        precisionGeometryDocumentDidChange()
        setPaintTarget(document.planeEditTarget)
        channelsPanel?.restoreVisibility(document.channelVisibility)
        activeLayerDidChange()
    }
}
