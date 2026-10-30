import AppKit

// The v1 quick fixes. Every case calls methods directly (the commit, the MCP
// dispatcher, the document), so none needs a key window.
//
// "crop delete cropped": the Crop tool's Delete Cropped Pixels trims each
// plain raster to the new canvas at 8 and 16 bits, from the UI commit and from
// MCP `crop delete_cropped:true`, for a plain crop and for a 5° straighten.
// "fill grows trimmed layer": Fill… and `fill_selection` with nothing
// selected grow a trimmed layer to its union with the canvas, as Stroke… does.
// "convert precision default": the Convert Precision sheet opens on
// Preserve Layers, and on Flatten with the core's sentence when that blocks.
// "new entries into group": New Layer and New Adjustment Layer with a
// group active land as its top child.
// "window memory": where a new editor window opens for a remembered frame.
// "long text line": a 1,000-character single line of prose renders at
// 24 px; a line is bounded by the outline's flattened-edge limit alone
// (524,288 edges, which 2,000 characters of prose pass).

private func v1Rect(_ doc: RasterDocument?, _ layer: Int) -> [Int] {
    doc?.layerInfo(layer).map { [$0.offsetX, $0.offsetY, $0.width, $0.height] } ?? []
}

/// `rect` of a `width`-wide RGBA buffer with `channels` values per pixel.
private func subrect<T>(_ pixels: [T], width: Int, channels: Int, rect: [Int]) -> [T] {
    var out: [T] = []
    out.reserveCapacity(rect[2] * rect[3] * channels)
    for y in rect[1]..<(rect[1] + rect[3]) {
        let start = (y * width + rect[0]) * channels
        out.append(contentsOf: pixels[start..<(start + rect[2] * channels)])
    }
    return out
}

private func samples16(_ doc: RasterDocument?, _ layer: Int) throws -> [UInt16] {
    guard let image = doc?.precision?.layerSource(layer) else {
        throw ProbeFailure(description: "layer \(layer) has no 16-bit source")
    }
    return try image.withRGBA16 { Array($0) }
}

// MARK: - Crop delete cropped

/// The Crop tool's commit over `rect` with Delete Cropped Pixels set to
/// `delete`; returns the steps it recorded, once the session it reopened is
/// closed and its preview worker has gone.
private func cropCommit(_ controller: EditorViewController, rect: CGRect,
                        delete: Bool, angle: Double = 0) throws -> [ActionStep] {
    let saved = ToolOptionsStore.shared.crop
    defer { ToolOptionsStore.shared.crop = saved }
    var options = saved
    options.deleteCroppedPixels = delete
    ToolOptionsStore.shared.crop = options
    controller.beginCropSession()
    controller.cropSession?.rect = rect
    controller.cropSession?.angle = angle
    ActionRecorder.shared.start()
    controller.commitCropSession()
    let steps = ActionRecorder.shared.stop()
    controller.endCropSession()
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
    return steps
}

