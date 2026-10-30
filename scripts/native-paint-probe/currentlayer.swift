import AppKit

/// All original representations are materialized before this case writes the
/// general pasteboard. Another owner's later change is never overwritten.
private final class CurrentLayerClipboard {
    private let pasteboard: NSPasteboard
    private let original: [[(NSPasteboard.PasteboardType, Data)]]
    private(set) var ownedChangeCount: Int

    init(pasteboard: NSPasteboard = .general) throws {
        self.pasteboard = pasteboard
        var saved: [[(NSPasteboard.PasteboardType, Data)]] = []
        for item in pasteboard.pasteboardItems ?? [] {
            var representations: [(NSPasteboard.PasteboardType, Data)] = []
            for type in item.types {
                guard let bytes = item.data(forType: type) else {
                    throw DoctoredCoreError(message: "The existing clipboard has an unavailable representation")
                }
                representations.append((type, bytes))
            }
            saved.append(representations)
        }
        original = saved
        ownedChangeCount = pasteboard.changeCount
    }

    func requireOwnership() throws {
        try require(pasteboard.changeCount == ownedChangeCount,
            "Another process changed the clipboard during the current-layer probe")
    }

    func noteWrite() { ownedChangeCount = pasteboard.changeCount }

    func noteCopyWrite(previous: Int) throws {
        try require(pasteboard.changeCount > previous,
            "Command-C did not write a new image clipboard generation")
        noteWrite()
    }

    func replaceContents(_ write: () -> Bool) throws {
        try requireOwnership()
        pasteboard.clearContents()
        noteWrite()
        try require(write(), "Owned clipboard write failed")
        noteWrite()
    }

    func setText(_ text: String) throws {
        try replaceContents { pasteboard.setString(text, forType: .string) }
    }

    func setImage(_ item: NSPasteboardItem) throws {
        try replaceContents { pasteboard.writeObjects([item]) }
    }

    func imageCopy() throws -> NSPasteboardItem {
        try requireOwnership()
        guard let source = pasteboard.pasteboardItems?.first else {
            throw DoctoredCoreError(message: "Copy produced no clipboard item")
        }
        // 12G-2: only the PUBLIC types are kept — an 8-bit Copy now writes
        // the private typed payload too, and the depth checks below need a
        // public-image-only pasteboard.
        let copy = NSPasteboardItem()
        for type in source.types where type == .tiff || type == .png {
            guard let bytes = source.data(forType: type) else {
                throw DoctoredCoreError(message: "Copy's image representation is unavailable")
            }
            copy.setData(bytes, forType: type)
        }
        try require(copy.types.contains(.tiff) || copy.types.contains(.png),
            "Copy did not publish a TIFF or PNG image")
        return copy
    }

    @discardableResult
    func restoreIfOwned() -> Bool {
        guard pasteboard.changeCount == ownedChangeCount else { return false }
        let items: [NSPasteboardItem] = original.map { representations in
            let item = NSPasteboardItem()
            for (type, bytes) in representations { item.setData(bytes, forType: type) }
            return item
        }
        pasteboard.clearContents()
        return items.isEmpty || pasteboard.writeObjects(items)
    }
}

private func currentLayerClipboardFailureCleanup() throws -> Bool {
    let board = NSPasteboard.withUniqueName()
    defer { board.releaseGlobally() }
    board.clearContents()
    try require(board.setString("private original", forType: .string),
        "Private clipboard setup failed")
    let clipboard = try CurrentLayerClipboard(pasteboard: board)
    do {
        try clipboard.replaceContents { false }
        throw DoctoredCoreError(message: "Injected clipboard write unexpectedly succeeded")
    } catch let error as DoctoredCoreError {
        try require(error.message == "Owned clipboard write failed",
            "Injected clipboard write failed for another reason")
    }
    try require(clipboard.restoreIfOwned()
                && board.string(forType: .string) == "private original",
        "Failed write did not restore the private clipboard")
    let later = try CurrentLayerClipboard(pasteboard: board)
    do {
        try later.replaceContents { false }
        throw DoctoredCoreError(message: "Second injected clipboard write unexpectedly succeeded")
    } catch let error as DoctoredCoreError {
        try require(error.message == "Owned clipboard write failed",
            "Second injected clipboard write failed for another reason")
    }
    board.clearContents()
    try require(board.setString("another owner", forType: .string),
        "Private clipboard intervening write failed")
    try require(!later.restoreIfOwned()
                && board.string(forType: .string) == "another owner",
        "Cleanup overwrote an intervening private clipboard write")
    return true
}

