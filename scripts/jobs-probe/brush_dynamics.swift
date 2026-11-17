import AppKit

// Brush dynamics (`designs/brush-dynamics.md`), off screen: the canvas cases hand events or
// `beginStroke` / `continueStroke` / `endStroke` to the view directly, so none needs a key
// window or takes the focus.
//
// (a) The stored options: only the Brush, Pencil and Eraser keep dynamics (Dodge's are pinned
//     nil whatever is written), a default-valued set is stored nil, Pen Tilt is stored while
//     the pressure checkbox's Off / Pen Pressure is not, and a preset with dynamics sets them on
//     the Brush, a built-in clears them, and only a dynamics tool lists and matches it.
// (b) A session with dynamics lays the dabs the core resolved and returns them as recorded.
// (c) Synthesized tablet events carrying tilt reach the canvas's pen state; with size on tilt a
//     drag at tilt 0.5 lays dabs of half the brush size, and a drag whose tilt changes between
//     two events lays diameters that ramp through the values between, per dab.
// (d) 8 bits: every pixel a scattered Pencil drag's preview painted holds at mouse-up; 16 bits: a
//     scattered Brush stroke's job completes. One history entry; one undo restores the base.
// (e) The tip shape: a shaped session records one (angle, roundness) per laid dab and hands them
//     to its stroke; an unshaped one records none. Tablet events carrying a barrel rotation reach
//     the pen state turned by the view rotation (tilt too), and a Brush drag with the angle on
//     rotation lays dabs at the tip's angle plus the pen's. A shaped Brush drag commits at 8 and 16
//     bits with one shape per dab, and at 8 bits every pixel its preview painted with full ink
//     holds at mouse-up (a preview drawing the tip's own shape would paint pixels the commit
//     does not).
// (f) Pencil shape pinning: a preset whose dynamics hold a tip shape, applied on the Pencil,
//     leaves `dynamics.shape` nil, is absent from the Pencil's list and its `,` `.` cycle, and a
//     Pencil canvas stroke still commits.
// (g) The texture: the Brush and the Eraser keep it, the Pencil's options drop it (keeping the
//     rest) and a Pencil stroke's session has none; depth 0 is stored as none; a textured preset
//     round-trips at schema 2, echoes its pattern without pixels and is absent from the Pencil's
//     list. The Dynamics… sheet shows the Texture, Scale and Depth rows with the options' values
//     for the Brush, and none for the Pencil. A textured Brush canvas stroke at 8 and 16 bits
//     records a `native_stroke` holding the texture, and its commit equals the replay of that
//     recorded step and differs from the replay without the texture.
// (h) The dual brush: `brush_dual.swift`'s `verifyBrushDual`.
// (i) Colour dynamics: `brush_colour.swift`'s `verifyBrushColour`.

func verifyBrushDynamics() throws {
    try verifyDynamicsOptions()
    try verifyDynamicsSession()
    try verifyDynamicsTilt()
    try verifyDynamicsCanvas(sixteen: false)
    try verifyDynamicsCanvas(sixteen: true)
    try verifyShapeSession()
    try verifyPenRotation()
    try verifyShapedCanvas(sixteen: false)
    try verifyShapedCanvas(sixteen: true)
    try verifyPencilShapePinning()
    try verifyTextureOptions()
    try verifyTextureSheet()
    try verifyTexturedCanvas(sixteen: false)
    try verifyTexturedCanvas(sixteen: true)
    try verifyBrushDual()
    try verifyBrushColour()
    print("PASS: brush dynamics' options, presets, session, tablet tilt and rotation, canvas "
          + "strokes, the tip shape, the Pencil's shape pinning and the texture")
}

private let shaped: BrushDynamics = {
    var d = BrushDynamics()
    d.shape = BrushDynamics.TipShape(angleControl: .direction, angleJitter: 0.5,
                                     roundnessControl: .off, roundnessMinimum: 0,
                                     roundnessJitter: 0.6)
    return d
}()

private let scatter: BrushDynamics = {
    var d = BrushDynamics()
    d.scatter = 1
    d.count = 2
    return d
}()

