import AppKit
import ObjectiveC.runtime

// The v1 16-bit editing batch and the "late busy reopen" case. Every case
// calls the real sheets' controls and the controller's own actions, so none
// needs a key window.
//
// "late busy reopen": a filter sheet's Apply on a text layer whose
// worker slot is taken while the rasterize prompt is up comes back as a fresh
// sheet with the refused values, at 8 and 16 bits, and the 8-bit job panel's
// bar ends at the job's final progress.
// "typed style preview": the Layer Style sheet previews live on a 16-bit
// document, reduced above 4 MP.
// "layer via typed": Layer Via Copy / Via Cut on a 16-bit document.

private func v16Fields(_ view: NSView) -> [NSTextField] {
    (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(v16Fields)
}

private func v16Sliders(_ view: NSView) -> [NSSlider] {
    (view as? NSSlider).map { [$0] } ?? view.subviews.flatMap(v16Sliders)
}

private func v16Bars(_ view: NSView) -> [NSProgressIndicator] {
    (view as? NSProgressIndicator).map { [$0] } ?? view.subviews.flatMap(v16Bars)
}

private func v16Field(_ view: NSView, _ identifier: String) -> NSTextField? {
    v16Fields(view).first { $0.accessibilityIdentifier() == identifier }
}

private func v16Press(_ button: NSButton?, _ what: String) throws {
    guard let button, let action = button.action else {
        throw ProbeFailure(description: "\(what): the button is missing")
    }
    try require(NSApp.sendAction(action, to: button.target, from: button), "\(what) was delivered")
}

private func v16Samples(_ doc: RasterDocument?, _ layer: Int) throws -> [UInt16] {
    guard let image = doc?.precision?.layerSource(layer) else {
        throw ProbeFailure(description: "layer \(layer) has no 16-bit source")
    }
    return try image.withRGBA16 { Array($0) }
}

// MARK: - Late busy reopen

/// One depth. The sheet is Gaussian Blur at sigma 7 on a text layer. A
/// probe-local `NSAlert.runModal` IMP (restored in a `defer`; the
/// `transform_prompts.swift` swap) answers "Rasterize" and, on the first
/// prompt only, takes the worker slot with an anonymous `WorkerHolder`, which
/// is the late occupant: it arrives after the sheet's own slot wait. Returns
/// the 8-bit job panel's bar as the second Apply left it.
private func verifyLateBusyReopen(depth: Int) throws -> Double? {
    let tag = "[late busy reopen \(depth)-bit]"
    let document = depth == 8
        ? try fixture(width: 320, height: 240) : try precisionFixture(width: 320, height: 240)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 40, "y": 60, "size": 48])
    guard let base = document.doc, base.isHighDepth == (depth == 16),
          let text = (0..<base.layerCount).first(where: { base.hasArtworkDescription($0) }) else {
        throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
    }
    controller.setSelectedLayers(.single(text))
    document.undoManager?.removeAllActions()
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count
    let sigma = 7.0

    let first = SliderSheetController.gaussianBlur(document: document, canvas: controller.canvas)
    controller.presentAsSheet(first)
    try pumpUntil({ first.view.window?.isVisible == true }, seconds: 30)
    guard let slider = v16Sliders(first.view).first else {
        throw ProbeFailure(description: "\(tag) the sheet has no slider")
    }
    slider.doubleValue = sigma
    slider.sendAction(slider.action, to: slider.target)

    guard let method = class_getInstanceMethod(NSAlert.self, #selector(NSAlert.runModal)) else {
        throw ProbeFailure(description: "\(tag) NSAlert.runModal missing")
    }
    var prompts: [String] = []
    var holder: WorkerHolder?
    var holderFailure: Error?
    let rasterize: @convention(block) (AnyObject) -> Int = { alert in
        prompts.append((alert as? NSAlert)?.messageText ?? "-")
        if prompts.count == 1 {
            do { holder = try WorkerHolder() } catch { holderFailure = error }
        }
        return NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
    }
    let replacement = imp_implementationWithBlock(rasterize)
    let original = method_setImplementation(method, replacement)
    defer {
        method_setImplementation(method, original)
        imp_removeBlock(replacement)
        if let holder, !holder.taken { try? holder.releaseNow() }
    }

    // The first Apply: refused late, after the sheet dismissed itself.
    try v16Press(buttons(first.view).first { $0.keyEquivalent == "\r" }, "\(tag) the first Apply")
    if let holderFailure { throw holderFailure }
    try require(prompts == ["Rasterize text layer?"],
                "\(tag) one rasterize prompt before the refusal: \(prompts)")
    try require(holder != nil && BackgroundWork.occupied,
                "\(tag) the worker slot was taken while the prompt was up")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) the late refusal recorded no alert: "
                + "\(AutomationAlerts.recorded.suffix(2).map(\.informativeText))")
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true,
                "\(tag) the refused Apply changed nothing")
    let reopened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? SliderSheetController }
            .last { $0 !== first }
    }
    try pumpUntil({ reopened()?.view.window?.isVisible == true }, seconds: 30)
    guard let second = reopened() else {
        throw ProbeFailure(description: "\(tag) no sheet came back")
    }
    let status = { v16Field(second.view, "tone.preview.status") }
    try pumpUntil({ status()?.stringValue == BackgroundWork.busyMessage }, seconds: 10)
    try require(status()?.isHidden == false, "\(tag) the busy sentence is shown")
    try require(v16Sliders(second.view).first?.doubleValue == sigma,
                "\(tag) the sheet came back at sigma \(sigma): "
                + "\(v16Sliders(second.view).first?.doubleValue ?? -1)")
    try require(document.doc === base, "\(tag) the document is untouched under the new sheet")

    // The slot is released; the second Apply commits.
    try holder?.releaseNow()
    for window in NSApp.windows where window is NSPanel {
        window.contentView.map(v16Bars)?.forEach { $0.doubleValue = 0 }
    }
    try v16Press(buttons(second.view).first { $0.keyEquivalent == "\r" }, "\(tag) the second Apply")
    // Read before any run-loop turn: the panel as the job's wait left it.
    var bar: Double?
    if depth == 8 {
        bar = NSApp.windows.first { $0 is NSPanel && $0.title == "Gaussian Blur" }?
            .contentView.map(v16Bars)?.first?.doubleValue
        try require((bar ?? -1) >= 0.999,
                    "\(tag) the job panel's bar ends at the job's final progress: "
                    + "\(bar.map { "\($0)" } ?? "no panel")")
    }
    try pumpUntil({ document.doc !== base && second.view.window?.isVisible != true }, seconds: 30)
    try require(prompts == ["Rasterize text layer?", "Rasterize text layer?"],
                "\(tag) the second Apply asked again: \(prompts)")
    try require(document.history.entries.count == entries + 1,
                "\(tag) one history entry: \(document.history.entries.map(\.name))")
    try require(document.doc?.hasArtworkDescription(text) == false,
                "\(tag) the committed blur rasterized the text layer")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.informativeText))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the document")
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 10)
    return bar
}