private func currentLayerOpenFlat(_ output: URL, _ label: String)
    throws -> (ImageDocument, EditorViewController, NSWindow)
{
    let width = 12, height = 9
    var pixels: [UInt8] = []
    for index in 0..<(width * height) {
        pixels.append(UInt8(17 + index % 200))
        pixels.append(UInt8(31 + index % 190))
        pixels.append(UInt8(47 + index % 180))
        pixels.append(255)
    }
    guard let image = RasterImage.from(rgba: pixels, width: width, height: height),
          let cgImage = image.makeCGImage(in: ColorProfile.sRGB),
          let png = NSBitmapImageRep(cgImage: cgImage).representation(using: .png, properties: [:])
    else { throw DoctoredCoreError(message: "Owned flat fixture unavailable") }
    let url = output.appendingPathComponent("current-layer-\(label).png")
    try png.write(to: url)
    let document = ImageDocument()
    try document.read(from: url, ofType: "public.png")
    // Direct read bypasses NSDocumentController's normal URL/type bookkeeping.
    document.fileType = "public.png"
    document.fileURL = url
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    document.showWindows()
    guard let window = document.windowControllers.first?.window,
          let editor = window.contentViewController as? EditorViewController else {
        document.close()
        throw DoctoredCoreError(message: "Actual flat-image editor window unavailable")
    }
    return (document, editor, window)
}

private func currentLayerKeyWindow(_ window: NSWindow) throws {
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    do {
        try waitUntil({ NSApp.keyWindow === window && window.isKeyWindow },
            "The owned editor never became the real key window")
    } catch {
        throw DoctoredCoreError(message: "The owned editor never became the real key window "
            + "(active=\(NSApp.isActive), visible=\(window.isVisible), "
            + "windowKey=\(window.isKeyWindow), keyWindow=\(NSApp.keyWindow?.title ?? "nil"))")
    }
}

private func currentLayerMenuItem(_ menu: NSMenu, _ action: Selector) throws -> NSMenuItem {
    func search(_ menu: NSMenu) -> NSMenuItem? {
        for item in menu.items {
            if item.action == action { return item }
            if let submenu = item.submenu, let found = search(submenu) { return found }
        }
        return nil
    }
    guard let item = search(menu) else {
        throw DoctoredCoreError(message: "The real menu has no \(NSStringFromSelector(action))")
    }
    try require(item.target == nil, "The menu's \(NSStringFromSelector(action)) has a fixed target")
    return item
}

private func currentLayerPress(_ letter: String, code: UInt16, window: NSWindow) throws {
    try require(NSApp.keyWindow === window && window.isKeyWindow,
        "A clipboard key equivalent has no owned key editor window")
    guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero,
        modifierFlags: .command, timestamp: ProcessInfo.processInfo.systemUptime,
        windowNumber: window.windowNumber, context: nil, characters: letter,
        charactersIgnoringModifiers: letter, isARepeat: false, keyCode: code) else {
        throw DoctoredCoreError(message: "Owned Command-\(letter) key event unavailable")
    }
    NSApp.sendEvent(event)
    pump()
}

private func currentLayerPixels(_ document: ImageDocument, _ index: Int) throws -> [UInt8] {
    guard let pixels = document.doc?.layerPixels(index) else {
        throw DoctoredCoreError(message: "Layer \(index) has no stored byte pixels")
    }
    return pixels
}

