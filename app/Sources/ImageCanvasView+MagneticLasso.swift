import AppKit

// The canvas invalidates only the old ∪ new preview bounds; `CenteringClipView` forwards margin
// hover and clicks while an outline is open. The input rules (⌥-drag, `,` / `.`, pressure, the
// Caps Lock ring) are the phase-6 ADR's "Magnetic Lasso input".

/// The Magnetic Lasso on the canvas: the session lives in
/// `MagneticLassoSession`; this extension points it at events and draws it.
/// A click starts the outline (its modifiers pick the combine mode), hovering
/// or dragging traces the live wire, a click fastens an anchor (⌥: a straight
/// segment), a drag with ⌥ held draws a freehand segment (not the drag of
/// the starting press, whose ⌥ chose Subtract), and a double-click, a click
/// within 8 screen px of the start or Return closes it (⌥: straight back to
/// the start). ⌫ removes the last anchor and Escape abandons the outline.
/// Bare `[` / `]` step the Width and `,` / `.` the Contrast. A pen bearing
/// down narrows the trace width (Pen Pressure), and with Caps Lock on the
/// pointer is a ring of that width (`magneticLassoRing`).
extension ImageCanvasView {
    var hasMagneticLassoSession: Bool { magneticLasso != nil }

    func magneticLassoMouseDown(at point: CGPoint, event: NSEvent) {
        let straight = event.modifierFlags.contains(.option)
        guard let session = magneticLasso else {
            guard let capture = onSelectionGestureCapture?() else { NSSound.beep(); return }
            do {
                magneticLasso = try MagneticLassoSession(
                    capture: capture, start: point,
                    mode: SelectionCombineMode.from(event.modifierFlags, base: selectionCombineBase),
                    feather: selectionFeather, options: MagneticLassoSettings.shared.options)
            } catch {
                NSSound.beep()
                return
            }
            magneticLasso?.startingPressHeld = true
            invalidateMagneticLasso(from: nil)
            window?.invalidateCursorRects(for: self)
            return
        }
        let start = session.start
        let vertices = session.fixed.count + session.live.count - 1
        if event.clickCount >= 2
            || (hypot(point.x - start.x, point.y - start.y) * magnification < 8 && vertices >= 3) {
            closeMagneticLasso(straight: straight)
            return
        }
        let pressure = magneticLassoPressure(of: event)
        updateMagneticLasso { try $0.click(at: point, straight: straight, pressure: pressure) }
    }

    /// A hover tick: the point is clamped to the canvas and moves closer than
    /// half a screen pixel to the last one are coalesced. A refused trace
    /// beeps and keeps the previous live wire.
    func moveMagneticLasso(to raw: CGPoint) {
        let point = clampedMagneticLassoPoint(raw)
        let minimum = 0.5 / magnification
        updateMagneticLasso { try $0.move(to: point, minimumDistance: minimum) }
    }

    /// A drag tick: with ⌥ held (outside the starting press) the point is
    /// joined as given, a freehand segment; otherwise it traces like a hover,
    /// at the event's pen pressure. The precise cursor's ring follows it.
    func dragMagneticLasso(to raw: CGPoint, event: NSEvent) {
        let point = clampedMagneticLassoPoint(raw)
        setHoverPoint(point)
        guard let session = magneticLasso else { return }
        let minimum = 0.5 / magnification
        if event.modifierFlags.contains(.option), !session.startingPressHeld {
            updateMagneticLasso { $0.freehand(to: point, minimumDistance: minimum) }
        } else {
            let pressure = magneticLassoPressure(of: event)
            updateMagneticLasso { try $0.move(to: point, minimumDistance: minimum, pressure: pressure) }
        }
    }

    /// Mouse-up: the starting press is over, and an open freehand segment
    /// ends, its last point an anchor.
    func magneticLassoMouseUp() {
        guard magneticLasso != nil else { return }
        magneticLasso?.startingPressHeld = false
        updateMagneticLasso { $0.endFreehand() }
    }

    /// The tool's keys; true when the key was consumed.
    func handleMagneticLassoKey(_ event: NSEvent) -> Bool {
        let bare = event.modifierFlags.intersection([.command, .control, .option]).isEmpty
        if tool == .magneticLasso, bare, !hasActiveTextSession,
           let key = event.charactersIgnoringModifiers {
            let options = magneticLasso?.options ?? MagneticLassoSettings.shared.options
            switch key {
            case "[", "]":
                let range = MagneticLassoOptions.widthRange
                onMagneticLassoWidthKey?(min(max(options.width + (key == "]" ? 1 : -1),
                                                 range.lowerBound), range.upperBound))
                return true
            case ",", ".":
                let range = MagneticLassoOptions.contrastRange
                onMagneticLassoContrastKey?(min(max(options.contrast + (key == "." ? 1 : -1),
                                                    range.lowerBound), range.upperBound))
                return true
            default:
                break
            }
        }
        guard magneticLasso != nil else { return false }
        switch event.keyCode {
        case 53: // Escape
            cancelMagneticLasso()
        case 51 where bare, 117 where bare: // Delete, forward delete
            var onlyTheStart = false
            updateMagneticLasso { onlyTheStart = !$0.removeLastAnchor() }
            if onlyTheStart { cancelMagneticLasso() }
        case 36, 76: // Return, keypad Enter
            closeMagneticLasso(straight: event.modifierFlags.contains(.option))
        default:
            return false
        }
        return true
    }

