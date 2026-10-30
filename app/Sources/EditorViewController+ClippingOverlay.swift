import AppKit

/// View ▸ Show Clipping (⌥⌘J), a persistent canvas overlay of
/// the clipped pixels — blue where any RGB channel sits at 0, red at the
/// maximum (255, or exactly 65535 at 16 bits), magenta for both — while
/// adjusting and as a plain view toggle. The overlay is the core's
/// `DR_CLIP_DISPLAY_OVERLAY` of whatever the canvas shows: a sheet's preview
/// while one is up, else the retained projection. It describes the
/// UNPROOFED document numbers.
///
/// Like Show Color Samplers it is an app-wide VIEW PREFERENCE
/// (`ViewToolOptions.showClipping`): no undo, no dirty flag, no Action step
/// and no MCP twin — an agent reads `histogram`, whose endpoint counts say
/// the same thing in numbers (app/CLAUDE.md's view-preference exemption).
extension EditorViewController {
    /// View ▸ Show Clipping: every editor answers the chrome broadcast.
    @objc func toggleShowClipping(_ sender: Any?) {
        ToolOptionsStore.shared.view.showClipping.toggle()
        viewChromeChanged()
    }

    /// The canvas's `onPreviewImageChange`: the Color Samplers' after
    /// readings, then the overlay of the new preview.
    func canvasPreviewImageDidChange(_ image: CGImage?) {
        colorSamplerPreviewChanged(image)
        refreshClippingOverlay()
    }

    /// The ONE writer of `canvas.clippingOverlay`. Clears it when the
    /// preference is off, on a 32-bit float document (no fixed endpoints)
    /// and while a ⌥-drag threshold display is on screen. Otherwise renders
    /// on `clippingRenderer` from values only, and lands a result only when
    /// it is still the canvas image's size.
    func refreshClippingOverlay() {
        guard ToolOptionsStore.shared.view.showClipping, let document, let doc = document.doc,
              !doc.isFloat, canvas.clipDisplayPreview == nil
        else {
            clippingRenderer.cancel()
            canvas.clippingOverlay = nil
            return
        }
        if let held = canvas.clippingOverlay, !matchesCanvas(held) {
            canvas.clippingOverlay = nil
        }
        let mode = ClipDisplayMode.overlay.code
        let compute: () -> CGImage?
        if let preview = canvas.previewImage {
            if let source = canvas.previewPrecisionSource, source.preview === preview {
                // A 16-bit sheet's authoritative projection: exact 65535.
                let projection = source.projection
                compute = { Self.overlay(of: projection, mode: mode) }
            } else {
                compute = {
                    let space = Bitmap.ingestSpace(of: preview).space
                    guard let bytes = Bitmap.straightRGBA(
                            from: preview, fitting: preview.width, height: preview.height,
                            space: space),
                          let image = RasterImage.from(
                            rgba: bytes, width: preview.width, height: preview.height)
                    else { return nil }
                    return image.clipDisplay(mode: mode)?.makeCGImage(in: ColorProfile.sRGB)
                }
            }
        } else if doc.isHighDepth {
            guard let projection = document.retainedPrecisionProjection else {
                // A scheduled refresh re-enters here when it lands; keep the
                // held overlay meanwhile.
                if document.precisionProjectionTimer != nil { return }
                clippingRenderer.cancel()
                canvas.clippingOverlay = nil
                return
            }
            compute = { Self.overlay(of: projection, mode: mode) }
        } else {
            let retained = document.retainedByteProjection
            compute = {
                (retained ?? doc.flattened())?.clipDisplay(mode: mode)?
                    .makeCGImage(in: ColorProfile.sRGB)
            }
        }
        clippingRenderer.onRender = { [weak self] image in self?.clippingOverlayDidRender(image) }
        clippingRenderer.request(compute)
    }

    private static func overlay(of projection: RasterPrecisionImage, mode: UInt32) -> CGImage? {
        (try? projection.clipDisplay(mode: mode))?.makeCGImage(in: ColorProfile.sRGB)
    }

    /// A render landing: kept only while the preference still holds and it
    /// is the canvas image's own size (a crop or resize meanwhile makes it
    /// stale, and the overlay held until then stays). A render that
    /// produced NO image clears the overlay: the one on the canvas belongs
    /// to an earlier picture, and keeping it would show clipping that is
    /// no longer there.
    private func clippingOverlayDidRender(_ image: CGImage?) {
        guard ToolOptionsStore.shared.view.showClipping, canvas.clipDisplayPreview == nil
        else { return }
        guard let image else {
            canvas.clippingOverlay = nil
            return
        }
        guard matchesCanvas(image) else { return }
        canvas.clippingOverlay = image
    }

    private func matchesCanvas(_ image: CGImage) -> Bool {
        guard let base = canvas.image else { return false }
        return image.width == base.width && image.height == base.height
    }
}