private func currentLayerReady(_ document: ImageDocument, _ editor: EditorViewController,
                               _ window: NSWindow, _ depth: String) throws -> [String: Any] {
    guard let model = document.doc else { throw DoctoredCoreError(message: "No \(depth) model") }
    try require(model.layerCount > 0 && document.activeLayerIndex >= 0
                && document.activeLayerIndex < model.layerCount,
        "The \(depth) window has no valid selected layer")
    try require(document.planeEditTarget == .layer, "The \(depth) window targets a plane")
    try require(window.initialFirstResponder === editor.canvas,
        "The \(depth) window does not designate its own canvas initially")
    try require(window.firstResponder === editor.canvas,
        "The \(depth) window did not actually focus its canvas")
    try require(!document.isDocumentEdited && document.history.entries.count == 1
                && document.history.cursor == 0,
        "Opening the \(depth) window changed dirty state or history")
    return ["depth": depth, "active_index": document.activeLayerIndex,
            "layer_count": model.layerCount, "initial_canvas": true,
            "actual_canvas": true, "history_count": document.history.entries.count]
}

private func currentLayerInvert(_ document: ImageDocument, _ editor: EditorViewController,
                                _ item: NSMenuItem, index: Int) throws -> [String: Any] {
    guard let before = document.doc else { throw DoctoredCoreError(message: "No invert model") }
    let layers = try (0..<before.layerCount).map { try currentLayerPixels(document, $0) }
    try require(document.activeLayerIndex == index, "Invert has the wrong current layer")
    try require((NSApp.target(forAction: #selector(EditorViewController.applyInvert(_:)),
        to: nil, from: item) as? EditorViewController) === editor,
        "Invert does not resolve to the current editor")
    try withUndoGroup(document.undoManager) {
        try require(NSApp.sendAction(#selector(EditorViewController.applyInvert(_:)),
            to: nil, from: item), "AppKit did not route Invert")
    }
    pump()
    try require(document.doc?.layerCount == layers.count, "Invert changed the layer count")
    for layer in layers.indices {
        let after = try currentLayerPixels(document, layer)
        try require((after != layers[layer]) == (layer == index),
            "Invert changed the wrong stored raster at index \(layer)")
    }
    return ["index": index, "only_current_changed": true]
}