private func verifyDynamicsOptions() throws {
    let tag = "[dynamics options]"
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.dodge, store.pencil)
    defer { (store.brush, store.dodge, store.pencil) = saved }
    var options = PaintToolOptions()
    options.dynamics = scatter
    store.setPaintOptions(options, for: .brush)
    store.setPaintOptions(options, for: .pencil)
    store.setPaintOptions(options, for: .dodge)
    try require(store.paintOptions(for: .brush)?.dynamics == scatter
                && store.paintOptions(for: .pencil)?.dynamics == scatter,
                "\(tag) the Brush and the Pencil keep dynamics")
    try require(store.dodge.dynamics == nil && store.paintOptions(for: .dodge)?.dynamics == nil,
                "\(tag) Dodge's dynamics are pinned nil")
    options.dynamics = BrushDynamics()
    store.setPaintOptions(options, for: .brush)
    try require(store.brush.dynamics == nil, "\(tag) default-valued dynamics are stored nil")
    var controls = PaintToolOptions()
    controls.pressureSize = true
    controls.sizeControl = .tilt
    try require(controls.dynamics?.size.control == .tilt && controls.sizeControl == .tilt,
                "\(tag) Pen Tilt is stored")
    controls.sizeControl = .off
    try require(controls.dynamics == nil && !controls.pressureSize && controls.sizeControl == .off,
                "\(tag) Off clears the stored control and the checkbox")
    controls.opacityControl = .pressure
    try require(controls.pressureOpacity == true && controls.dynamics == nil,
                "\(tag) Pen Pressure is the checkbox")
    // Presets: a user preset with dynamics beside the built-ins, passed as the list.
    let mine = BrushPreset(name: "Scatter", hardness: 100, flow: 100, spacing: 25, angle: 0,
                           roundness: 100, airbrush: false, dynamics: scatter)
    let presets = BrushPreset.builtIns + [mine]
    let applied = mine.applied(to: PaintToolOptions())
    try require(applied.dynamics == scatter
                && BrushPreset.matchIndex(of: applied, in: presets) == presets.count,
                "\(tag) a dynamics preset sets them and matches on the Brush")
    let cleared = BrushPreset.builtIns[0].applied(to: applied)
    try require(cleared.dynamics == nil && BrushPreset.matchIndex(of: cleared, in: presets) == 1,
                "\(tag) a built-in clears them")
    try require(!BrushPreset.all(dynamics: false).contains { $0.dynamics != nil },
                "\(tag) a tool without dynamics lists no preset holding them")
    try require(mine.repeats(BrushPreset(name: "Twin", hardness: 100, flow: 100, spacing: 25,
                                         angle: 0, roundness: 100, airbrush: false, dynamics: scatter))
                && !mine.repeats(BrushPreset.builtIns[0]),
                "\(tag) a twin repeats; Hard Round's tip without dynamics does not")
    let decoded = try BrushPreset.decode(mine.json)
    try require(mine.json["schema_version"] as? Int == 2 && decoded.dynamics == scatter
                && BrushPreset.builtIns[0].json["schema_version"] as? Int == 1,
                "\(tag) a preset with dynamics round-trips at schema 2; one without stays at 1")
}

private func verifyDynamicsSession() throws {
    let tag = "[dynamics session]"
    var dynamics = scatter
    dynamics.count = 3
    let handle = try BrushDynamics.handle(dynamics, size: 20, seed: 9, pressureSize: false,
                                          pressureOpacity: false, tip: BrushTip(hardness: 1))
    var session = NativeBrushStrokeSession(tip: BrushTip(hardness: 1), size: 20, pressureOpacity: false)
    session.dynamics = handle
    let first = session.stamp(at: CGPoint(x: 40, y: 30), pen: PenSample())
    let second = session.stamp(at: CGPoint(x: 45, y: 30), pen: PenSample())
    let laid: [Double] = (first + second).flatMap { dab -> [Double] in
        [Double(dab.point.x), Double(dab.point.y), Double(dab.diameter), 1]
    }
    try require(first.count == 3 && second.count == 3 && session.dabs == laid,
                "\(tag) each stamp lays the 3 resolved dabs and returns them: \(session.dabs)")
    // The second stamp travels along +x, so its scatter is across it: y moves, x does not.
    try require(second.allSatisfy { $0.point.x == 45 } && second.contains { $0.point.y != 30 },
                "\(tag) scatter runs across the direction of travel: \(second)")
}

/// A mouse event's CGEvent copy with the tablet subtype, pressure, tilt and barrel rotation.
private func tabletEvent(_ canvas: ImageCanvasView, _ type: NSEvent.EventType, _ point: CGPoint,
                         tilt: CGVector, rotation: Double = 0) throws -> NSEvent {
    guard let window = canvas.window, let base = NSEvent.mouseEvent(
        with: type, location: canvas.convert(point, to: nil), modifierFlags: [],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, eventNumber: 1, clickCount: 1, pressure: 1),
        let copy = base.cgEvent?.copy() else {
        throw ProbeFailure(description: "no \(type) event")
    }
    copy.setIntegerValueField(.mouseEventSubtype, value: 1)
    copy.setDoubleValueField(.mouseEventPressure, value: 1)
    copy.setDoubleValueField(.tabletEventPointPressure, value: 1)
    copy.setDoubleValueField(.tabletEventTiltX, value: Double(tilt.dx))
    copy.setDoubleValueField(.tabletEventTiltY, value: Double(tilt.dy))
    copy.setDoubleValueField(.tabletEventRotation, value: rotation)
    guard let tablet = NSEvent(cgEvent: copy), tablet.subtype == .tabletPoint else {
        throw ProbeFailure(description: "the CGEvent copy dropped the tablet subtype")
    }
    return tablet
}

