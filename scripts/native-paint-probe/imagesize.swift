import AppKit
import CryptoKit

private func imageSizeSheets(_ editor: EditorViewController) -> [ImageSizeSheetController] {
    (editor.presentedViewControllers ?? []).compactMap { $0 as? ImageSizeSheetController }
}

private func presentedImageSizeSheet(
    _ editor: EditorViewController, after count: Int
) throws -> ImageSizeSheetController {
    try waitUntil({ imageSizeSheets(editor).count > count }, "The Image Size sheet never came up")
    guard let sheet = imageSizeSheets(editor).last else {
        throw DoctoredCoreError(message: "The Image Size sheet never came up")
    }
    _ = sheet.view
    pump()
    return sheet
}

private func imageSizeControl<T: NSView>(
    _ sheet: ImageSizeSheetController, _ name: String, _ what: String
) throws -> T {
    let identifier = NSUserInterfaceItemIdentifier(name)
    guard let control = descendants(sheet.view).compactMap({ $0 as? T })
        .first(where: { $0.identifier == identifier }) else {
        throw DoctoredCoreError(message: "Image Size has no \(what) (\(name))")
    }
    return control
}

/// Types through AppKit's actual field editor. `insertText` posts the control
/// editing notifications which reach the sheet's NSTextFieldDelegate.
private func typeImageSize(_ field: NSTextField, _ text: String) throws {
    guard let window = field.window else {
        throw DoctoredCoreError(message: "Image Size field has no owned window")
    }
    window.makeFirstResponder(field)
    field.selectText(nil)
    guard let editor = field.currentEditor() as? NSTextView else {
        throw DoctoredCoreError(message: "Image Size field editor is unavailable")
    }
    editor.insertText(text, replacementRange: NSRange(location: 0, length: editor.string.utf16.count))
    pump()
}

private func chooseImageSizePopup(_ popup: NSPopUpButton, index: Int) {
    popup.selectItem(at: index)
    if let action = popup.action {
        _ = NSApp.sendAction(action, to: popup.target, from: popup)
    }
    pump()
}

private func openImageSizeSheet(
    _ editor: EditorViewController
) throws -> ImageSizeSheetController {
    let count = imageSizeSheets(editor).count
    editor.resizeImage(nil)
    return try presentedImageSizeSheet(editor, after: count)
}

