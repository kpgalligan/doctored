import AppKit

// Filters ▸ Liquify…: the workspace's sheet, driven through its own stroke entry points and
// its real buttons, at 8 and at 16 bits. The fixture is three vertical colour bands (red to x = 85,
// then green), so a 5 px push to the right across the first boundary has a known answer: the core's
// translation oracle (`core/tests/filter_liquify.rs`) says every pixel within 18 px of the stroke's
// end is the source pixel 5 px to its left.

private let liquifyWidth = 256, liquifyHeight = 192

private func liquifyControl<T: NSView>(_ sheet: LiquifySheetController,
                                       _ id: String, as type: T.Type) throws -> T {
    guard let value = allViews(sheet.view).first(where: { $0.identifier?.rawValue == "liquify.\(id)" }) as? T else {
        throw ProbeFailure(description: "Missing liquify control \(id)")
    }
    return value
}

/// Size 40, Pressure 100, Density 100, typed into the sheet's fields.
private func setLiquifyBrush(_ sheet: LiquifySheetController) throws {
    for (id, value) in [("size", "40"), ("pressure", "100"), ("density", "100")] {
        let field = try liquifyControl(sheet, id, as: NSTextField.self)
        field.stringValue = value
        _ = field.sendAction(field.action, to: field.target)
    }
}

private func liquifyPush(_ sheet: LiquifySheetController, from: CGPoint, to: CGPoint) throws {
    try require(sheet.strokeBegan(at: from, modifiers: []), "A liquify stroke begins at \(from)")
    sheet.strokeMoved(to: to)
    sheet.strokeEnded()
}

private func liquifyFrame(_ image: CGImage?) -> Data? { image?.dataProvider?.data as Data? }

private func liquifyBytes(_ image: RasterImage?) throws -> [UInt8] {
    guard let image, let bytes = image.withPlateBytes({
        Array(UnsafeBufferPointer(start: $0, count: image.width * image.height * 4))
    }) else { throw ProbeFailure(description: "Liquify image bytes") }
    return bytes
}

/// Layer 0 as RGBA8: its own bytes at 8 bits, its full-size byte rendering at 16.
private func liquifyLayer(_ document: ImageDocument) throws -> [UInt8] {
    guard let doc = document.doc else { throw ProbeFailure(description: "Liquify document") }
    if !doc.isHighDepth {
        guard let pixels = doc.layerPixels(0) else { throw ProbeFailure(description: "Liquify layer pixels") }
        return pixels
    }
    let image = doc.layerThumbnail(0, maxSide: liquifyWidth)
    try require(image?.width == liquifyWidth && image?.height == liquifyHeight,
                "The 16-bit layer reads back at its own size")
    return try liquifyBytes(image)
}

private func liquifyPixel(_ bytes: [UInt8], _ x: Int, _ y: Int) -> [UInt8] {
    let offset = (y * liquifyWidth + x) * 4
    return Array(bytes[offset..<offset + 4])
}

/// The canvas view as it draws itself: the probe's own view, drawn into a bitmap.
private func liquifySnapshot(_ view: NSView) throws -> NSBitmapImageRep {
    guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
        throw ProbeFailure(description: "Liquify canvas snapshot")
    }
    view.cacheDisplay(in: view.bounds, to: bitmap)
    return bitmap
}

/// The drawn colour at the centre of proxy pixel (x, y), row 0 at the top.
private func liquifyDrawn(_ bitmap: NSBitmapImageRep, _ x: Int, _ y: Int) throws -> (r: CGFloat, g: CGFloat, b: CGFloat) {
    let scale = Double(bitmap.pixelsWide) / Double(liquifyWidth)
    guard let color = bitmap.colorAt(x: Int((Double(x) + 0.5) * scale), y: Int((Double(y) + 0.5) * scale))?
        .usingColorSpace(.sRGB) else { throw ProbeFailure(description: "Liquify canvas pixel \(x), \(y)") }
    return (color.redComponent, color.greenComponent, color.blueComponent)
}