    /// Abandons the outline (Escape, a tool switch, Quick Mask, a stale
    /// capture); true when there was one.
    @discardableResult
    func cancelMagneticLasso(beep: Bool = false) -> Bool {
        guard let session = magneticLasso else { return false }
        let ring = brushOutlineDirtyRect()
        magneticLasso = nil
        invalidateMagneticLasso(from: session.previewBounds)
        pointerOutlineDidChange(from: ring)
        window?.invalidateCursorRects(for: self)
        if beep { NSSound.beep() }
        return true
    }

    /// The fixed path and the live wire as one dashed polyline, with a 4-px
    /// square at every anchor.
    func drawMagneticLassoPreview() {
        guard let session = magneticLasso else { return }
        let points = session.fixed + session.live.dropFirst()
        let path = NSBezierPath()
        path.move(to: points[0])
        for point in points.dropFirst() { path.line(to: point) }
        strokeMarquee(path)
        let side = 4 / magnification
        DS.marquee.setFill()
        for index in session.anchors {
            let anchor = session.fixed[index]
            NSBezierPath(rect: CGRect(x: anchor.x - side / 2, y: anchor.y - side / 2,
                                      width: side, height: side)).fill()
        }
    }

    /// The precise cursor: with Caps Lock on and the pointer over the canvas,
    /// a ring of radius the current trace width (the corridor the tracer
    /// searches) round the pointer; nil below `BrushOutline`'s 6 screen points.
    var magneticLassoRing: BrushOutline? {
        magneticLassoRing(capsLock: selectionCursorModifiers.contains(.capsLock))
    }

    /// The ring under a given Caps Lock state (the one drawn before a
    /// modifier change, for its redraw).
    func magneticLassoRing(capsLock: Bool) -> BrushOutline? {
        guard tool == .magneticLasso, capsLock, let hoverPoint, !isTransforming else { return nil }
        return BrushOutline(center: hoverPoint, diameter: 2 * magneticLassoTraceWidth,
                            angleDegrees: 0, roundness: 1, magnification: magnification)
    }

    /// The open outline's trace width, else the Width set.
    var magneticLassoTraceWidth: CGFloat {
        CGFloat(magneticLasso?.traceWidth ?? MagneticLassoSettings.shared.options.width)
    }

    /// The event's pen pressure for the trace width: nil for a mouse, or with
    /// Pen Pressure off.
    private func magneticLassoPressure(of event: NSEvent) -> CGFloat? {
        MagneticLassoSettings.shared.penPressure ? Self.tabletPressure(of: event) : nil
    }

    private func clampedMagneticLassoPoint(_ raw: CGPoint) -> CGPoint {
        CGPoint(x: min(max(0, raw.x), bounds.width), y: min(max(0, raw.y), bounds.height))
    }

    /// Runs one change on the open outline and redraws what it moved: the
    /// old and new preview bounds and the precise cursor's ring (pressure
    /// changes the trace width). A refused trace beeps and keeps the outline.
    private func updateMagneticLasso(_ change: (inout MagneticLassoSession) throws -> Void) {
        guard var session = magneticLasso else { return }
        let before = session.previewBounds
        let ring = brushOutlineDirtyRect()
        do { try change(&session) } catch {
            NSSound.beep()
            return
        }
        magneticLasso = session
        invalidateMagneticLasso(from: before)
        pointerOutlineDidChange(from: ring)
    }

    /// Closes the outline; the session is cleared before the commit fires.
    /// A refused outline (too many or too few points) beeps and stays open.
    private func closeMagneticLasso(straight: Bool) {
        guard let session = magneticLasso else { return }
        let outline: [CGPoint]
        do { outline = try session.closedOutline(straight: straight) } catch {
            NSSound.beep()
            return
        }
        cancelMagneticLasso()
        onMagneticLassoCommit?(session, outline)
    }

    /// Redraws only the old and new preview bounds, outset by the anchor
    /// squares and the dashed line — never the whole canvas per pointer move.
    private func invalidateMagneticLasso(from before: CGRect?) {
        var box = magneticLasso?.previewBounds ?? before ?? .zero
        if let before { box = box.union(before) }
        let margin = 6 / magnification
        setNeedsDisplay(box.insetBy(dx: -margin, dy: -margin))
    }
}