private func dimensionMatrix() throws -> [[String: Any]] {
    typealias Size = (width: Int, height: Int)
    struct Trial {
        let name: String
        let value: Double
        let axis: ImageSizeAxis
        let unit: ImageSizeUnit
        let original: Size
        let current: Size
        let lock: Bool
        let expected: Size?
    }
    let trials: [Trial] = [
        Trial(name: "fifty-width-locked", value: 50, axis: .width, unit: .percent,
              original: (7, 5), current: (7, 5), lock: true, expected: (4, 3)),
        Trial(name: "two-hundred-height-locked-original-base", value: 200, axis: .height,
              unit: .percent, original: (7, 5), current: (4, 3), lock: true,
              expected: (14, 10)),
        Trial(name: "fractional-width-unlocked", value: 12.5, axis: .width,
              unit: .percent, original: (72, 40), current: (72, 37), lock: false,
              expected: (9, 37)),
        Trial(name: "fractional-height-unlocked", value: 12.5, axis: .height,
              unit: .percent, original: (72, 40), current: (61, 40), lock: false,
              expected: (61, 5)),
        Trial(name: "tiny-positive", value: 0.000001, axis: .width, unit: .percent,
              original: (3, 9), current: (3, 9), lock: true, expected: (1, 1)),
        Trial(name: "two-by-five-quarter", value: 25, axis: .height, unit: .percent,
              original: (2, 5), current: (2, 5), lock: true, expected: (1, 1)),
        Trial(name: "half-ties-away", value: 50, axis: .width, unit: .percent,
              original: (3, 5), current: (3, 5), lock: true, expected: (2, 3)),
        Trial(name: "pixel-width-locked", value: 9, axis: .width, unit: .pixels,
              original: (6, 5), current: (6, 5), lock: true, expected: (9, 8)),
        Trial(name: "pixel-height-unlocked", value: 9, axis: .height, unit: .pixels,
              original: (6, 5), current: (4, 5), lock: false, expected: (4, 9)),
        Trial(name: "axis-boundary", value: 2_000_000, axis: .width, unit: .percent,
              original: (1, 1), current: (1, 1), lock: false, expected: (20_000, 1)),
        Trial(name: "hundred-megapixel-boundary", value: 1_000_000, axis: .width,
              unit: .percent, original: (1, 10_000), current: (1, 10_000), lock: false,
              expected: (10_000, 10_000)),
        Trial(name: "zero", value: 0, axis: .width, unit: .percent,
              original: (7, 5), current: (7, 5), lock: true, expected: nil),
        Trial(name: "negative", value: -1, axis: .height, unit: .percent,
              original: (7, 5), current: (7, 5), lock: true, expected: nil),
        Trial(name: "nan", value: .nan, axis: .width, unit: .percent,
              original: (7, 5), current: (7, 5), lock: true, expected: nil),
        Trial(name: "infinity", value: .infinity, axis: .width, unit: .percent,
              original: (7, 5), current: (7, 5), lock: true, expected: nil),
        Trial(name: "axis-overflow", value: 2_000_100, axis: .width, unit: .percent,
              original: (1, 1), current: (1, 1), lock: false, expected: nil),
        Trial(name: "product-overflow", value: 2_000_000, axis: .width,
              unit: .percent, original: (1, 20_000), current: (1, 20_000), lock: false,
              expected: nil),
        Trial(name: "fractional-pixels", value: 4.5, axis: .width, unit: .pixels,
              original: (7, 5), current: (7, 5), lock: true, expected: nil),
        Trial(name: "huge", value: Double.greatestFiniteMagnitude, axis: .width,
              unit: .percent, original: (7, 5), current: (7, 5), lock: true,
              expected: nil),
    ]
    return try trials.map { trial in
        let actual = ImageSizeDimensions.resolve(
            value: trial.value, axis: trial.axis, unit: trial.unit,
            original: trial.original, current: trial.current, lockAspect: trial.lock)
        try require(actual?.width == trial.expected?.width
            && actual?.height == trial.expected?.height,
            "\(trial.name): \(String(describing: actual)) != \(String(describing: trial.expected))")
        return ["name": trial.name, "accepted": actual != nil,
                "result": actual.map { [$0.width, $0.height] } ?? []]
    }
}

private func prepareImageSizeFixture(_ document: ImageDocument) throws {
    _ = try geometryCall(
        "set_resolution", ["ppi_x": 144.5, "ppi_y": 96.25], document)
    _ = try geometryCall("select_rect", ["x": 1, "y": 1, "width": 3, "height": 2], document)
    _ = try geometryCall("add_layer_mask", ["layer": 0, "kind": "from_selection"], document)
    _ = try geometryCall("save_selection", ["name": "Image Size Alpha"], document)
    _ = try geometryCall("deselect", [:], document)
    _ = try geometryCall("add_text_layer",
        ["text": "Percent", "x": 1, "y": 1, "size": 4], document)
}

