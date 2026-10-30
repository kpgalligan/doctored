import AppKit

/// View ▸ Show Clipping's overlay, drawn over the base image and
/// below Quick Mask and every washes/grid layer. The image is a TRANSPARENT
/// canvas-sized layer (`DR_CLIP_DISPLAY_OVERLAY`: blue where a channel sits at
/// 0, red at the maximum, magenta for both) that
/// `EditorViewController+ClippingOverlay` is the one writer of.
extension ImageCanvasView {
    /// Draws `clippingOverlay` unless the picture under it is not the one it
    /// describes: a free transform or crop (which move or rotate the image)
    /// and a channel display that replaces the composite.
    func drawClippingOverlay(in context: CGContext) {
        guard let overlay = clippingOverlay, transformPreview == nil, cropOverlay == nil,
              precisionCropPreview == nil, channelDisplay?.replacesComposite != true
        else { return }
        drawFlipped(overlay, in: context)
    }
}