private func verifyCropDeleteCropped(deep: Bool) throws {
    let tag = deep ? "[crop delete 16-bit]" : "[crop delete 8-bit]"
    let (width, height) = (64, 48)
    // The window, and the rect of an "Edge" layer that straddles its top-right
    // corner: after the crop it sits at (25, -4), so its part inside the new
    // canvas is (25, 0, 5, 6), read from the layer's own (0, 4, 5, 6).
    let crop = [10, 8, 30, 20]
    let rect = CGRect(x: crop[0], y: crop[1], width: crop[2], height: crop[3])
    let document = deep ? try precisionFixture(width: width, height: height)
        : try fixture(width: width, height: height)
    defer { document.close() }
    let controller = try editor(document)
    defer { _ = ActionRecorder.shared.stop() }
    if !deep {
        var patch = [UInt8](repeating: 255, count: 10 * 10 * 4)
        for i in 0..<100 {
            patch[i * 4] = UInt8(i)
            patch[i * 4 + 1] = UInt8(200 - i)
        }
        guard let image = RasterImage.from(rgba: patch, width: 10, height: 10) else {
            throw ProbeFailure(description: "\(tag) Edge layer image")
        }
        document.applyEdit("Add Edge", record: .notACommand) {
            try $0.addingImageLayer(above: 0, image, name: "Edge")?.withLayerOffset(1, 35, 4)
        }
        try require(v1Rect(document.doc, 1) == [35, 4, 10, 10], "\(tag) the Edge layer is placed")
    }
    document.undoManager?.removeAllActions()
    document.history.reset(document.captureEditingState())
    guard let base = document.doc, base.isHighDepth == deep else {
        throw ProbeFailure(description: "\(tag) fixture depth")
    }
    let alerts = AutomationAlerts.recorded.count

    // What the layers must hold after the trim, from the untouched source.
    let expected8: [UInt8] = deep ? []
        : subrect(base.layerPixels(0) ?? [], width: width, channels: 4, rect: crop)
    let expectedEdge: [UInt8] = deep ? []
        : subrect(base.layerPixels(1) ?? [], width: 10, channels: 4, rect: [0, 4, 5, 6])
    try require(deep || (expected8.count == crop[2] * crop[3] * 4 && expectedEdge.count == 120),
                "\(tag) the source pixels were read")
    let expected16: [UInt16] = try deep
        ? subrect(samples16(base, 0), width: width, channels: 4, rect: crop) : []
    func requireTrimmed(_ doc: RasterDocument?, _ step: String) throws {
        try require(doc?.width == crop[2] && doc?.height == crop[3],
                    "\(tag) \(step) the canvas is the window")
        try require(v1Rect(doc, 0) == [0, 0, crop[2], crop[3]],
                    "\(tag) \(step) layer 0 is trimmed to the canvas: \(v1Rect(doc, 0))")
        if deep {
            let kept = try samples16(doc, 0)
            try require(kept == expected16, "\(tag) \(step) layer 0 keeps the window's samples")
        } else {
            try require(doc?.layerPixels(0) == expected8,
                        "\(tag) \(step) layer 0 keeps the window's pixels")
            try require(v1Rect(doc, 1) == [25, 0, 5, 6],
                        "\(tag) \(step) Edge is its intersection with the canvas: \(v1Rect(doc, 1))")
            try require(doc?.layerPixels(1) == expectedEdge,
                        "\(tag) \(step) Edge keeps the pixels inside the canvas")
        }
    }
    func requireOneUndo(_ step: String) throws {
        try require(document.undoManager?.undoActionName == "Crop",
                    "\(tag) \(step) one \"Crop\" step: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) \(step) one undo restores the document")
    }

    // (a) The option off: the crop keeps the pixels outside the window.
    let plain = try cropCommit(controller, rect: rect, delete: false)
    try require(v1Rect(document.doc, 0) == [-crop[0], -crop[1], width, height],
                "\(tag) (a) a plain crop keeps layer 0 whole: \(v1Rect(document.doc, 0))")
    try require(plain.count == 1 && plain[0].tool == "crop"
                && plain[0].arguments["delete_cropped"] == nil,
                "\(tag) (a) the plain step has no delete_cropped: \(plain.map(\.arguments))")
    try requireOneUndo("(a)")

    // (b) The option on, from the Crop tool's commit.
    let trimmed = try cropCommit(controller, rect: rect, delete: true)
    try requireTrimmed(document.doc, "(b)")
    try require(trimmed.count == 1 && trimmed[0].tool == "crop"
                && trimmed[0].arguments["delete_cropped"] as? Bool == true,
                "\(tag) (b) the step records delete_cropped: \(trimmed.map(\.arguments))")
    try requireOneUndo("(b)")

    // (c) MCP crop delete_cropped:true lands the same document.
    ActionRecorder.shared.start()
    let reply = try agentOK("crop", document,
                            ["x": crop[0], "y": crop[1], "width": crop[2], "height": crop[3],
                             "delete_cropped": true])
    let agent = ActionRecorder.shared.stop()
    try require(reply["ok"] as? Bool == true, "\(tag) (c) MCP crop answered ok: \(reply)")
    try requireTrimmed(document.doc, "(c)")
    try require(agent.count == 1 && agent[0].tool == "crop"
                && agent[0].arguments["delete_cropped"] as? Bool == true,
                "\(tag) (c) the MCP step records delete_cropped: \(agent.map(\.arguments))")
    try requireOneUndo("(c)")

    // (d) MCP crop without the option keeps the pixels.
    _ = try agentOK("crop", document,
                    ["x": crop[0], "y": crop[1], "width": crop[2], "height": crop[3]])
    try require(v1Rect(document.doc, 0) == [-crop[0], -crop[1], width, height],
                "\(tag) (d) MCP crop without the option keeps layer 0 whole")
    try requireOneUndo("(d)")

    // (e) A 5° straighten. Without the option each layer keeps the whole
    // rotated raster, which reaches past the canvas; with it each plain raster
    // is that raster's intersection with the canvas, holding its samples.
    let angle = 5.0
    let layers = deep ? [0] : [0, 1]
    func wide(_ doc: RasterDocument?, _ layer: Int) throws -> [UInt16] {
        if deep { return try samples16(doc, layer) }
        guard let pixels = doc?.layerPixels(layer) else {
            throw ProbeFailure(description: "\(tag) layer \(layer) has no pixels")
        }
        return pixels.map(UInt16.init)
    }
    let kept = try cropCommit(controller, rect: rect, delete: false, angle: angle)
    guard let keptDoc = document.doc, keptDoc !== base else {
        throw ProbeFailure(description: "\(tag) (e) the plain straighten changed nothing")
    }
    try require(kept.count == 1 && kept[0].tool == "crop" && kept[0].arguments["angle"] != nil
                && kept[0].arguments["delete_cropped"] == nil,
                "\(tag) (e) the plain straighten's step: \(kept.map(\.arguments))")
    let whole = try layers.map { (rect: v1Rect(keptDoc, $0), data: try wide(keptDoc, $0)) }
    try require(whole[0].rect.count == 4 && whole[0].rect[0] < 0 && whole[0].rect[1] < 0
                && whole[0].rect[2] > crop[2] && whole[0].rect[3] > crop[3],
                "\(tag) (e) a plain straighten keeps layer 0 past the canvas: \(whole[0].rect)")
    try requireOneUndo("(e)")
    func requireStraightenTrimmed(_ doc: RasterDocument?, _ step: String) throws {
        try require(doc?.width == crop[2] && doc?.height == crop[3],
                    "\(tag) \(step) the canvas is the window")
        for (slot, layer) in layers.enumerated() {
            let source = whole[slot].rect
            let x0 = max(source[0], 0), y0 = max(source[1], 0)
            let x1 = min(source[0] + source[2], crop[2]), y1 = min(source[1] + source[3], crop[3])
            try require(x1 > x0 && y1 > y0, "\(tag) \(step) layer \(layer) meets the canvas")
            let inside = [x0, y0, x1 - x0, y1 - y0]
            try require(v1Rect(doc, layer) == inside,
                        "\(tag) \(step) layer \(layer) is the straightened raster's intersection "
                        + "with the canvas: \(v1Rect(doc, layer)) vs \(inside)")
            let expected = subrect(whole[slot].data, width: source[2], channels: 4,
                                   rect: [x0 - source[0], y0 - source[1], inside[2], inside[3]])
            let samples = try wide(doc, layer)
            try require(samples == expected,
                        "\(tag) \(step) layer \(layer) keeps the straightened samples inside")
        }
    }
    let cut = try cropCommit(controller, rect: rect, delete: true, angle: angle)
    try requireStraightenTrimmed(document.doc, "(e)")
    try require(cut.count == 1 && cut[0].tool == "crop" && cut[0].arguments["angle"] != nil
                && cut[0].arguments["delete_cropped"] as? Bool == true,
                "\(tag) (e) the straighten's step records delete_cropped: \(cut.map(\.arguments))")
    try requireOneUndo("(e)")

    // (f) The recorded step replayed over MCP lands the same document.
    _ = try agentOK("crop", document, cut[0].arguments)
    try requireStraightenTrimmed(document.doc, "(f)")
    try requireOneUndo("(f)")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
}

