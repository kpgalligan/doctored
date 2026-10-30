import AppKit

/// Invoke the actual popup's menu-item action after inspecting its complete
/// choices. The menu action is AppKit's selector; no product test hook exists.
func choosePopup(_ editor: EditorViewController, containing title: String, index: Int? = nil) throws -> [String] {
    let popups = descendants(editor.optionsBar).compactMap { $0 as? OptionPopupControl }
    guard let popup = popups.first(where: { (member($0, "items") as? [String])?.contains(title) == true }),
          let items = member(popup, "items") as? [String], let selected = index ?? items.firstIndex(of: title), items.indices.contains(selected),
          member(popup, "isActive") as? Bool == true else { throw DoctoredCoreError(message: "Enabled actual popup containing \(title) missing") }
    let choice = NSMenuItem(title: items[selected], action: nil, keyEquivalent: ""); choice.tag = selected
    let selector = NSSelectorFromString("itemPicked:")
    try require(popup.responds(to: selector), "Actual popup menu action missing")
    _ = popup.perform(selector, with: choice)
    return items
}
func extendedToolsUI(output: URL) throws -> [String: Any] {
    let originalFill = ToolOptionsStore.shared.fill, originalGradient = ToolOptionsStore.shared.gradient
    let originalDodge = ToolOptionsStore.shared.dodge
    defer {
        ToolOptionsStore.shared.fill = originalFill; ToolOptionsStore.shared.gradient = originalGradient
        ToolOptionsStore.shared.dodge = originalDodge
    }
    var cases: [[String: Any]] = []
    for kind in ["multiply-brush", "bucket-fill", "bucket-alpha-boundary", "linear-gradient", "radial-gradient", "burn-highlights"] {
        let (document, editor, source) = try openDocument()
        defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
        configure(editor, size: 28, opacity: 73)
        let coverage = (0..<(source.width * source.height)).map { [UInt16(0), 1, 32769, 65535][$0 % 4] }
        editor.agentSetSelection(CanvasSelection(shape: .mask16(coverage), canvasWidth: source.width, canvasHeight: source.height))
        document.undoManager?.removeAllActions()
        if kind == "multiply-brush" {
            let items = try choosePopup(editor, containing: "Multiply")
            try require(items.count == 27 && editor.paintStrokeBlendMode() == DR_BLEND_MULTIPLY,
                "Actual native Blend popup lost a mode or failed to select Multiply")
        } else if kind.hasPrefix("bucket") {
            ToolOptionsStore.shared.fill.tolerance = 0; ToolOptionsStore.shared.fill.contiguous = false
            ToolOptionsStore.shared.fill.opacity = kind == "bucket-alpha-boundary" ? 73 : 73.123
            if kind == "bucket-alpha-boundary" {
                editor.paintColor = NSColor(colorSpace: document.nsColorSpace,
                    components: [0.713219, 0.193517, 0.831271, 0.0027696169880990083], count: 4)
            }
            editor.selectTool(.fill); pump()
            guard let checkbox = descendants(editor.optionsBar).compactMap({ $0 as? OptionCheckboxControl }).first(where: {
                member($0, "label") as? String == "Contiguous"
            }) else { throw DoctoredCoreError(message: "Actual Fill Contiguous control missing") }
            try send(checkbox, .leftMouseDown, CGPoint(x: checkbox.bounds.midX, y: checkbox.bounds.midY))
            try require(ToolOptionsStore.shared.fill.contiguous, "Actual Fill Contiguous checkbox did not update")
            let fields = descendants(editor.optionsBar).compactMap { $0 as? NSTextField }
            try require(fields.contains { $0.stringValue == "0" && $0.isEditable }, "Actual Fill tolerance entry does not display native seed tolerance")
        } else if kind.contains("gradient") {
            ToolOptionsStore.shared.gradient.opacity = 73
            ToolOptionsStore.shared.gradient.typeIndex = kind == "radial-gradient" ? 1 : 0
            editor.backgroundColor = NSColor(colorSpace: document.nsColorSpace,
                components: [0.171213, 0.739517, 0.231271, 0.191123], count: 4)
            editor.selectTool(.gradient); pump()
        } else {
            var options = PaintToolOptions(); options.size = 28; options.opacity = 37
            options.hardness = 20; options.pressureSize = false; options.smoothing = 0
            ToolOptionsStore.shared.dodge = options; editor.selectTool(.dodge); pump()
            _ = try choosePopup(editor, containing: "Burn", index: 1)
            _ = try choosePopup(editor, containing: "Highlights", index: 2)
            try require(ToolOptionsStore.shared.dodge.burn && ToolOptionsStore.shared.dodge.rangeIndex == 2,
                "Actual Dodge/Burn mode/range controls did not update")
        }
        ActionRecorder.shared.start()
        let start = kind == "bucket-alpha-boundary" ? CGPoint(x: 35.5, y: 30.5) : CGPoint(x: 30.5, y: 30.5)
        try send(editor.canvas, .leftMouseDown, start)
        if kind.contains("gradient") { try send(editor.canvas, .leftMouseDragged, CGPoint(x: 62.5, y: 42.5)) }
        try send(editor.canvas, .leftMouseUp, kind.contains("gradient") ? CGPoint(x: 62.5, y: 42.5) : start)
        let steps = ActionRecorder.shared.stop()
        guard steps.count == 1, let result = document.doc, result !== source else { throw DoctoredCoreError(message: "Actual \(kind) did not publish/record one native edit") }
        if kind.hasPrefix("bucket") {
            let ink = try PrecisionInk(color: editor.paintColor, space: document.nsColorSpace)
            try require(steps[0].arguments["rgba"] as? [Double] == ink.rgba &&
                steps[0].arguments["opacity"] as? Double == ToolOptionsStore.shared.fill.opacity / 100 &&
                steps[0].arguments["fill_mode"] as? String == "color_match",
                "Native Fill Action must retain exact authored alpha and separate fractional opacity")
        }
        let expected = try raw(result), before = fixture()
        if kind == "bucket-alpha-boundary" {
            try require(before[(30 * 96 + 35) * 4 + 3] == 0 && expected[(30 * 96 + 35) * 4 + 3] == 132,
                "Fill alpha must round separate f32 ink/opacity to132; a prematurely collapsed alpha rounds to133")
        }
        try require(expected != before, "Actual \(kind) changed no native samples")
        for i in coverage.indices {
            if coverage[i] == 0 {
                try require(Array(expected[i*4..<i*4+4]) == Array(before[i*4..<i*4+4]), "\(kind) changed samples outside native selection")
            }
            if kind == "burn-highlights" { try require(expected[i*4+3] == before[i*4+3], "Burn changed alpha") }
        }
        try screenshot(editor.view, output.appendingPathComponent(kind + ".png"))
        try validateReplay(document, source: source, result: result, step: steps[0], expected: expected)
        cases.append(["name": kind, "native_components": expected.count, "action": steps[0].arguments,
            "samples_sha256": hashSamples(expected), "one_undo_exact_replay": true])
    }
    return ["name": "extended-tools-ui", "passed": true, "cases": cases]
}

