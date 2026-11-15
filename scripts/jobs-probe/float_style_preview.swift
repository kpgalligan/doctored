import AppKit

// "float style preview": the Layer Style sheet previews live on a 32-bit float document, as on a
// 16-bit one ("typed style preview"): `PrecisionStylePreview` over the sheet's capture, displayed
// through the document's own tone map. On the committed float fixture (48 × 32) and on a copy
// resized above 4 MP: the first preview lands at the expected size (reduced, with
// `PrecisionStylePreview.reducedStatus`, above 4 MP) and differs from the unstyled display; on the
// small document it equals the display of the document Apply publishes within
// `floatPreviewCodes`; Cancel leaves no preview, the preview registry at its entry value and no
// edit; Apply publishes one step holding the authored Color Overlay at full size, and one undo
// restores the document. How the preview looks on screen is the owner's hand check.

/// The bound between the preview and the published document's display, in 8-bit codes per
/// channel, declared before the first run: both are the same core render through the same
/// display preparation.
private let floatPreviewCodes = 1

private func fieldsOf(_ view: NSView) -> [NSTextField] {
    (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fieldsOf)
}

private func press(_ button: NSButton?, _ what: String) throws {
    guard let button, let action = button.action else {
        throw ProbeFailure(description: "\(what): the button is missing")
    }
    try require(NSApp.sendAction(action, to: button.target, from: button), "\(what) was delivered")
}

/// `image` as sRGB RGBA8 bytes at its own size.
private func rgba(_ image: CGImage) -> [UInt8]? {
    let (w, h) = (image.width, image.height)
    guard let space = CGColorSpace(name: CGColorSpace.sRGB),
          let context = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                                  bytesPerRow: w * 4, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { return nil }
    context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
    guard let data = context.data else { return nil }
    return Array(UnsafeBufferPointer(start: data.bindMemory(to: UInt8.self, capacity: w * h * 4),
                                     count: w * h * 4))
}

/// The largest per-channel difference between two equally sized images; nil when they differ
/// in size or cannot be drawn.
private func largestDifference(_ a: CGImage, _ b: CGImage) -> Int? {
    guard a.width == b.width, a.height == b.height, let x = rgba(a), let y = rgba(b) else {
        return nil
    }
    return zip(x, y).map { abs(Int($0) - Int($1)) }.max() ?? 0
}

/// The display `PreparedDocumentEdit` prepares for `doc`, the canvas's own path.
private func display(_ doc: RasterDocument, _ tag: String) throws -> CGImage {
    let prepared = PreparedDocumentEdit(doc)
    guard let image = prepared.preview else {
        throw ProbeFailure(description: "\(tag) no display: \(prepared.error ?? "unknown")")
    }
    return image
}

/// The sheet opened on Color Overlay (its default: #ff0000 at 100 %, Normal), with its first
/// preview once that has landed at `size`.
private func openSheet(_ controller: EditorViewController, size: (Int, Int), _ tag: String)
    throws -> (sheet: LayerStyleSheetController, preview: CGImage) {
    let known = (controller.presentedViewControllers ?? []).map(ObjectIdentifier.init)
    controller.editLayerStyle(0, initial: .colorOverlay)
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

private func verifyFloatStylePreview(resizeTo: (Int, Int)?) throws -> String {
    let document = try floatFixture()
    defer { document.close() }
    let controller = try editor(document)
    if let (width, height) = resizeTo {
        _ = try agentOK("image_size", document, ["width": width, "height": height])
        document.undoManager?.removeAllActions()
    }
    guard let base = document.doc, base.isFloat else {
        throw ProbeFailure(description: "[float style preview] a 32-bit float fixture")
    }
    let (width, height) = (base.width, base.height)
    let tag = "[float style preview \(width)×\(height)]"
    let scale = ProxyScale(width: width, height: height)
    let size = (scale?.width ?? width, scale?.height ?? height)
    let expectedStatus = scale == nil ? "" : PrecisionStylePreview.reducedStatus
    let unstyled = try display(base, tag)
    let entry = SelectionWorkBudget.scratchBytes
    let entries = document.history.entries.count
    let alerts = AutomationAlerts.recorded.count

    // Cancel: a preview that differs from the unstyled display, then no preview, no
    // reservation, no edit.
    let (cancelled, first) = try openSheet(controller, size: size, tag)
    let status = fieldsOf(cancelled.view).first { $0.accessibilityIdentifier() == "layer-style.status" }
    try require(status?.stringValue == expectedStatus, "\(tag) the status line reads "
                + "\"\(expectedStatus)\": \"\(status?.stringValue ?? "missing")\"")
    if scale == nil {
        try require((largestDifference(first, unstyled) ?? 0) > 0,
                    "\(tag) the preview differs from the unstyled display")
    }
    try press(buttons(cancelled.view).first { $0.keyEquivalent == "\u{1b}" }, "\(tag) Cancel")
    try require(controller.canvas.previewImage == nil, "\(tag) Cancel leaves no preview")
    try require(SelectionWorkBudget.scratchBytes == entry,
                "\(tag) Cancel leaves the registry at its entry value: "
                + "\(SelectionWorkBudget.scratchBytes) B against \(entry) B")
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    try require(controller.canvas.previewImage == nil, "\(tag) no preview lands after Cancel")
    try require(document.doc === base && document.history.entries.count == entries
                && document.undoManager?.canUndo != true, "\(tag) Cancel changed nothing")

    // Apply: one step holding the authored style at full size; on the small document the
    // preview equals the published document's display.
    let (applied, preview) = try openSheet(controller, size: size, tag)
    try press(buttons(applied.view).first { $0.identifier == LayerStyleKnockoutRows.applyIdentifier },
              "\(tag) Apply")
    try pumpUntil({ document.history.entries.count == entries + 1 }, seconds: 60)
    try require(controller.canvas.previewImage == nil, "\(tag) Apply leaves no preview")
    try require(SelectionWorkBudget.scratchBytes == entry,
                "\(tag) Apply leaves the registry at its entry value: "
                + "\(SelectionWorkBudget.scratchBytes) B against \(entry) B")
    guard let published = document.doc, published.isFloat else {
        throw ProbeFailure(description: "\(tag) Apply did not publish a float document")
    }
    let overlay = published.layerStylePayload(0)?.colorOverlays.first
    try require(overlay?.enabled == true && overlay?.color == "#ff0000",
                "\(tag) the layer carries the authored Color Overlay: \(String(describing: overlay))")
    try require(published.width == width && published.height == height,
                "\(tag) the committed document is full size")
    var detail = "\(size.0) × \(size.1)" + (scale == nil ? "" : " (reduced)")
    if scale == nil {
        guard let difference = largestDifference(preview, try display(published, tag)) else {
            throw ProbeFailure(description: "\(tag) the preview and the published display differ in size")
        }
        try require(difference <= floatPreviewCodes,
                    "\(tag) the preview is the published display within \(floatPreviewCodes) codes: "
                    + "\(difference)")
        detail += ", \(difference) codes from the published display"
    }
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) no alert: \(AutomationAlerts.recorded.suffix(2).map(\.informativeText))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) one undo restores the document")
    return detail
}

func verifyFloatStylePreviews() throws {
    let small = try verifyFloatStylePreview(resizeTo: nil)
    let large = try verifyFloatStylePreview(resizeTo: (2600, 1700))
    print("PASS: float style preview — the Layer Style sheet on a 32-bit float document previews a "
        + "Color Overlay live (\(small); \(large)), Cancel leaves no preview, reservation or edit, "
        + "and Apply publishes one step holding the authored style at full size")
}