func verifyCropDeleteCropped() throws {
    try verifyCropDeleteCropped(deep: false)
    try verifyCropDeleteCropped(deep: true)
    print("PASS: crop delete cropped — with Delete Cropped Pixels the Crop tool's commit and MCP "
        + "crop delete_cropped:true trim each plain raster to its intersection with the new "
        + "canvas, keeping the samples inside, as one undo step that records the flag, for a "
        + "plain crop and a 5° straighten at 8 and 16 bits; without it the pixels outside are "
        + "kept")
}

// MARK: - Fill grows trimmed layer

func verifyFillGrowsTrimmedLayer() throws {
    let tag = "[fill trimmed]"
    let (width, height) = (64, 48)
    let document = try fixture(width: width, height: height)
    defer { document.close() }
    let controller = try editor(document)
    var patch = [UInt8](repeating: 255, count: 10 * 10 * 4)
    for i in 0..<100 {
        patch[i * 4] = 200
        patch[i * 4 + 1] = 80
        patch[i * 4 + 2] = 40
    }
    guard let image = RasterImage.from(rgba: patch, width: 10, height: 10) else {
        throw ProbeFailure(description: "\(tag) trimmed layer image")
    }
    document.applyEdit("Add Trimmed", record: .notACommand) {
        try $0.addingImageLayer(above: 0, image, name: "Trimmed")?.withLayerOffset(1, 20, 15)
    }
    document.undoManager?.removeAllActions()
    document.history.reset(document.captureEditingState())
    guard let base = document.doc, v1Rect(base, 1) == [20, 15, 10, 10] else {
        throw ProbeFailure(description: "\(tag) the trimmed layer: \(v1Rect(document.doc, 1))")
    }
    controller.setSelectedLayers(.single(1))
    try require(document.canvasSelection == nil && document.activeLayerIndex == 1,
                "\(tag) the trimmed layer is active and nothing is selected")
    let alerts = AutomationAlerts.recorded.count
    let blue: [UInt8] = [0, 0, 255, 255]
    func settings(preserve: Bool) -> FillSettings {
        FillSettings(contents: .color, customColor: NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1),
                     mode: DR_BLEND_NORMAL, opacity: 100, preserveTransparency: preserve)
    }
    func allBlue(_ pixels: [UInt8]?, count: Int) -> Bool {
        guard let pixels, pixels.count == count * 4 else { return false }
        return stride(from: 0, to: pixels.count, by: 4).allSatisfy { Array(pixels[$0..<$0 + 4]) == blue }
    }
    func requireOneUndo(_ step: String) throws {
        try require(document.undoManager?.undoActionName == "Fill",
                    "\(tag) \(step) one \"Fill\" step: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) \(step) one undo restores the document")
    }

    // (a) Fill… with nothing selected grows the layer to the canvas union.
    controller.applyFill(settings(preserve: false))
    try require(v1Rect(document.doc, 1) == [0, 0, width, height],
                "\(tag) (a) Fill… grew the layer to the canvas: \(v1Rect(document.doc, 1))")
    let ui = document.doc?.layerPixels(1)
    try require(allBlue(ui, count: width * height), "\(tag) (a) every canvas pixel is the fill colour")
    try requireOneUndo("(a)")

    // (b) Under Preserve Transparency the layer keeps its rect.
    controller.applyFill(settings(preserve: true))
    try require(document.doc !== base && v1Rect(document.doc, 1) == [20, 15, 10, 10],
                "\(tag) (b) Preserve Transparency keeps the rect: \(v1Rect(document.doc, 1))")
    try require(allBlue(document.doc?.layerPixels(1), count: 100),
                "\(tag) (b) the layer's own pixels are the fill colour")
    try requireOneUndo("(b)")

    // (c) MCP fill_selection lands the UI's result.
    let reply = try agentOK("fill_selection", document, ["layer": 1, "color": "#0000FF"])
    try require(reply["changed"] as? Bool == true, "\(tag) (c) fill_selection changed: \(reply)")
    try require(v1Rect(document.doc, 1) == [0, 0, width, height]
                && document.doc?.layerPixels(1) == ui,
                "\(tag) (c) MCP lands the UI's rect and pixels: \(v1Rect(document.doc, 1))")
    try requireOneUndo("(c)")

    // (d) …and keeps the rect under preserve_transparency.
    _ = try agentOK("fill_selection", document,
                    ["layer": 1, "color": "#0000FF", "preserve_transparency": true])
    try require(v1Rect(document.doc, 1) == [20, 15, 10, 10],
                "\(tag) (d) MCP preserve_transparency keeps the rect: \(v1Rect(document.doc, 1))")
    try requireOneUndo("(d)")

    // (e) A selection inside the layer's rect grows nothing.
    var inside = [UInt8](repeating: 0, count: width * height)
    for y in 17..<22 { for x in 22..<27 { inside[y * width + x] = 255 } }
    document.setCanvasSelection(CanvasSelection(shape: .mask(inside), canvasWidth: width,
                                                canvasHeight: height))
    document.undoManager?.removeAllActions()
    controller.applyFill(settings(preserve: false))
    try require(document.doc !== base && v1Rect(document.doc, 1) == [20, 15, 10, 10],
                "\(tag) (e) a selection inside the layer keeps its rect: \(v1Rect(document.doc, 1))")
    document.undoManager?.undo()
    document.setCanvasSelection(nil)
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
    print("PASS: fill grows trimmed layer — Fill… and fill_selection with nothing selected grow a "
        + "trimmed layer to the canvas and fill all of it as one undo step; Preserve Transparency "
        + "and a selection inside the layer keep its rect")
}