private func verifyDynamicsTilt() throws {
    let tag = "[dynamics tilt]"
    let document = try fixture(width: 200, height: 120)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    canvas.enclosingScrollView?.magnification = 1
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved }
    var options = PaintToolOptions(size: 40, hardness: 0, smoothing: 0)
    options.pressureSize = false
    options.pressureOpacity = false
    options.sizeControl = .tilt
    store.setPaintOptions(options, for: .brush)
    controller.selectTool(.brush)
    try require(canvas.brushDynamics?.size.control == .tilt, "\(tag) the canvas holds Pen Tilt")
    // The event's tilt field is fixed point, so the canvas reads 0.5 back as 0.49998…; the
    // steady case compares with the tilt the canvas latched.
    var latched = CGVector.zero
    func diameters(_ tilts: [CGVector]) throws -> [Double] {
        let points = (0..<tilts.count).map { CGPoint(x: 30 + 40 * CGFloat($0), y: 60) }
        canvas.mouseDown(with: try tabletEvent(canvas, .leftMouseDown, points[0], tilt: tilts[0]))
        try require(canvas.strokeActive, "\(tag) the stroke began")
        let tilt = canvas.strokePen.eventTilt
        latched = tilt
        try require(abs(hypot(tilt.dx, tilt.dy) - hypot(tilts[0].dx, tilts[0].dy)) < 1e-3,
                    "\(tag) the event's tilt reached the canvas: \(tilt)")
        for (point, tilt) in zip(points, tilts).dropFirst() {
            canvas.mouseDragged(with: try tabletEvent(canvas, .leftMouseDragged, point, tilt: tilt))
        }
        // The spline runs one sample behind the cursor; mouse-up flushes the tail.
        let dabs = canvas.nativeStrokeSession?.dabs ?? []
        canvas.cancelStroke()
        return stride(from: 2, to: dabs.count, by: 4).map { dabs[$0] }
    }
    let steady = try diameters(Array(repeating: CGVector(dx: 0, dy: 0.5), count: 4))
    let half = 40 * Double(hypot(latched.dx, latched.dy))
    try require(steady.count > 4 && abs(half - 20) < 1e-3
                && steady.allSatisfy { abs($0 - half) < 1e-6 },
                "\(tag) tilt 0.5 lays half-size dabs: \(steady.prefix(8)) against \(half)")
    let ramp = try diameters([CGVector(dx: 0, dy: 0.2), CGVector(dx: 0, dy: 0.2),
                              CGVector(dx: 0, dy: 0.8), CGVector(dx: 0, dy: 0.8)])
    // A margin past the fixed point's error, so an endpoint never counts as between. The
    // 40 px span's tilt runs linearly, so at 25 % spacing no dab steps more than ~5 px; a
    // tilt that jumps to the next event's value at the span's first vertex steps ~16.
    let between = ramp.filter { $0 > 8 + 1e-2 && $0 < 32 - 1e-2 }
    let largestStep = zip(ramp, ramp.dropFirst()).map { $1 - $0 }.max() ?? 0
    try require(zip(ramp, ramp.dropFirst()).allSatisfy { $0 <= $1 + 1e-9 } && between.count >= 3
                && largestStep < 8,
                "\(tag) a tilt change ramps per dab, not per event (largest step "
                + "\(largestStep)): \(ramp)")
}

