import AppKit

// The Move tool's persistent float. Consecutive drags and nudges over one selection move
// ONE float lifted once, so the pixels it passes over come back: the drag preview and the
// marquee mid-drag, Esc cancelling only the drag, one undo step per drag or nudge (the float
// survives undo past its lift and redo), the commits (Deselect and a new selection, a layer
// switch, Save, a filter), ⌥ (the duplicate floats; ⌥ over a live float stamps a copy), and
// 16-bit, 32-bit float and CMYK documents against MCP `move_selected_pixels`. Every case calls
// methods directly (the Move callbacks, `canvas.keyDown(with:)`, the MCP dispatcher), so none
// needs a key window. Not measured: the take could not observe a float
// (`move_float_202_take.py`).

/// `require` with an eager condition, so a condition may call throwing readers.
private func ensure(_ value: Bool, _ message: String) throws {
    if !value { throw ProbeFailure(description: message) }
}

private let floatF: [Int] = [4, 8]
private let floatW: [Int] = [24, 8]
/// Corners and centre of an 8×8 square at `origin`.
private func square(_ x: Int, _ y: Int = 8) -> [(Int, Int)] {
    [(x, y), (x + 7, y), (x, y + 7), (x + 7, y + 7), (x + 3, y + 4)]
}

private func plainRight() -> NSEvent? {
    guard let scalar = UnicodeScalar(UInt32(NSRightArrowFunctionKey)) else { return nil }
    let characters = String(Character(scalar))
    return NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                            timestamp: ProcessInfo.processInfo.systemUptime,
                            windowNumber: 0, context: nil, characters: characters,
                            charactersIgnoringModifiers: characters, isARepeat: false,
                            keyCode: 124)
}

/// The composite at each point, as `sample_pixel` answers it without its coordinates.
private func readings(_ document: ImageDocument, _ points: [(Int, Int)]) throws -> [NSDictionary] {
    try points.map {
        var reply = try agentOK("sample_pixel", document, ["x": $0.0, "y": $0.1])
        reply["x"] = nil
        reply["y"] = nil
        return NSDictionary(dictionary: reply)
    }
}

private func alphas(_ document: ImageDocument, _ points: [(Int, Int)]) throws -> [Int] {
    try points.map { try agentOK("sample_pixel", document, ["x": $0.0, "y": $0.1])["a"] as? Int ?? -1 }
}

/// 64×32, transparent except F (200,100,50) at (4,8,8,8) and W black at
/// (24,8,8,8) on layer 0, an empty layer "Other" above it, layer 0 active,
/// the selection F's rect, the Move tool, and no undo history.
private func floatFixture() throws -> (ImageDocument, EditorViewController) {
    let (width, height) = (64, 32)
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    for (origin, color) in [(floatF, [200, 100, 50]), (floatW, [0, 0, 0])] {
        for y in origin[1]..<(origin[1] + 8) {
            for x in origin[0]..<(origin[0] + 8) {
                let p = (y * width + x) * 4
                bytes[p] = UInt8(color[0])
                bytes[p + 1] = UInt8(color[1])
                bytes[p + 2] = UInt8(color[2])
                bytes[p + 3] = 255
            }
        }
    }
    guard let image = RasterImage.from(rgba: bytes, width: width, height: height),
          let document = ImageDocument.makeUntitled(with: image) else {
        throw ProbeFailure(description: "[move float] fixture")
    }
    document.undoManager?.groupsByEvent = false
    document.updateChangeCount(.changeCleared)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    let controller = try editor(document)
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Other") }
    controller.setActiveLayer(0)
    selectF(document)
    controller.selectTool(.move)
    document.undoManager?.removeAllActions()
    try ensure(document.doc?.layerCount == 2 && document.activeLayerIndex == 0,
                "[move float] fixture: two layers, layer 0 active")
    return (document, controller)
}

private func selectF(_ document: ImageDocument) {
    document.setCanvasSelection(CanvasSelection(
        shape: .rect(CGRect(x: 4, y: 8, width: 8, height: 8)), canvasWidth: 64, canvasHeight: 32))
}

