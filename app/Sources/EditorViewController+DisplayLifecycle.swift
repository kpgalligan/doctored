import AppKit

// AppKit may retain closed windows beyond document teardown; no model/history or captured worker
// inputs are cleared, and cancelled closes keep their display. This file is also where a typed
// opacity drag interrupted mid-gesture is cancelled (`precisionOpacityDrag?.cancel()`, then nil) —
// `cancel()` publishes nothing, so this releases the gesture's capture and preview without touching
// the model. The will-close handling also calls the Layers, Channels, Info and Paths panels'
// `releaseForWindowClose()`: each
// removes its `.imageDocumentImageDidChange` observer (the Layers panel also empties its thumbnail
// cache), so a closed view's panels stop rebuilding rows, plane thumbnails and histograms on every
// edit of a document that stays open. Each `deinit` is unchanged, and the Info panel's
// `loader.cancel()` is deliberately NOT called at close — it blocks until an in-flight scan
// finishes.
//
// A closed view detaches: `releaseDisplayForWindowClose()` removes the three document-object
// observers, because since Phase 12D-2's second view a closed window no longer means a closed
// document.

extension EditorViewController {
    /// AppKit can retain a closed window and its views after NSDocument releases
    /// its controller. Drop only display images at actual window close, so their
    /// zero-copy providers do not keep full-size Rust preview buffers alive.
    /// Document state, history and captured worker inputs keep their own owners.
    ///
    /// Since 12D-2 a closed view's DOCUMENT can outlive it — closing a second
    /// view leaves the document open — so this also drops the three
    /// document-object observers. Without that, an editor AppKit still retains
    /// would answer the next edit by re-acquiring `document.previewImage`,
    /// re-running `zoomToFit`/`beginCropSession`/`refreshNavigator` with no
    /// window, and mirroring targets into a canvas nobody can see. Removing an
    /// observer is display-side only: the three handlers write no model state.
    /// The Layers, Channels and Info panels drop their document observers the
    /// same way, so a closed view's panels stop rebuilding rows,
    /// thumbnails and histograms on every edit.
    func releaseDisplayForWindowClose() {
        guard isViewLoaded else { return }
        canvas.cancelByteStrokeForWindowClose()
        let center = NotificationCenter.default
        center.removeObserver(self, name: .imageDocumentImageDidChange, object: document)
        center.removeObserver(self, name: .imageDocumentSelectionDidChange, object: document)
        center.removeObserver(self, name: .imageDocumentViewTargetsDidChange, object: document)
        releaseHDRPreviewForWindowClose()
        // A typed opacity drag interrupted without an `ended` tick — a window
        // closed mid-gesture — would otherwise hold its capture and its
        // on-screen preview until the controller deallocated. `cancel()` is
        // the publish-nothing half of the session's cancel-first prefix.
        precisionOpacityDrag?.cancel()
        precisionOpacityDrag = nil
        precisionMoveGesture?.cancel()
        precisionMoveGesture = nil
        moveSelectedSession?.preview?.cancel()
        moveSelectedSession = nil
        moveDuplicate = nil
        canvas.moveSelectionOffset = nil
        transformSession?.precisionGesture?.cancel()
        transformSession?.corePreview?.cancel()
        cropSession?.precisionGesture?.cancel()
        if transformSession?.precisionGesture != nil { transformSession = nil }
        if cropSession?.precisionGesture != nil { cropSession = nil }
        canvas.precisionMoveActive = false
        canvas.precisionCropPreview = nil
        canvas.precisionCropWindow = nil
        retouchSourceOverlay = nil
        canvas.previewImage = nil
        canvas.transformPreview = nil
        canvas.channelDisplay = nil
        // cancel() drops the subject outline but intentionally keeps its image
        // analysis cache during normal editing. A closed view needs neither.
        canvas.subjectSession = SubjectSession()
        // The side panels observe the document too, and the Layers
        // panel's thumbnail cache retains a RasterDocument.
        layersPanel?.releaseForWindowClose()
        channelsPanel?.releaseForWindowClose()
        infoPanel?.releaseForWindowClose()
        pathsPanel?.releaseForWindowClose()
        canvas.image = nil
        canvas.layer?.contents = nil
        // Flush the previous drawing while the closing window still owns its
        // backing surface. Clearing Swift references alone can leave it cached.
        canvas.needsDisplay = true
        canvas.displayIfNeeded()
    }
}
