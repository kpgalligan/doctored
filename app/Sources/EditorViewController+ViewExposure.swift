import AppKit

// The exposed image is rendered ONLY from `document.retainedPrecisionProjection`, off the main
// thread on `viewExposureRenderer`: this file never calls `currentPrecisionProjection()` or any
// other render. `refreshViewExposureImage()` is the float arm of `refreshProofImage()` and has no
// other caller; every other site calls `refreshProofImage()`. `canvas.image` is never assigned a
// view image, and `canvas.previewImage` never an exposed or candidate-mapped one, so
// `onPreviewImageChange`'s consumers (the samplers' after column, the clipping overlay) never see
// either. Since v1.x the same slot carries the EXTENDED-RANGE view on an EDR screen
// (`EditorViewController+HDRPreview.swift` owns the engage rule): `requestedFloatView` says which,
// the core's `dr_precision_doc_display_extended` renders it, and a refused extended render falls
// back to the SDR view.

/// View ▸ 32-bit Exposure… / Reset 32-bit Exposure.
///
/// A tone-mapped display hides what is above white: under `clip` every value
/// past 1.0 is the same white. The view exposure is how a window looks at
/// that range without editing anything — `gain = 2^stops` on RGB before the
/// tone map, alpha untouched, −20…+20 stops.
///
/// It is pure VIEW state, per WINDOW, as view rotation is: no pixel, no undo
/// step, no dirty flag, nothing a `save_copy` carries, and never an Action
/// step. It reaches only the canvas, through the display-only
/// `canvas.proofImage` slot — a float document has no soft proof, so the slot
/// is free (`EditorViewController+SoftProof.swift`). Readouts, the Color
/// Samplers, the clipping overlay, the Navigator, thumbnails, export and save
/// never see it. The stored stops survive a depth change but act only on a
/// float document.
///
/// The Tone Map sheet's live preview is the same mechanism: a CANDIDATE map
/// held in `ViewExposureState`, drawn in the same slot under the window's
/// exposure, and cleared on Cancel or after Apply.
///
/// MCP: `get_view` / `set_view` carry `exposure` and the read-only
/// `exposure_displayed` exactly as they carry `rotation`
/// (`AgentServer+Workspace.swift`, `AgentServer+Views.swift`).
extension EditorViewController {
    // MARK: - Reading

    /// The exposure this window shows: the stored stops on a 32-bit float
    /// document, 0 at every other depth.
    var viewExposureStops: Double {
        document?.doc?.isFloat == true ? viewExposure.stops : 0
    }

    /// True when the canvas is showing the image rendered for this window's
    /// current exposure and candidate map. The render is asynchronous, so it
    /// is false between a change and its landing — and false at 0 stops with
    /// no candidate, where the slot is empty unless HDR Preview shows the
    /// extended-range view, and off a float document.
    var viewExposureDisplayed: Bool {
        guard document?.doc?.isFloat == true, let shown = viewExposure.shown else { return false }
        return shown == requestedFloatView
    }

    /// What this window asks the slot for: the exposure and candidate map,
    /// extended-range when HDR Preview engages (`hdrPreviewEngages`).
    var requestedFloatView: FloatDisplayView {
        var view = viewExposure.view
        view.extendedRange = hdrPreviewEngages
        return view
    }

    /// The exposure segment of the status bar; nil at 0 stops and off a
    /// float document.
    var viewExposureStatusText: String? {
        let stops = viewExposureStops
        return stops == 0 ? nil : "Exposure \(ViewExposure.text(stops)) EV"
    }

    // MARK: - Writing

    /// Why `stops` cannot be this window's exposure, or nil: a number outside
    /// ±20 stops, and a non-zero exposure on a document that is not 32-bit
    /// float. BY NAME, never clamped.
    func viewExposureRefusal(_ stops: Double) -> String? {
        guard stops.isFinite, ViewExposure.range.contains(stops) else {
            return "exposure must be between \(Int(ViewExposure.range.lowerBound)) and "
                + "\(Int(ViewExposure.range.upperBound)) stops; got \(stops)."
        }
        guard stops == 0 || document?.doc?.isFloat == true else {
            return "exposure applies only to a 32-bit float document. "
                + "This document's display has no range above white to look at."
        }
        return nil
    }

    /// The one writer of the window's exposure; `viewExposureRefusal`'s
    /// refusals.
    func setViewExposure(_ stops: Double) throws {
        if let refusal = viewExposureRefusal(stops) { throw DoctoredCoreError(message: refusal) }
        viewExposure.stops = stops
        refreshProofImage()
        updateStatus()
    }

    /// The Tone Map sheet's pending choice. A map equal to the document's own
    /// — and nil — is no candidate at all.
    func setToneMapCandidate(_ map: ToneMap?) {
        viewExposure.candidateToneMap = map == document?.doc?.toneMap ? nil : map
        refreshProofImage()
    }

