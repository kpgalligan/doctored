import AppKit

/// Independent binary decoding and product-of-transmittance reference. This
/// deliberately does not call NativeBrushStroke.coverage or the core renderer.
func decodeDabs(_ canonical: [String: Any]) throws -> [Double] {
    guard let text = canonical["dabs"] as? String, let data = Data(base64Encoded: text), data.count % 32 == 0 else {
        throw DoctoredCoreError(message: "Recorded canonical dab data missing")
    }
    return data.withUnsafeBytes { bytes in
        stride(from: 0, to: data.count, by: 8).map {
            Double(bitPattern: UInt64(littleEndian: bytes.loadUnaligned(fromByteOffset: $0, as: UInt64.self)))
        }
    }
}
func coverageOracle(_ canonical: [String: Any], width: Int, height: Int, opacity: Double = 1) throws -> [UInt16] {
    let dabs = try decodeDabs(canonical)
    guard let hardness = canonical["hardness"] as? Double,
          let roundness = canonical["roundness"] as? Double,
          let angle = canonical["angle_degrees"] as? Double,
          let pixel = canonical["pixel"] as? Bool else { throw DoctoredCoreError(message: "Canonical geometry missing") }
    let radians = angle * .pi / 180, cosine = cos(radians), sine = sin(radians)
    func radial(_ x: Double, _ y: Double, _ i: Int) -> Double {
        let dx = x - dabs[i], dy = y - dabs[i + 1], radius = dabs[i + 2] / 2
        let r = hypot((dx * cosine - dy * sine) / radius, (dy * cosine + dx * sine) / (radius * roundness))
        if r <= hardness { return 1 }
        if r >= 1 { return 0 }
        let t = (r - hardness) / (1 - hardness)
        return (1 - t) * (1 - t) * (1 + 2 * t)
    }
    return (0..<(width * height)).map { index in
        let x = Double(index % width), y = Double(index / width)
        var transmission = 1.0
        for i in stride(from: 0, to: dabs.count, by: 4) {
            let radius = dabs[i + 2] / 2
            var area: Double
            if pixel {
                let dx = max(0, min(x + 1, dabs[i] + radius) - max(x, dabs[i] - radius))
                let dy = max(0, min(y + 1, dabs[i + 1] + radius) - max(y, dabs[i + 1] - radius))
                area = dx * dy
            } else if hardness < 1 { area = radial(x + 0.5, y + 0.5, i) }
            else {
                area = 0
                for sy in 0..<8 { for sx in 0..<8 {
                    area += radial(x + (Double(sx) + 0.5) / 8, y + (Double(sy) + 0.5) / 8, i) / 64
                }}
            }
            transmission *= 1 - area * dabs[i + 3]
        }
        return UInt16((min(1, max(0, (1 - transmission) * opacity)) * 65535).rounded())
    }
}
func verifyPaint(_ before: [UInt16], after: [UInt16], canonical: [String: Any], ink: [Double],
                 opacity: Double, selection: [UInt16]?, width: Int, height: Int) throws -> Int {
    let footprint = try coverageOracle(canonical, width: width, height: height)
    let ink = ink.map { Double(Float($0)) }, opacity = Double(Float(opacity))
    var checked = 0
    for i in footprint.indices {
        let p = i * 4
        let weight = Double(footprint[i]) / 65535 * Double(selection?[i] ?? 65535) / 65535
        if weight == 0 {
            try require(Array(before[p..<p+4]) == Array(after[p..<p+4]), "Outside stroke/selection changed at \(i)")
            continue
        }
        let sourceAlpha = ink[3] * opacity * weight, baseAlpha = Double(before[p+3]) / 65535
        let alpha = sourceAlpha + baseAlpha * (1 - sourceAlpha)
        for c in 0..<4 {
            let expected: Double
            if c == 3 { expected = alpha * 65535 }
            else { expected = (ink[c] * sourceAlpha + Double(before[p+c]) / 65535 * baseAlpha * (1 - sourceAlpha)) / alpha * 65535 }
            try require(abs(Double(after[p+c]) - expected.rounded()) <= 1,
                "Independent paint reference differs at pixel\(i) component\(c): \(after[p+c]) vs \(expected.rounded())")
            checked += 1
        }
    }
    return checked
}
func toggleActualPressureControls(_ editor: EditorViewController, output: URL) throws {
    editor.view.layoutSubtreeIfNeeded(); editor.optionsBar.layoutSubtreeIfNeeded()
    guard let more = descendants(editor.view).compactMap({ $0 as? MoreButton }).first else { throw DoctoredCoreError(message: "Actual More control missing") }
    try send(more, .leftMouseDown, CGPoint(x: more.bounds.midX, y: more.bounds.midY)); pump()
    defer { editor.optionsBar.closeOverflow() }
    // The overflow is the bar's owned OverflowPanel; its contentView is the
    // square surface, and the options content is that surface's one subview.
    guard let panel = member(editor.optionsBar, "overflowPanel") as? OverflowPanel, panel.isVisible,
          let content = panel.contentView?.subviews.first else { throw DoctoredCoreError(message: "Actual brush overflow did not open") }
    for name in ["Pressure size", "Pressure opacity"] {
        guard let label = descendants(content).compactMap({ $0 as? NSTextField }).first(where: { $0.stringValue == name }),
              let checkbox = content.subviews.compactMap({ $0 as? OptionCheckboxControl }).first(where: {
                  abs($0.frame.midY - label.frame.midY) < 3
              }) else { throw DoctoredCoreError(message: "Actual \(name) checkbox missing") }
        try send(checkbox, .leftMouseDown, CGPoint(x: checkbox.bounds.midX, y: checkbox.bounds.midY))
    }
    try require(editor.canvas.brushPressureSize && editor.canvas.brushPressureOpacity,
        "Actual pressure controls did not update canvas options")
    try screenshot(content, output.appendingPathComponent("pressure-controls.png"))
}
func validateReplay(_ document: ImageDocument, source: RasterDocument, result: RasterDocument,
                    step: ActionStep, expected: [UInt16]) throws {
    document.undoManager?.undo(); pump()
    try require(document.doc === source && !document.isDocumentEdited, "One Undo did not restore saved native owner")
    document.undoManager?.redo(); pump()
    try require(document.doc === result, "Redo did not restore exact native owner")
    document.undoManager?.undo(); pump()
    let reply = try replay(step, document: document)
    try require(try raw(document.doc ?? source) == expected, "Canonical Action changed native components: \(reply)")
}
func pressureUI(output: URL) throws -> [String: Any] {
    let (document, editor, source) = try openDocument()
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
    configure(editor)
    try toggleActualPressureControls(editor, output: output)
    let selection = (0..<(source.width * source.height)).map { [UInt16(0), 1, 129, 32769, 65535][$0 % 5] }
    editor.agentSetSelection(CanvasSelection(shape: .mask16(selection), canvasWidth: source.width, canvasHeight: source.height))
    document.undoManager?.removeAllActions()
    ActionRecorder.shared.start()
    let initialPressure = try send(editor.canvas, .leftMouseDown, CGPoint(x: 20.5, y: 28.5), pressure: 0.25)
    try require(document.doc === source, "Mouse-down published native pixels")
    guard let first = editor.canvas.nativeStrokeSession?.dabs else { throw DoctoredCoreError(message: "Actual native session absent") }
    try require(first.count == 4 && abs(first[2] - 24 * initialPressure) < 0.000001 && abs(first[3] - 0.37 * initialPressure) < 0.000001,
        "First UI dab did not resolve pressure size and opacity")
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 38.5, y: 29.5), pressure: 0.5)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 62.5, y: 30.5), pressure: 0.875)
    try screenshot(editor.view, output.appendingPathComponent("pressure-drag.png"))
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 62.5, y: 30.5), pressure: 0.875)
    let steps = ActionRecorder.shared.stop()
    guard steps.count == 1, let canonical = steps[0].arguments["native_stroke"] as? [String: Any],
          let ink = steps[0].arguments["rgba"] as? [Double], let result = document.doc else { throw DoctoredCoreError(message: "Pressure UI failed to record one canonical native stroke") }
    let dabs = try decodeDabs(canonical), diameters = stride(from: 2, to: dabs.count, by: 4).map { dabs[$0] }
    try require(Set(diameters).count > 3 && (diameters.max() ?? 0) > 12, "Changing tablet pressure did not change resolved diameters")
    for i in stride(from: 0, to: dabs.count, by: 4) {
        try require(abs(dabs[i+3] - 0.37 * dabs[i+2] / 24) < 0.0001, "Resolved pressure opacity and size diverged")
    }
    let before = fixture(), after = try raw(result)
    try require(result !== source && after != before && editor.canvas.nativeStrokeSession == nil, "Pressure stroke failed publication or session cleanup")
    let checked = try verifyPaint(before, after: after, canonical: canonical, ink: ink, opacity: 0.73,
        selection: selection, width: source.width, height: source.height)
    try validateReplay(document, source: source, result: result, step: steps[0], expected: after)
    let persistence = try verifyPersistence(document, expected: after, output: output)
    return ["name": "pressure-ui", "passed": true, "dabs": dabs.count / 4, "independent_components": checked,
        "samples_sha256": hashSamples(after), "action": steps[0].arguments, "persistence": persistence,
        "initial_event_pressure": initialPressure]
}
func airbrushUI(output: URL, pixel: Bool = false) throws -> [String: Any] {
    let (document, editor, source) = try openDocument()
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
    configure(editor, size: pixel ? 1 : 36, pressureOpacity: true, airbrush: true)
    document.undoManager?.removeAllActions(); ActionRecorder.shared.start()
    let point = CGPoint(x: 48.5, y: 30.5)
    let actualPressure = try send(editor.canvas, .leftMouseDown, point, pressure: 0.375)
    let firstCount = editor.canvas.nativeStrokeSession?.dabs.count ?? 0
    let start = Date()
    while Date().timeIntervalSince(start) < 0.28 { pump() }
    let live = editor.canvas.nativeStrokeSession?.dabs ?? []
    try require(live.count >= firstCount + 4 * 3, "Held real airbrush Timer did not add at least three deposits")
    for i in stride(from: 0, to: live.count, by: 4) {
        try require(live[i] == point.x && live[i+1] == point.y && abs(live[i+3] - 0.37 * actualPressure) < 0.000001,
            "Held timer changed center or pressure")
    }
    try send(editor.canvas, .leftMouseUp, point, pressure: 0.375)
    let steps = ActionRecorder.shared.stop()
    guard steps.count == 1, let canonical = steps[0].arguments["native_stroke"] as? [String: Any],
          let ink = steps[0].arguments["rgba"] as? [Double], let result = document.doc else { throw DoctoredCoreError(message: "Airbrush canonical Action missing") }
    try require(try decodeDabs(canonical) == live, "Held-airbrush Action changed actual deposited dab sequence")
    let after = try raw(result)
    _ = try verifyPaint(fixture(), after: after, canonical: canonical, ink: ink, opacity: 0.73,
        selection: nil, width: source.width, height: source.height)
    try validateReplay(document, source: source, result: result, step: steps[0], expected: after)
    return ["name": pixel ? "pixel-airbrush-ui" : "airbrush-ui", "passed": true, "actual_timer_deposits": live.count / 4 - 1,
        "canonical_exact": true, "samples_sha256": hashSamples(after), "event_pressure": actualPressure]
}
func zeroPressureUI() throws -> [String: Any] {
    let (document, editor, source) = try openDocument()
    defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
    configure(editor, size: 1, hardness: 1, opacity: 100, flow: 100, pressureOpacity: true)
    document.undoManager?.removeAllActions(); ActionRecorder.shared.start()
    try send(editor.canvas, .leftMouseDown, CGPoint(x: 12.5, y: 15.5), pressure: 0)
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 30.5, y: 15.5), pressure: 0)
    guard let dabs = editor.canvas.nativeStrokeSession?.dabs else { throw DoctoredCoreError(message: "Pixel session missing") }
    try require(dabs.count > 4 && stride(from: 3, to: dabs.count, by: 4).allSatisfy { dabs[$0] == 0 }, "Zero-pressure pixel movement created ink")
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 30.5, y: 15.5), pressure: 0)
    let steps = ActionRecorder.shared.stop()
    try require(document.doc === source && !document.isDocumentEdited && document.undoManager?.canUndo != true && steps.isEmpty,
        "Zero-pressure gesture changed pixels, dirty state, Undo or Actions")
    try require(editor.canvas.nativeStrokeSession == nil, "No-op pixel stroke retained its session")
    return ["name": "zero-pressure-pixel-ui", "passed": true, "zero_deposits": dabs.count / 4]
}
