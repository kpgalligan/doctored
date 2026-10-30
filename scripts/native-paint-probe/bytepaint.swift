import AppKit
import CryptoKit

func openBytePaintDocument(opaque: Bool = false, p3: Bool = false) throws
    -> (ImageDocument, EditorViewController, RasterDocument) {
    if !opaque && !p3 { return try openByteDocument() }
    var pixels = byteFixture()
    for i in stride(from: 3, to: pixels.count, by: 4) { pixels[i] = 255 }
    guard let image = RasterImage.from(rgba: pixels, width: 96, height: 64),
          let plain = RasterDocument.from(image: image) else {
        throw DoctoredCoreError(message: "Opaque byte paint fixture unavailable")
    }
    let source: RasterDocument
    if p3 {
        guard let assigned = try plain.assigningProfile(RasterProfile.builtin(.displayP3)) else {
            throw DoctoredCoreError(message: "Display P3 paint fixture unavailable")
        }
        source = assigned
    } else { source = plain }
    let document = ImageDocument(); document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source,
        selection: SelectionHistoryState(), layerSelection: .single(0),
        paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: 8, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document); document.makeWindowControllers()
    document.showWindows()
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw DoctoredCoreError(message: "Opaque byte paint editor missing")
    }
    _ = editor.view; editor.view.window?.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true); pump()
    return (document, editor, source)
}

func bytePaintSnapshot(_ document: ImageDocument) throws -> String {
    guard let doc = document.doc else { throw DoctoredCoreError(message: "Byte paint document missing") }
    var value: [String: Any] = ["width": doc.width, "height": doc.height,
        "layers": doc.layerCount, "channels": doc.channelCount,
        "composite": hashBytes(try flatBytes(doc)),
        "selection": document.canvasSelection.map { hashBytes($0.maskBytes()) } ?? "none",
        "previous": document.selectionHistory.previous.map { hashBytes($0.maskBytes()) } ?? "none"]
    if let samples = document.canvasSelection?.maskSamples16() {
        value["selection16"] = hashBytes(Array(samples.withUnsafeBytes { Data($0) }))
    } else { value["selection16"] = "none" }
    if let samples = document.selectionHistory.previous?.maskSamples16() {
        value["previous16"] = hashBytes(Array(samples.withUnsafeBytes { Data($0) }))
    } else { value["previous16"] = "none" }
    var rows: [[String: Any]] = []
    for i in 0..<doc.layerCount {
        let info = doc.layerInfo(i)
        var row: [String: Any] = ["pixels": doc.layerPixels(i).map(hashBytes) ?? "none",
            "name": info?.name ?? "", "opacity": info?.opacity ?? 0,
            "blend": Int(info?.blendMode.rawValue ?? 0),
            "visible": info?.visible ?? false, "depth": info?.depth ?? 0,
            "locks": info?.locks ?? 0, "link": info?.link ?? 0,
            "expanded": info?.open ?? false,
            "description": doc.layerMeta(i) ?? "none",
            "style": doc.layerStyle(i) ?? "none",
            "mask_properties": doc.layerMaskProperties(i)?.report ?? [:],
            "width": info?.width ?? 0, "height": info?.height ?? 0,
            "offset_x": info?.offsetX ?? 0, "offset_y": info?.offsetY ?? 0,
            "kind": Int(info?.kind.rawValue ?? 0)]
        if let storage = doc.layerMaskStorage(i) {
            let count = storage.image.width * storage.image.height * 4
            row["mask"] = storage.image.withPlateBytes { bytes in
                hashBytes(Array(UnsafeBufferPointer(start: bytes, count: count)))
            } ?? "unavailable"
            row["mask_offset_x"] = storage.offsetX
            row["mask_offset_y"] = storage.offsetY
        }
        rows.append(row)
    }
    value["raw_layers"] = rows
    value["raw_channels"] = (0..<doc.channelCount).map {
        doc.channelDisplayBytes($0).map(hashBytes) ?? "unavailable"
    }
    value["channel_metadata"] = (0..<doc.channelCount).map { index -> [String: Any] in
        guard let info = doc.channelInfo(index) else { return [:] }
        return ["name": info.name, "red": info.red, "green": info.green,
            "blue": info.blue, "opacity": info.opacity,
            "indicates_selected": info.colorIndicatesSelected]
    }
    let encoded = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    return hashBytes(Array(encoded))
}

