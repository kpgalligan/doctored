import AppKit

// Phase 12F-1b: the UI-SOURCE half of the EXACT CANVAS GEOMETRY at 32-bit
// float — Image ▸ Rotate 90° CW / 90° CCW / 180°, Image ▸ Flip Horizontal /
// Vertical and Image ▸ Crop to Selection.
//
// An MCP drive can call `rotate`, `flip` and `crop`; it proves the core ops
// and the tool gate. What only the REAL UI can prove lives here: that the six
// menu items VALIDATE in a 32-bit float document (they were disabled by the
// closed float whitelist in `EditorViewController+Precision.swift` until this
// run), that the two FILTERED items beside them — Image ▸ Image Size… and
// Edit ▸ Free Transform — validate too (Free Transform only with no
// selection, when it is a layer transform), that each item's committed
// document is EXACTLY what the numeric `RasterDocument` wrapper answers for
// the same op, and that the publication is ONE history step one Undo
// restores.

/// A probe-built menu item, the shape AppKit hands `validateUserInterfaceItem`.
private func geometryMenuItem(_ selector: Selector) -> NSMenuItem {
    let item = NSMenuItem(title: "Item", action: selector, keyEquivalent: "")
    item.target = nil
    return item
}

/// One MCP call through the REAL server, refusing to continue on an error.
@discardableResult
private func geometryMCP(_ tool: String, _ arguments: [String: Any],
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

/// The float document's own DISPLAY bytes — its tone-mapped projection
/// through `RasterDocument.display(of:context:)`, which is the one place a
/// typed projection becomes display bytes. Two documents that are the same
/// document answer the same bytes; it is used here only to compare a
/// committed result with the numeric wrapper's, never as a master.
private func floatDisplayBytes(_ doc: RasterDocument) throws -> [UInt8] {
    guard let precision = doc.precision else {
        throw DoctoredCoreError(message: "Owned float document has no typed owner")
    }
    let context = try PrecisionWorkContext(source: precision)
    let image = try precision.render(workingLimit: context.limit,
                                     otherRetained: context.otherRetained)
    let display = try doc.display(of: image, context: context)
    guard let bytes = display.withPlateBytes({ pointer -> [UInt8] in
        Array(UnsafeBufferPointer(start: pointer, count: display.width * display.height * 4))
    }) else {
        throw DoctoredCoreError(message: "Owned float display bytes unavailable")
    }
    return bytes
}

/// Every record's `(name, offsetX, offsetY)`, in the panel's own order.
private func geometryPlacements(_ doc: RasterDocument) -> [String] {
    (0..<doc.layerCount).compactMap { index in
        guard let info = doc.layerInfo(index) else { return nil }
        return "\(info.name)@\(info.offsetX),\(info.offsetY)"
    }
}

private func geometryGuides(_ doc: RasterDocument) -> [String] {
    (0..<doc.guideCount).compactMap { index in
        guard let guide = doc.guideInfo(index) else { return nil }
        return "\(guide.orientation.rawValue):\(guide.position)"
    }
}

/// The whole comparable state of a float document: canvas, placements,
/// guides, ruler origin and the display bytes' digest.
private func geometryState(_ doc: RasterDocument) throws -> [String] {
    [
        "canvas:\(doc.width)x\(doc.height)",
        "layers:" + geometryPlacements(doc).joined(separator: "|"),
        "guides:" + geometryGuides(doc).joined(separator: "|"),
        "ruler:\(doc.rulerOrigin.x),\(doc.rulerOrigin.y)",
        "display:" + hashBytes(try floatDisplayBytes(doc)),
    ]
}

/// One menu item driven for real, its result compared with the numeric
/// wrapper's answer for the same op, then undone.
private func floatGeometryItem(
    _ editor: EditorViewController, _ document: ImageDocument, _ name: String,
    _ run: (EditorViewController) -> Void,
    _ reference: (RasterDocument) throws -> RasterDocument?
) throws -> [String: Any] {
    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    try require(editor.validateUserInterfaceItem(geometryMenuItem(
        NSSelectorFromString(name))), "\(name) must validate in a float document")
    guard let expected = try reference(before) else {
        throw DoctoredCoreError(message: "\(name): the numeric wrapper refused")
    }
    let expectedState = try geometryState(expected)
    let beforeState = try geometryState(before)
    document.undoManager?.removeAllActions()
    let steps = document.history.entries.count

    run(editor)
    pump()
    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after \(name)")
    }
    let afterState = try geometryState(after)
    try require(afterState == expectedState,
        "\(name): the committed document is \(afterState), the wrapper's is \(expectedState)")
    try require(document.history.entries.count == steps + 1,
        "\(name): the menu item made \(document.history.entries.count - steps) history steps")
    try require(after.isFloat, "\(name): the result is no longer a float document")

    document.undoManager?.undo()
    pump()
    guard let undone = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after Undo of \(name)")
    }
    try require((try geometryState(undone)) == beforeState,
        "\(name): Undo did not restore the document")
    return ["item": name, "canvas": [after.width, after.height],
            "equals_numeric_wrapper": true, "history_steps": 1, "undo_restored": true]
}