private func imageSizeState(_ document: ImageDocument) throws -> [String: Any] {
    guard let doc = document.doc else { throw DoctoredCoreError(message: "Image Size document missing") }
    var layers: [[String: Any]] = []
    for index in 0..<doc.layerCount {
        let info = doc.layerInfo(index)
        var row: [String: Any] = [
            "name": info?.name ?? "", "width": info?.width ?? 0,
            "height": info?.height ?? 0, "x": info?.offsetX ?? 0,
            "y": info?.offsetY ?? 0, "meta": doc.layerMeta(index) ?? "none",
            "mask": doc.layerMaskProperties(index)?.report ?? [:],
        ]
        if let source = doc.precision?.layerSource(index) {
            let samples = try source.withRGBA16 { Array($0) }
            row["storage_depth"] = "rgba16_unorm"
            row["stored_samples"] = hashSamples(samples)
            row["stored_sample_count"] = samples.count
        } else if let pixels = doc.layerPixels(index) {
            row["storage_depth"] = "rgba8"
            row["stored_samples"] = hashBytes(pixels)
            row["stored_sample_count"] = pixels.count
        } else {
            row["storage_depth"] = "none"
            row["stored_samples"] = "none"
            row["stored_sample_count"] = 0
        }
        if doc.hasArtworkDescription(index), let metadata = doc.layerMeta(index) {
            row["artwork_source"] = [
                "metadata": metadata,
                "metadata_sha256": hashBytes(Array(metadata.utf8)),
                "render_payload_sha256": row["stored_samples"] ?? "none",
            ]
        }
        if let retained = try doc.retainedSourceInfo(index) {
            row["retained_source"] = retained
        }
        if doc.precision != nil, doc.layerMaskProperties(index) != nil {
            row["mask_samples"] = hashSamples(
                try PrecisionScalarOperations.read(doc, target: "mask", index: index).samples)
        } else if let storage = doc.layerMaskStorage(index) {
            let count = storage.image.width * storage.image.height * 4
            row["mask_samples"] = storage.image.withPlateBytes {
                hashBytes(Array(UnsafeBufferPointer(start: $0, count: count)))
            } ?? "unavailable"
        }
        layers.append(row)
    }
    let channels: [String] = try (0..<doc.channelCount).map { index in
        if doc.precision != nil {
            return hashSamples(
                try PrecisionScalarOperations.read(doc, target: "channel", index: index).samples)
        }
        return doc.channelDisplayBytes(index).map(hashBytes) ?? "unavailable"
    }
    return [
        "width": doc.width, "height": doc.height, "depth": doc.isHighDepth ? 16 : 8,
        "ppi": [doc.resolution.x, doc.resolution.y],
        "projection": doc.precision != nil ? hashSamples(try raw(doc)) : hashBytes(try flatBytes(doc)),
        "layers": layers, "channels": channels,
    ]
}

