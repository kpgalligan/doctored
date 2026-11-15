import AppKit
import CoreGraphics

// "magnetic lasso input": the Magnetic Lasso's input half (the phase-6 ADR, "Magnetic Lasso
// input"), run off screen by handing mouse, key and flags events to the canvas view of an 8-bit
// picture: white with a black rectangle over pixels x 40–159, y 40–99 (its top side on y = 40).
// Every row runs and reports; the case fails at the end naming each failed row.
//
// - freehand: a ⌥-press then ⌥-drag ticks 6 px above the top side (inside Width) join the path
//   as given, the last an anchor, and a hover traces on from it; one ⌫ removes the segment.
// - wire kept: ⌥ pressed mid-drag fixes the wire traced so far, then joins the point.
// - starting press: the drag of the press that starts a ⌥ (Subtract) outline traces.
// - delete past the cap: ⌫ after a freehand segment too long to re-trace removes it anyway.
// - contrast keys: `.` `.` `,` step the stored Contrast 10 → 11 → 12 → 11, the open outline's
//   with it; 100 stays 100; with the Brush, `,` leaves Contrast alone.
// - precise cursor: Caps Lock on a still pointer shows a ring of diameter 2 × Width at it,
//   redraws it and hides the pointer; `]` grows it by 2; Caps Lock off removes it.
// - pressure: a tablet drag tick at pressure 0.9 traces at width 1 and keeps the wire on the
//   trail; pressure 0, a mouse tick, or 0.9 with Pen Pressure off snap it to the side.
// - one step: an outline with a freehand segment, closed by Return, is one "Magnetic Lasso" step
//   recorded as select_freehand, undone in one.

private struct LassoProbe {
    let document: ImageDocument
    let controller: EditorViewController
    var canvas: ImageCanvasView { controller.canvas }
    var session: MagneticLassoSession? { controller.canvas.magneticLasso }

    func event(_ type: NSEvent.EventType, _ point: CGPoint, _ flags: NSEvent.ModifierFlags = [],
               clicks: Int = 1, pressure: Float = 1) throws -> NSEvent {
        guard let window = canvas.window, let event = NSEvent.mouseEvent(
            with: type, location: canvas.convert(point, to: nil), modifierFlags: flags,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 1, clickCount: clicks, pressure: pressure) else {
            throw ProbeFailure(description: "no \(type) event at \(point)")
        }
        return event
    }

    func press(_ point: CGPoint, _ flags: NSEvent.ModifierFlags = []) throws {
        canvas.mouseDown(with: try event(.leftMouseDown, point, flags))
    }

    func drag(_ point: CGPoint, _ flags: NSEvent.ModifierFlags = []) throws {
        canvas.mouseDragged(with: try event(.leftMouseDragged, point, flags))
    }

    func release(_ point: CGPoint, _ flags: NSEvent.ModifierFlags = []) throws {
        canvas.mouseUp(with: try event(.leftMouseUp, point, flags))
    }

    func click(_ point: CGPoint, _ flags: NSEvent.ModifierFlags = []) throws {
        try press(point, flags)
        try release(point, flags)
    }

    func hover(_ point: CGPoint, _ flags: NSEvent.ModifierFlags = []) throws {
        canvas.mouseMoved(with: try event(.mouseMoved, point, flags))
    }

    func key(_ characters: String, _ code: UInt16) {
        guard let event = NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0, context: nil,
            characters: characters, charactersIgnoringModifiers: characters, isARepeat: false,
            keyCode: code) else { return }
        canvas.keyDown(with: event)
    }

    func flags(_ modifiers: NSEvent.ModifierFlags) throws {
        guard let window = canvas.window, let event = NSEvent.keyEvent(
            with: .flagsChanged, location: .zero, modifierFlags: modifiers,
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, characters: "", charactersIgnoringModifiers: "", isARepeat: false,
            keyCode: 57) else { throw ProbeFailure(description: "no flagsChanged event") }
        canvas.flagsChanged(with: event)
    }

    /// A drag tick carrying tablet pressure: a mouse event's CGEvent copy with the tablet
    /// subtype, as `scripts/native-paint-probe/main.swift` makes them. nil when the copy
    /// does not keep the subtype.
    func tabletDrag(_ point: CGPoint, pressure: Double) throws -> NSEvent? {
        let base = try event(.leftMouseDragged, point, pressure: Float(pressure))
        guard let copy = base.cgEvent?.copy() else { return nil }
        copy.setIntegerValueField(.mouseEventSubtype, value: 1)
        copy.setDoubleValueField(.mouseEventPressure, value: pressure)
        copy.setDoubleValueField(.tabletEventPointPressure, value: pressure)
        guard let tablet = NSEvent(cgEvent: copy), tablet.subtype == .tabletPoint else { return nil }
        return tablet
    }

    func escape() { key("\u{1b}", 53) }
}