private func drag(_ controller: EditorViewController, _ dx: Int, option: Bool = false) {
    controller.moveDidBegin(at: CGPoint(x: 30, y: 12), modifiers: option ? [.option] : [])
    controller.moveDidUpdate(dx, 0, [.control])
    controller.moveDidFinish()
}

private func selectionBounds(_ document: ImageDocument) -> CGRect? { document.canvasSelection?.bounds }

/// The preview image's pixel at (x, y), drawn into an sRGB RGBA8 context.
private func previewPixel(_ image: CGImage?, _ x: Int, _ y: Int) -> [UInt8] {
    guard let image, let space = CGColorSpace(name: CGColorSpace.sRGB) else { return [] }
    var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
    let drawn: Bool = pixels.withUnsafeMutableBytes { buffer in
        guard let context = CGContext(
            data: buffer.baseAddress, width: image.width, height: image.height,
            bitsPerComponent: 8, bytesPerRow: image.width * 4, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return true
    }
    guard drawn, x < image.width, y < image.height else { return [] }
    let p = (y * image.width + x) * 4
    return Array(pixels[p..<(p + 4)])
}

/// One drag to +dx held mid-way until its 8-bit preview lands; nil when no preview opened.
private func heldPreview(_ controller: EditorViewController, _ dx: Int, _ tag: String) throws -> CGImage? {
    controller.moveDidBegin(at: CGPoint(x: 30, y: 12), modifiers: [])
    controller.moveDidUpdate(dx, 0, [.control])
    try ensure(controller.canvas.moveSelectionOffset == CGPoint(x: dx, y: 0),
                "\(tag) the marquee moves by the drag mid-drag: "
                    + "\(String(describing: controller.canvas.moveSelectionOffset))")
    try pumpUntil({
        controller.canvas.previewImage != nil
            && controller.moveSelectedSession?.preview?.isPending == false
    }, seconds: 20)
    return controller.canvas.previewImage
}

func verifyMoveFloat() throws {
    guard let right = plainRight() else { throw ProbeFailure(description: "[move float] arrow event") }
    let wPoints = square(24), fPoints = square(4), hole = square(4)
    // (a)–(d) at 8 bits.
    do {
        let tag = "[move float 8]"
        let (document, controller) = try floatFixture()
        defer { document.close() }
        guard let base = document.doc else { throw ProbeFailure(description: "\(tag) base") }
        let w0 = try readings(document, wPoints)
        let f0 = try readings(document, fPoints)

        // (a) The first drag lifts F over W; the preview shows F at W mid-drag.
        let first = try heldPreview(controller, 20, "\(tag) (a)")
        let atW = previewPixel(first, 27, 11)
        try ensure(atW.count == 4 && atW[3] == 255 && atW[0] > 100,
                    "\(tag) (a) the drag preview shows F over W: \(atW)")
        controller.moveDidFinish()
        try ensure(document.undoManager?.undoActionName == "Move",
                    "\(tag) (a) one \"Move\" step: \(document.undoManager?.undoActionName ?? "-")")
        try ensure(readings(document, wPoints) == f0, "\(tag) (a) W reads F")
        try ensure(selectionBounds(document) == CGRect(x: 24, y: 8, width: 8, height: 8),
                    "\(tag) (a) the selection moved: \(String(describing: selectionBounds(document)))")
        try ensure(document.moveFloat != nil, "\(tag) (a) the drag left a float")
        let afterA = document.doc

        // (b) The second drag continues the float: W comes back.
        let second = try heldPreview(controller, 20, "\(tag) (b)")
        let restored = previewPixel(second, 27, 11)
        try ensure(restored.count == 4 && restored[3] == 255 && restored[0] < 30,
                    "\(tag) (b) the drag preview shows W restored: \(restored)")
        controller.moveDidFinish()
        try ensure(readings(document, wPoints) == w0,
                    "\(tag) (b) W equals its first samples: the float passed over it")
        try ensure(readings(document, square(44)) == f0, "\(tag) (b) F landed at x 44..51")
        try ensure(alphas(document, hole).allSatisfy { $0 == 0 }, "\(tag) (b) the hole at x 4..11")
        try ensure(selectionBounds(document) == CGRect(x: 44, y: 8, width: 8, height: 8),
                    "\(tag) (b) the selection at (44,8): \(String(describing: selectionBounds(document)))")
        try ensure(document.moveFloat != nil && document.undoManager?.undoActionName == "Move",
                    "\(tag) (b) the float persists; one \"Move\" step")
        let afterB = document.doc

        // (c) Esc cancels the drag only; a → nudge then continues the float.
        let selection = document.selectionHistory
        controller.moveDidBegin(at: CGPoint(x: 30, y: 12), modifiers: [])
        controller.moveDidUpdate(5, 0, [.control])
        controller.moveDidCancel()
        try ensure(document.doc === afterB && document.selectionHistory == selection
                    && controller.canvas.moveSelectionOffset == nil,
                    "\(tag) (c) Esc left the handle and the selection")
        try ensure(document.liveMoveFloat(layer: 0, target: .layer) != nil,
                    "\(tag) (c) the float survives Esc")
        controller.canvas.keyDown(with: right)
        try ensure(readings(document, square(45)) == f0 && (try readings(document, wPoints)) == w0,
                    "\(tag) (c) → lands F at x 45 with W intact")
        try ensure(document.undoManager?.undoActionName == "Move", "\(tag) (c) the nudge is one \"Move\"")

        // (d) One undo per step; the float survives undo past its lift and redo.
        document.undoManager?.undo()
        try ensure(document.doc === afterB, "\(tag) (d) undo restores the second drag's handle")
        document.undoManager?.undo()
        try ensure(document.doc === afterA, "\(tag) (d) undo restores the first drag's handle")
        document.undoManager?.undo()
        try ensure(document.doc === base && selectionBounds(document) == CGRect(x: 4, y: 8, width: 8, height: 8),
                    "\(tag) (d) undo past the lift restores the pixels and the selection")
        document.undoManager?.redo()
        try ensure(document.doc === afterA && document.liveMoveFloat(layer: 0, target: .layer) != nil,
                    "\(tag) (d) redo back onto the first drag continues the float")
        drag(controller, 20)
        try ensure(readings(document, wPoints) == w0 && (try readings(document, square(44))) == f0,
                    "\(tag) (d) after undo and redo the drag still restores W (U-STEP)")
    }

    // (e) Commits: each leaves F over W, commits, and the next drag lifts F
    // fresh from the composite, so W is gone.
    let commits: [(String, (ImageDocument, EditorViewController) throws -> Void)] = [
        ("deselect", { document, _ in
            _ = try agentOK("deselect", document)
            _ = try agentOK("select_rect", document, ["x": 24, "y": 8, "width": 8, "height": 8])
        }),
        ("layer switch", { _, controller in
            controller.setActiveLayer(1)
            controller.setActiveLayer(0)
        }),
        ("save", { document, _ in
            var finished = false
            var failure: Error?
            document.save(to: directory.appendingPathComponent("move-float-\(UUID().uuidString).rz"),
                          ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) {
                failure = $0
                finished = true
            }
            try pumpUntil({ finished })
            if let failure { throw failure }
        }),
        ("filter", { document, _ in
            _ = try agentOK("apply_filter", document, ["layer": 0, "filter": "blur", "sigma": 1])
        }),
    ]
    for (name, event) in commits {
        let tag = "[move float 8] (e) \(name)"
        let (document, controller) = try floatFixture()
        defer { document.close() }
        drag(controller, 20)
        try ensure(document.moveFloat != nil, "\(tag): the first drag left a float")
        try event(document, controller)
        try ensure(document.liveMoveFloat(layer: 0, target: .layer) == nil, "\(tag) committed the float")
        drag(controller, 20)
        try ensure(alphas(document, wPoints).allSatisfy { $0 == 0 },
                    "\(tag): the next drag lifted F fresh and left W empty")
    }

    // (f) ⌥: the duplicate floats (D-FLOAT); ⌥ over a live float stamps (A-STAMP).
    do {
        let tag = "[move float 8] (f)"
        let (document, controller) = try floatFixture()
        defer { document.close() }
        let w0 = try readings(document, wPoints)
        let f0 = try readings(document, fPoints)
        drag(controller, 20, option: true)
        try ensure(document.undoManager?.undoActionName == "Duplicate"
                    && (try readings(document, fPoints)) == f0 && (try readings(document, wPoints)) == f0,
                    "\(tag) ⌥-drag: the original stays and the copy covers W")
        drag(controller, 20)
        try ensure(readings(document, fPoints) == f0 && (try readings(document, wPoints)) == w0
                    && (try readings(document, square(44))) == f0,
                    "\(tag) a later drag moves the copy: the original stays and W comes back")

        let (stamped, stamping) = try floatFixture()
        defer { stamped.close() }
        drag(stamping, 20)
        drag(stamping, 20, option: true)
        try ensure(stamped.undoManager?.undoActionName == "Duplicate"
                    && (try readings(stamped, wPoints)) == f0 && (try readings(stamped, square(44))) == f0,
                    "\(tag) ⌥ over a live float leaves a copy at W and moves a new one")
        drag(stamping, 10)
        try ensure(readings(stamped, wPoints) == f0 && (try readings(stamped, square(54))) == f0
                    && (try alphas(stamped, [(45, 11), (52, 11)])).allSatisfy { $0 == 0 },
                    "\(tag) the stamped copy floats on: the stamp stays and x 44..53 comes back empty")
    }

    // (g) 16-bit, 32-bit float and CMYK: the same float, equal to MCP.
    let conversions: [(String, String, [String: Any])] = [
        ("16", "convert_precision", ["mode": "rgba16", "structure": "preserve"]),
        ("32", "convert_precision", ["mode": "rgba32", "structure": "preserve"]),
        // The probe sets no working CMYK profile, so it names macOS's Generic CMYK Profile.
        ("cmyk", "convert_mode", ["mode": "cmyk", "profile": "file",
                                  "path": "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"]),
    ]
    for (depth, tool, arguments) in conversions {
        let tag = "[move float \(depth)] (g)"
        let (document, controller) = try floatFixture()
        defer { document.close() }
        _ = try agentOK(tool, document, arguments)
        controller.setActiveLayer(0)
        selectF(document)
        controller.selectTool(.move)
        document.undoManager?.removeAllActions()
        guard let base = document.doc, base.isHighDepth || base.isCmyk else {
            throw ProbeFailure(description: "\(tag) the conversion did not land")
        }
        let w0 = try readings(document, wPoints)
        let f0 = try readings(document, fPoints)
        let all = wPoints + square(44) + hole
        drag(controller, 20)
        try ensure(readings(document, wPoints) == f0, "\(tag) the first drag covers W")
        drag(controller, 20)
        try ensure(readings(document, wPoints) == w0, "\(tag) the second drag restores W exactly")
        try ensure(readings(document, square(44)) == f0, "\(tag) F landed at x 44..51")
        let ui = try readings(document, all)
        document.undoManager?.undo()
        document.undoManager?.undo()
        try ensure(document.doc === base, "\(tag) two undos restore the converted handle")
        _ = try agentOK("move_selected_pixels", document, ["dx": 20, "dy": 0])
        _ = try agentOK("move_selected_pixels", document, ["dx": 20, "dy": 0])
        try ensure(readings(document, all) == ui, "\(tag) MCP dx 20 twice equals the drags")
    }
    print("PASS: move float — consecutive Move drags and nudges move ONE float (W restored, F at "
        + "44, the hole at 4), the preview and marquee follow mid-drag, Esc cancels only the drag, "
        + "one \"Move\" step per drag or nudge with the float surviving undo past its lift and "
        + "redo, Deselect / a layer switch / Save / a filter commit it, the ⌥ copy floats and ⌥ "
        + "over a float stamps, and 16-bit, 32-bit float and CMYK equal MCP")
}