private func verifyDynamicsCanvas(sixteen: Bool) throws {
    let tag = sixteen ? "[dynamics canvas 16-bit]" : "[dynamics canvas 8-bit]"
    let document = try sixteen ? precisionFixture(width: 80, height: 60) : fixture(width: 80, height: 60)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base document") }
    let tool: EditorTool = sixteen ? .brush : .pencil
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.pencil)
    defer { (store.brush, store.pencil) = saved }
    var options = PaintToolOptions(size: 7, smoothing: 0)
    options.pressureSize = false
    options.dynamics = scatter
    store.setPaintOptions(options, for: tool)
    controller.selectTool(tool)
    canvas.paintColor = NSColor(srgbRed: 0.85, green: 0.15, blue: 0.3, alpha: 1)
    canvas.brushOpacity = 1
    let history = document.history.entries.count
    canvas.beginStroke(at: CGPoint(x: 12.3, y: 20.7))
    try require(canvas.nativeStrokeSession?.dynamics != nil, "\(tag) the session has dynamics")
    for point in [CGPoint(x: 30.2, y: 24.1), CGPoint(x: 48.6, y: 31.9), CGPoint(x: 66.1, y: 38.4)] {
        canvas.continueStroke(to: point, pressure: nil)
    }
    let preview = sixteen ? nil : document.doc?.layerPixels(0)
    canvas.endStroke()
    if sixteen {
        guard let job = DocumentJobs.active else {
            throw ProbeFailure(description: "\(tag) the stroke started no job")
        }
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "\(tag) the stroke's job completed: \(job.report)")
    }
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "\(tag) the stroke published nothing")
    }
    try require(document.history.entries.count == history + 1, "\(tag) one history entry")
    if !sixteen {
        guard let before = base.layerPixels(0), let after = painted.layerPixels(0), let preview else {
            throw ProbeFailure(description: "\(tag) no layer pixels")
        }
        let pixels = before.count / 4
        func px(_ a: [UInt8], _ i: Int) -> ArraySlice<UInt8> { a[i * 4..<i * 4 + 4] }
        let changed = (0..<pixels).filter { px(before, $0) != px(after, $0) }
        let previewed = (0..<pixels).filter { px(before, $0) != px(preview, $0) }
        // Mouse-up lays the spline's tail span too, so the commit may paint more than the
        // preview did; every pixel the preview painted must hold.
        let shifted = previewed.filter { px(preview, $0) != px(after, $0) }
        try require(changed.count > 150 && previewed.count > 100 && shifted.isEmpty,
                    "\(tag) the preview's \(previewed.count) pixels hold at mouse-up; "
                    + "\(shifted.count) moved")
    }
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the base")
}

/// The angles of a shapes list (every other value from the first).
private func angles(_ shapes: [Double]) -> [Double] {
    stride(from: 0, to: shapes.count, by: 2).map { shapes[$0] }
}

private func verifyShapeSession() throws {
    let tag = "[shape session]"
    let tip = BrushTip(hardness: 1, angleDegrees: 20, roundness: 0.5)
    var dynamics = scatter
    dynamics.shape = shaped.shape
    var session = NativeBrushStrokeSession(tip: tip, size: 20, pressureOpacity: false)
    session.dynamics = try BrushDynamics.handle(dynamics, size: 20, seed: 3, pressureSize: false,
                                                pressureOpacity: false, tip: tip)
    session.dynamicsShaped = true
    let laid = session.stamp(at: CGPoint(x: 30, y: 30), pen: PenSample())
        + session.stamp(at: CGPoint(x: 30, y: 42), pen: PenSample())
    let recorded = laid.flatMap { [Double($0.angleDegrees), Double($0.roundness)] }
    try require(session.dabs.count == 16 && session.shapes == recorded,
                "\(tag) one shape per laid dab, as returned: \(session.shapes)")
    try require(Set(angles(session.shapes)).count > 1
                && session.shapes.enumerated().allSatisfy { $0.offset % 2 == 0 || $0.element <= 0.5 },
                "\(tag) the angle jitters and the roundness only shrinks: \(session.shapes)")
    let stroke = try session.resolved()
    let back = try NativeBrushStroke(canonical: stroke.canonical)
    try require(stroke.shapes == session.shapes && back.shapes == session.shapes
                && back.tip(forDab: 3).angleDegrees == CGFloat(session.shapes[6])
                && back.tip(forDab: 3).roundness == CGFloat(session.shapes[7]),
                "\(tag) the stroke and its canonical form carry the shapes")
    var plain = NativeBrushStrokeSession(tip: tip, size: 20, pressureOpacity: false)
    plain.dynamics = try BrushDynamics.handle(scatter, size: 20, seed: 3, pressureSize: false,
                                              pressureOpacity: false, tip: tip)
    plain.stamp(at: CGPoint(x: 30, y: 30), pen: PenSample())
    let plainStroke = try plain.resolved()
    let plainCanonical = try plainStroke.canonical
    try require(plain.shapes.isEmpty && plainStroke.shapes == nil && plainCanonical["shapes"] == nil
                && Array(plain.dabs) == Array(session.dabs.prefix(8)),
                "\(tag) without a tip shape no shapes are recorded and the dabs are the same")
}

