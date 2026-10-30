import AppKit

/// View-only feedback for selection gestures. The committed selection stays
/// visible while a new rectangle or ellipse is proposed by a drag.
extension ImageCanvasView {
    /// Flags events go to the window's first responder, which may be a
    /// selection option field while the pointer remains over the canvas.
    func updateSelectionModifierObservation() {
        if let selectionModifierEventMonitor {
            NSEvent.removeMonitor(selectionModifierEventMonitor)
            self.selectionModifierEventMonitor = nil
        }
        guard window != nil else { return }
        selectionModifierEventMonitor = NSEvent.addLocalMonitorForEvents(
            matching: .flagsChanged
        ) { [weak self] event in
            guard let self, let window = self.window,
                  event.window === window, self.selectionCursorHovered else { return event }
            self.selectionCursorModifiers = event.modifierFlags
            return event
        }
    }

    func drawSelectionMarqueePreview() {
        guard let rect = marqueePreview, rect.width > 0, rect.height > 0 else { return }
        let path = tool == .ellipseSelect
            ? NSBezierPath(ovalIn: rect) : NSBezierPath(rect: rect)
        let scale = magnification
        path.lineWidth = 1.5 / scale
        let dash: [CGFloat] = [3 / scale, 3 / scale]
        path.setLineDash(dash, count: dash.count, phase: 0)
        NSColor.white.withAlphaComponent(0.9).setStroke()
        path.stroke()
        path.lineWidth = 0.75 / scale
        DS.marquee.setStroke()
        path.stroke()
    }

    /// The cursor describes the next selection's effective combine mode. A
    /// gesture already in progress keeps the mode it captured at mouse-down.
    func selectionFeedbackCursor(latchedMode: SelectionCombineMode?) -> NSCursor {
        switch tool {
        case .select, .ellipseSelect, .lasso, .freehandLasso, .magneticLasso, .quickSelect,
             .objectSelect, .wand, .subject:
            break
        case .move:
            // ⌥ over the Move tool: the next drag duplicates.
            return selectionCursorModifiers.contains(.option) ? .dragCopy : tool.cursor
        default:
            return tool.cursor
        }
        let mode = latchedMode
            ?? SelectionCombineMode.from(selectionCursorModifiers, base: selectionCombineBase)
        switch mode {
        case .replace: return .crosshair
        case .add: return SelectionFeedbackCursors.add
        case .subtract: return SelectionFeedbackCursors.subtract
        case .intersect: return SelectionFeedbackCursors.intersect
        }
    }
}

/// Stable instances keep cursor-rect invalidation cheap. The hotspot stays
/// at the crosshair's centre; the operation badge sits away from the pixel
/// under the pointer.
private enum SelectionFeedbackCursors {
    static let add = make(.add)
    static let subtract = make(.subtract)
    static let intersect = make(.intersect)

    private static func make(_ mode: SelectionCombineMode) -> NSCursor {
        let image = NSImage(size: NSSize(width: 24, height: 24), flipped: true) { _ in
            let cross = NSBezierPath()
            cross.move(to: NSPoint(x: 8, y: 1))
            cross.line(to: NSPoint(x: 8, y: 15))
            cross.move(to: NSPoint(x: 1, y: 8))
            cross.line(to: NSPoint(x: 15, y: 8))
            cross.lineCapStyle = .round
            cross.lineWidth = 3
            NSColor.white.setStroke()
            cross.stroke()
            cross.lineWidth = 1.25
            NSColor.black.setStroke()
            cross.stroke()

            let badge = NSBezierPath(roundedRect: NSRect(x: 14, y: 14, width: 9, height: 9),
                                     xRadius: 2, yRadius: 2)
            NSColor.white.setFill()
            badge.fill()
            badge.lineWidth = 1
            NSColor.black.setStroke()
            badge.stroke()

            let mark = NSBezierPath()
            mark.lineWidth = 1.4
            mark.lineCapStyle = .round
            switch mode {
            case .add, .subtract:
                mark.move(to: NSPoint(x: 16, y: 18.5))
                mark.line(to: NSPoint(x: 21, y: 18.5))
                if mode == .add {
                    mark.move(to: NSPoint(x: 18.5, y: 16))
                    mark.line(to: NSPoint(x: 18.5, y: 21))
                }
            case .intersect:
                // Two overlapping corners for the Shift+Option intersection.
                mark.move(to: NSPoint(x: 16, y: 19))
                mark.line(to: NSPoint(x: 16, y: 16))
                mark.line(to: NSPoint(x: 19, y: 16))
                mark.move(to: NSPoint(x: 18, y: 21))
                mark.line(to: NSPoint(x: 21, y: 21))
                mark.line(to: NSPoint(x: 21, y: 18))
            case .replace:
                break
            }
            mark.stroke()
            return true
        }
        return NSCursor(image: image, hotSpot: NSPoint(x: 8, y: 8))
    }
}