    /// The float arm of `refreshProofImage()`; no other caller.
    ///
    /// At the document's own map and 0 stops the slot is empty. Otherwise the
    /// exposed image is rendered from the RETAINED projection on
    /// `viewExposureRenderer`: while that projection waits on a scheduled
    /// refresh no request is made (the refresh's change notification
    /// re-enters here), and when none is retained and none is coming the slot
    /// is cleared. A held image of another size than `canvas.image` is
    /// dropped at once, before the projection is asked for, so it never
    /// outlives a pending projection refresh; a same-size one stays up while
    /// the next renders. A landing is accepted only when the document handle,
    /// the view and the canvas size are still the ones it was requested for
    /// (`viewExposureDidRender`), and a render that FAILED for that still
    /// current request clears the slot: the image of the previous exposure
    /// is not left up under the new one's name.
    func refreshViewExposureImage() {
        guard let document, let doc = document.doc, doc.isFloat else { return }
        // A slot nothing describes holds a soft proof from before a Redo into
        // float: it is not this view's picture.
        if viewExposure.shown == nil {
            canvas.proofImage = nil
            endExtendedRange()
        }
        let view = requestedFloatView
        guard view != .document else {
            viewExposureRenderer.cancel()
            canvas.proofImage = nil
            viewExposure.shown = nil
            endExtendedRange()
            return
        }
        let size = canvas.image.map { NSSize(width: $0.width, height: $0.height) }
        if let held = canvas.proofImage, let size,
           NSSize(width: held.width, height: held.height) != size {
            canvas.proofImage = nil
            viewExposure.shown = nil
            endExtendedRange()
        }
        guard let projection = document.retainedPrecisionProjection else {
            viewExposureRenderer.cancel()
            if document.precisionProjectionTimer == nil {
                canvas.proofImage = nil
                viewExposure.shown = nil
                endExtendedRange()
            }
            return
        }
        // `PreviewRenderer` delivers every finished render to its CURRENT
        // `onRender`, so a render requested for an earlier view can arrive
        // here. Each request's compute keeps its own outcome in its own box
        // and the landing reads THAT box, never the delivered image, so only
        // this request's own render can land or clear.
        let own = ViewExposureRender()
        let space = doc.colorSpace
        viewExposureRenderer.onRender = { [weak self] _ in
            self?.viewExposureDidRender(own, document: doc, view: view, size: size)
        }
        viewExposureRenderer.request {
            guard let context = try? PrecisionWorkContext(image: projection) else {
                own.store(nil)
                return nil
            }
            // The EDR view: the core's binary16 linear buffer wrapped in the
            // document space's extended-linearized twin. A refusal (its plan
            // past the limit, a space with no linear form) falls back to the
            // SDR view of the same exposure.
            if view.extendedRange, let precision = doc.precision,
               let extended = try? precision.displayExtended(
                   of: projection, exposureStops: view.exposureStops,
                   workingLimit: context.limit, otherRetained: context.otherRetained),
               let image = EDRDisplay.image(halfFloat: extended.pixels, width: projection.width,
                                            height: projection.height, documentSpace: space) {
                own.store(image, peak: extended.peak)
                return image
            }
            var sdr = view
            sdr.extendedRange = false
            guard let rendered = try? doc.display(of: projection, context: context, view: sdr)
            else {
                own.store(nil)
                return nil
            }
            let image = rendered.makeCGImage(in: space)
            own.store(image)
            return image
        }
    }

    /// One exposure request's landing, on the main thread. `own` holds an
    /// outcome only once that request's own compute has returned, so a
    /// delivery that belongs to an earlier request finds it empty and does
    /// nothing. While `doc`, `view` and `size` are still the window's, a
    /// rendered image of that size is installed, and a failed render clears
    /// the slot and `shown`.
    private func viewExposureDidRender(_ own: ViewExposureRender, document doc: RasterDocument,
                                       view: FloatDisplayView, size: NSSize?) {
        guard let (outcome, peak) = own.take(), document?.doc === doc, requestedFloatView == view,
              canvas.image.map({ NSSize(width: $0.width, height: $0.height) }) == size
        else { return }
        guard let image = outcome else {
            canvas.proofImage = nil
            viewExposure.shown = nil
            endExtendedRange()
            return
        }
        guard size == nil || NSSize(width: image.width, height: image.height) == size
        else { return }
        canvas.proofImage = image
        viewExposure.shown = view
        if let peak { beginExtendedRange(peak: peak) } else { endExtendedRange() }
    }

    // MARK: - Menu commands

    /// View ▸ 32-bit Exposure…. The sheet previews live and Cancel puts the
    /// exposure the sheet opened on back.
    @objc func showViewExposure(_ sender: Any?) {
        guard document?.doc?.isFloat == true else { NSSound.beep(); return }
        let start = viewExposure.stops
        let committed = ViewExposureSheetController.run(
            current: start, onChange: { [weak self] stops in try? self?.setViewExposure(stops) })
        try? setViewExposure(committed ?? start)
    }

    /// View ▸ Reset 32-bit Exposure.
    @objc func resetViewExposure(_ sender: Any?) {
        try? setViewExposure(0)
    }

    /// The two items this file owns. nil means "not one of mine".
    func validateViewExposureItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        switch item.action {
        case #selector(showViewExposure(_:)):
            return document?.doc?.isFloat == true
        case #selector(resetViewExposure(_:)):
            return viewExposureStops != 0
        default:
            return nil
        }
    }
}

/// One exposure request's own outcome, stored on the renderer's queue and
/// taken on the main thread: its rendered image, or nil for a render that
/// failed, and the extended-range peak when the image is the EDR view.
private final class ViewExposureRender {
    private let lock = NSLock()
    /// nil until the compute returns; then the image, or `.some(nil)` for a
    /// failure, with the peak of an extended-range image.
    private var outcome: (CGImage?, Float?)?

    func store(_ image: CGImage?, peak: Float? = nil) {
        lock.lock()
        outcome = (image, peak)
        lock.unlock()
    }

    /// This request's outcome, once, after its compute has stored one: nil
    /// means "not this request's delivery".
    func take() -> (CGImage?, Float?)? {
        lock.lock()
        defer { lock.unlock() }
        let own = outcome
        outcome = nil
        return own
    }
}
