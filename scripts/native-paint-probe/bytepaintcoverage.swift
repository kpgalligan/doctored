import AppKit

private func byteNativeSelectionValues() -> [UInt16] {
    var values: [UInt16] = []
    values.reserveCapacity(96 * 64)
    for index in 0..<(96 * 64) { values.append(UInt16((index * 109 + 17) % 65536)) }
    return values
}

func setupBytePaintTarget(_ document: ImageDocument, _ editor: EditorViewController,
                          target: PaintTarget, tool: EditorTool,
                          maskVariant: String = "plain", softSelection: Bool = false) throws {
    switch target {
    case .layer, .plane, .ink: break
    case .mask:
        var layer = 0
        switch maskVariant {
        case "plain", "translated": break
        case "group":
            _ = try call("duplicate_layer", ["index": 0], document: document)
            _ = try call("group_layers", ["layers": [0, 1],
                "name": "Coverage Group"], document: document)
            guard let source = document.doc,
                  let group = (0..<source.layerCount).first(where: source.layerIsGroup) else {
                throw DoctoredCoreError(message: "Coverage group fixture unavailable")
            }
            layer = group
        case "adjustment":
            _ = try call("add_adjustment_layer", ["op": "invert", "name": "Coverage Adjustment"],
                         document: document)
            guard let source = document.doc,
                  let adjustment = (0..<source.layerCount).first(where: source.layerIsAdjustment) else {
                throw DoctoredCoreError(message: "Coverage adjustment fixture unavailable")
            }
            layer = adjustment
        default: throw DoctoredCoreError(message: "Unknown coverage mask variant")
        }
        _ = try call("add_layer_mask", ["layer": layer,
            "kind": tool == .eraser ? "reveal_all" : "hide_all"], document: document)
        if maskVariant == "translated" {
            _ = try call("transform_mask", ["layer": layer,
                "matrix": [1, 0, 0, 1, 7, -3], "filter": "nearest"], document: document)
        }
        document.activeLayerIndex = layer
    case .channel:
        _ = try call("add_channel", ["name": "Byte Paint Coverage"], document: document)
        if tool == .eraser {
            _ = try call("brush_stroke", ["target": "channel:Byte Paint Coverage",
                "points": [[18, 22], [69, 39]], "size": 30, "color": "#FFFFFF"],
                document: document)
        }
    }
    if softSelection {
        var values = [UInt8]()
        values.reserveCapacity(96 * 64)
        for index in 0..<(96 * 64) {
            values.append(UInt8((index * 71 + 19) % 229 + 1))
        }
        document.setCanvasSelection(CanvasSelection(shape: .mask(values),
            canvasWidth: 96, canvasHeight: 64))
    }
    editor.setPaintTarget(target)
    pump()
}

func byteCoverageRecordUI() throws -> [String: Any] {
    let targets: [(String, PaintTarget)] = [
        ("mask", .mask), ("red", .plane(.red)), ("green", .plane(.green)),
        ("blue", .plane(.blue)), ("alpha", .plane(.alpha)),
        ("channel", .channel(0)),
    ]
    var trials: [[String: Any]] = []
    for (label, target) in targets {
        for tool in [EditorTool.brush, .eraser] {
            trials.append(try bytePaintGestureTrial(tool,
                name: "coverage-\(label)-\(tool.displayName)",
                variant: "pressure", held: true, target: target,
                softSelection: true))
        }
    }
    for maskVariant in ["translated", "group", "adjustment"] {
        for tool in [EditorTool.brush, .eraser] {
            trials.append(try bytePaintGestureTrial(tool,
                name: "coverage-\(maskVariant)-\(tool.displayName)",
                variant: "pressure", held: true, target: .mask,
                maskVariant: maskVariant, softSelection: true))
        }
    }
    for (family, target) in [("mask", PaintTarget.mask),
                             ("plane", .plane(.red)), ("channel", .channel(0))] {
        for tool in [EditorTool.brush, .eraser] {
            for tip in ["soft", "flow", "round"] {
                trials.append(try bytePaintGestureTrial(tool,
                    name: "coverage-mouse-\(family)-\(tool.displayName)-\(tip)",
                    variant: tip, target: target, softSelection: true))
            }
        }
    }
    return ["case": "byte-coverage-record-ui", "trials": trials]
}

