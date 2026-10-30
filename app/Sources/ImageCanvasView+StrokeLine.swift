import AppKit

// When ⇧ is down and a `strokeLineAnchor` is set (latched in `endStroke`, cleared on a real tool
// change or a canvas-size change), `beginStrokePress` begins at the anchor and lays one straight
// chord to the click past the leash and spline, so the result is an ordinary stroke — own undo
// step, recorded Action, agent replay.

/// ⇧-click straight lines for the stroke tools (brush, eraser, clone, dodge/burn and both
/// healing brushes): a ⇧-press paints one straight segment from where the last stroke ended
/// to the click, as one ordinary stroke — its own undo step, recorded Action and agent replay
/// (`points: [anchor, point]`). The view is frozen, so the logic lives here and `mouseDown`
/// keeps only its call.
extension ImageCanvasView {
    /// mouseDown's single entry for the six stroke tools. Without ⇧, or with no anchor, it is
    /// exactly `beginStroke(at:pressure:)`. With ⇧ and an anchor: beginStroke(at: anchor,
    /// pressure:), then — only if the stroke actually began (`strokeActive`; a refusal beeped
    /// inside beginStroke) — extendStrokeStraight(to: point, pressure:). The press then
    /// continues as an ordinary drag from `point`.
    func beginStrokePress(at point: CGPoint, pressure: CGFloat?, modifiers: NSEvent.ModifierFlags) {
        guard modifiers.contains(.shift), let anchor = strokeLineAnchor else {
            beginStroke(at: point, pressure: pressure)
            return
        }
        beginStroke(at: anchor, pressure: pressure)
        guard strokeActive else { return }
        extendStrokeStraight(to: point, pressure: pressure)
    }

    /// One straight chord from the stroke's start to `point`, bypassing the leash and the spline,
    /// then restarts both at `point` so a following drag continues from there.
    private func extendStrokeStraight(to point: CGPoint, pressure: CGFloat?) {
        guard strokeActive, let context = overlayContext else { return }
        // The leash would stop short of the click by its radius; parking it on the point makes
        // `pull` the identity here and starts the following drag's leash at the click.
        strokeLeash.position = point
        if brushSize <= 1 {
            // The pixel branch already draws exact chords between snapped centres.
            continueStroke(to: point, pressure: pressure)
            return
        }
        // Straight past the spline, whose one-sample lag would hold the chord back.
        renderStroke(
            through: [StrokeVertex(point: point, pressure: strokeCursorPressure)], in: context)
        strokeCursor = point
        strokeSpline.begin(at: point, pressure: strokeCursorPressure)
        emitStrokeUpdate()
    }
}