private func verifyLiquify(deep: Bool) throws {
    let depth = deep ? "16-bit" : "8-bit"
    let document = try fixture(width: liquifyWidth, height: liquifyHeight)
    defer { document.close() }
    guard let source = document.doc?.layerImage(0), let original = document.doc?.layerPixels(0) else {
        throw ProbeFailure(description: "Liquify fixture layer")
    }
    if deep {
        _ = try call(document, "convert_precision", ["mode": "rgba16", "structure": "preserve"])
        document.undoManager?.removeAllActions()
        try require(document.doc?.isHighDepth == true, "The fixture converts to 16 bits")
    }
    let controller = try editor(document)
    guard let before = document.doc else { throw ProbeFailure(description: "Liquify document") }

    // The menu's own path: validation, then Filters ▸ Liquify… presents the workspace.
    let item = NSMenuItem(title: "Liquify…", action: #selector(EditorViewController.showLiquify(_:)),
                          keyEquivalent: "")
    try require(controller.validateUserInterfaceItem(item), "\(depth): Filters ▸ Liquify… is enabled on a raster layer")
    controller.showLiquify(nil)
    guard let sheet = controller.presentedViewControllers?.last as? LiquifySheetController else {
        throw ProbeFailure(description: "\(depth): Filters ▸ Liquify… did not present the workspace")
    }
    try pumpUntil({ sheet.view.window != nil }, message: "the Liquify workspace to appear")
    sheet.view.window?.animationBehavior = .none
    try require(SelectionWorkBudget.scratchBytes > 0, "\(depth): the open workspace holds its reservation")
    try setLiquifyBrush(sheet)
    let apply = try liquifyControl(sheet, "apply", as: NSButton.self)
    let idle = liquifyFrame(sheet.canvasView.image)
    try require(idle != nil && !apply.isEnabled, "\(depth): the workspace shows the layer and Apply waits for a stroke")
    let start = CGPoint(x: 80, y: 60), end = CGPoint(x: 85, y: 60)
    try liquifyPush(sheet, from: start, to: end)
    let strokes = sheet.session.strokes
    try require(strokes == [LiquifyStroke(tool: .forwardWarp, size: 40, strength: 100, density: 100,
                                          points: [start, end])],
                "\(depth): the drag leaves one Forward Warp stroke with the sheet's settings: \(strokes)")
    let pushed = liquifyFrame(sheet.canvasView.image)
    try require(pushed != idle && apply.isEnabled, "\(depth): the stroke changes the preview and enables Apply")
    try require(document.doc === before && document.undoManager?.canUndo != true,
                "\(depth): the preview leaves the document untouched")

    // The full-size result the core gives the same strokes on the 8-bit source.
    let expected = try liquifyBytes(try source.filterOp(
        "liquify", params: LiquifyStroke.kernelParameters(strokes, origin: (x: 0, y: 0))))
    apply.performClick(nil)
    try require(document.doc !== before && document.undoManager?.canUndo == true,
                "\(depth): Apply changes the document handle: \(sheet.status.stringValue)")
    let applied = try liquifyLayer(document)
    try require(liquifyPixel(original, 83, 60) != liquifyPixel(original, 88, 60)
                && liquifyPixel(applied, 88, 60) == liquifyPixel(original, 83, 60),
                "\(depth): layer pixel (88, 60) is the original (83, 60): \(liquifyPixel(applied, 88, 60))")
    if deep {
        // Within one level of the byte result wherever the layer is visible.
        var worst = 0
        for pixel in 0..<liquifyWidth * liquifyHeight where expected[pixel * 4 + 3] > 0 {
            for channel in 0..<4 {
                worst = max(worst, abs(Int(applied[pixel * 4 + channel]) - Int(expected[pixel * 4 + channel])))
            }
        }
        try require(worst <= 1, "16-bit: the layer agrees with the byte filter within one level: \(worst)")
    } else {
        try require(applied == expected, "8-bit: the layer equals filterOp(\"liquify\") of the same strokes")
    }
    document.undoManager?.undo()
    try require(document.doc === before && document.undoManager?.canUndo != true,
                "\(depth): one Undo restores the handle")

    // Undo Stroke, Restore All and Cancel on a second workspace.
    let second = try LiquifySheetController(editor: controller)
    try present(second, in: controller)
    let proxy = try liquifyBytes(second.session.proxy)
    try setLiquifyBrush(second)
    try liquifyPush(second, from: start, to: end)
    // Across the green/blue boundary at x = 171, so this one shows too.
    try liquifyPush(second, from: CGPoint(x: 166, y: 100), to: CGPoint(x: 172, y: 100))
    let two = liquifyFrame(second.canvasView.image)
    try require(second.session.strokes.count == 2 && two != pushed, "\(depth): a second stroke joins the list")
    try liquifyControl(second, "undo", as: NSButton.self).performClick(nil)
    // The list replayed whole and rendered in full is the picture the first
    // workspace built a segment and a rectangle at a time.
    try require(second.session.strokes == strokes && liquifyFrame(second.canvasView.image) == pushed,
                "\(depth): Undo Stroke drops the last stroke and rebuilds the one-stroke preview")
    let restore = try liquifyControl(second, "restore", as: NSButton.self)
    restore.performClick(nil)
    let restored = try liquifyBytes(second.session.preview)
    let secondApply = try liquifyControl(second, "apply", as: NSButton.self)
    try require(second.session.strokes.isEmpty && restored == proxy
                && liquifyFrame(second.canvasView.image) == idle
                && !secondApply.isEnabled && !restore.isEnabled,
                "\(depth): Restore All empties the list and returns the preview to the proxy's bytes")
    try liquifyPush(second, from: start, to: end)
    try require(second.session.strokes.count == 1, "\(depth): a stroke after Restore All starts a new list")

    // ⌥ reverses the tool, and a tool that dwells keeps working while the pointer rests.
    let tools = try liquifyControl(second, "tool", as: NSPopUpButton.self)
    func choose(_ tool: LiquifyTool) throws {
        guard let index = LiquifySheetController.tools.firstIndex(of: tool) else {
            throw ProbeFailure(description: "Liquify tool \(tool)")
        }
        tools.selectItem(at: index)
        _ = tools.sendAction(tools.action, to: tools.target)
    }
    try choose(.pucker)
    try require(second.strokeBegan(at: CGPoint(x: 86, y: 100), modifiers: [.option])
                && second.session.draft?.tool == .bloat, "\(depth): ⌥ at mouse-down turns Pucker into Bloat")
    try pumpUntil({ (second.session.draft?.points.count ?? 0) >= 3 }, message: "a resting Bloat to dwell")
    second.strokeEnded()
    try require(second.session.strokes.count == 2 && second.session.strokes.last?.tool == .bloat
                && second.session.draft == nil, "\(depth): the dwelling stroke commits as Bloat")

    // What the view draws: the picture the right way up, the freeze tint and the mesh.
    let plain = try liquifySnapshot(second.canvasView)
    let top = try liquifyDrawn(plain, 10, 2), hidden = try liquifyDrawn(plain, 10, 190)
    let band = try liquifyDrawn(plain, 200, 150), bare = try liquifyDrawn(plain, 50, 40)
    let column = try liquifyDrawn(plain, 16, 150)
    try require(top.r > max(top.g, top.b) + 0.3 && min(hidden.r, hidden.g, hidden.b) > 0.8
                && band.b > max(band.r, band.g) + 0.3,
                "\(depth): the view draws red at the top, blue on the right and the checkerboard under the transparent rows: \(top) \(hidden) \(band)")
    try choose(.freeze)
    try liquifyPush(second, from: CGPoint(x: 40, y: 40), to: CGPoint(x: 60, y: 40))
    try liquifyControl(second, "mesh", as: NSButton.self).performClick(nil)
    try require(second.canvasView.freezeImage != nil && second.canvasView.meshImage != nil,
                "\(depth): Show Mask and Show Mesh hand the view their overlays")
    let dressed = try liquifySnapshot(second.canvasView)
    let frozen = try liquifyDrawn(dressed, 50, 40), free = try liquifyDrawn(dressed, 50, 130)
    let line = try liquifyDrawn(dressed, 16, 150)
    try require(frozen.g > bare.g + 0.08 && free.g < bare.g + 0.04,
                "\(depth): the freeze tint covers the painted disc only: \(bare) \(frozen) \(free)")
    try require(column.r > column.g && line.g > line.r,
                "\(depth): the mesh draws its lines over the picture: \(column) \(line)")

    try liquifyControl(second, "cancel", as: NSButton.self).performClick(nil)
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the liquify reservation to release")
    try require(document.doc === before && document.undoManager?.canUndo != true,
                "\(depth): Cancel leaves the document untouched")

    // With a selection the workspace shows, and Apply writes, the warp inside it only: the push
    // turns x = 86…90 red, and the selection ends at x = 88.
    document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 60, y: 40, width: 28, height: 40)),
        canvasWidth: liquifyWidth, canvasHeight: liquifyHeight))
    let clipped = try LiquifySheetController(editor: controller)
    try present(clipped, in: controller)
    try setLiquifyBrush(clipped)
    try require(clipped.session.clipMask?[60 * liquifyWidth + 87] == 255
                && clipped.session.clipMask?[60 * liquifyWidth + 88] == 0,
                "\(depth): the workspace maps the selection onto the proxy")
    try liquifyPush(clipped, from: start, to: end)
    let confined = try liquifySnapshot(clipped.canvasView)
    let inside = try liquifyDrawn(confined, 87, 60), outside = try liquifyDrawn(confined, 88, 60)
    let flat = try liquifyDrawn(confined, 70, 60), far = try liquifyDrawn(confined, 200, 150)
    // The bitmap is in the display's space, so the bands are told apart by which channel leads.
    try require(inside.r > inside.g + 0.3 && outside.g > outside.r + 0.3
                && flat.r > flat.g + 0.3 && far.b > max(far.r, far.g) + 0.3,
                "\(depth): the view shows the warp inside the selection and the layer outside it, each once: \(inside) \(outside) \(flat) \(far)")
    try liquifyControl(clipped, "apply", as: NSButton.self).performClick(nil)
    let selected = try liquifyLayer(document)
    try require(liquifyPixel(selected, 87, 60) == liquifyPixel(original, 82, 60)
                && liquifyPixel(selected, 88, 60) == liquifyPixel(original, 88, 60),
                "\(depth): Apply writes the warp inside the selection only: \(clipped.status.stringValue)")
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the applied workspace's reservation to release")
}

