import AppKit

extension ImageCanvasView {
    /// Native Crop returns a new canvas-sized display. Draw its pixels at
    /// the old window origin, filling the window: an exact preview has the
    /// window's own dimensions, and a reduced one (the window at the
    /// proxy's size) is scaled up to it. The Crop overlay dims the rest of the
    /// old canvas. CoreGraphics never rotates it again.
    func drawPrecisionCropPreview(_ image: CGImage, at window: CGRect,
                                  in context: CGContext) {
        context.saveGState()
        context.interpolationQuality = imageInterpolation
        context.translateBy(x: 0, y: bounds.height)
        context.scaleBy(x: 1, y: -1)
        context.draw(image, in: CGRect(origin: window.origin, size: window.size))
        context.restoreGState()
    }
}