private func byteMaskStorage(_ doc: RasterDocument, _ index: Int) -> [UInt8]? {
    guard let storage = doc.layerMaskStorage(index) else { return nil }
    let count = storage.image.width * storage.image.height * 4
    return storage.image.withPlateBytes {
        Array(UnsafeBufferPointer(start: $0, count: count))
    }
}

/// A scalar stroke may change only its named storage; every other raw plane,
/// mask, channel and structural property must survive byte-for-byte.
private func requireCoverageUntouched(_ before: RasterDocument, _ after: RasterDocument,
                                      target: PaintTarget, layer: Int) throws {
    try require(before.layerCount == after.layerCount
        && before.channelCount == after.channelCount,
        "Coverage stroke changed layer/channel structure")
    let editedPlane: Int?
    if case .plane(let plane) = target {
        switch plane {
        case .red: editedPlane = 0
        case .green: editedPlane = 1
        case .blue: editedPlane = 2
        case .alpha: editedPlane = 3
        case .luma, .mask: editedPlane = nil
        }
    } else { editedPlane = nil }
    for index in 0..<before.layerCount {
        let old = before.layerInfo(index), new = after.layerInfo(index)
        try require(old?.name == new?.name && old?.opacity == new?.opacity
            && old?.blendMode == new?.blendMode && old?.visible == new?.visible
            && old?.offsetX == new?.offsetX && old?.offsetY == new?.offsetY
            && old?.width == new?.width && old?.height == new?.height
            && old?.depth == new?.depth && old?.locks == new?.locks
            && old?.link == new?.link && old?.open == new?.open
            && before.layerMeta(index) == after.layerMeta(index)
            && before.layerStyle(index) == after.layerStyle(index)
            && before.layerMaskProperties(index) == after.layerMaskProperties(index),
            "Coverage stroke changed untouched layer metadata")
        let oldPixels = before.layerPixels(index), newPixels = after.layerPixels(index)
        if let editedPlane, index == layer, let oldPixels, let newPixels {
            try require(oldPixels.count == newPixels.count,
                "Plane stroke changed raw layer dimensions")
            for offset in stride(from: 0, to: oldPixels.count, by: 4) {
                for component in 0..<4 where component != editedPlane {
                    try require(oldPixels[offset + component] == newPixels[offset + component],
                        "Plane stroke changed an untouched raw component")
                }
            }
        } else {
            try require(oldPixels == newPixels,
                "Coverage stroke changed unrelated raw layer pixels")
        }
        if target != .mask || index != layer {
            try require(byteMaskStorage(before, index) == byteMaskStorage(after, index),
                "Coverage stroke changed an unrelated mask")
        }
    }
    for index in 0..<before.channelCount {
        let old = before.channelInfo(index), new = after.channelInfo(index)
        try require(old?.name == new?.name && old?.red == new?.red
            && old?.green == new?.green && old?.blue == new?.blue
            && old?.opacity == new?.opacity
            && old?.colorIndicatesSelected == new?.colorIndicatesSelected,
            "Coverage stroke changed channel metadata")
        if target != .channel(index) {
            try require(before.channelDisplayBytes(index) == after.channelDisplayBytes(index),
                "Coverage stroke changed an unrelated channel")
        }
    }
}

func decodedByteStep(_ steps: [ActionStep], expected tool: String) throws -> ActionStep {
    try require(steps.count == 1 && steps[0].tool == tool && !steps[0].isUnrecorded,
                "Byte gesture did not record exactly one \(tool) step")
    for key in ["stroke_version", "pressures", "times", "duration",
                "pressure_size", "pressure_opacity", "airbrush"] {
        try require(steps[0].arguments[key] == nil, "Byte Action kept generation field \(key)")
    }
    guard let canonical = steps[0].arguments["native_stroke"] as? [String: Any] else {
        throw DoctoredCoreError(message: "Byte Action lacks canonical RZBR dabs")
    }
    try require(try decodeDabs(canonical).count >= 8, "Byte Action has no nontrivial dabs")
    let action = Action(name: "Byte paint probe", steps: steps, recordedCanvas: (96, 64))
    switch Action.decode(action.encoded()) {
    case .success(let decoded):
        try require(decoded.steps.count == 1 && decoded.steps[0].tool == tool,
                    "Saved byte Action failed its decode round trip")
        return decoded.steps[0]
    case .failure(let reason): throw reason
    }
}