// MARK: - Convert precision default

private func sheetControl(_ sheet: ConvertPrecisionSheetController, _ label: String) throws -> Any {
    guard let value = Mirror(reflecting: sheet).children.first(where: { $0.label == label })?.value
    else { throw ProbeFailure(description: "Convert Precision sheet field \(label) is missing") }
    return value
}

func verifyConvertPrecisionDefault() throws {
    let tag = "[convert default]"
    // (a) A layered 8-bit document opens on Preserve Layers.
    let document = try fixture(width: 32, height: 24)
    defer { document.close() }
    let controller = try editor(document)
    document.applyEdit("Add", record: .notACommand) { try $0.addingLayer(above: 0, name: "Two") }
    guard let layered = document.doc, layered.layerCount == 2 else {
        throw ProbeFailure(description: "\(tag) a two-layer document")
    }
    let a = ConvertPrecisionSheetController.initialStructure(source: layered, mode: .rgba16)
    try require(a.structure == .preserve && a.refusal == nil,
                "\(tag) (a) a layered document opens on Preserve Layers: \(a)")
    let sheet = ConvertPrecisionSheetController(editor: controller, document: document)
    _ = sheet.view
    guard let preserve = try sheetControl(sheet, "preserveRadio") as? NSButton,
          let flatten = try sheetControl(sheet, "flattenRadio") as? NSButton,
          let status = try sheetControl(sheet, "statusLabel") as? NSTextField else {
        throw ProbeFailure(description: "\(tag) the sheet's radios and status line")
    }
    try require(preserve.state == .on && flatten.state == .off && status.stringValue.isEmpty,
                "\(tag) (a) the sheet's Preserve Layers radio is on with no status: "
                + "\"\(status.stringValue)\"")

    // (b) A 16-bit document opens on Preserve Layers too (the narrowing plan).
    let typed = try precisionFixture(width: 32, height: 24)
    defer { typed.close() }
    guard let deep = typed.doc, deep.isHighDepth else {
        throw ProbeFailure(description: "\(tag) a 16-bit document")
    }
    let b = ConvertPrecisionSheetController.initialStructure(source: deep, mode: .rgba8)
    try require(b.structure == .preserve && b.refusal == nil,
                "\(tag) (b) a 16-bit document opens on Preserve Layers: \(b)")

    // (c) A plan the core blocks: a host record (a Live Photo claim) on layer
    // 0. Flatten, with the core's sentence, in `initialStructure` and in the
    // sheet's radios and status line.
    let claim = #"{"type":"live_photo","version":999}"#
    document.applyEdit("Probe Claim", record: .notACommand) { $0.withLayerMeta(0, claim) }
    guard let claimed = document.doc, claimed.layerMeta(0) == claim,
          let name = claimed.layerInfo(0)?.name else {
        throw ProbeFailure(description: "\(tag) (c) the Live Photo claim was not applied")
    }
    let c = ConvertPrecisionSheetController.initialStructure(source: claimed, mode: .rgba16)
    let sentence = c.refusal ?? ""
    try require(c.structure == .flatten
                && sentence.hasPrefix("Preserve Layers cannot convert this document: "
                                      + "layer 0 \"\(name)\": ")
                && sentence.contains("Live Photo"),
                "\(tag) (c) a blocked plan opens on Flatten with the core's sentence: \(c)")
    let blocked = ConvertPrecisionSheetController(editor: controller, document: document)
    _ = blocked.view
    guard let blockedPreserve = try sheetControl(blocked, "preserveRadio") as? NSButton,
          let blockedFlatten = try sheetControl(blocked, "flattenRadio") as? NSButton,
          let blockedStatus = try sheetControl(blocked, "statusLabel") as? NSTextField else {
        throw ProbeFailure(description: "\(tag) (c) the sheet's radios and status line")
    }
    try require(blockedFlatten.state == .on && blockedPreserve.state == .off
                && blockedStatus.stringValue == sentence,
                "\(tag) (c) the sheet opens on Flatten and says why: "
                + "\"\(blockedStatus.stringValue)\"")

    // (d) The record cap. The most layers the host can build is 1,024 (the
    // core refuses to grow a document past it), and that many still convert,
    // so a 1,025-layer document — the cap's blocked plan — cannot be made
    // here; the core's own tests hold that row.
    guard var big = RasterDocument.blank(width: 8, height: 8, rgba: (255, 255, 255, 255)) else {
        throw ProbeFailure(description: "\(tag) a blank document")
    }
    while big.layerCount < 1024 {
        guard let next = try big.addingLayer(above: big.layerCount - 1,
                                             name: "L\(big.layerCount + 1)") else {
            throw ProbeFailure(description: "\(tag) (d) layer \(big.layerCount + 1) was not added")
        }
        big = next
    }
    let d = ConvertPrecisionSheetController.initialStructure(source: big, mode: .rgba16)
    try require(d.structure == .preserve && d.refusal == nil,
                "\(tag) (d) a 1,024-layer document opens on Preserve Layers: \(d)")
    let over = try big.addingLayer(above: big.layerCount - 1, name: "L1025")
    try require(over == nil, "\(tag) (d) the host cannot build a 1,025-layer document")
    print("PASS: convert precision default — the sheet opens on Preserve Layers for a layered "
        + "8-bit, a 16-bit and a 1,024-layer document, and on Flatten for a plan the core blocks, "
        + "saying \"\(sentence)\"")
}

