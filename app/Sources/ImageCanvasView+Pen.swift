import AppKit

/// The canvas half of the Pen tool (PenSession.swift): the press, drag,
/// release, pointer and key routing `mouseDown` / `mouseDragged` / `mouseUp` /
/// `mouseMoved` / `keyDown` ask first, and the drawing. The gesture rules are
/// the session's; this file hands it events, draws what it holds and passes a
/// finished gesture to `onPenCommit`.
///
/// The current path's outline draws under every tool; its anchors, handles
/// and rubber band only while the Pen is the tool. Points are UNCLAMPED: a
/// handle is routinely dragged past the canvas edge, and a path may lie
/// outside the canvas.
extension ImageCanvasView {
    /// An anchor's square, in screen points.
    private static let penAnchorSize: CGFloat = 6
    /// A handle tip's circle, in screen points.
    private static let penHandleSize: CGFloat = 5

    /// The session changed. A pointer move with a rubber band up repaints
    /// that band's two boxes only: it arrives at pointer rate, and a whole
    /// canvas repaint per event is what `setHoverPoint` avoids for the same
    /// reason.
    func penSessionDidChange(from old: PenSession) {
        guard penSession != old else { return }
        var before = old, after = penSession
        before.hover = nil
        after.hover = nil
        guard before == after else {
            needsDisplay = true
            return
        }
        for session in [old, penSession] {
            if let rect = penRubberBandRect(session) { setNeedsDisplay(rect) }
        }
    }

    private func penRubberBandRect(_ session: PenSession) -> CGRect? {
        guard tool == .pen, let band = session.rubberBandSegment else { return nil }
        let pad = 4 / magnification
        return Self.penBounds(band.from.point, band.from.handleOut, band.to, band.to)
            .insetBy(dx: -pad, dy: -pad)
    }