private func actualPercentageResize(_ typed: Bool, output: URL) throws -> [String: Any] {
    let opened = typed ? try openDocument(width: 9, height: 7)
        : try openByteDocument(width: 7, height: 5)
    let referenceOpened = typed ? try openDocument(width: 9, height: 7)
        : try openByteDocument(width: 7, height: 5)
    let replayOpened = typed ? try openDocument(width: 9, height: 7)
        : try openByteDocument(width: 7, height: 5)
    let (document, editor, _) = opened
    let (reference, _, _) = referenceOpened
    let (replayed, _, _) = replayOpened
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); reference.close(); replayed.close(); pump()
    }
    try prepareImageSizeFixture(document)
    try prepareImageSizeFixture(reference)
    try prepareImageSizeFixture(replayed)
    try require(document.doc?.artworkIndices.count == 1
        && reference.doc?.artworkIndices.count == 1
        && replayed.doc?.artworkIndices.count == 1,
        "Image Size parity fixture lacks its retained artwork source")
    document.undoManager?.removeAllActions()
    document.history.reset(document.captureEditingState())
    document.updateChangeCount(.changeCleared)
    let before = document.doc
    let history = document.history.entries.count
    ActionRecorder.shared.start()

    let sheet = try openImageSizeSheet(editor)
    let unit: NSPopUpButton = try imageSizeControl(sheet, "image-size-unit", "dimension unit")
    let width: NSTextField = try imageSizeControl(sheet, "image-size-width", "width field")
    let height: NSTextField = try imageSizeControl(sheet, "image-size-height", "height field")
    let filter: NSPopUpButton = try imageSizeControl(sheet, "image-size-filter", "filter")
    let result: NSTextField = try imageSizeControl(sheet, "image-size-result", "result readout")
    let apply: NSButton = try imageSizeControl(sheet, "image-size-apply", "Apply button")
    try require(unit.titleOfSelectedItem == "Pixels", "Image Size did not default to Pixels")
    chooseImageSizePopup(unit, index: ImageSizeUnit.percent.rawValue)
    let percent = typed ? "62.5" : "150"
    try typeImageSize(width, percent)
    let resolved = typed ? (width: 6, height: 4) : (width: 11, height: 8)
    try require(result.stringValue.contains("\(resolved.width) × \(resolved.height) px"),
        "Image Size result readout was \(result.stringValue)")
    try require(height.doubleValue != Double(percent),
        "Asymmetric achieved percentages were hidden")
    let layoutEvidence = output.appendingPathComponent(
        "image-size-percentage-\(typed ? "u16" : "byte").png")
    try screenshot(sheet.view, layoutEvidence)
    filter.selectItem(at: typed ? 1 : 0)
    let filterName = typed ? "bilinear" : "nearest"
    apply.performClick(nil)
    try waitUntil({ document.doc?.width == resolved.width && document.doc?.height == resolved.height },
                  "Percentage Image Size did not publish resolved pixels")
    let steps = ActionRecorder.shared.stop()
    try require(steps.count == 1 && steps[0].tool == "image_size"
        && steps[0].arguments["width"] as? Int == resolved.width
        && steps[0].arguments["height"] as? Int == resolved.height
        && steps[0].arguments["filter"] as? String == filterName,
        "Percentage Image Size did not record canonical resolved pixels")
    _ = try geometryCall("image_size",
        ["width": resolved.width, "height": resolved.height, "filter": filterName], reference)
    let uiState = try imageSizeState(document)
    let referenceState = try imageSizeState(reference)
    try require(uiState as NSDictionary == referenceState as NSDictionary,
        "UI percentage result differs from an identical MCP resolved-pixel resize")
    let actionName = "image-size-percent-\(typed ? "u16" : "byte")-\(UUID().uuidString)"
    _ = try geometryCall("save_action", [
        "name": actionName, "steps": steps.map { $0.jsonObject() },
    ], replayed)
    let replayReport = try geometryCall("run_action", ["name": actionName], replayed)
    try require(replayReport["ok"] as? Bool == true
        && (replayReport["steps_run"] as? NSNumber)?.intValue == 1,
        "Recorded Image Size Action did not replay one successful step: \(replayReport)")
    try require(try imageSizeState(replayed) as NSDictionary == uiState as NSDictionary,
        "Recorded Image Size Action did not reproduce the full native UI result")
    try require(document.history.entries.count == history + 1 && document.isDocumentEdited,
        "Percentage Image Size did not make one dirty history transition")
    guard let resultOwner = document.doc else {
        throw DoctoredCoreError(message: "Percentage Image Size lost its result")
    }
    document.undoManager?.undo(); pump()
    try require(document.doc === before, "Percentage Image Size Undo lost the source owner")
    document.undoManager?.redo(); pump()
    try require(document.doc === resultOwner, "Percentage Image Size Redo lost the result owner")

    return ["depth": typed ? 16 : 8, "percent": percent,
            "resolved": [resolved.width, resolved.height], "filter": filterName,
            "history_steps": 1, "native_mcp_equal": true,
            "native_action_replay_equal": true, "artwork_source_preserved": true,
            "layout_evidence": layoutEvidence.path,
            "recorded": steps[0].arguments]
}