private func clickByteOption(_ view: NSView) throws {
    guard let window = view.window,
          let event = NSEvent.mouseEvent(with: .leftMouseDown,
            location: view.convert(CGPoint(x: 5, y: 5), to: nil),
            modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil,
            eventNumber: 0, clickCount: 1, pressure: 1) else {
        throw DoctoredCoreError(message: "Owned pressure-opacity control has no mouse event")
    }
    view.mouseDown(with: event)
    pump()
}

func activateBytePressureOpacity(_ editor: EditorViewController,
                                 tool: EditorTool) throws {
    editor.optionsBar.layoutSubtreeIfNeeded()
    let identifier = NSUserInterfaceItemIdentifier("paint.pressureOpacity")
    func visibleControl() -> OptionCheckboxControl? {
        NSApp.windows.filter(\.isVisible).compactMap(\.contentView)
            .flatMap(descendants).compactMap { $0 as? OptionCheckboxControl }
            .first { $0.identifier == identifier && !$0.isHidden }
    }
    if visibleControl() == nil {
        guard let more = descendants(editor.optionsBar).compactMap({ $0 as? MoreButton }).first
        else { throw DoctoredCoreError(message: "Pressure opacity More control missing") }
        try clickByteOption(more)
    }
    guard let control = visibleControl() else {
        throw DoctoredCoreError(message: "paint.pressureOpacity control unavailable at byte depth")
    }
    try clickByteOption(control)
    try require(ToolOptionsStore.shared.paintOptions(for: tool)?.pressureOpacity == true
        && editor.canvas.brushPressureOpacity,
        "Pressure opacity UI control did not activate byte stroke pressure")
    editor.optionsBar.closeOverflow(); pump()
}

func configureBytePaint(_ editor: EditorViewController, tool: EditorTool,
                        variant: String, held: Bool) throws {
    var options = PaintToolOptions()
    options.size = 23; options.opacity = 73; options.smoothing = 0
    options.hardness = variant == "flow" || variant == "round" ? 100 : 34
    options.flow = variant == "flow" ? 43 : variant == "pressure" ? 57 : 100
    options.roundness = variant == "round" || variant == "pressure" ? 69 : 100
    options.spacing = 31; options.angle = 19
    options.pressureSize = variant == "pressure"
    options.pressureOpacity = false
    options.airbrush = held
    if tool == .heal || tool == .spotHeal { options.flow = 100 }
    switch tool {
    case .brush: ToolOptionsStore.shared.brush = options
    case .eraser: ToolOptionsStore.shared.eraser = options
    case .clone: ToolOptionsStore.shared.clone = options
    case .heal: ToolOptionsStore.shared.heal = options
    case .spotHeal: ToolOptionsStore.shared.spotHeal = options
    case .dodge: ToolOptionsStore.shared.dodge = options
    default: throw DoctoredCoreError(message: "Unsupported byte paint fixture tool")
    }
    editor.selectTool(tool); editor.syncCanvasPaintState(); pump()
    if variant == "pressure" { try activateBytePressureOpacity(editor, tool: tool) }
    if tool == .clone || tool == .heal { editor.canvas.cloneSource = CGPoint(x: 8.5, y: 8.5) }
}