func verifyLateBusyReopen() throws {
    let bar = try verifyLateBusyReopen(depth: 8)
    _ = try verifyLateBusyReopen(depth: 16)
    print("PASS: late busy reopen — at 8 and 16 bits a Gaussian Blur Apply on a text layer whose "
        + "worker slot is taken during the rasterize prompt records no alert and comes back as a "
        + "sheet at the refused sigma with \"\(BackgroundWork.busyMessage)\"; after the slot is "
        + "released a second Apply commits as one step; the 8-bit job panel's bar read "
        + "\(bar.map { "\($0)" } ?? "-") when Apply returned")
}

// MARK: - The typed Layer Style preview

/// The mean colour of `image`, drawn into one sRGB pixel.
private func v16MeanColor(_ image: CGImage) -> [Int]? {
    guard let space = CGColorSpace(name: CGColorSpace.sRGB),
          let context = CGContext(data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                                  space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    context.draw(image, in: CGRect(x: 0, y: 0, width: 1, height: 1))
    guard let data = context.data else { return nil }
    let pixel = data.bindMemory(to: UInt8.self, capacity: 4)
    return (0..<4).map { Int(pixel[$0]) }
}

/// The Layer Style sheet opened on `initial`, with its first preview once
/// that has landed at `size`.
private func v16OpenStyleSheet(
    _ controller: EditorViewController, initial: LayerStyleEffectKind, size: (Int, Int), _ tag: String
) throws -> (sheet: LayerStyleSheetController, preview: CGImage) {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.editLayerStyle(0, initial: initial)
    let opened = {
        (controller.presentedViewControllers ?? []).compactMap { $0 as? LayerStyleSheetController }
            .last { !known.contains(ObjectIdentifier($0)) }
    }
    try pumpUntil({ opened()?.view.window?.isVisible == true }, seconds: 30)
    guard let sheet = opened() else { throw ProbeFailure(description: "\(tag) no Layer Style sheet") }
    try pumpUntil({ controller.canvas.previewImage != nil }, seconds: 60)
    guard let preview = controller.canvas.previewImage else {
        throw ProbeFailure(description: "\(tag) no preview landed")
    }
    try require(preview.width == size.0 && preview.height == size.1,
                "\(tag) the preview is \(size.0) × \(size.1): \(preview.width) × \(preview.height)")
    return (sheet, preview)
}

/// The Layer Style sheet opened on Color Overlay (its default: #ff0000 at
/// 100%, Normal), once its first preview has landed at `size`.
private func v16StyleSheet(_ controller: EditorViewController, size: (Int, Int),
                           _ tag: String) throws -> LayerStyleSheetController {
    let (sheet, preview) = try v16OpenStyleSheet(controller, initial: .colorOverlay, size: size, tag)
    // The oracle: an opaque canvas-sized layer under a 100% Normal red Color
    // Overlay is red everywhere. 5 of 255 covers the one-pixel mean's rounding.
    let mean = v16MeanColor(preview) ?? []
    try require(mean.count == 4 && mean[0] >= 250 && mean[1] <= 5 && mean[2] <= 5,
                "\(tag) the preview shows the red Color Overlay: \(mean)")
    return sheet
}

private func verifyTypedStylePreview(width: Int, height: Int) throws -> String {
    let tag = "[typed style preview \(width)×\(height)]"
    let document = try precisionFixture(width: width, height: height)
    defer { document.close() }
    let controller = try editor(document)
    guard let base = document.doc, base.isHighDepth else {
        throw ProbeFailure(description: "\(tag) a 16-bit fixture")
    }
    let scale = ProxyScale(width: width, height: height)
    let size = (scale?.width ?? width, scale?.height ?? height)
    let expectedStatus = scale == nil ? "" : PrecisionStylePreview.reducedStatus
    let entry = SelectionWorkBudget.scratchBytes
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    // Cancel: no preview, no reservation, no edit.
    let cancelled = try v16StyleSheet(controller, size: size, tag)
    let status = v16Field(cancelled.view, "layer-style.status")?.stringValue
    try require(status == expectedStatus, "\(tag) the status line reads \"\(expectedStatus)\": "
                + "\"\(status ?? "missing")\"")
    try v16Press(buttons(cancelled.view).first { $0.keyEquivalent == "\u{1b}" }, "\(tag) Cancel")
    try require(controller.canvas.previewImage == nil, "\(tag) Cancel leaves no preview")
    try require(SelectionWorkBudget.scratchBytes == entry,
                "\(tag) Cancel leaves the registry at its entry value: "
                + "\(SelectionWorkBudget.scratchBytes) B against \(entry) B")
    // A render already on its way must not land after Cancel.
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    try require(controller.canvas.previewImage == nil, "\(tag) no preview lands after Cancel")
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true, "\(tag) Cancel changed nothing")

    // Apply: one step holding the authored style.
    let applied = try v16StyleSheet(controller, size: size, tag)
    try v16Press(buttons(applied.view).first { $0.identifier == LayerStyleKnockoutRows.applyIdentifier },
                 "\(tag) Apply")
    try pumpUntil({ document.history.entries.count == entries + 1 }, seconds: 60)
    try require(controller.canvas.previewImage == nil, "\(tag) Apply leaves no preview")
    try require(SelectionWorkBudget.scratchBytes == entry,
                "\(tag) Apply leaves the registry at its entry value: "
                + "\(SelectionWorkBudget.scratchBytes) B against \(entry) B")
    let overlay = document.doc?.layerStylePayload(0)?.colorOverlays.first
    try require(overlay?.enabled == true && overlay?.color == "#ff0000",
                "\(tag) the layer carries the authored Color Overlay: \(String(describing: overlay))")
    try require(document.doc?.width == width && document.doc?.height == height,
                "\(tag) the committed document is full size")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.informativeText))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the document")
    return "\(size.0) × \(size.1)" + (scale == nil ? "" : " (reduced)")
}