private func percentageNoOpsAndValidation() throws -> [String: Any] {
    let (document, editor, source) = try openByteDocument(width: 13, height: 7)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    document.undoManager?.removeAllActions()
    let history = document.history.entries.count

    // Unit-only + 100% is a complete no-op.
    ActionRecorder.shared.start()
    var sheet = try openImageSizeSheet(editor)
    var unit: NSPopUpButton = try imageSizeControl(sheet, "image-size-unit", "dimension unit")
    var width: NSTextField = try imageSizeControl(sheet, "image-size-width", "width field")
    var apply: NSButton = try imageSizeControl(sheet, "image-size-apply", "Apply button")
    chooseImageSizePopup(unit, index: ImageSizeUnit.percent.rawValue)
    try typeImageSize(width, "100")
    apply.performClick(nil); pump()
    let noOpSteps = ActionRecorder.shared.stop()
    try require(document.doc === source && document.history.entries.count == history
        && !document.isDocumentEdited && noOpSteps.isEmpty,
        "100 percent/unit-only Image Size published a change")

    // Every refusal is entered through the real Percent editor. Apply and a
    // unit switch are both attempted while the bad text is still active;
    // neither may fall back to the last accepted integer result.
    let badPercent: [(name: String, text: String)] = [
        ("empty", ""), ("zero", "0"), ("negative", "-1"),
        ("nan", "nan"), ("infinity", "inf"), ("huge", "1e300"),
        ("axis-overflow", "200000"), ("product-overflow", "110000"),
    ]
    var refused: [String] = []
    for bad in badPercent {
        sheet = try openImageSizeSheet(editor)
        unit = try imageSizeControl(sheet, "image-size-unit", "dimension unit")
        width = try imageSizeControl(sheet, "image-size-width", "width field")
        apply = try imageSizeControl(sheet, "image-size-apply", "Apply button")
        chooseImageSizePopup(unit, index: ImageSizeUnit.percent.rawValue)
        try require(unit.titleOfSelectedItem == "Percent",
            "Invalid matrix did not enter Percent mode")
        try typeImageSize(width, bad.text)
        let entered = width.stringValue
        try require(!apply.isEnabled,
            "Invalid Percent value \(bad.name) left Apply enabled")
        ActionRecorder.shared.start()
        apply.performClick(nil); pump()
        chooseImageSizePopup(unit, index: ImageSizeUnit.pixels.rawValue)
        let rejectedSteps = ActionRecorder.shared.stop()
        try require(unit.titleOfSelectedItem == "Percent" && width.stringValue == entered,
            "Invalid Percent value \(bad.name) was replaced while switching units")
        try require(document.doc === source && document.history.entries.count == history
            && !document.isDocumentEdited && rejectedSteps.isEmpty,
            "Invalid Percent value \(bad.name) published history, pixels or an Action")
        refused.append(bad.name)
        sheet.dismiss(nil); pump()
    }

    // Cancel after a valid pending percentage remains a no-op.
    sheet = try openImageSizeSheet(editor)
    unit = try imageSizeControl(sheet, "image-size-unit", "dimension unit")
    width = try imageSizeControl(sheet, "image-size-width", "width field")
    let cancel: NSButton = try imageSizeControl(sheet, "image-size-cancel", "Cancel button")
    chooseImageSizePopup(unit, index: ImageSizeUnit.percent.rawValue)
    try typeImageSize(width, "50")
    cancel.performClick(nil); pump()
    try require(document.doc === source && document.history.entries.count == history,
        "Cancelled percentage Image Size published")
    return ["no_op_steps": noOpSteps.count, "invalid_values": refused,
            "cancel_unchanged": true, "invalid_switch_preserved": true]
}