    /// The box around four points — a segment's control polygon. Spelled out
    /// rather than folded over an array: it runs for every segment and every
    /// handle of the path on each repaint.
    private static func penBounds(
        _ a: CGPoint, _ b: CGPoint, _ c: CGPoint, _ d: CGPoint
    ) -> CGRect {
        let minX = min(min(a.x, b.x), min(c.x, d.x)), maxX = max(max(a.x, b.x), max(c.x, d.x))
        let minY = min(min(a.y, b.y), min(c.y, d.y)), maxY = max(max(a.y, b.y), max(c.y, d.y))
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    /// True when the press is the Pen's.
    func penMouseDown(_ event: NSEvent) -> Bool {
        guard tool == .pen else { return false }
        // A double-click's second press lands on the anchor its first one
        // just placed; taking it would delete that anchor again.
        guard event.clickCount < 2 else { return true }
        let point = convert(event.locationInWindow, from: nil)
        var session = penSession
        session.hover = point
        let taken = session.press(
            at: point, modifiers: event.modifierFlags, magnification: magnification)
        penSession = session
        if !taken { NSSound.beep() }
        return true
    }

    /// A drag tick.
    func penMouseDragged(_ raw: CGPoint) -> Bool {
        guard penSession.drag != nil else { return false }
        var session = penSession
        session.hover = raw
        session.dragged(to: raw, magnification: magnification)
        penSession = session
        return true
    }

    /// The release: a gesture that changed the shape is committed as one
    /// edit, and the subpath it leaves open becomes the one the next click
    /// extends. A refused commit leaves the session where it was.
    func penMouseUp() -> Bool {
        guard penSession.drag != nil else { return false }
        var session = penSession
        let finished = session.release()
        penSession = session
        if let finished, onPenCommit?(finished.shape, finished.name) == true {
            penSession.activeSubpath = finished.activeAfter
        }
        return true
    }

    /// The pointer moved with no button down: the rubber band's far end.
    func penMouseMoved(_ raw: CGPoint) {
        guard tool == .pen, penSession.drag == nil, penSession.hover != raw else { return }
        penSession.hover = raw
    }

    /// Escape drops a gesture in flight; with none, Escape and Return end the
    /// open subpath, so the next click starts a new one. Inert in Quick
    /// Mask, where the Pen's presses are.
    func penKeyDown(_ event: NSEvent) -> Bool {
        guard tool == .pen, !quickMaskActive else { return false }
        if penSession.drag != nil {
            guard event.keyCode == 53 else { return false }
            penSession.drag = nil
            return true
        }
        guard penSession.activeSubpath != nil else { return false }
        switch event.keyCode {
        case 53, 36, 76: // Escape, Return, keypad Enter
            penSession.end()
            return true
        default:
            return false
        }
    }

    /// Every run's dark halo, then every run's white line, so no run's halo
    /// covers a neighbour's line. Round caps close the seam between runs, and
    /// the halo is close to opaque so the caps' overlap does not show as a
    /// darker dot.
    private static func strokePenLines(_ runs: [NSBezierPath], scale: CGFloat) {
        for (width, color) in [
            (3 / scale, NSColor.black.withAlphaComponent(0.85)), (1 / scale, NSColor.white),
        ] {
            color.setStroke()
            for run in runs {
                run.lineWidth = width
                run.lineCapStyle = .round
                run.lineJoinStyle = .round
                run.stroke()
            }
        }
    }

    /// The current path with a dark halo; under the Pen, also the rubber
    /// band, each handle off its point and a square on each anchor, the open
    /// subpath's last one filled to show where the next segment starts.
    /// Everything outside the rectangles being drawn is skipped, so the
    /// rubber band's repaint costs the marks under it and not the whole path.
    func drawPenPath() {
        guard let shape = penSession.displayShape, !shape.subpaths.isEmpty else { return }
        let scale = magnification
        let reach = 2 / scale
        var outline = PenDrawingRuns()
        for subpath in shape.subpaths {
            guard let first = subpath.anchors.first else { continue }
            var previous = first
            for next in subpath.anchors.dropFirst() + (subpath.closed ? [first] : []) {
                let box = Self.penBounds(
                    previous.point, previous.handleOut, next.handleIn, next.point)
                if needsToDraw(box.insetBy(dx: -reach, dy: -reach)) {
                    let from = previous
                    outline.add { run, starts in
                        if starts { run.move(to: from.point) }
                        run.curve(
                            to: next.point, controlPoint1: from.handleOut,
                            controlPoint2: next.handleIn)
                    }
                } else {
                    outline.cut()
                }
                previous = next
            }
            // A run never joins two subpaths.
            outline.cut()
        }
        Self.strokePenLines(outline.finished, scale: scale)
        guard tool == .pen else { return }

        if let band = penSession.rubberBandSegment {
            let preview = NSBezierPath()
            preview.move(to: band.from.point)
            preview.curve(to: band.to, controlPoint1: band.from.handleOut, controlPoint2: band.to)
            preview.lineWidth = 1 / scale
            NSColor.black.withAlphaComponent(0.45).setStroke()
            preview.stroke()
            preview.setLineDash([4 / scale, 3 / scale], count: 2, phase: 0)
            NSColor.white.withAlphaComponent(0.95).setStroke()
            preview.stroke()
        }

        let side = Self.penAnchorSize / scale
        let tip = Self.penHandleSize / scale
        func box(_ point: CGPoint, _ size: CGFloat) -> CGRect {
            CGRect(x: point.x - size / 2, y: point.y - size / 2, width: size, height: size)
        }
        let active = penSession.displayedActiveSubpath
        var stems = PenDrawingRuns()
        var tips = PenDrawingRuns()
        var open = PenDrawingRuns()
        var filled = PenDrawingRuns()
        for (index, subpath) in shape.subpaths.enumerated() {
            for (position, anchor) in subpath.anchors.enumerated() {
                for handle in [anchor.handleIn, anchor.handleOut] where handle != anchor.point {
                    let span = Self.penBounds(anchor.point, handle, anchor.point, handle)
                        .insetBy(dx: -tip, dy: -tip)
                    guard needsToDraw(span) else { continue }
                    stems.add { run, _ in
                        run.move(to: anchor.point)
                        run.line(to: handle)
                    }
                    tips.add { run, _ in run.appendOval(in: box(handle, tip)) }
                }
                let square = box(anchor.point, side)
                guard needsToDraw(square.insetBy(dx: -side, dy: -side)) else { continue }
                if index == active, position == subpath.anchors.count - 1 {
                    filled.add { run, _ in run.appendRect(square) }
                } else {
                    open.add { run, _ in run.appendRect(square) }
                }
            }
        }
        stems.cut()
        tips.cut()
        open.cut()
        filled.cut()
        Self.strokePenLines(stems.finished, scale: scale)
        let dark = NSColor.black.withAlphaComponent(0.85)
        for (marks, fill, edge) in [
            (tips.finished, NSColor.white, dark), (open.finished, NSColor.white, dark),
            (filled.finished, dark, NSColor.white),
        ] {
            fill.setFill()
            edge.setStroke()
            for run in marks {
                run.fill()
                run.lineWidth = 1 / scale
                run.stroke()
            }
        }
    }
}

/// A drawing built as many short paths instead of one long one.
///
/// AppKit's cost to stroke or fill ONE path grows much faster than its
/// segment count when many of its edges cross the same scanlines, so a path
/// with a row of anchors is far slower drawn whole than in pieces. Measured:
/// one round-capped 3-pixel stroke of a 2,000-segment zigzag (256 anchors to
/// a row) into a 4000 x 3000 bitmap, best of five, takes 296 ms as one path,
/// 20 ms in runs of 32 segments, 12.5 ms in runs of 8 and 12.8 ms one segment
/// at a time; 32,767 segments take 5,148 ms as one path and 350 ms in runs
/// of 32. Eight to a run costs what single segments do in an eighth of the
/// calls.
private struct PenDrawingRuns {
    private static let length = 8

    /// The runs cut so far.
    private(set) var finished: [NSBezierPath] = []
    private var current = NSBezierPath()
    private var held = 0

    /// Adds one element to the run being built. `build` is told whether it
    /// starts that run — a connected outline moves to its first point then.
    mutating func add(_ build: (NSBezierPath, Bool) -> Void) {
        build(current, held == 0)
        held += 1
        if held == Self.length { cut() }
    }

    /// Ends the run being built; the next element starts a new one.
    mutating func cut() {
        guard held > 0 else { return }
        finished.append(current)
        current = NSBezierPath()
        held = 0
    }
}