// ------------------------------------------------------------- the case --

func floatExactGeometryMenuUI() throws -> [String: Any] {
    let (document, editor, _) = try openFloatDocument()
    defer { document.close(); pump() }

    // Chrome, through the REAL tools, so every op has a guide and a ruler
    // origin to move as well as layer offsets.
    try geometryMCP("add_guide", ["orientation": "vertical", "position": 7.0], document)
    try geometryMCP("add_guide", ["orientation": "horizontal", "position": 4.0], document)
    try geometryMCP("set_ruler_origin", ["x": 5.0, "y": 3.0], document)

    // The FILTERED items beside them validate too — they resample
    // premultiplied extended-range f32 in the core.
    for name in ["resizeImage:", "freeTransform:"] {
        try require(editor.validateUserInterfaceItem(
            geometryMenuItem(NSSelectorFromString(name))),
            "\(name) must validate in a float document with no selection")
    }

    var items: [[String: Any]] = []
    items.append(try floatGeometryItem(editor, document, "rotateCW:",
        { $0.rotateCW(nil) }, { try $0.rotated90() }))
    items.append(try floatGeometryItem(editor, document, "rotateCCW:",
        { $0.rotateCCW(nil) }, { try $0.rotated270() }))
    items.append(try floatGeometryItem(editor, document, "rotate180:",
        { $0.rotate180(nil) }, { try $0.rotated180() }))
    items.append(try floatGeometryItem(editor, document, "flipH:",
        { $0.flipH(nil) }, { try $0.flippedH() }))
    items.append(try floatGeometryItem(editor, document, "flipV:",
        { $0.flipV(nil) }, { try $0.flippedV() }))

    // Crop to Selection needs a selection that would actually shrink the
    // canvas; without one the item is disabled at EVERY depth.
    try require(!editor.validateUserInterfaceItem(
        geometryMenuItem(#selector(EditorViewController.cropToSelection(_:)))),
        "Crop to Selection must be disabled with no selection")
    guard let canvas = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let window = (x: 4, y: 3, w: Int(canvas.width) - 10, h: Int(canvas.height) - 7)
    try geometryMCP("select_rect", ["x": window.x, "y": window.y,
                                    "width": window.w, "height": window.h], document)
    // With a selection these transform the selected PIXELS, which float does
    // not admit, so they disable.
    for name in ["freeTransform:", "freeTransformCopy:", "transformAgain:",
                 "transformAgainWithCopy:"] {
        try require(!editor.validateUserInterfaceItem(
            geometryMenuItem(NSSelectorFromString(name))),
            "\(name) must be disabled in a float document with a selection")
    }
    items.append(try floatGeometryItem(editor, document, "cropToSelection:",
        { $0.cropToSelection(nil) },
        { try $0.cropped(x: window.x, y: window.y, w: window.w, h: window.h) }))

    return ["case": "float-exact-geometry-menu-ui", "items": items,
            "crop_window": [window.x, window.y, window.w, window.h],
            "filtered_items_enabled": ["resizeImage:", "freeTransform:"],
            "scope": "The REAL Image menu items driven in a 32-bit float document; each "
                + "committed document is compared with the numeric RasterDocument wrapper's "
                + "answer for the same op, and one Undo restores it"]
}