/// The rectangle picture (or a flat white one), its window off screen, the Magnetic Lasso chosen.
private func lassoProbe(width: Int = 200, height: Int = 140, rectangle: Bool = true) throws
    -> LassoProbe {
    var bytes = [UInt8](repeating: 255, count: width * height * 4)
    if rectangle {
        for y in 40..<100 {
            for x in 40..<160 {
                let p = (y * width + x) * 4
                bytes[p] = 0; bytes[p + 1] = 0; bytes[p + 2] = 0
            }
        }
    }
    guard let image = RasterImage.from(rgba: bytes, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "Magnetic Lasso fixture allocation")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    let controller = try editor(document)
    controller.selectTool(.magneticLasso)
    try require(controller.currentTool == .magneticLasso, "the Magnetic Lasso is selected")
    // 100 %, so a press 10 px from the start is not "within 8 screen points" of it (a close).
    controller.canvas.enclosingScrollView?.magnification = 1
    return LassoProbe(document: document, controller: controller)
}

/// The rects `canvas` asks to redraw (`setNeedsDisplayInRect:`) while `body` runs: NSView's
/// method swapped, for the body alone, for a recorder that passes every call on.
private func lassoRedraws(of canvas: ImageCanvasView, during body: () throws -> Void) throws
    -> [NSRect] {
    let selector = NSSelectorFromString("setNeedsDisplayInRect:")
    guard let method = class_getInstanceMethod(NSView.self, selector) else {
        throw ProbeFailure(description: "NSView.setNeedsDisplayInRect: missing")
    }
    typealias Redraw = @convention(c) (AnyObject, Selector, NSRect) -> Void
    let original = method_getImplementation(method)
    let forward = unsafeBitCast(original, to: Redraw.self)
    var rects: [NSRect] = []
    let recorder: @convention(block) (AnyObject, NSRect) -> Void = { view, rect in
        if view === canvas { rects.append(rect) }
        forward(view, selector, rect)
    }
    let replacement = imp_implementationWithBlock(recorder)
    method_setImplementation(method, replacement)
    defer {
        method_setImplementation(method, original)
        imp_removeBlock(replacement)
    }
    try body()
    return rects
}

private func near(_ a: CGPoint, _ b: CGPoint, _ tolerance: CGFloat = 1e-6) -> Bool {
    abs(a.x - b.x) <= tolerance && abs(a.y - b.y) <= tolerance
}