private func retouchVariant(_ editor: EditorViewController, _ document: ImageDocument,
                            tool: EditorTool, variant: String) throws {
    guard !variant.isEmpty else { return }
    if variant == "ignore_adjustments" {
        _ = try call("add_adjustment_layer", ["op": "invert", "name": "Retouch Adjustment"],
                     document: document)
        document.activeLayerIndex = 0
        editor.setPaintTarget(.layer)
    }
    if variant == "expand" {
        document.applyEdit("Fixture Offset", record: .notACommand) {
            try $0.withLayerOffset(0, 35, 0)
        }
    }
    guard var options = ToolOptionsStore.shared.paintOptions(for: tool) else {
        throw DoctoredCoreError(message: "Retouch variant options unavailable")
    }
    switch variant {
    case "source_transform":
        options.sourceScale = 125.5; options.sourceRotation = 19.25
        options.sourceFlipX = true; options.aligned = false
    case "aligned_heal":
        options.aligned = true; options.sourceScale = 112.5
        options.sourceRotation = 13.5; options.sourceFlipY = true
    case "sample_current": options.sample = .current
    case "sample_below": options.sample = .currentAndBelow
    case "ignore_adjustments": options.sample = .all; options.ignoreAdjustments = true
    case "new_layer": options.outputToNewLayer = true
    case "expand": options.expandDestination = true
    case "blend": options.blendIndex = Int(DR_BLEND_MULTIPLY.rawValue)
    case "pixel_held": options.size = 1; options.airbrush = true
    case "fractional":
        options.size = 23.375; options.hardness = 34.25
        options.opacity = 73.125; options.flow = 57.125
        options.spacing = 31.5; options.angle = 19.5; options.roundness = 69.5
    default: throw DoctoredCoreError(message: "Unknown byte option variant")
    }
    switch tool {
    case .brush: ToolOptionsStore.shared.brush = options
    case .eraser: ToolOptionsStore.shared.eraser = options
    case .clone: ToolOptionsStore.shared.clone = options
    case .heal: ToolOptionsStore.shared.heal = options
    case .spotHeal: ToolOptionsStore.shared.spotHeal = options
    case .dodge: ToolOptionsStore.shared.dodge = options
    default: throw DoctoredCoreError(message: "Unsupported byte option tool")
    }
    editor.selectTool(tool); editor.syncCanvasPaintState(); pump()
    if tool == .clone || tool == .heal {
        editor.canvas.cloneSource = CGPoint(x: variant == "expand" ? 55.5 : 8.5, y: 8.5)
    }
}

/// The first actual Heal drag establishes the aligned source offset and
/// transformed source matrix carried into a second, separately recorded drag.
private func establishAlignedByteHeal(_ editor: EditorViewController) throws -> RetouchGeometry {
    try require(editor.canvas.strokeAligned, "Aligned Heal option was not active")
    let start = CGPoint(x: 18.5, y: 19.5)
    try send(editor.canvas, .leftMouseDown, start)
    guard let geometry = editor.canvas.strokeCloneGeometry else {
        throw DoctoredCoreError(message: "First aligned Heal captured no source matrix")
    }
    try send(editor.canvas, .leftMouseDragged, CGPoint(x: 29.5, y: 24.5))
    try send(editor.canvas, .leftMouseUp, CGPoint(x: 34.5, y: 27.5))
    // The Healing Brush commits on the worker: the second drag
    // must start on this one's result, not be refused by the busy worker.
    try waitUntil({ !BackgroundWork.occupied }, "Aligned Heal worker did not finish")
    return geometry
}

private func requireRetouchNewLayer(_ before: RasterDocument, _ after: RasterDocument,
                                    sourceIndex: Int, name: String) throws {
    let inserted = sourceIndex + 1
    try require(after.layerCount == before.layerCount + 1,
        "\(name) did not add exactly one retouch layer")
    guard let sourceBefore = before.layerInfo(sourceIndex),
          let sourceAfter = after.layerInfo(sourceIndex),
          let layer = after.layerInfo(inserted) else {
        throw DoctoredCoreError(message: "\(name) lost the source or inserted layer")
    }
    try require(sourceBefore.id == sourceAfter.id
        && sourceBefore.name == sourceAfter.name
        && sourceBefore.opacity == sourceAfter.opacity
        && sourceBefore.blendMode == sourceAfter.blendMode
        && sourceBefore.visible == sourceAfter.visible
        && sourceBefore.offsetX == sourceAfter.offsetX
        && sourceBefore.offsetY == sourceAfter.offsetY
        && sourceBefore.width == sourceAfter.width
        && sourceBefore.height == sourceAfter.height
        && sourceBefore.kind == sourceAfter.kind
        && sourceBefore.depth == sourceAfter.depth
        && sourceBefore.locks == sourceAfter.locks
        && sourceBefore.link == sourceAfter.link
        && sourceBefore.open == sourceAfter.open
        && before.layerClipped(sourceIndex) == after.layerClipped(sourceIndex)
        && before.layerPixels(sourceIndex) == after.layerPixels(sourceIndex)
        && before.layerMeta(sourceIndex) == after.layerMeta(sourceIndex)
        && before.layerStyle(sourceIndex) == after.layerStyle(sourceIndex)
        && before.layerMaskProperties(sourceIndex) == after.layerMaskProperties(sourceIndex)
        && byteMaskStorage(before, sourceIndex) == byteMaskStorage(after, sourceIndex),
        "\(name) altered the original source layer while creating a repair")
    try require(layer.id != 0 && layer.id != sourceBefore.id
        && layer.name == EditorViewController.retouchLayerName
        && layer.kind == DR_LAYER_RASTER && layer.visible
        && layer.opacity == 1 && layer.blendMode == DR_BLEND_NORMAL
        && layer.offsetX == 0 && layer.offsetY == 0
        && layer.width == after.width && layer.height == after.height
        && layer.depth == sourceBefore.depth && layer.locks == 0
        && layer.link == 0 && layer.open
        && after.layerClipped(inserted) == before.layerClipped(sourceIndex)
        && after.layerMeta(inserted) == nil && after.layerStyle(inserted) == nil
        && !after.layerHasMask(inserted),
        "\(name) inserted the repair at the wrong index or with wrong metadata")
    try require(after.channelCount == before.channelCount,
        "\(name) changed saved-channel structure")
    for index in 0..<before.channelCount {
        try require(before.channelDisplayBytes(index) == after.channelDisplayBytes(index),
            "\(name) changed an unrelated saved channel")
    }
    for index in 0..<before.layerCount where index != sourceIndex {
        let mapped = index < inserted ? index : index + 1
        try require(before.layerInfo(index)?.id == after.layerInfo(mapped)?.id
            && before.layerPixels(index) == after.layerPixels(mapped),
            "\(name) moved or rewrote an unrelated layer")
    }
    try require(after.layerPixels(inserted)?.contains(where: { $0 != 0 }) == true,
        "\(name) inserted an empty repair layer")
}