/// `image`'s row `y` as sRGB RGBA bytes, drawn 1:1 at the image's own size.
private func v16Row(_ image: CGImage, y: Int) -> [[Int]]? {
    let (w, h) = (image.width, image.height)
    guard y >= 0, y < h, let space = CGColorSpace(name: CGColorSpace.sRGB),
          let context = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                                  bytesPerRow: w * 4, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
    guard let data = context.data else { return nil }
    let bytes = data.bindMemory(to: UInt8.self, capacity: w * h * 4)
    return (0..<w).map { x in (0..<4).map { Int(bytes[(y * w + x) * 4 + $0]) } }
}

/// The reduced preview shrinks an effect's LENGTHS with the picture: a
/// 3000 × 2000 layer carrying a 100 px inside Stroke previews at 600 × 400
/// with a band of 100 / `ProxyScale.factor` = 20 px. The oracle is that
/// geometry, read on the middle row (the same from either end, so the bitmap's
/// row order does not matter): the column half a band in from the left edge
/// is the stroke's green, and the column a band and a half in is the layer's
/// own colour, which is never green (the fixture's green stays under 196 of
/// 255 and its blue at or above 11). A style sent unscaled draws a 100 px
/// band and one scaled the wrong way a 250 px one (the size ceiling); either
/// paints that second column green.
private func verifyTypedStylePreviewLengths() throws -> String {
    let tag = "[typed style preview stroke]"
    let (width, height) = (3000, 2000)
    let size = 100.0
    var style = LayerStyle()
    var stroke = StrokeEffect()
    stroke.size = size
    stroke.position = "inside"
    stroke.color = "#00ff00"
    style.strokes = [stroke]
    let document = try precisionFixture(width: width, height: height) { source in
        guard let styled = try source.withLayerStylePayload(0, style) else {
            throw ProbeFailure(description: "\(tag) the fixture took no Stroke")
        }
        return styled
    }
    defer { document.close() }
    let controller = try editor(document)
    guard let base = document.doc, base.isHighDepth,
          let scale = ProxyScale(width: width, height: height) else {
        throw ProbeFailure(description: "\(tag) a 16-bit fixture above the proxy threshold")
    }
    let band = Int((size / scale.factor).rounded())
    // The Stroke is already on, so opening on it leaves the style as authored.
    let (sheet, preview) = try v16OpenStyleSheet(
        controller, initial: .stroke, size: (scale.width, scale.height), tag)
    guard band >= 4, let row = v16Row(preview, y: scale.height / 2), row.count > band * 2 else {
        throw ProbeFailure(description: "\(tag) the preview's middle row could not be read")
    }
    // 5 of 255 covers the display conversion's rounding of a pure #00ff00.
    let isGreen = { (p: [Int]) in p[0] <= 5 && p[1] >= 250 && p[2] <= 5 }
    let (inBand, pastBand) = (row[band / 2], row[band + band / 2])
    try require(isGreen(inBand),
                "\(tag) column \(band / 2) is inside the \(band) px band: \(inBand)")
    try require(!isGreen(pastBand),
                "\(tag) column \(band + band / 2) is past the \(band) px band: \(pastBand)")
    try v16Press(buttons(sheet.view).first { $0.keyEquivalent == "\u{1b}" }, "\(tag) Cancel")
    try require(controller.canvas.previewImage == nil && document.doc === base,
                "\(tag) Cancel leaves no preview and no edit")
    return "\(band) px"
}