private func verifyPenRotation() throws {
    let tag = "[pen rotation]"
    let document = try fixture(width: 200, height: 120)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    canvas.enclosingScrollView?.magnification = 1
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved; controller.setViewRotation(0) }
    var options = PaintToolOptions(size: 30, hardness: 100, smoothing: 0)
    options.pressureSize = false
    options.pressureOpacity = false
    options.angle = 10
    options.roundness = 40
    var dynamics = BrushDynamics()
    dynamics.shape = BrushDynamics.TipShape(angleControl: .rotation)
    options.dynamics = dynamics
    store.setPaintOptions(options, for: .brush)
    controller.selectTool(.brush)
    // The event's rotation and tilt reach the pen state, turned by the view rotation.
    for view in [0.0, 90.0] {
        controller.setViewRotation(view)
        canvas.mouseDown(with: try tabletEvent(canvas, .leftMouseDown, CGPoint(x: 40, y: 60),
                                               tilt: CGVector(dx: 0.5, dy: 0), rotation: 30))
        let pen = canvas.strokePen
        canvas.cancelStroke()
        let radians = view * .pi / 180
        try require(abs(Double(pen.eventRotation) - (30 + view)) < 0.01
                    && abs(Double(pen.eventTilt.dx) - 0.5 * cos(radians)) < 1e-3
                    && abs(Double(pen.eventTilt.dy) - 0.5 * sin(radians)) < 1e-3,
                    "\(tag) at view rotation \(view) the pen reads rotation \(pen.eventRotation) "
                    + "and tilt \(pen.eventTilt)")
    }
    controller.setViewRotation(0)
    let points = (0..<4).map { CGPoint(x: 30 + 40 * CGFloat($0), y: 60) }
    canvas.mouseDown(with: try tabletEvent(canvas, .leftMouseDown, points[0], tilt: .zero, rotation: 30))
    let latched = Double(canvas.strokePen.eventRotation)
    for point in points.dropFirst() {
        canvas.mouseDragged(with: try tabletEvent(canvas, .leftMouseDragged, point, tilt: .zero,
                                                  rotation: 30))
    }
    let shapes = canvas.nativeStrokeSession?.shapes ?? []
    canvas.cancelStroke()
    try require(angles(shapes).count > 4 && angles(shapes).allSatisfy { abs($0 - (10 + latched)) < 1e-6 },
                "\(tag) every dab lies at the tip's angle plus the pen's \(latched): \(angles(shapes))")
}

private func verifyShapedCanvas(sixteen: Bool) throws {
    let tag = sixteen ? "[shaped canvas 16-bit]" : "[shaped canvas 8-bit]"
    let document = try sixteen ? precisionFixture(width: 80, height: 60) : fixture(width: 100, height: 70)
    defer { document.close() }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base document") }
    let store = ToolOptionsStore.shared
    let saved = store.brush
    defer { store.brush = saved }
    // Wide enough that a dab's interior is solid ink: a narrow stamped dab's rim band (its
    // capped hardness) covers most of it, and more paint at mouse-up moves a rim pixel.
    var options = PaintToolOptions(size: 31, hardness: 100, smoothing: 0)
    options.pressureSize = false
    options.pressureOpacity = false
    options.dynamics = shaped
    store.setPaintOptions(options, for: .brush)
    controller.selectTool(.brush)
    canvas.paintColor = NSColor(srgbRed: 0.85, green: 0.15, blue: 0.3, alpha: 1)
    canvas.brushOpacity = 1
    let history = document.history.entries.count
    canvas.beginStroke(at: CGPoint(x: 12.3, y: 20.7))
    for point in [CGPoint(x: 30.2, y: 24.1), CGPoint(x: 48.6, y: 31.9), CGPoint(x: 66.1, y: 38.4)] {
        canvas.continueStroke(to: point, pressure: nil)
    }
    let preview = sixteen ? nil : document.doc?.layerPixels(0)
    let shapes = canvas.nativeStrokeSession?.shapes ?? []
    let dabs = canvas.nativeStrokeSession?.dabs.count ?? 0
    try require(dabs > 16 && shapes.count == dabs / 2 && Set(angles(shapes)).count > 4,
                "\(tag) one jittered shape per dab: \(shapes.count) for \(dabs / 4) dabs")
    canvas.endStroke()
    if sixteen {
        guard let job = DocumentJobs.active else {
            throw ProbeFailure(description: "\(tag) the stroke started no job")
        }
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "\(tag) the stroke's job completed: \(job.report)")
    }
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "\(tag) the stroke published nothing")
    }
    try require(document.history.entries.count == history + 1, "\(tag) one history entry")
    if !sixteen {
        guard let before = base.layerPixels(0), let after = painted.layerPixels(0), let preview else {
            throw ProbeFailure(description: "\(tag) no layer pixels")
        }
        func px(_ a: [UInt8], _ i: Int) -> ArraySlice<UInt8> { a[i * 4..<i * 4 + 4] }
        let previewed = (0..<before.count / 4).filter { px(before, $0) != px(preview, $0) }
        // The full ink is the commonest painted value (a hard tip at full flow and opacity);
        // more paint at mouse-up keeps a full-ink pixel full.
        var counts: [ArraySlice<UInt8>: Int] = [:]
        for i in previewed { counts[px(preview, i), default: 0] += 1 }
        guard let ink = counts.max(by: { $0.value < $1.value })?.key else {
            throw ProbeFailure(description: "\(tag) the preview painted nothing")
        }
        // A pixel just short of full coverage rounds to the ink too, and the 8-bit overlay's
        // rounding drifts a few codes where many dabs overlap at mouse-up (6 seen); a pixel the
        // commit leaves unpainted moves to the base, tens of codes away.
        let full = previewed.filter { px(preview, $0) == ink }
        let lost = full.filter { i in zip(px(after, i), ink).contains { abs(Int($0) - Int($1)) > 16 } }
        try require(full.count > 200 && lost.isEmpty,
                    "\(tag) the preview's \(full.count) full-ink pixels hold at mouse-up; "
                    + "\(lost.count) do not: ink \(Array(ink)) "
                    + lost.prefix(4).map { "\(Array(px(after, $0)))" }.joined(separator: " "))
    }
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the base")
}