func bytePaintGestureTrial(_ tool: EditorTool, name: String,
                           variant: String, held: Bool = false,
                           range: Int = 1, burn: Bool = false,
                           target: PaintTarget = .layer,
                           maskVariant: String = "plain",
                           softSelection: Bool = false,
                           optionVariant: String = "",
                           midDragOptions: Bool = false,
                           p3: Bool = false,
                           alignedHealCarry: Bool = false) throws -> [String: Any] {
    let opaque = tool == .heal || tool == .spotHeal
    let (document, editor, _) = try openBytePaintDocument(opaque: opaque, p3: p3)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try configureBytePaint(editor, tool: tool, variant: variant, held: held)
    try setupBytePaintTarget(document, editor, target: target, tool: tool,
                             maskVariant: maskVariant, softSelection: softSelection)
    try retouchVariant(editor, document, tool: tool, variant: optionVariant)
    if tool == .dodge {
        var options = ToolOptionsStore.shared.dodge
        options.rangeIndex = range; options.burn = burn
        ToolOptionsStore.shared.dodge = options
        editor.syncCanvasPaintState()
    }
    let carriedGeometry = alignedHealCarry ? try establishAlignedByteHeal(editor) : nil
    let expectedStamped = SoftBrush.isStamped(tip: BrushTip(
        hardness: editor.canvas.brushHardness, flow: tool == .heal || tool == .spotHeal
            ? 1 : editor.canvas.brushFlow,
        spacingPercent: editor.canvas.brushSpacingPercent,
        angleDegrees: editor.canvas.brushAngle,
        roundness: editor.canvas.brushRoundness), size: editor.canvas.brushSize)
    if variant != "pressure" {
        try require(expectedStamped, "Ordinary mouse fixture was not actually stamped")
    }
    let baseline = try bytePaintSnapshot(document)
    let originalOwner = document.doc
    let sourceIndex = document.activeLayerIndex
    document.undoManager?.removeAllActions()
    ActionRecorder.shared.start()
    let start = alignedHealCarry ? CGPoint(x: 37.5, y: 29.5)
        : CGPoint(x: 21.5, y: 23.5)
    let middle = alignedHealCarry ? CGPoint(x: 54.5, y: 34.5)
        : CGPoint(x: 42.5, y: 30.5)
    let end = alignedHealCarry ? CGPoint(x: 72.5, y: 40.5)
        : CGPoint(x: 64.5, y: 37.5)
    try send(editor.canvas, .leftMouseDown, start,
             pressure: variant == "pressure" ? 0.25 : nil)
    var alignedMatrix: [Double]?
    var alignedSource: CGPoint?
    if let carriedGeometry {
        guard let latched = editor.canvas.strokeCloneGeometry,
              let source = editor.canvas.strokeCloneSource else {
            throw DoctoredCoreError(message: "Second aligned Heal lost its carried source")
        }
        let expected = start.applying(carriedGeometry.matrix)
        try require(abs(source.x - expected.x) < 0.000000001
            && abs(source.y - expected.y) < 0.000000001
            && hypot(source.x - 8.5, source.y - 8.5) > 1,
            "Second Heal did not carry the first stroke's source offset")
        alignedMatrix = latched.values
        alignedSource = source
    }
    if editor.canvas.nativeStrokeSession == nil {
        var sourceReason = "not retouch"
        if let doc = document.doc, let options = ToolOptionsStore.shared.paintOptions(for: tool) {
            do {
                let choice = RetouchSamplingChoice(mode: options.sample,
                    ignoreAdjustments: options.ignoreAdjustments)
                let plate = try RetouchSource.plate(doc, layer: document.activeLayerIndex,
                    choice: choice)
                sourceReason = plate == nil ? "source plate nil" : "source plate available"
            } catch { sourceReason = error.localizedDescription }
        }
        throw DoctoredCoreError(message: "\(name) did not open the actual byte dab session; "
            + "tool \(editor.currentTool), canvas \(editor.canvas.tool), "
            + "active \(document.activeLayerIndex), layers \(document.doc?.layerCount ?? -1), "
            + "source \(String(describing: editor.canvas.cloneSource)), "
            + "capture \(editor.byteStrokeCapture != nil), plate \(editor.strokePlate != nil), "
            + "direct \(sourceReason)")
    }
    if held {
        let deadline = Date().addingTimeInterval(0.25)
        while Date() < deadline { pump() }
        try require((editor.canvas.nativeStrokeSession?.dabs.count ?? 0) >= 12,
                    "\(name) lacked real Timer deposits")
    }
    try send(editor.canvas, .leftMouseDragged, middle,
             pressure: variant == "pressure" ? 0.53 : nil)
    if midDragOptions {
        var options = ToolOptionsStore.shared.paintOptions(for: tool) ?? PaintToolOptions()
        options.sample = .current; options.opacity = 19; options.sourceRotation = 61
        if tool == .clone { ToolOptionsStore.shared.clone = options }
        if tool == .heal { ToolOptionsStore.shared.heal = options }
        editor.canvas.cloneSource = CGPoint(x: 77.5, y: 53.5)
        editor.syncCanvasPaintState(); pump()
    }
    try send(editor.canvas, .leftMouseDragged, end,
             pressure: variant == "pressure" ? 0.87 : nil)
    try send(editor.canvas, .leftMouseUp, end,
             pressure: variant == "pressure" ? 0.87 : nil)
    if tool == .spotHeal || tool == .heal {
        try waitUntil({ !BackgroundWork.occupied }, "Healing worker did not finish")
    }
    let steps = ActionRecorder.shared.stop()
    let toolName: String
    switch tool {
    case .brush: toolName = "brush_stroke"
    case .eraser: toolName = "eraser_stroke"
    case .clone: toolName = "clone_stamp"
    case .heal: toolName = "heal_stroke"
    case .spotHeal: toolName = "spot_heal_stroke"
    default: toolName = "dodge_burn"
    }
    let step = try decodedByteStep(steps, expected: toolName)
    if let alignedMatrix, let alignedSource {
        guard let recorded = step.arguments["source_transform"] as? [Double] else {
            throw DoctoredCoreError(message: "Aligned Heal Action lost its source matrix")
        }
        try require(recorded == alignedMatrix
            && step.arguments["source_x"] as? Double == Double(alignedSource.x)
            && step.arguments["source_y"] as? Double == Double(alignedSource.y),
            "Aligned Heal Action did not record its latched source matrix/point")
    }
    if tool == .dodge {
        let names = ["shadows", "midtones", "highlights"]
        try require(step.arguments["burn"] as? Bool == burn
            && step.arguments["range"] as? String == names[range],
            "Dodge/Burn Action did not record its actual direction and tonal range")
    }
    if midDragOptions {
        try require(step.arguments["sample"] as? String == RetouchSampling.all.rawValue
            && (step.arguments["source_x"] as? Double) == 8.5
            && (step.arguments["source_y"] as? Double) == 8.5,
            "Retouch Action used changed mid-drag source/options")
    }
    let after = try bytePaintSnapshot(document)
    try require(after != baseline, "\(name) changed no raw byte plane")
    guard let committed = document.doc else {
        throw DoctoredCoreError(message: "\(name) lost committed owner")
    }
    if optionVariant == "new_layer", let originalOwner {
        try requireRetouchNewLayer(originalOwner, committed, sourceIndex: sourceIndex,
                                  name: name)
        try require(step.arguments["output"] as? String == RetouchOutput.newLayer.rawValue,
            "\(name) Action did not request a new retouch layer")
    }
    if target.isCoverage, let originalOwner {
        try requireCoverageUntouched(originalOwner, committed, target: target,
                                     layer: document.activeLayerIndex)
    }
    document.undoManager?.undo(); pump()
    try require(document.doc === originalOwner && (try bytePaintSnapshot(document)) == baseline,
                "\(name) Undo did not restore original owner/bytes")
    document.undoManager?.redo(); pump()
    try require(document.doc === committed && (try bytePaintSnapshot(document)) == after,
                "\(name) Redo did not restore committed owner/bytes")
    let (fresh, freshEditor, _) = try openBytePaintDocument(opaque: opaque, p3: p3)
    defer { fresh.close(); pump() }
    try setupBytePaintTarget(fresh, freshEditor, target: target, tool: tool,
                             maskVariant: maskVariant, softSelection: softSelection)
    try retouchVariant(freshEditor, fresh, tool: tool, variant: optionVariant)
    if alignedHealCarry {
        _ = try establishAlignedByteHeal(freshEditor)
        try require((try bytePaintSnapshot(fresh)) == baseline,
            "Aligned Heal replay fixture did not reproduce the first stroke")
    }
    let reply = try replayBinding(step, document: fresh)
    try require(!(try responseIsError(reply)), "\(name) decoded Action refused: \(reply)")
    try require((try bytePaintSnapshot(fresh)) == after,
                "\(name) Action replay differs in raw bytes/coverage/composite")
    return ["case": name, "tool": toolName, "variant": variant, "held": held,
            "option_variant": optionVariant, "p3": p3,
            "aligned_carry": alignedHealCarry, "new_layer": optionVariant == "new_layer",
            "direction": tool == .dodge ? (burn ? "burn" : "dodge") : "none",
            "range": tool == .dodge ? range : -1,
            "raw_digest": after, "replay_exact": true,
            "dabs": (try decodeDabs(step.arguments["native_stroke"] as? [String: Any] ?? [:])).count / 4]
}

