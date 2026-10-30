import AppKit

// The canvas invalidates only the old ∪ new preview bounds; `CenteringClipView` forwards margin
// hover and clicks while an outline is open.

/// The Magnetic Lasso on the canvas: the session lives in
/// `MagneticLassoSession`; this extension points it at events and draws it.
/// A click starts the outline (its modifiers pick the combine mode), hovering
/// traces the live wire, a click fastens an anchor (⌥: a straight segment),
/// and a double-click, a click within 8 screen px of the start or Return
/// closes it (⌥: straight back to the start). ⌫ removes the last anchor and
/// Escape abandons the outline. Bare `[` / `]` step the Width.
extension ImageCanvasView {
    var hasMagneticLassoSession: Bool { magneticLasso != nil }

    func magneticLassoMouseDown(at point: CGPoint, event: NSEvent) {
        let straight = event.modifierFlags.contains(.option)
        guard var session = magneticLasso else {
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
        let before = session.previewBounds
        do { try session.click(at: point, straight: straight) } catch { NSSound.beep(); return }
        magneticLasso = session
        invalidateMagneticLasso(from: before)
    }

    /// A hover or drag tick: the point is clamped to the canvas and moves
    /// closer than half a screen pixel to the last one are coalesced. A
    /// refused trace beeps and keeps the previous live wire.
    func moveMagneticLasso(to raw: CGPoint) {
        guard var session = magneticLasso else { return }
        let point = CGPoint(x: min(max(0, raw.x), bounds.width),
                            y: min(max(0, raw.y), bounds.height))
        let before = session.previewBounds
        do {
            try session.move(to: point, minimumDistance: 0.5 / magnification)
        } catch {
            NSSound.beep()
            return
        }
        magneticLasso = session
        invalidateMagneticLasso(from: before)
    }

    /// The tool's keys; true when the key was consumed.
    func handleMagneticLassoKey(_ event: NSEvent) -> Bool {
        let bare = event.modifierFlags.intersection([.command, .control, .option]).isEmpty
        if tool == .magneticLasso, bare, !hasActiveTextSession,
           let key = event.charactersIgnoringModifiers, key == "[" || key == "]" {
            let width = magneticLasso?.options.width ?? MagneticLassoSettings.shared.options.width
            let range = MagneticLassoOptions.widthRange
            onMagneticLassoWidthKey?(min(max(width + (key == "]" ? 1 : -1), range.lowerBound),
                                         range.upperBound))
            return true
        }
        guard var session = magneticLasso else { return false }
        switch event.keyCode {
        case 53: // Escape
            cancelMagneticLasso()
        case 51 where bare, 117 where bare: // Delete, forward delete
            let before = session.previewBounds
            do {
                guard try session.removeLastAnchor() else {
                    cancelMagneticLasso()
                    return true
                }
            } catch {
                NSSound.beep()
                return true
            }
            magneticLasso = session
            invalidateMagneticLasso(from: before)
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
        magneticLasso = nil
        invalidateMagneticLasso(from: session.previewBounds)
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