private func verifyPencilShapePinning() throws {
    let tag = "[pencil shape pinning]"
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.pencil)
    defer { (store.brush, store.pencil) = saved }
    var withShape = BrushDynamics()
    withShape.scatter = 3.75
    withShape.count = 5
    withShape.shape = shaped.shape
    let name = "zz Shaped Probe"
    try BrushPresetLibrary.save(name: name, values: EditorViewController.brushPresetValues(
        BrushPreset(name: name, hardness: 90, flow: 100, spacing: 30, angle: 15, roundness: 70,
                    airbrush: false, dynamics: withShape).applied(to: PaintToolOptions())),
        replace: true)
    defer { try? BrushPresetLibrary.delete(name: name) }
    BrushPresetLibrary.reload()
    guard let mine = BrushPreset.all().first(where: { $0.name == name }) else {
        throw ProbeFailure(description: "\(tag) the preset was not saved")
    }
    store.setPaintOptions(mine.applied(to: PaintToolOptions(size: 9, smoothing: 0)), for: .pencil)
    let pencil = store.paintOptions(for: .pencil)?.dynamics
    try require(pencil?.shape == nil && pencil?.scatter == 3.75,
                "\(tag) applied on the Pencil, the shape is pinned nil and the rest kept: \(String(describing: pencil))")
    store.setPaintOptions(mine.applied(to: PaintToolOptions()), for: .brush)
    try require(store.paintOptions(for: .brush)?.dynamics?.shape == shaped.shape,
                "\(tag) the Brush keeps it")
    try require(!BrushPreset.all(dynamics: EditorTool.pencil.takesBrushDynamics,
                                 shapes: EditorTool.pencil.takesTipShapeDynamics).contains { $0.name == name }
                && BrushPreset.all(dynamics: true, shapes: true).contains { $0.name == name },
                "\(tag) the Pencil's list leaves it out; the Brush's lists it")
    // The `.` `,` keys' last row: the Brush reaches it, the Pencil does not.
    let document = try fixture(width: 80, height: 60)
    defer { document.close() }
    let controller = try editor(document)
    for (tool, reaches) in [(EditorTool.brush, true), (.pencil, false)] {
        controller.selectTool(tool)
        controller.brushPresetKeyStepped(.last)
        let scatterAmount = store.paintOptions(for: tool)?.dynamics?.scatter
        try require((scatterAmount == 3.75) == reaches,
                    "\(tag) Shift+. on \(tool) \(reaches ? "reaches" : "skips") it: \(String(describing: scatterAmount))")
    }
    // A Pencil canvas stroke still commits with the pinned dynamics.
    store.setPaintOptions(mine.applied(to: PaintToolOptions(size: 9, smoothing: 0)), for: .pencil)
    controller.selectTool(.pencil)
    let canvas: ImageCanvasView = controller.canvas
    canvas.paintColor = NSColor(srgbRed: 0.1, green: 0.4, blue: 0.8, alpha: 1)
    canvas.brushOpacity = 1
    let base = document.doc
    let history = document.history.entries.count
    canvas.beginStroke(at: CGPoint(x: 10.5, y: 10.5))
    try require(canvas.nativeStrokeSession?.dynamics != nil
                && canvas.nativeStrokeSession?.dynamicsShaped == false,
                "\(tag) the Pencil's session has dynamics and no tip shape")
    canvas.continueStroke(to: CGPoint(x: 60.5, y: 40.5), pressure: nil)
    canvas.endStroke()
    try require(document.doc !== base && document.history.entries.count == history + 1,
                "\(tag) the Pencil stroke commits")
}

/// A two-level texture: an opaque white and black 2 × 2 checker at depth 0.8.
private let textured: BrushDynamics = {
    let rgba: [UInt8] = [255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255]
    var d = BrushDynamics()
    d.texture = BrushDynamics.Texture(
        pattern: PatternRef(name: "Probe Checker", width: 2, height: 2,
                            rgba: Data(rgba).base64EncodedString()),
        scale: 1, depth: 0.8)
    return d
}()

