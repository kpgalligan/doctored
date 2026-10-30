import AppKit

// The canvas draws only the translated marquee (`moveSelectionOffset`).

extension ImageCanvasView {
    /// Move Selected Pixels: only the marquee, translated by the drag's
    /// offset — no dim wash, which would sit over the moving pixels. The
    /// marquee leads; the pixels follow as each background preview lands.
    func drawMovingSelection(_ selection: CanvasSelection, offset: CGPoint) {
        guard let path = (selection.marqueePath ?? NSBezierPath(rect: selection.bounds))
            .copy() as? NSBezierPath else { return }
        let shift = AffineTransform(translationByX: offset.x, byY: offset.y)
        path.transform(using: shift)
        strokeMarquee(path)
    }

    /// The Move drag's cursor: ⌥ latched at the press (`moveDragDuplicate`)
    /// shows the drag-copy cursor for the whole drag.
    var moveDragCursor: NSCursor { moveDragDuplicate ? .dragCopy : .closedHand }
}
