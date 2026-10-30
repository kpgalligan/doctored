import AppKit

// Phase 12F-1: the UI-SOURCE half of CANVAS SIZE at depth.
//
// An MCP drive can call `canvas_size`; it proves the core op and the tool
// gate. What only the REAL UI can prove lives here: that Image ▸ Canvas
// Size… VALIDATES at 8 bits, at 16 bits and in a 32-bit float document, that
// the sheet's own width/height fields and anchor grid reach the edit its
// Apply button publishes, that the published document is EXACTLY what
// `RasterDocument.canvasResized` answers for the same numbers, and that the
// publication is ONE history step one Undo restores.

/// A probe-built menu item, the shape AppKit hands `validateUserInterfaceItem`.
private func canvasMenuItem(_ selector: Selector) -> NSMenuItem {
    let item = NSMenuItem(title: "Item", action: selector, keyEquivalent: "")
    item.target = nil
    return item
}

private func canvasSheets(_ editor: EditorViewController) -> [CanvasSizeSheetController] {
    (editor.presentedViewControllers ?? []).compactMap { $0 as? CanvasSizeSheetController }
}

/// The sheet just presented — the LAST one AppKit filed. A sheet whose Apply
/// already ran can linger in `presentedViewControllers` in a probe process
/// (its dismissal animation needs a real window session), which is why the
/// case dismisses each one explicitly when it is done with it and never
/// reads `.first`.
private func presentedCanvasSheet(_ editor: EditorViewController, after count: Int) throws
    -> CanvasSizeSheetController {
    try waitUntil({ canvasSheets(editor).count > count }, "The Canvas Size sheet never came up")
    guard let sheet = canvasSheets(editor).last else {
        throw DoctoredCoreError(message: "The Canvas Size sheet never came up")
    }
    _ = sheet.view
    pump()
    return sheet
}

/// The sheet's control carrying `identifier` — the probe's whole contract
/// with `Sheets.swift`'s three `CanvasSizeSheetController` identifiers.
private func canvasControl<T: NSView>(_ sheet: CanvasSizeSheetController,
                                      _ name: String, _ what: String) throws -> T {
    let identifier = NSUserInterfaceItemIdentifier(name)
    guard let found = descendants(sheet.view).compactMap({ $0 as? T })
        .first(where: { $0.identifier == identifier }) else {
        throw DoctoredCoreError(message: "The Canvas Size sheet has no \(what) (\(name))")
    }
    return found
}

/// The anchor grid's nine cells, found through the grid view that owns them
/// (`AnchorGridView` is private to `Sheets.swift`, so the probe matches its
/// runtime name rather than its type). Their tags are `row * 3 + col`.
private func clickAnchor(_ sheet: CanvasSizeSheetController, _ tag: Int) throws {
    guard let grid = descendants(sheet.view)
        .first(where: { String(describing: type(of: $0)).contains("AnchorGridView") }) else {
        throw DoctoredCoreError(message: "The Canvas Size sheet has no anchor grid")
    }
    let cells = descendants(grid).compactMap { $0 as? NSButton }
    try require(cells.count == 9, "The anchor grid has \(cells.count) cells")
    guard let cell = cells.first(where: { $0.tag == tag }) else {
        throw DoctoredCoreError(message: "The Canvas Size sheet has no anchor cell \(tag)")
    }
    cell.performClick(nil)
    pump()
}

/// The origin `CanvasSizeSheetController.applyClicked` computes for `tag`:
/// the anchor's column/row choose how much of the delta lands left/above.
private func anchorOrigin(_ tag: Int, _ from: (Int, Int), _ to: (Int, Int)) -> (Int, Int) {
    let origin = RasterDocument.canvasSizeOrigin(oldWidth: from.0, oldHeight: from.1,
        newWidth: to.0, newHeight: to.1, anchor: (col: tag % 3, row: tag / 3))
    return (origin.x, origin.y)
}