/// The MCP twin and the recorded step: the sheet's Apply on one fixture under a running recording,
/// `liquify` with the same strokes on a second, then the recorded step replayed on the second.
private func verifyLiquifyTwin(deep: Bool) throws {
    let depth = deep ? "16-bit" : "8-bit"
    let document = try fixture(width: liquifyWidth, height: liquifyHeight)
    let twin = try fixture(width: liquifyWidth, height: liquifyHeight)
    defer { _ = ActionRecorder.shared.stop(); document.close(); twin.close() }
    if deep {
        for each in [document, twin] {
            _ = try call(each, "convert_precision", ["mode": "rgba16", "structure": "preserve"])
            each.undoManager?.removeAllActions()
        }
    }
    let untouched = try liquifyLayer(twin)
    let controller = try editor(document)
    let sheet = try LiquifySheetController(editor: controller)
    try present(sheet, in: controller)
    try setLiquifyBrush(sheet)
    try liquifyPush(sheet, from: CGPoint(x: 80, y: 60), to: CGPoint(x: 85, y: 60))
    try liquifyPush(sheet, from: CGPoint(x: 166, y: 100), to: CGPoint(x: 172, y: 100))
    let strokes = sheet.session.strokes
    ActionRecorder.shared.start()
    try liquifyControl(sheet, "apply", as: NSButton.self).performClick(nil)
    let recorded = ActionRecorder.shared.stop()
    let applied = try liquifyLayer(document)
    try require(applied != untouched, "\(depth): the sheet's Apply changes the layer: \(sheet.status.stringValue)")
    try require(recorded.count == 1 && recorded.first?.tool == "liquify" && recorded.first?.isUnrecorded != true,
                "\(depth): Apply records exactly one liquify step: \(recorded.map(\.tool))")
    let step = recorded[0]
    let parsed = try LiquifyStroke.parse(step.arguments["strokes"])
    try require(parsed == strokes && strokes.count == 2
                && (step.arguments["layer"] as? [String: Any])?["$layer"] as? String == "active"
                && Set(step.arguments.keys) == ["layer", "strokes"],
                "\(depth): the recorded step carries the sheet's strokes on the active layer: \(step.arguments)")

    let reply = try call(twin, "liquify", ["strokes": strokes.map(\.arguments)])
    try require(reply["changed"] as? Bool == true && reply["strokes"] as? Int == 2
                && reply["layer"] as? Int == 0 && reply["target"] as? String == "layer",
                "\(depth): liquify reports the edit: \(reply)")
    let called = try liquifyLayer(twin)
    try require(called == applied && twin.undoManager?.canUndo == true,
                "\(depth): liquify gives the same layer bytes as the sheet's Apply")
    twin.undoManager?.undo()
    let reverted = try liquifyLayer(twin)
    try require(reverted == untouched && twin.undoManager?.canUndo != true,
                "\(depth): one Undo reverses liquify")
    let played = ActionPlayer.run(Action(name: "Liquify replay probe \(depth)", steps: recorded), on: twin)
    let replayed = try liquifyLayer(twin)
    try require(played.ok && replayed == applied,
                "\(depth): the recorded step replays to the same layer bytes")
    try pumpUntil({ SelectionWorkBudget.scratchBytes == 0 }, message: "the twin workspace's reservation to release")
}

