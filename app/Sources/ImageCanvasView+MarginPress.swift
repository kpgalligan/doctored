import AppKit

// `acceptsMarginPress` (the tool's `EditorTool.acceptsMarginPress` — marquee, ellipse, freehand
// lasso, Quick Selection, Object Selection — and not transforming or in Quick Mask) gates
// `CenteringClipView`'s
// `mouseDown`/`mouseDragged`/`mouseUp`, which hold a void press until the first drag tick and then
// forward the ORIGINAL press and the drag to the canvas, whose own clamping lands the anchor on the
// nearest image edge. A tick-less void press forwards nothing, so a click there stays inert for
// every tool. The Crop tool with an open box is admitted too, so a box can be drawn or
// a handle dragged from outside the image; crop's point is unclamped (`onCropMouse*`), so the
// press lands where it was made and the box may reach past the canvas.

/// A marquee, freehand-lasso or Quick Selection DRAG may begin on the pasteboard
/// around the image. The canvas is exactly image-sized, so such a press reaches the
/// `CenteringClipView`, which holds it until the first drag tick and then forwards the
/// original press and the drag to the canvas. The canvas's own clamping lands the anchor on
/// the nearest image edge; a click on the void (no drag tick) forwards nothing.
extension ImageCanvasView {
    /// Whether a DRAG that begins OUTSIDE this canvas should be delivered to it. A transform
    /// press is an unclamped handle grab, and the marquee tools are inert in Quick Mask, so
    /// both stay inert on the void too.
    var acceptsMarginPress: Bool {
        (tool.acceptsMarginPress || (tool == .crop && cropOverlay != nil))
            && !isTransforming && !quickMaskActive
    }
}
