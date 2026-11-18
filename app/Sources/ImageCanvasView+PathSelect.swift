import AppKit

/// The canvas half of Path Selection and Direct Selection (PathSelectSession.swift): the press,
/// drag, release and key routing `mouseDown` / `mouseDragged` / `mouseUp` / `keyDown` ask after
/// the Pen's, and the marks `drawPenPath` adds under the two tools. The gesture rules are the
/// session's; this file hands it the current path (`penSession`'s cache), draws what it holds and
/// passes a finished gesture to `onPathSelectCommit`. Points are UNCLAMPED, as the Pen's are.
extension ImageCanvasView {
    /// An anchor's square, in screen points (the Pen's).
    private static let selectAnchorSize: CGFloat = 6
    /// A handle tip's circle, in screen points (the Pen's).
    private static let selectHandleSize: CGFloat = 5

    /// The session changed: redraw, and tell the controller when the
    /// selection did (the options bar's Combine buttons follow it).
    func pathSelectSessionDidChange(from old: PathSelectSession) {
        guard pathSelectSession != old else { return }
        needsDisplay = true
        if pathSelectSession.selection != old.selection { onPathSelectionChange?() }
    }

    /// True while ⌫ is the selection tools' — one of the two is the tool,
    /// something is selected and Quick Mask is off (where the keys are inert)
    /// — so Edit ▸ Clear stands down and the key reaches `pathSelectKeyDown`
    /// (`EditorViewController.validateUserInterfaceItem`).
    var pathSelectionOwnsDelete: Bool {
        tool.selectsPath && pathSelectSession.hasSelection && !quickMaskActive
    }

    /// True when the press is the two tools'. With no current path the
    /// controller is asked for one (`onPathSelectNeedsTarget`: the active
    /// path shape layer's outline); with none there is nothing to select, and
    /// the press beeps and changes nothing.
    func pathSelectMouseDown(_ event: NSEvent) -> Bool {
        guard tool.selectsPath else { return false }
        if penSession.shape == nil { _ = onPathSelectNeedsTarget?() }
        guard let base = penSession.shape, let target = penSession.target else {
            NSSound.beep()
            return true
        }
        var session = pathSelectSession
        session.press(
            at: convert(event.locationInWindow, from: nil), modifiers: event.modifierFlags,
            magnification: magnification, base: base, target: target, tool: tool)
        pathSelectSession = session
        return true
    }

    /// A drag tick.
    func pathSelectMouseDragged(_ raw: CGPoint) -> Bool {
        guard pathSelectSession.drag != nil else { return false }
        var session = pathSelectSession
        session.dragged(to: raw, magnification: magnification)
        pathSelectSession = session
        return true
    }

    /// The release: a gesture that changed the path is committed as one edit.
    func pathSelectMouseUp() -> Bool {
        guard pathSelectSession.drag != nil else { return false }
        var session = pathSelectSession
        let finished = session.release()
        pathSelectSession = session
        if let finished { commitPathSelect(finished) }
        return true
    }

    @discardableResult
    private func commitPathSelect(_ commit: PathSelectCommit) -> Bool {
        let accepted = onPathSelectCommit?(commit.shape, commit.name) == true
        pathSelectSession.commitEnded(accepted: accepted)
        return accepted
    }

    /// The options bar's Operation: the selected subpaths take `op`
    /// (`PathSelectSession.settingOperation`), committed as ⌫'s edit is. True
    /// when the document took it; false for nothing to change, which beeps
    /// nowhere (the popup shows the operation already held).
    func setSelectedSubpathsOperation(_ op: PathCombineOperation) -> Bool {
        guard tool.selectsPath, !quickMaskActive, let base = penSession.shape else { return false }
        var session = pathSelectSession
        guard let commit = session.settingOperation(op, base: base) else { return false }
        pathSelectSession = session
        return commitPathSelect(commit)
    }

    /// Escape drops a gesture in flight, else the selection; ⌫ removes what
    /// is selected. Inert in Quick Mask, as the presses are.
    func pathSelectKeyDown(_ event: NSEvent) -> Bool {
        guard tool.selectsPath, !quickMaskActive else { return false }
        switch event.keyCode {
        case 53: // Escape
            if pathSelectSession.drag != nil {
                pathSelectSession.cancelDrag()
                return true
            }
            guard pathSelectSession.hasSelection else { return false }
            pathSelectSession.clearSelection()
            return true
        case 51, 117: // Delete, forward delete
            guard event.modifierFlags.intersection([.command, .control, .option]).isEmpty,
                  let base = penSession.shape
            else { return false }
            var session = pathSelectSession
            guard let commit = session.deleteSelection(base: base) else { return false }
            pathSelectSession = session
            commitPathSelect(commit)
            return true
        default:
            return false
        }
    }

    /// Under the two tools, over the outline `drawPenPath` has drawn: every
    /// anchor's square — filled when it, or its subpath, is selected — the
    /// handles of Direct Selection's selected anchors, and the marquee.
    func drawPathSelectMarks(_ shape: PathShape) {
        let scale = magnification
        let side = Self.selectAnchorSize / scale
        let tip = Self.selectHandleSize / scale
        func box(_ point: CGPoint, _ size: CGFloat) -> CGRect {
            CGRect(x: point.x - size / 2, y: point.y - size / 2, width: size, height: size)
        }
        let selection = pathSelectSession.selection
        let dark = NSColor.black.withAlphaComponent(0.85)
        var open = PenDrawingRuns()
        var filled = PenDrawingRuns()
        var stems = PenDrawingRuns()
        var tips = PenDrawingRuns()
        for (subpath, path) in shape.subpaths.enumerated() {
            let whole = selection.subpaths.contains(subpath)
            for (index, anchor) in path.anchors.enumerated() {
                let picked = selection.anchors.contains(PathAnchorRef(subpath: subpath, index: index))
                if picked {
                    for handle in [anchor.handleIn, anchor.handleOut] where handle != anchor.point {
                        stems.add { run, _ in
                            run.move(to: anchor.point)
                            run.line(to: handle)
                        }
                        tips.add { run, _ in run.appendOval(in: box(handle, tip)) }
                    }
                }
                let square = box(anchor.point, side)
                guard needsToDraw(square.insetBy(dx: -side, dy: -side)) else { continue }
                if whole || picked {
                    filled.add { run, _ in run.appendRect(square) }
                } else {
                    open.add { run, _ in run.appendRect(square) }
                }
            }
        }
        open.cut()
        filled.cut()
        stems.cut()
        tips.cut()
        for (width, color) in [(3 / scale, dark), (1 / scale, NSColor.white)] {
            color.setStroke()
            for run in stems.finished {
                run.lineWidth = width
                run.stroke()
            }
        }
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
        if let marquee = pathSelectSession.drag?.marquee {
            let outline = NSBezierPath(rect: marquee)
            outline.lineWidth = 1 / scale
            NSColor.black.withAlphaComponent(0.45).setStroke()
            outline.stroke()
            outline.setLineDash([4 / scale, 3 / scale], count: 2, phase: 0)
            NSColor.white.withAlphaComponent(0.95).setStroke()
            outline.stroke()
        }
    }
}