private func verifyTextureOptions() throws {
    let tag = "[texture options]"
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.eraser, store.pencil, store.dodge)
    defer { (store.brush, store.eraser, store.pencil, store.dodge) = saved }
    var both = textured
    both.scatter = 2
    var options = PaintToolOptions(size: 9, smoothing: 0)
    options.dynamics = both
    for tool in [EditorTool.brush, .eraser, .pencil, .dodge] {
        store.setPaintOptions(options, for: tool)
    }
    try require(store.paintOptions(for: .brush)?.dynamics == both
                && store.paintOptions(for: .eraser)?.dynamics == both,
                "\(tag) the Brush and the Eraser keep the texture")
    let pencil = store.paintOptions(for: .pencil)?.dynamics
    try require(pencil?.texture == nil && pencil?.scatter == 2,
                "\(tag) the Pencil drops the texture and keeps the rest: \(String(describing: pencil))")
    try require(store.paintOptions(for: .dodge)?.dynamics == nil, "\(tag) Dodge keeps none")
    var flat = textured
    flat.texture?.depth = 0
    try require(flat.normalized == nil, "\(tag) a texture at depth 0 is stored as none")
    let roundTrip = try BrushDynamics(json: textured.json)
    try require(roundTrip == textured, "\(tag) the texture round-trips through its JSON")
    let mine = BrushPreset(name: "Textured", hardness: 100, flow: 100, spacing: 25, angle: 0,
                           roundness: 100, airbrush: false, dynamics: textured)
    let decoded = try BrushPreset.decode(mine.json)
    let echo = (mine.fields["dynamics"] as? [String: Any])?["texture"] as? [String: Any]
    let pattern = echo?["pattern"] as? [String: Any]
    try require(mine.json["schema_version"] as? Int == 2 && decoded.dynamics == textured
                && pattern?["rgba"] == nil && pattern?["name"] as? String == "Probe Checker",
                "\(tag) a textured preset round-trips at schema 2 and echoes no pixels: \(String(describing: echo))")
    let presets = BrushPreset.builtIns + [mine]
    try require(BrushPreset.matchIndex(of: mine.applied(to: PaintToolOptions()), in: presets)
                    == presets.count,
                "\(tag) the Brush matches a textured preset")
    try BrushPresetLibrary.save(name: "zz Textured Probe", values: EditorViewController.brushPresetValues(
        BrushPreset(name: "zz Textured Probe", hardness: 70, flow: 100, spacing: 25, angle: 0,
                    roundness: 100, airbrush: false, dynamics: textured).applied(to: PaintToolOptions())),
        replace: true)
    defer { try? BrushPresetLibrary.delete(name: "zz Textured Probe") }
    BrushPresetLibrary.reload()
    try require(!BrushPreset.all(dynamics: true, shapes: EditorTool.pencil.takesTipShapeDynamics)
                    .contains { $0.name == "zz Textured Probe" }
                && BrushPreset.all().contains { $0.name == "zz Textured Probe" },
                "\(tag) the Pencil's list leaves a textured preset out; the Brush's lists it")
}

/// The view under `root` whose accessibility identifier is `id`.
private func subview(_ id: String, in root: NSView) -> NSView? {
    if root.accessibilityIdentifier() == id { return root }
    for child in root.subviews {
        if let found = subview(id, in: child) { return found }
    }
    return nil
}

private func verifyTextureSheet() throws {
    let tag = "[texture sheet]"
    var options = PaintToolOptions(size: 9, smoothing: 0)
    options.dynamics = textured
    let brush = BrushDynamicsSheetController(options: options, shapes: true, onApply: { _ in })
    guard let popup = subview("brush-dynamics-texture", in: brush.view) as? NSPopUpButton,
          let scale = subview("brush-dynamics-texture-scale", in: brush.view) as? NSTextField,
          let depth = subview("brush-dynamics-texture-depth", in: brush.view) as? NSTextField else {
        throw ProbeFailure(description: "\(tag) the Brush's sheet has no Texture rows")
    }
    let presets = RasterDocument.patternPresets().count
    try require(popup.itemTitles.first == "None" && popup.numberOfItems >= 2 + presets
                && popup.titleOfSelectedItem == "Probe Checker"
                && scale.stringValue == "100" && depth.stringValue == "80",
                "\(tag) the Brush's sheet shows the texture: \(popup.itemTitles) "
                + "\(String(describing: popup.titleOfSelectedItem)) \(scale.stringValue) \(depth.stringValue)")
    let pencil = BrushDynamicsSheetController(options: options, shapes: false, onApply: { _ in })
    try require(subview("brush-dynamics-texture", in: pencil.view) == nil
                && subview("brush-dynamics-texture-depth", in: pencil.view) == nil,
                "\(tag) the Pencil's sheet shows no Texture rows")
}

