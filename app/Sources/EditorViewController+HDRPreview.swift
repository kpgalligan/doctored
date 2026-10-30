import AppKit

// View ▸ HDR Preview (v1.x): on an EDR screen a 32-bit float window shows values above 1.0
// brighter than white, through the same display-only `canvas.proofImage` slot as the view exposure
// (`EditorViewController+ViewExposure.swift` renders it; this file owns the engage rule, the
// toggle and the window's colour space). Pure VIEW state, per window: no pixel, no undo step, no
// dirty flag, no Action step and nothing a save carries; Info, the samplers, the eyedropper,
// `sample_pixel`, `render`, thumbnails, the Navigator and save read document values only. The
// observers registered here are removed at window close
// (`EditorViewController+DisplayLifecycle.swift` calls `releaseHDRPreviewForWindowClose`).

extension EditorViewController {
    // MARK: - Reading

    /// The engage rule: HDR Preview is on, the window's screen can show more
    /// than SDR white (`EDRDisplay.potentialHeadroom`, which a test instance
    /// overrides with `DR_EDR_HEADROOM`), the document is 32-bit float, and
    /// no candidate tone map is pending (the Tone Map sheet previews SDR).
    var hdrPreviewEngages: Bool {
        guard viewExposure.hdrPreview, document?.doc?.isFloat == true,
              viewExposure.candidateToneMap == nil else { return false }
        return edrHeadroom > 1
    }

    /// The window's screen's potential EDR headroom (MCP `edr_headroom`).
    var edrHeadroom: Double { EDRDisplay.potentialHeadroom(of: viewIfLoaded?.window?.screen) }

    /// True while the canvas shows the extended-range image rendered for
    /// this window's current view (MCP `hdr_preview_displayed`).
    var hdrPreviewDisplayed: Bool {
        canvas.extendedRangeDisplay && viewExposure.shown?.extendedRange == true
            && viewExposureDisplayed
    }

    /// The peak of the landed extended-range image; nil when none is shown
    /// (MCP `edr_content_peak`).
    var extendedContentPeak: Float? { canvas.extendedRangeDisplay ? viewExposure.extendedPeak : nil }

    // MARK: - Writing

    /// The one writer of the window's HDR Preview switch. Accepted at every
    /// depth — it acts only on a 32-bit float document, as the stored
    /// exposure does.
    func setHDRPreview(_ on: Bool) throws {
        viewExposure.hdrPreview = on
        refreshProofImage()
        updateStatus()
    }

    /// The slot now holds an extended-range image whose largest RGB
    /// component is `peak`: the canvas layer goes EDR and the window draws
    /// in extended Display P3 (gamma-encoded, so overlay blending is
    /// unchanged), its previous space kept for the way back.
    func beginExtendedRange(peak: Float) {
        viewExposure.extendedPeak = peak
        if !canvas.extendedRangeDisplay { canvas.extendedRangeDisplay = true }
        if viewExposure.priorWindowColorSpace == nil, let window = viewIfLoaded?.window,
           let extended = CGColorSpace(name: CGColorSpace.extendedDisplayP3) {
            viewExposure.priorWindowColorSpace = .some(window.colorSpace)
            window.colorSpace = NSColorSpace(cgColorSpace: extended)
        }
    }

    /// The slot holds no extended-range image: RGBA8, SDR, and the window's
    /// own colour space back.
    func endExtendedRange() {
        viewExposure.extendedPeak = nil
        if canvas.extendedRangeDisplay { canvas.extendedRangeDisplay = false }
        if let prior = viewExposure.priorWindowColorSpace {
            viewIfLoaded?.window?.colorSpace = prior
            viewExposure.priorWindowColorSpace = nil
        }
    }

    // MARK: - Screens

    /// The two notifications that can change the engage rule's answer: a
    /// display's parameters (headroom included) and the window moving to
    /// another screen. Called once from `viewDidLoad`.
    func installHDRPreviewObservers() {
        let center = NotificationCenter.default
        center.addObserver(self, selector: #selector(hdrScreenDidChange(_:)),
                           name: NSApplication.didChangeScreenParametersNotification, object: nil)
        center.addObserver(self, selector: #selector(hdrScreenDidChange(_:)),
                           name: NSWindow.didChangeScreenNotification, object: nil)
    }

    /// At window close: the two observers go, and the window keeps no
    /// extended colour space.
    func releaseHDRPreviewForWindowClose() {
        let center = NotificationCenter.default
        center.removeObserver(self, name: NSApplication.didChangeScreenParametersNotification,
                              object: nil)
        center.removeObserver(self, name: NSWindow.didChangeScreenNotification, object: nil)
        endExtendedRange()
    }

    @objc private func hdrScreenDidChange(_ note: Notification) {
        if let window = note.object as? NSWindow, window !== viewIfLoaded?.window { return }
        guard viewIfLoaded?.window != nil, document?.doc?.isFloat == true else { return }
        refreshProofImage()
    }

    // MARK: - Menu

    /// View ▸ HDR Preview.
    @objc func toggleHDRPreview(_ sender: Any?) {
        try? setHDRPreview(!viewExposure.hdrPreview)
    }

    /// The item this file owns, checked when on; enabled on a 32-bit float
    /// document. nil means "not one of mine".
    func validateHDRPreviewItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard item.action == #selector(toggleHDRPreview(_:)) else { return nil }
        (item as? NSMenuItem)?.state = viewExposure.hdrPreview ? .on : .off
        return document?.doc?.isFloat == true
    }
}