func verifyTypedStylePreview() throws {
    let small = try verifyTypedStylePreview(width: 512, height: 512)
    let large = try verifyTypedStylePreview(width: 3000, height: 2000)
    let band = try verifyTypedStylePreviewLengths()
    print("PASS: typed style preview — the Layer Style sheet on a 16-bit document previews a red "
        + "Color Overlay at \(small) on 512 × 512 and at \(large) on 3000 × 2000 with "
        + "\"\(PrecisionStylePreview.reducedStatus)\", and a 100 px inside Stroke on 3000 × 2000 as "
        + "a \(band) band; Cancel leaves no preview, the preview "
        + "registry at its entry value and no edit; Apply publishes one step holding the authored "
        + "style at full size")
}

// MARK: - Layer Via Copy / Cut at 16 bits

func verifyLayerViaTyped() throws {
    let tag = "[layer via typed]"
    let (width, height) = (64, 48)
    let document = try precisionFixture(width: width, height: height)
    defer { document.close() }
    let controller = try editor(document)
    guard let base = document.doc, base.isHighDepth else {
        throw ProbeFailure(description: "\(tag) a 16-bit fixture")
    }
    // The selection: the rect (10, 8, 30, 20) at full coverage.
    var mask = [UInt8](repeating: 0, count: width * height)
    for y in 8..<28 { for x in 10..<40 { mask[y * width + x] = 255 } }
    controller.agentSetSelection(CanvasSelection(shape: .mask(mask), canvasWidth: width,
                                                 canvasHeight: height))
    document.undoManager?.removeAllActions()
    let item = { (action: Selector) in
        controller.validateUserInterfaceItem(NSMenuItem(title: "", action: action, keyEquivalent: ""))
    }
    let copyAction = #selector(EditorViewController.layerViaCopy(_:))
    let cutAction = #selector(EditorViewController.layerViaCut(_:))
    try require(item(copyAction) && item(cutAction),
                "\(tag) Layer Via Copy and Layer Via Cut validate with a selection")
    let source = try v16Samples(base, 0)
    let name = (base.layerInfo(0)?.name ?? "Layer") + " copy"
    let alerts = AutomationAlerts.recorded.count

    /// The oracle, sample by sample: the new layer holds the source's RGBA
    /// inside the selection and alpha 0 outside it; the source is whole after
    /// a copy, and after a cut has alpha 0 inside and its own RGBA outside.
    func requireVia(cut: Bool, _ step: String) throws {
        // The cursor, not the entry count: the copy's undone step is still a
        // redo entry when the cut replaces it.
        let cursor = document.history.cursor
        if cut { controller.layerViaCut(nil) } else { controller.layerViaCopy(nil) }
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
        guard let doc = document.doc, doc !== base, doc.layerCount == 2 else {
            throw ProbeFailure(description: "\(tag) \(step) added no layer")
        }
        try require(document.activeLayerIndex == 1 && doc.layerInfo(1)?.name == name,
                    "\(tag) \(step) the new layer \"\(name)\" is above the source and active: "
                    + "\(doc.layerInfo(1)?.name ?? "-")")
        let lifted = try v16Samples(doc, 1), left = try v16Samples(doc, 0)
        try require(lifted.count == source.count && left.count == source.count,
                    "\(tag) \(step) both layers keep the source's size")
        for i in mask.indices {
            let was = Array(source[i * 4..<i * 4 + 4])
            let now = Array(left[i * 4..<i * 4 + 4])
            if mask[i] == 255 {
                try require(Array(lifted[i * 4..<i * 4 + 4]) == was,
                            "\(tag) \(step) sample \(i) of the new layer is the source's")
                try require(cut ? now[3] == 0 : now == was,
                            "\(tag) \(step) sample \(i) of the source: \(now) from \(was)")
            } else {
                try require(lifted[i * 4 + 3] == 0,
                            "\(tag) \(step) sample \(i) outside the selection is transparent")
                try require(now == was, "\(tag) \(step) sample \(i) of the source is untouched")
            }
        }
        let title = cut ? "Layer Via Cut" : "Layer Via Copy"
        try require(document.history.cursor == cursor + 1
                    && document.undoManager?.undoActionName == title,
                    "\(tag) \(step) one \"\(title)\" step: "
                    + "\(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) \(step) one undo restores the document")
        try require(document.activeLayerIndex == 0, "\(tag) \(step) the source is active again")
    }
    try requireVia(cut: false, "⌘J")
    try requireVia(cut: true, "⇧⌘J")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.informativeText))")
    print("PASS: layer via typed — on a 16-bit document with a selection Layer Via Copy and Layer "
        + "Via Cut validate, and their actions each add \"\(name)\" above the source holding the "
        + "selected samples exactly (the cut leaving alpha 0 behind) as one undo step")
}