// MARK: - New entries into group

private func verifyNewEntriesIntoGroup(deep: Bool) throws {
    let tag = deep ? "[into group 16-bit]" : "[into group 8-bit]"
    let document = deep ? try precisionFixture(width: 64, height: 48)
        : try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    defer { _ = ActionRecorder.shared.stop() }
    let grouped = try agentOK("group_layers", document, ["layers": [0]])
    guard let g = grouped["group"] as? Int, g == 1, let base = document.doc,
          let open = base.layerInfo(g)?.open, base.layerInfo(g)?.isGroup == true else {
        throw ProbeFailure(description: "\(tag) group_layers returned no group: \(grouped)")
    }
    let alerts = AutomationAlerts.recorded.count

    // (a) New Layer with the group active: its top child, selected.
    controller.setSelectedLayers(.single(g))
    ActionRecorder.shared.start()
    controller.newLayer(nil)
    let layerSteps = ActionRecorder.shared.stop()
    guard let one = document.doc, one !== base, one.layerCount == 3 else {
        throw ProbeFailure(description: "\(tag) (a) New Layer added no layer")
    }
    try require(one.layerInfo(1)?.isGroup == false && one.layerInfo(1)?.depth == 1
                && one.layerInfo(2)?.isGroup == true && one.layerInfo(2)?.depth == 0
                && one.layerInfo(0)?.depth == 1,
                "\(tag) (a) the new layer is the group's top child: "
                + "\((0..<3).map { one.layerInfo($0)?.depth ?? -1 })")
    try require(document.activeLayerIndex == 1, "\(tag) (a) the new layer is selected")
    try require(one.layerInfo(2)?.open == open, "\(tag) (a) the group's disclosure is unchanged")
    let recorded = layerSteps.filter { $0.tool == "new_layer" }
    try require(recorded.count == 1 && recorded[0].arguments["position"] as? String == "into_group",
                "\(tag) (a) the step records into_group: \(layerSteps.map(\.arguments))")

    // (b) The recorded step replays to the same place over MCP.
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) (b) one undo removes the new layer")
    controller.setSelectedLayers(.single(g))
    _ = try agentOK("new_layer", document, recorded[0].arguments)
    try require(document.doc?.layerCount == 3 && document.doc?.layerInfo(1)?.depth == 1
                && document.doc?.layerInfo(1)?.isGroup == false
                && document.doc?.layerInfo(2)?.isGroup == true,
                "\(tag) (b) the replayed step lands inside the group")

    // (c) A parameterless New Adjustment Layer with the group active.
    guard let two = document.doc else { throw ProbeFailure(description: "\(tag) (c) source") }
    controller.setSelectedLayers(.single(2))
    ActionRecorder.shared.start()
    controller.newAdjustmentLayer(.invert)
    let adjustmentSteps = ActionRecorder.shared.stop()
    guard let three = document.doc, three !== two, three.layerCount == 4 else {
        throw ProbeFailure(description: "\(tag) (c) New Adjustment Layer added no layer")
    }
    try require(three.layerIsAdjustment(2) && three.layerInfo(2)?.depth == 1
                && three.layerInfo(3)?.isGroup == true && three.layerInfo(3)?.depth == 0,
                "\(tag) (c) the adjustment is the group's top child: "
                + "\((0..<4).map { three.layerInfo($0)?.depth ?? -1 })")
    try require(document.activeLayerIndex == 2, "\(tag) (c) the adjustment is selected")
    try require(three.layerInfo(3)?.open == open, "\(tag) (c) the group's disclosure is unchanged")
    let adjustment = adjustmentSteps.filter { $0.tool == "add_adjustment_layer" }
    try require(adjustment.count == 1
                && adjustment[0].arguments["position"] as? String == "into_group",
                "\(tag) (c) the step records into_group: \(adjustmentSteps.map(\.arguments))")

    // (d) With a plain layer active both land above it and record no position.
    controller.setSelectedLayers(.single(0))
    ActionRecorder.shared.start()
    controller.newLayer(nil)
    controller.newAdjustmentLayer(.invert)
    let plain = ActionRecorder.shared.stop().filter {
        $0.tool == "new_layer" || $0.tool == "add_adjustment_layer"
    }
    try require(document.doc?.layerCount == 6 && document.activeLayerIndex == 2
                && document.doc?.layerInfo(1)?.depth == 1
                && document.doc?.layerIsAdjustment(2) == true,
                "\(tag) (d) a plain layer active lands each entry above it")
    try require(plain.count == 2 && plain.allSatisfy { $0.arguments["position"] == nil },
                "\(tag) (d) the steps record no position: \(plain.map(\.arguments))")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
}