private func resampleOffAndCombinedPPI() throws -> [String: Any] {
    let (document, editor, _) = try openDocument(width: 10, height: 6)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    document.undoManager?.removeAllActions()
    let history = document.history.entries.count
    ActionRecorder.shared.start()
    let sheet = try openImageSizeSheet(editor)
    let unit: NSPopUpButton = try imageSizeControl(sheet, "image-size-unit", "dimension unit")
    let width: NSTextField = try imageSizeControl(sheet, "image-size-width", "width field")
    let height: NSTextField = try imageSizeControl(sheet, "image-size-height", "height field")
    let lock: NSButton = try imageSizeControl(sheet, "image-size-lock", "aspect lock")
    let resample: NSButton = try imageSizeControl(sheet, "image-size-resample", "Resample")
    let filter: NSPopUpButton = try imageSizeControl(sheet, "image-size-filter", "filter")
    let resolution: NSTextField = try imageSizeControl(
        sheet, "image-size-resolution", "resolution field")
    let printWidth: NSTextField = try imageSizeControl(
        sheet, "image-size-print-width", "print width")
    let printUnit: NSPopUpButton = try imageSizeControl(
        sheet, "image-size-print-unit", "print unit")
    let result: NSTextField = try imageSizeControl(
        sheet, "image-size-result", "result readout")
    let apply: NSButton = try imageSizeControl(sheet, "image-size-apply", "Apply button")

    // Unlocked axes survive repeated presentation-unit switches without
    // round-tripping through displayed percentages. A later locked edit is
    // resolved from the original 10 x 6 baseline, not the pending 15 x 3.
    chooseImageSizePopup(unit, index: ImageSizeUnit.percent.rawValue)
    lock.performClick(nil); pump()
    try require(lock.state == .off, "Image Size lock did not turn off")
    try typeImageSize(width, "150")
    try require(result.stringValue.contains("15 × 6 px")
        && abs(height.doubleValue - 100) < 0.000001,
        "Unlocked Percent width did not preserve the independent height")
    chooseImageSizePopup(unit, index: ImageSizeUnit.pixels.rawValue)
    try require(width.integerValue == 15 && height.integerValue == 6,
        "Percent to Pixels switch changed the pending integer size")
    chooseImageSizePopup(unit, index: ImageSizeUnit.percent.rawValue)
    try require(abs(width.doubleValue - 150) < 0.000001
        && abs(height.doubleValue - 100) < 0.000001,
        "Repeated Pixels/Percent switch changed pending dimensions")
    try typeImageSize(height, "50")
    try require(result.stringValue.contains("15 × 3 px"),
        "Unlocked Percent height did not preserve the independent width")
    chooseImageSizePopup(unit, index: ImageSizeUnit.pixels.rawValue)
    try require(width.integerValue == 15 && height.integerValue == 3,
        "Second unit switch changed independently edited dimensions")
    chooseImageSizePopup(unit, index: ImageSizeUnit.percent.rawValue)
    try require(abs(width.doubleValue - 150) < 0.000001
        && abs(height.doubleValue - 50) < 0.000001,
        "Independent edits did not survive a repeated unit switch")
    lock.performClick(nil); pump()
    try require(lock.state == .on, "Image Size lock did not turn back on")
    try typeImageSize(width, "125")
    try require(result.stringValue.contains("13 × 8 px"),
        "Locked Percent edit did not use the original aspect/base")
    try typeImageSize(height, "200")
    try require(result.stringValue.contains("20 × 12 px"),
        "Repeated locked Percent edit compounded the pending result")

    // Off discards the pending pixel result. On re-exposes that same original
    // 10 x 6 result with all pixel controls live again.
    resample.performClick(nil); pump()
    try require(!width.isEnabled && !height.isEnabled && !unit.isEnabled
        && !lock.isEnabled && !filter.isEnabled,
        "Resample off did not disable all pixel controls")
    try require(width.doubleValue == 100 && height.doubleValue == 100,
        "Resample off did not expose the original pixel size as 100 percent")
    resample.performClick(nil); pump()
    try require(width.isEnabled && height.isEnabled && unit.isEnabled
        && lock.isEnabled && filter.isEnabled
        && result.stringValue.contains("10 × 6 px")
        && width.doubleValue == 100 && height.doubleValue == 100,
        "Resample on did not restore the original enabled pixel state")
    resample.performClick(nil); pump()
    chooseImageSizePopup(printUnit, index: PrintUnit.centimetres.rawValue)
    try typeImageSize(printWidth, "2.54")
    try typeImageSize(resolution, "60")
    apply.performClick(nil); pump()
    let steps = ActionRecorder.shared.stop()
    try require(document.doc?.width == 10 && document.doc?.height == 6,
        "Resample-off print edit changed pixels")
    try require(steps.count == 1 && steps[0].tool == "set_resolution"
        && document.history.entries.count == history + 1,
        "Resample-off print edit was not one Undo transition/one PPI step")
    document.undoManager?.undo(); pump()
    try require(document.doc?.resolution.x == 72 && document.doc?.resolution.y == 72,
        "Resample-off Undo did not restore PPI")
    document.undoManager?.redo(); pump()
    try require(document.doc?.resolution.x == 60 && document.doc?.resolution.y == 60,
        "Resample-off Redo did not restore the print/PPI result")

    // A combined percent + PPI edit remains one history transition while
    // recording the existing two resolved operations.
    let combinedHistory = document.history.entries.count
    ActionRecorder.shared.start()
    let combined = try openImageSizeSheet(editor)
    let combinedUnit: NSPopUpButton = try imageSizeControl(
        combined, "image-size-unit", "dimension unit")
    let combinedWidth: NSTextField = try imageSizeControl(
        combined, "image-size-width", "width field")
    let combinedResolution: NSTextField = try imageSizeControl(
        combined, "image-size-resolution", "resolution field")
    let combinedApply: NSButton = try imageSizeControl(
        combined, "image-size-apply", "Apply button")
    chooseImageSizePopup(combinedUnit, index: ImageSizeUnit.percent.rawValue)
    try typeImageSize(combinedWidth, "150")
    try typeImageSize(combinedResolution, "144")
    combinedApply.performClick(nil); pump()
    let combinedSteps = ActionRecorder.shared.stop()
    try require(combinedSteps.map(\.tool) == ["image_size", "set_resolution"]
        && document.history.entries.count == combinedHistory + 1,
        "Combined percentage/PPI edit lost its two steps or one Undo transition")
    return ["resample_off_pixels": [10, 6], "print_units": ["inches", "cm"],
            "unlocked_switches": [15, 3], "locked_original_base": [20, 12],
            "resample_reenabled_original": true,
            "resolution_only_steps": steps.map(\.tool),
            "combined_steps": combinedSteps.map(\.tool), "combined_one_undo": true]
}