func byteQuickMaskRecordUI() throws -> [String: Any] {
    var trials: [[String: Any]] = []
    for tool in [EditorTool.brush, .eraser] {
        for nativeSelection in [false, true] {
            let (document, editor, source) = try openByteDocument()
            defer {
                if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
                document.close(); pump()
            }
            try configureBytePaint(editor, tool: tool, variant: "pressure", held: true)
            if nativeSelection {
                let values = byteNativeSelectionValues()
                document.setCanvasSelection(CanvasSelection(shape: .mask16(values),
                    canvasWidth: 96, canvasHeight: 64))
            } else if tool == .eraser {
                document.setCanvasSelection(CanvasSelection(shape: .mask(
                    [UInt8](repeating: 220, count: 96 * 64)),
                    canvasWidth: 96, canvasHeight: 64))
            }
            document.setQuickMaskActive(true)
            editor.syncSelectionHistory(); pump()
            try require(editor.canvas.quickMaskActive, "Quick Mask did not enter its actual canvas mode")
            let before = try bytePaintSnapshot(document)
            let initialSelection = document.selectionHistory
            ActionRecorder.shared.start()
            let start = CGPoint(x: 24.5, y: 22.5)
            try send(editor.canvas, .leftMouseDown, start, pressure: 0.25)
            try send(editor.canvas, .leftMouseDragged, CGPoint(x: 45.5, y: 29.5), pressure: 0.56)
            let deadline = Date().addingTimeInterval(0.2)
            while Date() < deadline { pump() }
            try send(editor.canvas, .leftMouseUp, CGPoint(x: 61.5, y: 35.5), pressure: 0.87)
            let step = try decodedByteStep(ActionRecorder.shared.stop(),
                                           expected: "quick_mask_stroke")
            let after = try bytePaintSnapshot(document)
            try require(after != before && document.doc === source && !document.isDocumentEdited,
                        "Quick Mask did not change selection without dirtying bytes")
            let resultSelection = document.selectionHistory
            document.undoManager?.undo(); pump()
            try require(document.selectionHistory == initialSelection &&
                        (try bytePaintSnapshot(document)) == before,
                        "Quick Mask one Undo did not restore selection and previous state")
            document.undoManager?.redo(); pump()
            try require(document.selectionHistory == resultSelection &&
                        (try bytePaintSnapshot(document)) == after,
                        "Quick Mask Redo did not restore exact coverage")
            let (fresh, freshEditor, _) = try openByteDocument()
            defer { fresh.close(); pump() }
            if nativeSelection {
                let values = byteNativeSelectionValues()
                fresh.setCanvasSelection(CanvasSelection(shape: .mask16(values),
                    canvasWidth: 96, canvasHeight: 64))
            } else if tool == .eraser {
                fresh.setCanvasSelection(CanvasSelection(shape: .mask(
                    [UInt8](repeating: 220, count: 96 * 64)),
                    canvasWidth: 96, canvasHeight: 64))
            }
            fresh.setQuickMaskActive(true)
            freshEditor.syncSelectionHistory()
            let reply = try replayBinding(step, document: fresh)
            try require(!(try responseIsError(reply)) &&
                        (try bytePaintSnapshot(fresh)) == after,
                        "Quick Mask canonical Action did not replay exact byte/u16 selection")
            trials.append(["tool": tool.displayName, "native_selection": nativeSelection,
                "selection_digest": after, "file_clean": !document.isDocumentEdited])
        }
    }
    return ["case": "byte-quick-mask-record-ui", "trials": trials]
}