/// The first layer's red and green: bytes at 8 bits, u16 codes at 16.
private func redGreen(_ doc: RasterDocument?, sixteen: Bool) throws -> [Int] {
    if sixteen {
        guard let doc else { throw ProbeFailure(description: "no 16-bit document") }
        return try [RasterPlane.red, .green].flatMap {
            try PrecisionScalarOperations.read(doc, target: $0.agentName, index: 0).samples.map(Int.init)
        }
    }
    guard let pixels = doc?.layerPixels(0) else { throw ProbeFailure(description: "no pixels") }
    return pixels.enumerated().filter { $0.offset % 4 < 2 }.map { Int($0.element) }
}

private func verifyTexturedCanvas(sixteen: Bool) throws {
    let tag = sixteen ? "[textured canvas 16-bit]" : "[textured canvas 8-bit]"
    let document = try sixteen ? precisionFixture(width: 80, height: 60) : fixture(width: 80, height: 60)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let canvas: ImageCanvasView = controller.canvas
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no base document") }
    let store = ToolOptionsStore.shared
    let saved = (store.brush, store.pencil)
    defer { (store.brush, store.pencil) = saved }
    var options = PaintToolOptions(size: 15, hardness: 100, smoothing: 0)
    options.pressureSize = false
    options.pressureOpacity = false
    options.dynamics = textured
    store.setPaintOptions(options, for: .brush)
    controller.selectTool(.brush)
    // The recorder takes the stroke's colour from the controller's capture (the foreground,
    // black here), so the canvas paints that colour too and the replay is the same stroke.
    canvas.paintColor = NSColor(srgbRed: 0, green: 0, blue: 0, alpha: 1)
    canvas.brushOpacity = 1
    let history = document.history.entries.count
    func settle() throws {
        guard let job = DocumentJobs.active else { return }
        try pumpUntil({ job.state != "running" })
        try require(job.state == "completed", "\(tag) the job completed: \(job.report)")
    }
    let steps = try uiRecorded {
        canvas.beginStroke(at: CGPoint(x: 12.3, y: 20.7))
        try require(canvas.nativeStrokeSession?.texture != nil, "\(tag) the session has the texture")
        for point in [CGPoint(x: 30.2, y: 24.1), CGPoint(x: 48.6, y: 31.9), CGPoint(x: 66.1, y: 38.4)] {
            canvas.continueStroke(to: point, pressure: nil)
        }
        canvas.endStroke()
        try settle()
    }
    guard let painted = document.doc, painted !== base else {
        throw ProbeFailure(description: "\(tag) the stroke published nothing")
    }
    try require(document.history.entries.count == history + 1, "\(tag) one history entry")
    guard steps.count == 1, steps[0].tool == "brush_stroke",
          let native = steps[0].arguments["native_stroke"] as? [String: Any],
          let texture = native["texture"] as? [String: Any] else {
        throw ProbeFailure(description: "\(tag) the stroke records no textured native_stroke: "
                           + "\(steps.map { "\($0.tool) \($0.arguments.keys.sorted())" })")
    }
    try require(texture["depth"] as? Double == 0.8, "\(tag) the step holds the texture: \(texture)")
    let committed = try redGreen(painted, sixteen: sixteen)
    func replayed(_ arguments: [String: Any]) throws -> [Int] {
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) one undo restores the base")
        _ = try agentOK(steps[0].tool, document, arguments)
        try settle()
        return try redGreen(document.doc, sixteen: sixteen)
    }
    let replay = try replayed(steps[0].arguments)
    try require(replay == committed,
                "\(tag) the commit equals the replay of its recorded step: "
                + "\(zip(replay, committed).filter { $0 != $1 }.count) samples differ")
    var bare = steps[0].arguments
    bare["native_stroke"] = native.filter { $0.key != "texture" }
    let untextured = try replayed(bare)
    try require(untextured != committed, "\(tag) the texture changed the commit")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the base")
    // The Pencil drops the texture: its stroke's session has dynamics and no texture.
    var pencilOptions = PaintToolOptions(size: 9, smoothing: 0)
    var pencilDynamics = textured
    pencilDynamics.scatter = 1
    pencilOptions.dynamics = pencilDynamics
    store.setPaintOptions(pencilOptions, for: .pencil)
    controller.selectTool(.pencil)
    canvas.beginStroke(at: CGPoint(x: 10.5, y: 10.5))
    try require(canvas.nativeStrokeSession?.dynamics != nil
                && canvas.nativeStrokeSession?.texture == nil,
                "\(tag) the Pencil's session has dynamics and no texture")
    canvas.continueStroke(to: CGPoint(x: 60.5, y: 40.5), pressure: nil)
    canvas.endStroke()
    try settle()
    try require(document.doc !== base, "\(tag) the Pencil stroke commits")
}