func verifyMagneticLassoInput() throws {
    let settings = MagneticLassoSettings.shared
    let stored = (settings.options, settings.penPressure)
    defer { settings.options = stored.0; settings.penPressure = stored.1 }
    let standard = MagneticLassoOptions(width: 10, contrast: 10, frequency: 57)
    var failures: [String] = []
    var passes: [String] = []
    func row(_ name: String, _ body: () throws -> Void) {
        settings.options = standard
        settings.penPressure = true
        do {
            try body()
            passes.append(name)
        } catch {
            failures.append("\(name): \(error)")
        }
    }
    let start = CGPoint(x: 60.3, y: 40.2)

    row("freehand") {
        let probe = try lassoProbe()
        defer { probe.document.close() }
        try probe.click(start)
        try probe.hover(CGPoint(x: 66, y: 35))
        let p1 = CGPoint(x: 70, y: 34)
        let ticks = [CGPoint(x: 80, y: 34), CGPoint(x: 90, y: 34), CGPoint(x: 100, y: 34)]
        try probe.press(p1, .option)
        for tick in ticks { try probe.drag(tick, .option) }
        try probe.release(ticks[2], .option)
        guard let session = probe.session else { throw ProbeFailure(description: "no outline") }
        let fixed = session.fixed
        try require(Array(fixed.suffix(4)) == [p1] + ticks,
                    "the drag ticks are joined as given: \(fixed.suffix(6))")
        let at = fixed.count - 4
        try require(session.anchors.contains(at) && !session.anchors.contains(at + 1)
                    && !session.anchors.contains(at + 2) && session.anchors.last == fixed.count - 1,
                    "the press point and the last tick are anchors, the ticks between are not: "
                    + "\(session.anchors) of \(fixed.count)")
        try probe.hover(CGPoint(x: 110, y: 36))
        guard let traced = probe.session?.live, let end = traced.last else {
            throw ProbeFailure(description: "no live wire")
        }
        try require(traced[0] == ticks[2] && abs(end.y - 40) <= 1,
                    "a hover traces on from the segment's end to the side: \(traced)")
        probe.key("\u{7f}", 51)
        guard let after = probe.session else { throw ProbeFailure(description: "⌫ ended the outline") }
        try require(after.fixed.last == p1 && after.anchors.last == after.fixed.count - 1,
                    "one ⌫ removes the whole segment: \(after.fixed.suffix(3))")
    }

    row("wire kept") {
        let probe = try lassoProbe()
        defer { probe.document.close() }
        try probe.click(start)
        try probe.press(CGPoint(x: 70, y: 37))
        try probe.drag(CGPoint(x: 80, y: 36))
        try probe.drag(CGPoint(x: 90, y: 36))
        guard let wire = probe.session?.live, wire.count >= 2 else {
            throw ProbeFailure(description: "no traced wire before ⌥")
        }
        let tick = CGPoint(x: 100, y: 30)
        try probe.drag(tick, .option)
        try probe.release(tick, .option)
        guard let session = probe.session else { throw ProbeFailure(description: "no outline") }
        let expected = Array(wire.dropFirst()) + [tick]
        try require(Array(session.fixed.suffix(expected.count)) == expected,
                    "the wire is fixed as it stood, then the tick joined: \(session.fixed.suffix(6)) "
                    + "after the wire \(wire)")
        try require(session.anchors.contains(session.fixed.count - 2),
                    "the wire's end is an anchor: \(session.anchors)")
    }

    row("starting press") {
        let probe = try lassoProbe()
        defer { probe.document.close() }
        try probe.press(start, .option)
        try probe.drag(CGPoint(x: 70, y: 36), .option)
        try probe.drag(CGPoint(x: 80, y: 36), .option)
        guard let session = probe.session else { throw ProbeFailure(description: "no outline") }
        try require(session.mode == .subtract, "⌥ at the starting press is Subtract: \(session.mode)")
        try require(session.fixed.count == 1 && !session.freehandOpen && session.live.count >= 2,
                    "the starting press's ⌥-drag traces: fixed \(session.fixed), live \(session.live)")
        try probe.release(CGPoint(x: 80, y: 36), .option)
    }

    row("delete past the cap") {
        settings.options = MagneticLassoOptions(width: 256, contrast: 10, frequency: 57)
        let probe = try lassoProbe(width: 2400, height: 600, rectangle: false)
        defer { probe.document.close() }
        try probe.click(CGPoint(x: 100, y: 300))
        let p1 = CGPoint(x: 110, y: 300)
        try probe.press(p1, .option)
        for x in stride(from: 120, through: 2300, by: 20) {
            try probe.drag(CGPoint(x: CGFloat(x), y: 300), .option)
        }
        try probe.release(CGPoint(x: 2300, y: 300), .option)
        // Points come back through window coordinates, so compare within a thousandth.
        guard let before = probe.session, let end = before.fixed.last,
              near(end, CGPoint(x: 2300, y: 300), 1e-3) else {
            throw ProbeFailure(description: "the long segment was not joined: "
                + "\(String(describing: probe.session?.fixed.suffix(3)))")
        }
        probe.key("\u{7f}", 51)
        guard let after = probe.session, let anchor = after.fixed.last else {
            throw ProbeFailure(description: "⌫ ended the outline")
        }
        try require(near(anchor, p1, 1e-3) && after.live == [anchor],
                    "⌫ removes a segment it cannot re-trace and rests the wire on the anchor: "
                    + "\(after.fixed.suffix(2)), live \(after.live.prefix(3))")
    }

    row("contrast keys") {
        let probe = try lassoProbe()
        defer { probe.document.close() }
        try probe.click(start)
        var seen: [Double] = []
        for (characters, code) in [(".", UInt16(47)), (".", 47), (",", 43)] {
            probe.key(characters, code)
            seen.append(settings.options.contrast)
            try require(probe.session?.options.contrast == settings.options.contrast,
                        "the open outline takes the stored Contrast")
        }
        try require(seen == [11, 12, 11], "`.` `.` `,` step Contrast 10 → 11 → 12 → 11: \(seen)")
        probe.escape()
        settings.options.contrast = 100
        probe.key(".", 47)
        try require(settings.options.contrast == 100, "Contrast stops at 100")
        settings.options.contrast = 10
        probe.controller.selectTool(.brush)
        probe.key(",", 43)
        try require(settings.options.contrast == 10, "with the Brush, `,` leaves Contrast alone")
    }

    row("precise cursor") {
        let probe = try lassoProbe()
        defer { probe.document.close() }
        let canvas = probe.canvas
        let spot = CGPoint(x: 120, y: 20)
        try probe.hover(spot)
        try require(canvas.magneticLassoRing == nil, "no ring with Caps Lock off")
        let shown = try lassoRedraws(of: canvas) { try probe.flags([.capsLock]) }
        guard let ring = canvas.magneticLassoRing else {
            throw ProbeFailure(description: "Caps Lock on shows no ring (magnification "
                + "\(canvas.magnification))")
        }
        try require(near(ring.center, spot, 1e-3) && ring.diameter == 20,
                    "the ring is 2 × Width across at the pointer: \(ring.center) \(ring.diameter)")
        try require(shown.contains { $0.contains(ring.dirtyRect) },
                    "the ring is drawn under a still pointer: \(shown)")
        try require(canvas.brushOutlineCursor === ImageCanvasView.brushOutlineHiddenCursor,
                    "the pointer is hidden behind the ring")
        probe.key("]", 30)
        try require(canvas.magneticLassoRing?.diameter == 22, "`]` grows the ring by 2")
        let last = canvas.magneticLassoRing?.dirtyRect ?? .zero
        let hidden = try lassoRedraws(of: canvas) { try probe.flags([]) }
        try require(canvas.magneticLassoRing == nil && canvas.brushOutlineCursor == nil,
                    "Caps Lock off removes the ring and shows the pointer")
        try require(hidden.contains { $0.contains(last) }, "the old ring is drawn away: \(hidden)")
        settings.options.width = 1
        try probe.flags([.capsLock])
        let visible = 2 * canvas.magnification >= BrushOutline.minimumScreenDiameter
        try require((canvas.magneticLassoRing != nil) == visible,
                    "Width 1 draws a ring only when it is 6 screen points across "
                    + "(magnification \(canvas.magnification))")
        try probe.flags([])
    }

    row("pressure") {
        let probe = try lassoProbe()
        defer { probe.document.close() }
        // The trail runs 6 px above the top side: at Width 10 the wire snaps to the side, at
        // width 1 the corridor cannot reach it.
        let trailY: CGFloat = 34
        func wireEnd(_ pressure: Double?, penPressure: Bool = true) throws -> CGFloat? {
            settings.penPressure = penPressure
            try probe.click(CGPoint(x: 60, y: trailY))
            let point = CGPoint(x: 90, y: trailY)
            let event: NSEvent
            if let pressure {
                guard let tablet = try probe.tabletDrag(point, pressure: pressure) else { return nil }
                event = tablet
            } else {
                event = try probe.event(.leftMouseDragged, point)
            }
            probe.canvas.dragMagneticLasso(to: point, event: event)
            let end = probe.session?.live.last?.y
            probe.escape()
            return end
        }
        guard let pressed = try wireEnd(0.9) else {
            throw ProbeFailure(description: "SKIP: the synthesized drag kept no tablet subtype")
        }
        try require(abs(pressed - trailY) <= 1, "pressure 0.9 keeps the wire on the trail: \(pressed)")
        let light = try wireEnd(0)
        let mouse = try wireEnd(nil)
        let off = try wireEnd(0.9, penPressure: false)
        for (name, end) in [("pressure 0", light), ("a mouse", mouse), ("Pen Pressure off", off)] {
            try require(end.map { abs($0 - 40) <= 1 } == true, "\(name) snaps to the side: \(String(describing: end))")
        }
    }

    row("one step") {
        let probe = try lassoProbe()
        defer {
            if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
            probe.document.close()
        }
        let entries = probe.document.history.cursor
        let steps = try uiRecorded {
            try probe.click(start)
            try probe.hover(CGPoint(x: 100, y: 37))
            try probe.press(CGPoint(x: 120, y: 34), .option)
            try probe.drag(CGPoint(x: 140, y: 30), .option)
            try probe.release(CGPoint(x: 156, y: 37), .option)
            try probe.hover(CGPoint(x: 163, y: 70))
            try probe.click(CGPoint(x: 160, y: 100))
            try probe.hover(CGPoint(x: 100, y: 103))
            try probe.click(CGPoint(x: 40, y: 100))
            probe.key("\r", 36)
        }
        try require(probe.session == nil, "Return closes the outline")
        try require(probe.document.canvasSelection != nil, "the outline set a selection")
        try require(probe.document.history.cursor == entries + 1
                    && probe.document.undoManager?.undoActionName == "Magnetic Lasso",
                    "one \"Magnetic Lasso\" step: \(probe.document.history.entries.map(\.name))")
        try require(steps.count == 1 && steps[0].tool == "select_freehand",
                    "one recorded select_freehand step: \(steps.map(\.tool))")
        probe.document.undoManager?.undo()
        try require(probe.document.canvasSelection == nil && probe.document.undoManager?.canUndo != true,
                    "one undo clears it")
    }

    for name in passes { print("PASS [magnetic lasso input] \(name)") }
    guard failures.isEmpty else {
        throw ProbeFailure(description: "[magnetic lasso input] " + failures.joined(separator: " ;; "))
    }
    print("PASS: magnetic lasso input — ⌥-drag freehand segments (one ⌫ each, the wire kept, not on "
        + "the starting press, removable past the corridor cap), `,` / `.` Contrast, the Caps Lock "
        + "ring, pen pressure narrowing the trace width, and one recorded step")
}