/// Actual AppKit routing of current-layer commands after a flat image opens.
/// The unfixed baseline was measured before EditorWindowController changed:
/// initial FileDropView, actual NSTextView, active 0/1, .layer, clean, history 1.
func currentLayerCommandsUI(output: URL) throws -> [String: Any] {
    let clipboardFailureCleanup = try currentLayerClipboardFailureCleanup()
    trace("PRIVATE CLIPBOARD FAILURE CLEANUP: \(clipboardFailureCleanup)")
    let previousPolicy = NSApp.activationPolicy()
    let previousMenu = NSApp.mainMenu
    let clipboard = try CurrentLayerClipboard()
    _ = NSApp.setActivationPolicy(.regular)
    let delegate = AppDelegate()
    let menu = delegate.makeMainMenu()
    NSApp.mainMenu = menu
    defer {
        let restored = clipboard.restoreIfOwned()
        trace("CURRENT LAYER CLIPBOARD RESTORED: \(restored)")
        NSApp.mainMenu = previousMenu
        _ = NSApp.setActivationPolicy(previousPolicy)
        withExtendedLifetime(delegate) {}
    }
    let copyItem = try currentLayerMenuItem(menu, #selector(EditorViewController.copy(_:)))
    let pasteItem = try currentLayerMenuItem(menu, #selector(EditorViewController.paste(_:)))
    let pasteInPlaceItem = try currentLayerMenuItem(
        menu, #selector(EditorViewController.pasteInPlace(_:)))
    let invertItem = try currentLayerMenuItem(menu, #selector(EditorViewController.applyInvert(_:)))

    let (first, firstEditor, firstWindow) = try currentLayerOpenFlat(output, "first")
    defer { first.close(); pump() }
    try currentLayerKeyWindow(firstWindow)
    let firstReady = try currentLayerReady(first, firstEditor, firstWindow, "8-bit")
    try require(firstEditor.canvas.selection == nil, "Flat open unexpectedly has a selection")
    try require((NSApp.target(forAction: #selector(EditorViewController.copy(_:)),
        to: nil, from: copyItem) as? EditorViewController) === firstEditor,
        "Copy does not resolve to the first editor")
    try require((NSApp.target(forAction: #selector(EditorViewController.paste(_:)),
        to: nil, from: pasteItem) as? EditorViewController) === firstEditor,
        "Paste does not resolve to the first editor")
    let sourcePixels = try currentLayerPixels(first, 0)
    let firstHistory = first.history.entries.count
    try clipboard.requireOwnership()
    let beforeCopyCount = clipboard.ownedChangeCount
    try currentLayerPress("c", code: 8, window: firstWindow)
    try clipboard.noteCopyWrite(previous: beforeCopyCount)
    let copiedItem = try clipboard.imageCopy()
    try require(first.history.entries.count == firstHistory && !first.isDocumentEdited,
        "Copy changed the open document")
    try currentLayerPress("v", code: 9, window: firstWindow)
    try require(first.doc?.layerCount == 2 && first.activeLayerIndex == 1,
        "Command-V did not insert and select one new layer")
    try require(first.doc?.layerInfo(1)?.name == "Pasted Layer", "Paste used another layer name")
    try require(try currentLayerPixels(first, 0) == sourcePixels,
        "Paste changed the original layer's stored pixels")
    try require(try currentLayerPixels(first, 1) == sourcePixels,
        "Paste did not copy the exact opaque byte pixels")
    try require(first.history.entries.count == firstHistory + 1 && first.history.cursor == 1
                && first.isDocumentEdited, "Paste did not make one dirty/history transition")
    first.undoManager?.undo()
    pump()
    try require(first.doc?.layerCount == 1 && first.activeLayerIndex == 0
                && first.history.cursor == 0 && !first.isDocumentEdited
                && (try currentLayerPixels(first, 0)) == sourcePixels,
        "One Undo did not restore the clean original and current layer")
    first.undoManager?.redo()
    pump()
    try require(first.doc?.layerCount == 2 && first.activeLayerIndex == 1
                && first.history.cursor == 1 && first.isDocumentEdited
                && (try currentLayerPixels(first, 1)) == sourcePixels,
        "One Redo did not restore the pasted layer and selection")

    let (second, secondEditor, secondWindow) = try currentLayerOpenFlat(output, "second")
    defer { second.close(); pump() }
    try currentLayerKeyWindow(secondWindow)
    let secondReady = try currentLayerReady(second, secondEditor, secondWindow, "8-bit destination")
    let secondOriginal = try currentLayerPixels(second, 0)
    try currentLayerPress("v", code: 9, window: secondWindow)
    try require(second.doc?.layerCount == 2 && second.activeLayerIndex == 1
                && (try currentLayerPixels(second, 0)) == secondOriginal
                && (try currentLayerPixels(second, 1)) == sourcePixels,
        "Immediate Paste into another opened image missed the new-layer route")

    var extraPixels: [UInt8] = []
    for _ in 0..<(12 * 9) { extraPixels += [39, 73, 151, 255] }
    guard let extra = RasterImage.from(rgba: extraPixels, width: 12, height: 9) else {
        throw DoctoredCoreError(message: "Owned extra raster unavailable")
    }
    for number in 1...2 {
        let anchor = second.doc?.layerCount ?? 0
        second.applyEdit("Probe Layer", record: .unrecorded("Probe fixture")) {
            $0.addingImageLayer(above: anchor - 1, extra, name: "Neighbor \(number)")
        }
    }
    try require(second.doc?.layerCount == 4, "Multi-layer fixture did not form")
    secondEditor.setActiveLayer(0)
    pump()
    let neighbors = try (0..<4).map { try currentLayerPixels(second, $0) }
    let names = (0..<4).map { second.doc?.layerInfo($0)?.name ?? "" }
    try currentLayerPress("v", code: 9, window: secondWindow)
    try require(second.doc?.layerCount == 5 && second.activeLayerIndex == 1
                && second.doc?.layerInfo(1)?.name == "Pasted Layer",
        "Paste did not land above the non-top current raster")
    try require(try currentLayerPixels(second, 1) == sourcePixels,
        "Non-top Paste stored different image pixels")
    for index in 0..<4 {
        let newIndex = index < 1 ? index : index + 1
        try require((try currentLayerPixels(second, newIndex)) == neighbors[index]
                    && second.doc?.layerInfo(newIndex)?.name == names[index],
            "Non-top Paste changed a neighboring raster")
    }
    secondEditor.setActiveLayer(2)
    pump()
    let changedInvert = try currentLayerInvert(second, secondEditor, invertItem, index: 2)

    let (third, thirdEditor, thirdWindow) = try currentLayerOpenFlat(output, "invert")
    defer { third.close(); pump() }
    try currentLayerKeyWindow(thirdWindow)
    let thirdReady = try currentLayerReady(third, thirdEditor, thirdWindow, "8-bit invert")
    let openedInvert = try currentLayerInvert(third, thirdEditor, invertItem, index: 0)

    try currentLayerKeyWindow(secondWindow)
    guard let search = descendants(secondEditor.layersPanel.view)
        .compactMap({ $0 as? NSSearchField })
        .first(where: { $0.placeholderString == "Filter by name" }) else {
        throw DoctoredCoreError(message: "The editor's real Layers search field is missing")
    }
    search.stringValue = ""
    try require(secondWindow.makeFirstResponder(search), "Could not focus the Layers search field")
    try require(secondWindow.firstResponder is NSText,
        "The Layers search field has no text field editor")
    try require(NSApp.target(forAction: #selector(EditorViewController.paste(_:)),
        to: nil, from: pasteItem) is NSText,
        "Text Paste did not resolve to the field editor")
    let beforeTextLayers = second.doc?.layerCount
    let beforeTextPixels = try (0..<(beforeTextLayers ?? 0)).map { try currentLayerPixels(second, $0) }
    let beforeTextHistory = second.history.entries.count
    let beforeTextCursor = second.history.cursor
    let beforeTextDirty = second.isDocumentEdited
    try clipboard.setText("probe text stays in field")
    try currentLayerPress("v", code: 9, window: secondWindow)
    try require(search.stringValue == "probe text stays in field", "Command-V did not paste into text")
    try require(second.doc?.layerCount == beforeTextLayers
                && second.history.entries.count == beforeTextHistory
                && second.history.cursor == beforeTextCursor
                && second.isDocumentEdited == beforeTextDirty,
        "Text Paste changed image structure, history or dirty state")
    for index in beforeTextPixels.indices {
        try require((try currentLayerPixels(second, index)) == beforeTextPixels[index],
            "Text Paste changed stored layer \(index)")
    }
    try currentLayerKeyWindow(secondWindow)
    try require(secondWindow.firstResponder is NSText,
        "Bringing the existing window forward stole text focus")
    let sheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 200, height: 100),
        styleMask: [.titled], backing: .buffered, defer: false)
    sheet.isReleasedWhenClosed = false
    secondWindow.beginSheet(sheet, completionHandler: nil)
    pump()
    secondWindow.endSheet(sheet)
    sheet.close()
    pump()
    try require(secondWindow.firstResponder is NSText,
        "Dismissing a sheet stole the first view's text focus")
    let selectionBeforeView = second.layerSelection
    guard let newEditor = second.openSecondView(), let newWindow = newEditor.view.window else {
        throw DoctoredCoreError(message: "A second view did not open")
    }
    try currentLayerKeyWindow(newWindow)
    try require(newWindow.initialFirstResponder === newEditor.canvas
                && newWindow.firstResponder === newEditor.canvas,
        "The second view did not start with its own canvas focused")
    try require(second.layerSelection == selectionBeforeView
                && secondWindow.firstResponder is NSText,
        "The second view reset shared selection or the first view's text focus")

    try clipboard.setImage(copiedItem)
    let (typed, typedEditor, _) = try openDocument()
    defer { typed.close(); pump() }
    guard let typedWindow = typedEditor.view.window else {
        throw DoctoredCoreError(message: "16-bit window missing")
    }
    try currentLayerKeyWindow(typedWindow)
    let typedReady = try currentLayerReady(typed, typedEditor, typedWindow, "16-bit")
    // 12G-2: with a public image only, a 16-bit Paste is live and
    // so is Paste in Place (an image with no source position lands like
    // Paste); the plan is `.external`, and the promoted layer's codes are
    // exactly ×257 of the CoreGraphics decode into the document's drawing
    // space.
    try require(typedEditor.validateUserInterfaceItem(copyItem)
                && typedEditor.validateUserInterfaceItem(pasteItem)
                && typedEditor.validateUserInterfaceItem(pasteInPlaceItem),
        "16-bit clipboard menu state changed")
    let typedPlan = try PastePlan.resolve(for: typed, pasteboard: Clipboard.pasteboard,
                                          payloadFile: nil)
    guard case .external = typedPlan else {
        throw DoctoredCoreError(message: "A public image did not resolve to the 16-bit promote arm")
    }
    guard case .external(let external) = typedPlan,
          let decoded = typed.doc.flatMap({ external.pixels(into: $0, profile: .convert) }),
          let expectedBytes = decoded.withPlateBytes({
              Array(UnsafeBufferPointer(start: $0, count: decoded.width * decoded.height * 4))
          }) else {
        throw DoctoredCoreError(message: "The public image did not decode")
    }
    let typedLayers = typed.doc?.layerCount ?? 0
    typedEditor.performPaste(typedPlan, placement: .centered, profile: .convert)
    pump()
    try require(typed.doc?.layerCount == typedLayers + 1,
        "The 16-bit promote paste did not add one layer")
    guard let promoted = typed.doc?.precision?.layerSource(typed.activeLayerIndex) else {
        throw DoctoredCoreError(message: "The promoted layer has no typed source")
    }
    let promotedCodes = try promoted.withRGBA16 { Array($0) }
    try require(promotedCodes.count == expectedBytes.count
                && zip(promotedCodes, expectedBytes).allSatisfy { UInt32($0) == UInt32($1) * 257 },
        "The promoted layer is not exactly ×257 of the decoded public image")
    let (float, floatEditor, _) = try openFloatDocument()
    defer { float.close(); pump() }
    guard let floatWindow = floatEditor.view.window else {
        throw DoctoredCoreError(message: "Float window missing")
    }
    try currentLayerKeyWindow(floatWindow)
    let floatReady = try currentLayerReady(float, floatEditor, floatWindow, "32-bit float")
    // A public image is promoted INTO float —
    // Paste is live with a public image only, and the plan is `.external`.
    try require(floatEditor.validateUserInterfaceItem(copyItem)
                && floatEditor.validateUserInterfaceItem(pasteItem),
        "Float clipboard menu state changed")
    guard case .external = try PastePlan.resolve(for: float, pasteboard: Clipboard.pasteboard,
                                                 payloadFile: nil) else {
        throw DoctoredCoreError(message: "A public image did not resolve to the float promotion")
    }
    try clipboard.requireOwnership()

    return ["case": "current-layer-commands-ui",
            "initial_canvas_focus": true, "paste_new_layer": true,
            "current_layer_only": true, "undo_redo": true,
            "text_focus_preserved": true, "depth_gates_preserved": true,
            "clipboard_failure_cleanup": clipboardFailureCleanup,
            "baseline": ["initial_responder": "FileDropView",
                         "actual_responder": "NSTextView",
                         "active_index": 0, "layer_count": 1,
                         "paint_target": "layer", "dirty": false,
                         "history_count": 1,
                         "source": "pre-fix actual-AppKit baseline run"],
            "opened": [firstReady, secondReady, thirdReady, typedReady, floatReady],
            "nil_target_key_equivalents": ["Command-C", "Command-V"],
            "invert": [openedInvert, changedInvert],
            "second_view_shared_active": selectionBeforeView.primary]
}