/// Drives the REAL sheet: types the two numbers, picks the anchor, clicks
/// Apply, and waits for the sheet to go away.
private func commitCanvasSheet(_ editor: EditorViewController, _ document: ImageDocument,
                               width: Int, height: Int, anchor: Int) throws {
    let open = canvasSheets(editor).count
    editor.showCanvasSize(nil)
    let sheet = try presentedCanvasSheet(editor, after: open)
    let widthField: NSTextField = try canvasControl(sheet, "canvas-size-width", "width field")
    let heightField: NSTextField = try canvasControl(sheet, "canvas-size-height", "height field")
    let apply: NSButton = try canvasControl(sheet, "canvas-size-apply", "Apply button")
    widthField.stringValue = String(width)
    heightField.stringValue = String(height)
    try require(widthField.integerValue == width && heightField.integerValue == height,
        "The Canvas Size fields read back \(widthField.integerValue) x "
        + "\(heightField.integerValue), expected \(width) x \(height)")
    try clickAnchor(sheet, anchor)
    try require(apply.isEnabled, "The Canvas Size Apply button is disabled")
    apply.performClick(nil)
    try waitUntil({ document.doc.map { ($0.width, $0.height) } ?? (0, 0) == (width, height) },
        "Apply left the canvas at \(document.doc?.width ?? -1) x \(document.doc?.height ?? -1)")
    sheet.dismiss(nil)
    pump()
}

/// One MCP call through the REAL server, refusing to continue on an error.
@discardableResult
private func canvasMCP(_ tool: String, _ arguments: [String: Any],
                       _ document: ImageDocument) throws -> [String: Any] {
    let response = try call(tool, arguments, document: document)
    try require(!(try responseIsError(response)), "\(tool) refused: \(response)")
    guard let envelope = try JSONSerialization.jsonObject(with: Data(response.utf8))
            as? [String: Any],
          let content = envelope["content"] as? [[String: Any]],
          let text = content.first?["text"] as? String else {
        throw DoctoredCoreError(message: "Invalid MCP response: \(response)")
    }
    return (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
}

/// Every record's `(name, offsetX, offsetY)`, in the panel's own order.
private func placements(_ doc: RasterDocument) -> [String] {
    (0..<doc.layerCount).compactMap { index in
        guard let info = doc.layerInfo(index) else { return nil }
        return "\(info.name)@\(info.offsetX),\(info.offsetY)"
    }
}

private func guideList(_ doc: RasterDocument) -> [String] {
    (0..<doc.guideCount).compactMap { index in
        guard let guide = doc.guideInfo(index) else { return nil }
        return "\(guide.orientation.rawValue):\(guide.position)"
    }
}

/// Channel 0's samples in the document's own depth, as integers.
private func channelSamples(_ doc: RasterDocument) throws -> [Int] {
    guard doc.channelCount > 0 else {
        throw DoctoredCoreError(message: "The owned document has no saved channel")
    }
    if doc.precision != nil {
        return try PrecisionScalarOperations.read(doc, target: "channel", index: 0)
            .samples.map { Int($0) }
    }
    guard let plane = doc.channelPlane(0) else {
        throw DoctoredCoreError(message: "The owned byte channel plane is unavailable")
    }
    return plane.map { Int($0) }
}

private func openCanvasScene(_ depth: String) throws -> (ImageDocument, EditorViewController) {
    switch depth {
    case "byte":
        let (document, editor, _) = try openByteDocument(width: 40, height: 28)
        return (document, editor)
    case "float":
        let (document, editor, _) = try openFloatDocument()
        return (document, editor)
    default:
        let (document, editor, _) = try openDocument(width: 40, height: 28)
        return (document, editor)
    }
}

// ------------------------------------------- the plain-document half --

private func plainCanvasSize(_ depth: String) throws -> [String: Any] {
    let (document, editor) = try openCanvasScene(depth)
    defer { document.close(); pump() }
    guard let start = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let (from, to) = ((start.width, start.height), (start.width + 9, start.height + 7))

    // Chrome and one saved channel, through the REAL tools, so the reference
    // has something to move besides the layer offsets.
    try canvasMCP("add_guide", ["orientation": "vertical", "position": 3.5], document)
    try canvasMCP("add_guide", ["orientation": "horizontal", "position": 2.0], document)
    try canvasMCP("select_rect", ["x": 1, "y": 1, "width": 6, "height": 5], document)
    try canvasMCP("save_selection", ["name": "Cover"], document)
    try canvasMCP("deselect", [:], document)

    let item = canvasMenuItem(#selector(EditorViewController.showCanvasSize(_:)))
    try require(editor.validateUserInterfaceItem(item),
        "\(depth): Image ▸ Canvas Size… must validate")

    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let anchor = 8
    let origin = anchorOrigin(anchor, from, to)
    guard let reference = try before.canvasResized(
        w: to.0, h: to.1, originX: origin.0, originY: origin.1) else {
        throw DoctoredCoreError(message: "\(depth): the reference canvasResized refused")
    }
    document.undoManager?.removeAllActions()
    let steps = document.history.entries.count
    let restoreTarget = (placements(before), guideList(before), try channelSamples(before))

    try commitCanvasSheet(editor, document, width: to.0, height: to.1, anchor: anchor)
    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after the sheet")
    }
    try require((after.width, after.height) == to,
        "\(depth): the sheet committed \(after.width) x \(after.height), expected \(to)")
    try require(placements(after) == placements(reference),
        "\(depth): offsets \(placements(after)) != \(placements(reference))")
    try require(guideList(after) == guideList(reference),
        "\(depth): guides \(guideList(after)) != \(guideList(reference))")
    let committedChannel = try channelSamples(after)
    try require(committedChannel == (try channelSamples(reference)),
        "\(depth): the saved channel is not the reference's padded plane")
    try require(document.history.entries.count == steps + 1,
        "\(depth): the sheet made \(document.history.entries.count - steps) history steps")

    document.undoManager?.undo()
    pump()
    guard let undone = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after Undo")
    }
    try require((undone.width, undone.height) == from, "\(depth): Undo left the canvas at "
        + "\(undone.width) x \(undone.height)")
    try require(placements(undone) == restoreTarget.0, "\(depth): Undo left the offsets moved")
    try require(guideList(undone) == restoreTarget.1, "\(depth): Undo left the guides moved")
    try require((try channelSamples(undone)) == restoreTarget.2,
        "\(depth): Undo left the channel padded")

    return ["depth": depth, "canvas": [to.0, to.1], "anchor": anchor,
            "origin": [origin.0, origin.1], "history_steps": 1,
            "channel_samples": committedChannel.count,
            "equals_canvasResized": true]
}