/// A Liquify step spends the recording's point budget by the points inside its strokes.
private func verifyLiquifyRecordingBudget() throws {
    let document = try fixture(width: liquifyWidth, height: liquifyHeight)
    defer { _ = ActionRecorder.shared.stop(); document.close() }
    let stroke = LiquifyStroke(tool: .forwardWarp, size: 40, strength: 50, density: 50,
                               points: Array(repeating: CGPoint(x: 10, y: 10), count: LiquifyStroke.maxStrokePoints))
    let heavy = Array(repeating: stroke, count: 13)
    try require(heavy.count * LiquifyStroke.maxStrokePoints > ActionRecorder.maxRecordedPoints / 2
                && heavy.count * LiquifyStroke.maxStrokePoints < ActionRecorder.maxRecordedPoints,
                "Two heavy liquify steps cross the recording budget and one does not")
    ActionRecorder.shared.start()
    for _ in 0..<3 { ActionRecorder.shared.record(.liquify(heavy), on: document) }
    let bounded = ActionRecorder.shared.stop()
    try require(bounded.filter { $0.tool == "liquify" }.count == 2 && bounded.count == 3
                && bounded.last?.isUnrecorded == true,
                "Liquify stroke points participate in the recording size limit: \(bounded.map(\.tool))")
}

func verifyLiquifyWorkspace() throws {
    try verifyLiquify(deep: false)
    try verifyLiquify(deep: true)
    try verifyLiquifyTwin(deep: false)
    try verifyLiquifyTwin(deep: true)
    try verifyLiquifyRecordingBudget()
    print("PASS: Liquify workspace stroke, preview, Apply against the core filter, one Undo, Undo Stroke, Restore All and Cancel at 8 and 16 bits")
    print("PASS: liquify MCP twin equals the sheet's Apply, and Apply records one liquify step that replays, at 8 and 16 bits")
}