func verifyNewEntriesIntoGroup() throws {
    try verifyNewEntriesIntoGroup(deep: false)
    try verifyNewEntriesIntoGroup(deep: true)
    print("PASS: new entries into group — with a group active New Layer and a parameterless New "
        + "Adjustment Layer land as its top child, select it, leave the disclosure alone and "
        + "record position into_group (which replays over MCP); with a plain layer active they "
        + "land above it and record no position, at 8 and 16 bits")
}

// MARK: - Window memory

func verifyWindowMemory() throws {
    let tag = "[window memory]"
    try require(AutomationAlerts.isEnabled && !EditorWindowMemory.isEnabled,
                "\(tag) the runner turns the memory off")

    // Placement, on a synthetic 1440 × 900 display.
    let screen = CGRect(x: 0, y: 0, width: 1440, height: 900)
    let frame = CGRect(x: 100, y: 200, width: 800, height: 600)
    let step = EditorWindowMemory.cascadeStep
    func place(_ frame: CGRect, _ occupied: [CGRect]) -> CGRect {
        EditorWindowMemory.placement(frame, occupied: occupied, visibleFrames: [screen],
                                     fallback: screen)
    }
    try require(step == 22, "\(tag) the cascade step is 22 points: \(step)")
    try require(place(frame, []) == frame, "\(tag) (a) with no window open the frame is kept")
    try require(place(frame, [CGRect(x: 400, y: 100, width: 800, height: 600)]) == frame,
                "\(tag) (b) a window elsewhere moves nothing")
    try require(place(frame, [frame]) == frame.offsetBy(dx: step, dy: -step),
                "\(tag) (c) a window at the same origin steps it once: \(place(frame, [frame]))")
    let twice = frame.offsetBy(dx: 2 * step, dy: -2 * step)
    try require(place(frame, [frame, frame.offsetBy(dx: step, dy: -step)]) == twice
                && place(frame, [frame.offsetBy(dx: step, dy: -step), frame]) == twice,
                "\(tag) (d) two cascaded windows step it twice, in either order")
    // A different size at the same origin still counts.
    try require(place(frame, [CGRect(x: 100, y: 200, width: 300, height: 200)])
                    == frame.offsetBy(dx: step, dy: -step),
                "\(tag) (e) the origin alone decides")
    let off = place(CGRect(x: 5000, y: 5000, width: 800, height: 600), [])
    try require(screen.contains(off) && off.size == CGSize(width: 800, height: 600),
                "\(tag) (f) a frame off every display is moved onto the fallback: \(off)")
    let huge = place(CGRect(x: 0, y: 0, width: 4000, height: 3000), [])
    try require(huge == screen, "\(tag) (g) a frame larger than the display is shrunk to it: \(huge)")
    let edge = CGRect(x: 640, y: 0, width: 800, height: 600)
    try require(screen.contains(place(edge, [edge])),
                "\(tag) (h) a cascade past the display's edge is fitted back: \(place(edge, [edge]))")

    // Load and save, through a private suite.
    let suite = "com.kgalligan.doctored.jobs-probe.window-memory.\(getpid())"
    guard let defaults = UserDefaults(suiteName: suite) else {
        throw ProbeFailure(description: "\(tag) a private defaults suite")
    }
    defer { defaults.removePersistentDomain(forName: suite) }
    try require(EditorWindowMemory.load(from: defaults) == nil, "\(tag) (i) nothing stored loads nil")
    let memory = EditorWindowMemory(frame: frame, panelVisible: false,
                                    panelTab: EditorPanelTab.channels.name)
    memory.save(to: defaults)
    try require(EditorWindowMemory.load(from: defaults) == memory,
                "\(tag) (j) a saved memory loads back: "
                + "\(String(describing: EditorWindowMemory.load(from: defaults)))")
    try require(EditorPanelTab(name: memory.panelTab) == .channels, "\(tag) (j) the tab is a name")
    EditorWindowMemory(frame: CGRect(x: 10, y: 10, width: 0, height: 600), panelVisible: true,
                       panelTab: "layers").save(to: defaults)
    try require(EditorWindowMemory.load(from: defaults) == nil, "\(tag) (k) an empty frame loads nil")
    defaults.set(Data("not a memory".utf8), forKey: EditorWindowMemory.defaultsKey)
    try require(EditorWindowMemory.load(from: defaults) == nil, "\(tag) (l) unreadable data loads nil")

    // Under the runner a real window neither reads nor writes the app's own
    // memory, and the panel half of a layout applies through `applyPanelLayout`.
    let stored = UserDefaults.standard.data(forKey: EditorWindowMemory.defaultsKey)
    let document = try fixture(width: 64, height: 48)
    let controller = try editor(document)
    guard let window = document.windowControllers.first?.window else {
        document.close()
        throw ProbeFailure(description: "\(tag) an editor window")
    }
    let saved = controller.captureWorkspace(name: "")
    window.setFrameOrigin(NSPoint(x: -9000, y: -9000))
    controller.applyPanelLayout(visible: false, tab: .channels)
    let hidden = controller.captureWorkspace(name: "")
    controller.applyPanelLayout(visible: true, tab: .navigator)
    let shown = controller.captureWorkspace(name: "")
    controller.applyPanelLayout(visible: saved.panelVisible, tab: saved.panelTab)
    document.close()
    try require(!hidden.panelVisible && hidden.panelTab == .channels
                && shown.panelVisible && shown.panelTab == .navigator,
                "\(tag) (m) applyPanelLayout sets the panel's visibility and tab")
    try require(UserDefaults.standard.data(forKey: EditorWindowMemory.defaultsKey) == stored,
                "\(tag) (n) a moved and closed window wrote no memory under the runner")
    print("PASS: window memory — a remembered frame is kept, stepped (22, −22) off each open "
        + "window at its origin and fitted onto a display; the memory round-trips through "
        + "UserDefaults and refuses an empty or unreadable one; and it is off under the runner")
}