// ------------------------------------ the UNCHANGED-SIZE Apply sub-check --

/// Phase 12F-1b: applying the sheet at the size the canvas ALREADY IS.
/// `Sheets.swift`'s `applyClicked` dismisses and then returns early
/// (`guard w != originalWidth || h != originalHeight`), so there is no code
/// change behind this — it is the MEASUREMENT that the sheet already answers
/// the house no-op at every depth, beside the MCP tool's new successful
/// `changed: false` reply. No step, no alert, the IDENTICAL handle, and the
/// document is still clean.
private func unchangedCanvasSize(_ depth: String) throws -> [String: Any] {
    let (document, editor) = try openCanvasScene(depth)
    defer { document.close(); pump() }
    guard let start = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let size = (start.width, start.height)
    document.undoManager?.removeAllActions()
    let steps = document.history.entries.count

    let open = canvasSheets(editor).count
    editor.showCanvasSize(nil)
    let sheet = try presentedCanvasSheet(editor, after: open)
    let widthField: NSTextField = try canvasControl(sheet, "canvas-size-width", "width field")
    let heightField: NSTextField = try canvasControl(sheet, "canvas-size-height", "height field")
    let apply: NSButton = try canvasControl(sheet, "canvas-size-apply", "Apply button")
    widthField.stringValue = String(size.0)
    heightField.stringValue = String(size.1)
    try clickAnchor(sheet, 4)   // centre: the origin is (0, 0) whatever it is
    try require(apply.isEnabled, "\(depth): the Canvas Size Apply button is disabled")
    apply.performClick(nil)
    pump()
    sheet.dismiss(nil)
    pump()

    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after Apply")
    }
    try require((after.width, after.height) == size,
        "\(depth): an unchanged-size Apply moved the canvas to \(after.width) x \(after.height)")
    try require(after === start,
        "\(depth): an unchanged-size Apply replaced the document handle")
    try require(document.history.entries.count == steps,
        "\(depth): an unchanged-size Apply made "
        + "\(document.history.entries.count - steps) history steps")
    try require(document.undoManager?.canUndo != true,
        "\(depth): an unchanged-size Apply left an Undo step")
    try require(!document.isDocumentEdited,
        "\(depth): an unchanged-size Apply dirtied the document")
    return ["depth": depth, "canvas": [size.0, size.1], "history_steps": 0,
            "can_undo": false, "is_dirty": false, "same_handle": true]
}

// ------------------------------------------------------------ the case --

func canvasSizeSheetUI() throws -> [String: Any] {
    var plain: [[String: Any]] = []
    var unchanged: [[String: Any]] = []
    for depth in ["byte", "typed", "float"] {
        plain.append(try plainCanvasSize(depth))
        unchanged.append(try unchangedCanvasSize(depth))
    }
    return ["case": "canvas-size-sheet-ui", "plain": plain,
            "unchanged_size_apply": unchanged,
            "scope": "The REAL Canvas Size sheet driven through its three identifiers and its "
                + "anchor grid; the committed document is compared with RasterDocument."
                + "canvasResized for the same numbers"]
}