/// The seed is excluded while all other pixels remain selected and match the
/// maximum tolerance. A zero gate must stop growth, while native code1 must
/// permit the same enabled tool to operate; this cannot pass via a disabled UI.
func excludedSeedFillUI(output: URL) throws -> [String: Any] {
    let originalFill = ToolOptionsStore.shared.fill
    defer { ToolOptionsStore.shared.fill = originalFill }
    var cases: [[String: Any]] = []
    let targets: [(PaintTarget, String)] = [(.layer, "rgb"), (.plane(.red), "red")]
    for (target, name) in targets { for contiguous in [true, false] {
        let (document, editor, source) = try openDocument()
        defer { if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }; document.close(); pump() }
        let point = CGPoint(x: 30.5, y: 30.5), seed = 30 * source.width + 30
        var coverage = [UInt16](repeating: 65535, count: source.width * source.height)
        coverage[seed] = 0
        editor.agentSetSelection(CanvasSelection(shape: .mask16(coverage), canvasWidth: source.width, canvasHeight: source.height))
        editor.setPaintTarget(target)
        ToolOptionsStore.shared.fill.tolerance = 255
        ToolOptionsStore.shared.fill.opacity = 100
        ToolOptionsStore.shared.fill.contiguous = contiguous
        editor.paintColor = .white; editor.selectTool(.fill); pump()
        try require(editor.canvas.tool == .fill && editor.paintTarget == target && DocumentPrecision.allows(.fill, document: document),
            "Actual Fill is unavailable for \(name)")
        document.undoManager?.removeAllActions(); ActionRecorder.shared.start()
        try send(editor.canvas, .leftMouseDown, point)
        try send(editor.canvas, .leftMouseUp, point)
        let refusedSteps = ActionRecorder.shared.stop()
        try require(document.doc === source && document.undoManager?.canUndo != true && !document.isDocumentEdited && refusedSteps.isEmpty,
            "Excluded seed changed \(name) pixels, Undo, dirty state or Actions (contiguous=\(contiguous))")
        try require(try raw(source) == fixture(), "Excluded-seed Fill mutated captured \(name) source")
        try require(document.canvasSelection?.maskSamples16() == coverage, "Excluded-seed Fill changed the native selection")
        if contiguous { try screenshot(editor.view, output.appendingPathComponent("excluded-seed-\(name).png")) }
        coverage[seed] = 1
        editor.agentSetSelection(CanvasSelection(shape: .mask16(coverage), canvasWidth: source.width, canvasHeight: source.height))
        document.undoManager?.removeAllActions(); ActionRecorder.shared.start()
        try send(editor.canvas, .leftMouseDown, point)
        try send(editor.canvas, .leftMouseUp, point)
        let positiveSteps = ActionRecorder.shared.stop()
        try require(document.doc !== source && positiveSteps.count == 1 &&
            positiveSteps[0].arguments["fill_mode"] as? String == "color_match",
            "Native seed code1 must enable \(name) Fill and record its explicit new meaning")
        try require(try raw(document.doc ?? source) != fixture(), "Positive native seed Fill changed no \(name) samples")
        document.undoManager?.undo(); pump()
        try require(document.doc === source && !document.isDocumentEdited, "Positive native seed Fill lost one-Undo restoration")
        cases.append(["target": name, "contiguous": contiguous, "excluded_seed_noop": true,
            "native_code1_seed_operates": true, "no_phantom_history": true])
    }}
    return ["name": "excluded-seed-fill-ui", "passed": true, "cases": cases]
}