// MARK: - Long text line

func verifyLongTextLine() throws {
    let tag = "[long text line]"
    let document = try fixture(width: 400, height: 80)
    defer { document.close() }
    _ = try editor(document)
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
    func line(_ words: String, _ count: Int) -> String {
        String(String(repeating: words, count: count / words.count + 1).prefix(count))
    }
    let alerts = AutomationAlerts.recorded.count

    /// One 24 px point-text line through MCP: it must render as ONE line,
    /// wider than `minimumWidth`, with opaque ink and ink in its last tenth,
    /// and one undo must remove it. Returns "<w> × <h>, <n> inked pixels".
    func rendered(_ text: String, _ step: String, minimumWidth: Int) throws -> String {
        try require(!text.contains("\n"), "\(tag) \(step) a single line")
        let reply = try agentOK("add_text_layer", document,
                                ["text": text, "x": 4, "y": 20, "size": 24, "wrap_width": 0])
        guard let doc = document.doc, doc !== base,
              let layer = (0..<doc.layerCount).first(where: { doc.hasArtworkDescription($0) }),
              let info = doc.layerInfo(layer), let pixels = doc.layerPixels(layer) else {
            throw ProbeFailure(description: "\(tag) \(step) add_text_layer made no text layer: \(reply)")
        }
        // One line: far wider than the canvas, and under two 24 px lines tall.
        try require(info.width > minimumWidth && info.height < 60,
                    "\(tag) \(step) the layer is one long line: \(info.width) × \(info.height)")
        let inked = stride(from: 3, to: pixels.count, by: 4).filter { pixels[$0] != 0 }.count
        let opaque = stride(from: 3, to: pixels.count, by: 4).contains { pixels[$0] == 255 }
        let tail = info.width - info.width / 10
        var tailInk = false
        scan: for y in 0..<info.height {
            for x in tail..<info.width where pixels[(y * info.width + x) * 4 + 3] != 0 {
                tailInk = true
                break scan
            }
        }
        try require(inked > 1000 && opaque && tailInk,
                    "\(tag) \(step) the line has ink to its end: \(inked) inked pixels, "
                    + "opaque \(opaque), tail \(tailInk)")
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) \(step) one undo removes the layer")
        return "\(info.width) × \(info.height), \(inked) inked pixels"
    }

    // (a) 1,000 characters of prose: past the ~300 the row-wide sweep took,
    // and past the 581 the 262,144-edge bound stopped before it was raised.
    let prose = "The quick brown fox jumps over the lazy dog. "
    let thousand = line(prose, 1000)
    try require(thousand.count == 1000, "\(tag) (a) 1,000 characters")
    let a = try rendered(thousand, "(a)", minimumWidth: 9000)

    // (b) 2,000 characters are past the outline's flattened-edge bound
    // (524,288; prose is about 450 edges a character at 24 px). They refuse
    // with that bound's own sentence, never the scan-work one, and change
    // nothing.
    let over = try agentCall("add_text_layer", document,
                             ["text": line(prose, 2000), "x": 4, "y": 20, "size": 24,
                              "wrap_width": 0])
    try require(over.isError && over.text.contains("artwork outline exceeds the flattened edge limit")
                && document.doc === base && document.undoManager?.canUndo != true,
                "\(tag) (b) 2,000 characters of prose refuse at the edge bound only: \(over.text)")

    // (c) The bound's coverage bookkeeping (106 MiB) is reserved by one
    // coverage call at a time, so a geometry job over two text layers charges
    // it once beside each layer's own planes, and the straighten still
    // re-renders both.
    for (index, text) in ["First probe line", "Second probe line"].enumerated() {
        _ = try agentOK("add_text_layer", document,
                        ["text": text, "x": 10 + 40 * index, "y": 4 + 30 * index, "size": 24])
    }
    guard let two = document.doc, two.artworkIndices.count == 2 else {
        throw ProbeFailure(description: "\(tag) (c) two text layers")
    }
    let work = try ArtworkGeometryWork(source: two,
                                       affineMap: CGAffineTransform(rotationAngle: 0.05),
                                       canvas: (two.width, two.height))
    let bookkeeping = NativeArtworkEvaluation.coverageBookkeepingBytes
    let own = work.members.reduce(UInt64(0)) { $0 + $1.evaluation.ownScratchBytes }
    try require(work.members.count == 2 && bookkeeping == 106 * 1024 * 1024
                && work.scratchBytes == bookkeeping + own
                && work.members.allSatisfy {
                    $0.evaluation.scratchBytes == bookkeeping + $0.evaluation.ownScratchBytes
                },
                "\(tag) (c) two text layers charge the coverage bookkeeping once: "
                + "\(work.scratchBytes) vs \(bookkeeping) + \(own)")
    let turned = try agentOK("crop", document,
                             ["x": 20, "y": 10, "width": 300, "height": 60, "angle": 3])
    try require(turned["ok"] as? Bool == true && turned["rasterized_layers"] == nil
                && document.doc !== two && document.doc?.artworkIndices.count == 2,
                "\(tag) (c) the straighten re-rendered both text layers: \(turned)")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
    print("PASS: long text line — MCP add_text_layer renders a 1,000-character single line of "
        + "prose at 24 px (\(a)); 2,000 characters refuse with the flattened-edge sentence "
        + "alone; two text layers straighten under one \(bookkeeping)-byte coverage charge")
}