func byteBrushRecordUI() throws -> [String: Any] {
    let trials = try [bytePaintGestureTrial(.brush, name: "brush-pressure", variant: "pressure"),
        bytePaintGestureTrial(.brush, name: "brush-held", variant: "soft", held: true),
        bytePaintGestureTrial(.brush, name: "brush-mouse-soft", variant: "soft"),
        bytePaintGestureTrial(.brush, name: "brush-mouse-flow", variant: "flow"),
        bytePaintGestureTrial(.brush, name: "brush-mouse-round", variant: "round"),
        bytePaintGestureTrial(.brush, name: "brush-multiply", variant: "soft",
                              optionVariant: "blend"),
        bytePaintGestureTrial(.brush, name: "brush-pixel-held", variant: "pressure",
                              held: true, optionVariant: "pixel_held"),
        bytePaintGestureTrial(.brush, name: "brush-fractional", variant: "soft",
                              optionVariant: "fractional"),
        bytePaintGestureTrial(.brush, name: "brush-p3", variant: "soft", p3: true)]
    return ["case": "byte-brush-record-ui", "trials": trials]
}

func byteEraserRecordUI() throws -> [String: Any] {
    let trials = try [bytePaintGestureTrial(.eraser, name: "eraser-pressure", variant: "pressure"),
        bytePaintGestureTrial(.eraser, name: "eraser-held", variant: "soft", held: true),
        bytePaintGestureTrial(.eraser, name: "eraser-mouse-soft", variant: "soft"),
        bytePaintGestureTrial(.eraser, name: "eraser-mouse-flow", variant: "flow"),
        bytePaintGestureTrial(.eraser, name: "eraser-mouse-round", variant: "round"),
        bytePaintGestureTrial(.eraser, name: "eraser-pixel-held", variant: "pressure",
                              held: true, optionVariant: "pixel_held"),
        bytePaintGestureTrial(.eraser, name: "eraser-p3", variant: "soft", p3: true)]
    return ["case": "byte-eraser-record-ui", "trials": trials]
}