private func staleAndUnsupportedImageSize() throws -> [String: Any] {
    var stale: [[String: Any]] = []
    for typed in [false, true] {
        let opened = typed ? try openDocument(width: 8, height: 6)
            : try openByteDocument(width: 8, height: 6)
        let (document, editor, source) = opened
        defer { document.close(); pump() }
        let sheet = try openImageSizeSheet(editor)
        let width: NSTextField = try imageSizeControl(sheet, "image-size-width", "width field")
        let apply: NSButton = try imageSizeControl(sheet, "image-size-apply", "Apply button")
        try typeImageSize(width, "7")
        _ = try geometryCall("add_guide", ["orientation": "vertical", "position": 1.5], document)
        let foreign = document.doc
        let history = document.history.entries.count
        apply.performClick(nil); pump()
        try require(document.doc === foreign && document.doc !== source
            && document.history.entries.count == history,
            "Stale \(typed ? 16 : 8)-bit Image Size overwrote the foreign edit")
        sheet.dismiss(nil); pump()
        stale.append(["depth": typed ? 16 : 8, "refused": true])
    }

    let (floatDocument, floatEditor, floatSource) = try openFloatDocument()
    defer { floatDocument.close(); pump() }
    let resizeItem = NSMenuItem(title: "Image Size", action:
        #selector(EditorViewController.resizeImage(_:)), keyEquivalent: "")
    // A float document resamples its linear-light samples directly.
    try require(floatEditor.validateUserInterfaceItem(resizeItem),
        "Float Image Size must validate")
    let floatReply = try call("image_size", ["width": 2, "height": 2], document: floatDocument)
    try require(!(try responseIsError(floatReply)) && floatDocument.doc !== floatSource,
        "Float image_size did not publish")

    let (linearDocument, linearEditor, _) = try openByteDocument(width: 8, height: 6)
    defer { linearDocument.close(); pump() }
    _ = try geometryCall("set_evaluation", ["policy": "linear_light"], linearDocument)
    let linear = linearDocument.doc
    let history = linearDocument.history.entries.count
    linearEditor.resizeImage(nil); pump()
    try require(imageSizeSheets(linearEditor).isEmpty && linearDocument.doc === linear
        && linearDocument.history.entries.count == history,
        "Linear-light Image Size opened or published")
    return ["stale": stale, "float_admitted": true, "linear_light_refused": true]
}

/// The live `tools/list`, over a scratch agent socket this probe binds (`DR_AGENT_SOCKET` pointed
/// at a fresh `0700` folder under NSTemporaryDirectory(), so the person's socket is never bound)
/// and read with `/usr/bin/curl --unix-socket`. tools/list is answered on the core's serve thread,
/// so waiting for curl on main cannot deadlock.
private func liveImageSizeCatalog() throws -> [[String: Any]] {
    let server = AgentServer.shared
    try require(!server.isRunning, "Image Size probe did not own the live agent transport")
    var template = Array((NSTemporaryDirectory() as NSString)
        .appendingPathComponent("rz-imagesize-XXXXXX").utf8CString)
    guard let folder = template.withUnsafeMutableBufferPointer({ buffer -> String? in
        guard let base = buffer.baseAddress, mkdtemp(base) != nil else { return nil }
        return String(cString: base)
    }) else { throw DoctoredCoreError(message: "Image Size probe could not make a socket folder") }
    let socket = (folder as NSString).appendingPathComponent("agent.sock")
    let old = ProcessInfo.processInfo.environment[AgentServer.socketEnvironmentKey]
    try require(setenv(AgentServer.socketEnvironmentKey, socket, 1) == 0,
        "Image Size probe could not point DR_AGENT_SOCKET at a scratch socket")
    defer {
        if let old {
            setenv(AgentServer.socketEnvironmentKey, old, 1)
        } else {
            unsetenv(AgentServer.socketEnvironmentKey)
        }
    }
    server.bindSocket()
    defer { server.unbindSocket() }
    try require(server.boundSocketPath == socket,
        "Live agent transport did not bind the scratch socket \(socket)")

    let object: [String: Any] = [
        "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": [:],
    ]
    let curl = Process()
    curl.executableURL = URL(fileURLWithPath: "/usr/bin/curl")
    curl.arguments = ["--unix-socket", socket, "-sS", "--max-time", "30", "-H",
                      "Content-Type: application/json", "--data-binary", "@-", "http://localhost/mcp"]
    let input = Pipe(), output = Pipe()
    curl.standardInput = input
    curl.standardOutput = output
    try curl.run()
    input.fileHandleForWriting.write(try JSONSerialization.data(withJSONObject: object, options: .sortedKeys))
    try input.fileHandleForWriting.close()
    let data = output.fileHandleForReading.readDataToEndOfFile()
    curl.waitUntilExit()
    guard curl.terminationStatus == 0,
          let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          root["error"] == nil,
          let result = root["result"] as? [String: Any],
          let tools = result["tools"] as? [[String: Any]] else {
        throw DoctoredCoreError(message: "Live tools/list response was invalid (curl exit "
            + "\(curl.terminationStatus))")
    }
    return tools
}

private func imageSizeCatalog() throws -> [String: Any] {
    let tools = try liveImageSizeCatalog()
    guard
        let tool = tools.first(where: { $0["name"] as? String == "image_size" }),
        let schema = tool["inputSchema"] as? [String: Any],
        let required = schema["required"] as? [String],
        let properties = schema["properties"] as? [String: Any]
    else { throw DoctoredCoreError(message: "Live image_size catalog entry is unavailable") }
    try require(Set(required) == Set(["width", "height"])
        && properties["width"] != nil && properties["height"] != nil
        && properties["percent"] == nil,
        "image_size public schema changed for UI-only percentage entry")
    let listedNames = Set(tools.compactMap { $0["name"] as? String })
    let registeredNames = Set(AgentServer.handlers.keys)
    try require(listedNames == registeredNames,
        "Live tools/list omitted or added registered tools: missing "
            + "\(registeredNames.subtracting(listedNames).sorted()), extra "
            + "\(listedNames.subtracting(registeredNames).sorted())")
    return ["tool_count": tools.count, "required": required.sorted(),
            "percent_argument": false, "transport": "mcp-socket-tools/list",
            "complete_registration": true]
}

func imageSizePercentageUI(output: URL) throws -> [String: Any] {
    let conversions = try dimensionMatrix()
    let actual = try [actualPercentageResize(false, output: output),
                      actualPercentageResize(true, output: output)]
    let validation = try percentageNoOpsAndValidation()
    let print = try resampleOffAndCombinedPPI()
    let refusals = try staleAndUnsupportedImageSize()
    let catalog = try imageSizeCatalog()
    return ["case": "image-size-percentage-ui", "conversion_trials": conversions,
            "actual_sheets": actual, "validation": validation, "print": print,
            "refusals": refusals, "catalog": catalog,
            "owned_view_evidence": output.path,
            "scope": "Actual Image Size sheets at byte/u16 depth, field-editor entry, "
                + "resolved-pixel MCP/Action parity, Undo/Redo, PPI, refusals and live catalog"]
}
