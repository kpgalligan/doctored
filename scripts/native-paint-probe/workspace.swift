import AppKit

// Phase 12C-2: the UI-SOURCE half of the Navigator, the saved workspace
// layouts and the customizable shortcuts. MCP can ask `get_view`/`set_view`
// and the four workspace tools, but it cannot see the proxy rectangle the
// Navigator draws, a real window's frame, the width an AppKit panel actually
// laid out at, or a key equivalent firing through `NSApp.mainMenu` — so those
// are driven here, through the real controls of the real controllers, on the
// `panel.swift` / `replay.swift` precedent.

/// The layout both workspace assertions are written against, so one
/// description drives them all.
///
/// The 900 pt wide frame is anchored on `anchor` — the frame of the window
/// this case opened: its left edge, and centred on its middle. A literal
/// frame is not machine-independent: `applyWorkspace` runs the frame through
/// `WindowFrameFit`, and on a machine whose display does not start at the
/// global origin a frame outside the usable area is moved onto a display,
/// which is correct behaviour and not something this gate should call a
/// failure.
///
/// The HEIGHT is derived for the same reason, one axis over: 700 pt on a
/// display whose usable height is shorter would be clamped too, so B is at
/// most as tall as the anchor — a window that is on screen by definition —
/// and centred inside it. The panel, the chrome and the tool stay exactly
/// what this file chose.
private func layoutB(anchor: CGRect, name: String = "B",
                     tab: EditorPanelTab = .navigator,
                     width: Double = 420) -> WorkspaceLayout {
    let height = min(700, anchor.height.rounded(.down))
    return WorkspaceLayout(
        name: name,
        windowFrame: CGRect(x: anchor.minX, y: (anchor.midY - height / 2).rounded(),
                            width: 900, height: height),
        panelVisible: true, panelTab: tab, panelWidth: width,
        rulers: false, grid: true, snap: false, tool: .brush)
}

/// What the proxy rectangle MUST be, computed here from the scroll view and
/// the thumbnail's box rather than from anything the Navigator produced.
private func expectedProxy(_ editor: EditorViewController) throws -> CGRect {
    let view = editor.navigatorPanel.navigatorView
    guard let canvas = editor.document?.doc?.canvasSize, canvas.width > 0 else {
        throw DoctoredCoreError(message: "Owned document has no canvas")
    }
    let image = view.imageRect
    try require(image.width > 0 && image.height > 0, "The Navigator drew no thumbnail box")
    let scale = image.width / canvas.width
    let visible = editor.scrollView.contentView.bounds
        .intersection(CGRect(origin: .zero, size: canvas))
    try require(!visible.isNull, "The well shows no part of the canvas")
    return CGRect(x: image.minX + visible.minX * scale, y: image.minY + visible.minY * scale,
                  width: visible.width * scale, height: visible.height * scale)
}

private func near(_ a: CGRect, _ b: CGRect, _ tolerance: CGFloat) -> Bool {
    abs(a.minX - b.minX) <= tolerance && abs(a.minY - b.minY) <= tolerance
        && abs(a.width - b.width) <= tolerance && abs(a.height - b.height) <= tolerance
}

/// navigator-ui.
///
/// The proxy tracks the scroll view at 100 %, 200 % and 400 % within half a
/// point, and a 30 pt drag on the proxy scrolls the well while changing NO
/// pixel: the same document handle, the same flattened bytes, nothing to undo
/// and a document that is still not edited.
func navigatorUI(output: URL) throws -> [String: Any] {
    let (document, editor, _) = try openByteDocument(width: 400, height: 300)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    editor.showNavigator(nil)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    try require(editor.panelTab == 4, "View > Navigator did not select the Navigator tab")
    let panel = editor.navigatorPanel!
    try require(!panel.view.isHidden, "The Navigator tab stayed hidden")

    guard let before = document.doc else { throw DoctoredCoreError(message: "Owned document empty") }
    let beforeBytes = try flatBytes(before)
    let beforeHash = hashBytes(beforeBytes)

    var proxies: [[String: Any]] = []
    for zoom in [1.0, 2.0, 4.0] {
        _ = try editor.applyViewState(zoom: zoom, center: nil)
        editor.view.layoutSubtreeIfNeeded()
        pump()
        editor.refreshNavigator()
        let expected = try expectedProxy(editor)
        let actual = panel.navigatorView.proxyRect
        try require(near(actual, expected, 0.5),
            "At \(Int(zoom * 100)) % the proxy is \(actual), expected \(expected)")
        proxies.append(["zoom": zoom, "expected": NSStringFromRect(expected),
                        "actual": NSStringFromRect(actual)])
    }
    try screenshot(panel.view, output.appendingPathComponent("navigator-ui.png"))

    // A 30 pt drag inside the proxy pans the well.
    let start = CGPoint(x: panel.navigatorView.proxyRect.midX,
                        y: panel.navigatorView.proxyRect.midY)
    let originBefore = editor.scrollView.contentView.bounds.origin
    try send(panel.navigatorView, .leftMouseDown, start)
    try send(panel.navigatorView, .leftMouseDragged, CGPoint(x: start.x + 30, y: start.y))
    try send(panel.navigatorView, .leftMouseUp, CGPoint(x: start.x + 30, y: start.y))
    pump()
    editor.refreshNavigator()
    let originAfter = editor.scrollView.contentView.bounds.origin
    try require(originAfter.x != originBefore.x,
        "A 30 pt proxy drag did not move the clip origin: \(originBefore) -> \(originAfter)")

    guard let after = document.doc else { throw DoctoredCoreError(message: "Owned document empty") }
    try require(ObjectIdentifier(after) == ObjectIdentifier(before),
        "The Navigator replaced the document handle")
    try require(hashBytes(try flatBytes(after)) == beforeHash,
        "The Navigator changed the composite")
    try require(document.undoManager?.canUndo != true,
        "The Navigator registered an undo step")
    try require(!document.isDocumentEdited, "The Navigator marked the document edited")
    // …and the proxy still describes the well it just moved.
    let expected = try expectedProxy(editor)
    try require(near(panel.navigatorView.proxyRect, expected, 0.5),
        "After the drag the proxy is \(panel.navigatorView.proxyRect), expected \(expected)")

    return ["name": "navigator-ui", "passed": true, "proxies": proxies,
            "clip_origin_before": NSStringFromPoint(originBefore),
            "clip_origin_after": NSStringFromPoint(originAfter),
            "composite_sha256": beforeHash, "can_undo": document.undoManager?.canUndo ?? false,
            "document_edited": document.isDocumentEdited]
}

/// workspace-restore-ui.
///
/// A layout applied to a real window: the frame, the tab, the panel's real
/// laid-out width, the chrome toggles and the tool. A SECOND window opened
/// while it is applied gets the same width (the store is the one source), the
/// Info tab comes back through `showInfoTab()` and so has its selection
/// provider, and at the narrowest admitted width the Assistant panel's
/// content still fits.
func workspaceRestoreUI(output: URL) throws -> [String: Any] {
    let previousChrome = ToolOptionsStore.shared.view
    defer { ToolOptionsStore.shared.view = previousChrome }
    let (document, editor, _) = try openByteDocument(width: 320, height: 240)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    // A is whatever this window looks like NOW, so restoring it at the end is
    // a real round trip rather than a second invented layout.
    let a = editor.captureWorkspace(name: "A")

    let b = layoutB(anchor: a.windowFrame)
    editor.applyWorkspace(b)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    guard let window = editor.view.window else {
        throw DoctoredCoreError(message: "Owned editor window missing")
    }
    try require(window.frame == b.windowFrame,
        "The restored frame is \(NSStringFromRect(window.frame)), expected "
            + NSStringFromRect(b.windowFrame))
    try require(editor.panelTab == 4, "The restored tab is \(editor.panelTab), expected 4")
    try require(editor.navigatorPanel.view.frame.width == 420,
        "The Navigator laid out at \(editor.navigatorPanel.view.frame.width), expected 420")
    let chrome = ToolOptionsStore.shared.view
    try require(chrome.panelWidth == 420 && !chrome.rulers && chrome.showGrid && !chrome.snapEnabled,
        "The restored chrome is \(chrome.panelWidth)/\(chrome.rulers)/\(chrome.showGrid)/\(chrome.snapEnabled)")
    try require(editor.currentTool == .brush,
        "The restored tool is \(editor.currentTool.identifier), expected brush")
    let captured = editor.captureWorkspace(name: "B")
    try require(captured == b, "The capture does not round-trip: \(captured.json) vs \(b.json)")
    try screenshot(editor.view, output.appendingPathComponent("workspace-restore-ui.png"))

    // A second window, opened while B is applied, takes the same width: the
    // store is read at creation, not copied from the first window.
    let (second, secondEditor, _) = try openByteDocument(width: 64, height: 48)
    secondEditor.view.layoutSubtreeIfNeeded()
    pump()
    let secondWidth = secondEditor.navigatorPanel.view.frame.width
    try require(secondWidth == 420,
        "A second window's panel laid out at \(secondWidth), expected 420")
    second.close()
    pump()
    editor.view.window?.makeKeyAndOrderFront(nil)
    pump()

    // The Info tab comes back through its own entry point, so it has the
    // selection provider only that path installs.
    editor.applyWorkspace(layoutB(anchor: a.windowFrame, tab: .info))
    editor.view.layoutSubtreeIfNeeded()
    pump()
    try require(editor.panelTab == 3, "The restored Info tab is \(editor.panelTab), expected 3")
    try require(editor.infoPanel.selectionProvider != nil,
        "A restored Info tab has no selection provider")

    // The narrowest admitted width: the Assistant panel's content must still
    // fit inside it (the DS.panelWidth - 24 key field was the one that did
    // not).
    editor.applyWorkspace(layoutB(anchor: a.windowFrame, tab: .assistant, width: 240))
    editor.view.layoutSubtreeIfNeeded()
    pump()
    guard let assistant = member(editor, "assistantPanel") as? NSViewController else {
        throw DoctoredCoreError(message: "Actual assistant panel missing")
    }
    try require(assistant.view.frame.width == 240,
        "The Assistant laid out at \(assistant.view.frame.width), expected 240")
    assistant.view.layoutSubtreeIfNeeded()
    var overflow: [String] = []
    for child in descendants(assistant.view) where !child.isHidden && child !== assistant.view {
        let box = child.convert(child.bounds, to: assistant.view)
        if box.maxX > 240.5 { overflow.append("\(type(of: child)) \(NSStringFromRect(box))") }
    }
    try require(overflow.isEmpty, "Assistant content overflows 240 pt: \(overflow)")

    // …and back to A.
    editor.applyWorkspace(a)
    editor.view.layoutSubtreeIfNeeded()
    pump()
    let restored = editor.captureWorkspace(name: "A")
    try require(restored == a, "Restoring A did not round-trip: \(restored.json) vs \(a.json)")

    return ["name": "workspace-restore-ui", "passed": true, "b": b.json, "captured": captured.json,
            "second_window_panel_width": secondWidth, "a": a.json, "restored": restored.json,
            "assistant_overflow": overflow]
}

/// The main menu a shortcut case works on: a fresh one built by the real
/// `AppDelegate`, installed as `NSApp.mainMenu` because `CommandPalette
/// .commands()` returns `[]` without one and `performKeyEquivalent` has
/// nothing to walk. The previous menu (nil, in this probe) is restored by the
/// caller's `defer`.
private func installMainMenu() -> (menu: NSMenu, delegate: AppDelegate) {
    // The delegate is handed BACK because `NSMenu.delegate` is weak: the
    // Workspaces submenu's `menuNeedsUpdate` never runs if the only reference
    // to the `AppDelegate` that built the menu dies with this call.
    let delegate = AppDelegate()
    let menu = delegate.makeMainMenu()
    NSApp.mainMenu = menu
    return (menu, delegate)
}

private func shortcutItem(_ items: [ShortcutMenu.Item], _ identity: String) throws
    -> ShortcutMenu.Item
{
    guard let match = items.first(where: { $0.identity == identity }) else {
        throw DoctoredCoreError(message: "No customizable item with identity \(identity)")
    }
    return match
}

/// shortcut-edit-ui.
///
/// The walk lists both New commands with their 12C-1 meanings and nothing
/// from the app menu; a colliding chord is refused NAMING the holder; the
/// recorder field catches a chord an always-enabled item holds (⌘W) ahead of
/// the menu, so it is refused by name instead of closing a window; an
/// assignment reaches the live menu item, the command palette's row and the
/// file; a chord on Window ▸ Workspaces ▸ Save Workspace… survives the
/// `menuNeedsUpdate` rebuild; Reset All puts every default back; and the
/// sheet lists exactly the customizable set. It finishes by re-assigning
/// ⌥⇧⌘K, which pass 2 reads back off disk.
///
/// The test chord's LETTER matters and is not arbitrary: AppKit matches a menu
/// key equivalent while IGNORING an extra Shift on a lower-case letter
/// (measured — an item at ⌘R fires on a ⇧⌘R press whose
/// `charactersIgnoringModifiers` is "r"), so a chord whose Shift-less sibling
/// any shipped item holds would be swallowed by that item. ⌥⌘K and ⌥⇧⌘K are
/// both free; ⌥⇧⌘R stopped being usable when View ▸ Rotate View… took ⌥⌘R.
func shortcutEditUI() throws -> [String: Any] {
    let previous = NSApp.mainMenu
    let (menu, delegate) = installMainMenu()
    defer { withExtendedLifetime(delegate) { NSApp.mainMenu = previous } }
    let (document, editor, _) = try openByteDocument()
    defer { document.close(); pump() }

    let items = ShortcutMenu.customizableItems(in: menu)
    try require(!items.contains { $0.path.hasPrefix("Doctored ") },
        "The walk offered an app-menu item")
    let newDocument = try shortcutItem(items, "newDocument:")
    try require(newDocument.chord == ShortcutChord(key: "n", modifiers: [.command]),
        "File > New… is \(newDocument.chord?.display ?? "—"), expected ⌘N")
    let fromClipboard = try shortcutItem(items, "newFromClipboard:")
    try require(fromClipboard.chord == ShortcutChord(key: "n", modifiers: [.command, .option]),
        "File > New from Clipboard is \(fromClipboard.chord?.display ?? "—"), expected ⌥⌘N")

    // A colliding chord is refused by NAME. ⌘J is Layer ▸ Layer Via Copy.
    let zoomIn = try shortcutItem(items, "zoomInAction:")
    var collision = ""
    do {
        try ShortcutMenu.assign(ShortcutChord(key: "j", modifiers: [.command]),
                                to: zoomIn.identity, in: menu)
    } catch {
        collision = error.localizedDescription
    }
    try require(collision.contains("Layer ▸ Layer Via Copy"),
        "A colliding assignment was not refused naming the holder: \"\(collision)\"")
    try require(try shortcutItem(ShortcutMenu.customizableItems(in: menu), "zoomInAction:").chord
        == zoomIn.chord, "The refused assignment changed Zoom In anyway")

    // A chord without ⌘ is refused too.
    var bare = ""
    do {
        try ShortcutMenu.assign(ShortcutChord(key: "j", modifiers: [.option]),
                                to: zoomIn.identity, in: menu)
    } catch {
        bare = error.localizedDescription
    }
    try require(!bare.isEmpty, "A chord with no ⌘ was accepted")

    // A chord an ALWAYS-ENABLED item holds must reach the RECORDER, not the
    // menu. Menu key equivalents are resolved AHEAD of the first responder,
    // so a field that only overrode `keyDown` would let ⌘W close the window
    // the sheet is sitting on instead of catching the chord. ⌘W is File ▸
    // Close (`performClose:`), a system selector the sheet never offers for
    // editing — which is exactly why it has to be REFUSED BY NAME rather than
    // run.
    let closeChord = ShortcutChord(key: "w", modifiers: [.command])
    guard let holder = ShortcutMenu.conflict(closeChord, excluding: "", in: menu) else {
        throw DoctoredCoreError(message: "No menu item holds ⌘W; this gate has no holder")
    }
    try require(holder.path == "File ▸ Close", "⌘W is \(holder.path), expected File ▸ Close")
    let recorder = ShortcutRecorderField()
    var recorded: ShortcutChord?
    recorder.onRecord = { recorded = $0 }
    recorder.translatesAutoresizingMaskIntoConstraints = false
    let host = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 300, height: 120),
                        styleMask: [.titled, .closable], backing: .buffered, defer: false)
    // A programmatically created NSWindow is released when closed, which with
    // a strong local reference is an over-release.
    host.isReleasedWhenClosed = false
    guard let hostContent = host.contentView else {
        throw DoctoredCoreError(message: "Owned host window has no content view")
    }
    hostContent.addSubview(recorder)
    NSLayoutConstraint.activate([
        recorder.centerXAnchor.constraint(equalTo: hostContent.centerXAnchor),
        recorder.centerYAnchor.constraint(equalTo: hostContent.centerYAnchor),
    ])
    host.orderFront(nil)
    hostContent.layoutSubtreeIfNeeded()
    pump()
    try require(host.makeFirstResponder(recorder), "The recorder refused first responder")
    guard let closePress = NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: [.command],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: host.windowNumber,
        context: nil, characters: "w", charactersIgnoringModifiers: "w", isARepeat: false,
        keyCode: 13) else {
        throw DoctoredCoreError(message: "Owned key event unavailable")
    }
    let visibleBefore = NSApp.windows.filter(\.isVisible).count
    // The view hierarchy is offered a key equivalent before `NSApp.mainMenu`
    // is, so a `true` here IS "the menu never saw it".
    let intercepted = host.performKeyEquivalent(with: closePress)
    pump()
    try require(intercepted,
        "⌘W was not taken by the recorder, so File ▸ Close would have run it")
    try require(recorded == closeChord,
        "The recorder caught \(recorded?.display ?? "—"), expected ⌘W")
    try require(NSApp.windows.filter(\.isVisible).count == visibleBefore,
        "A window closed while ⌘W was being recorded")
    host.close()
    pump()
    editor.view.window?.makeKeyAndOrderFront(nil)
    pump()
    // …and what the recorder hands the sheet is refused NAMING the holder.
    var held = ""
    do {
        try ShortcutMenu.assign(closeChord, to: zoomIn.identity, in: menu)
    } catch {
        held = error.localizedDescription
    }
    try require(held.contains("File ▸ Close"),
        "⌘W was not refused naming its holder: \"\(held)\"")

    // The real assignment, through the real API.
    let chord = ShortcutChord(key: "k", modifiers: [.command, .option, .shift])
    try require(chord.display == "⌥⇧⌘K", "The chord draws as \(chord.display)")
    let rulers = try shortcutItem(items, "toggleRulers:")
    try ShortcutMenu.assign(chord, to: rulers.identity, in: menu)
    try require(ShortcutChord(item: rulers.menuItem) == chord,
        "The menu item kept \(ShortcutChord(item: rulers.menuItem)?.display ?? "—")")
    pump()
    // `CommandPalette.commands()` lists only ENABLED items, and enablement
    // for a nil-target item is resolved through the KEY window's responder
    // chain — which an `.accessory` probe that never becomes the active app
    // does not have (`NSApp.keyWindow` is nil here, so every editor item
    // validates false). The item is therefore pointed at the real editor for
    // the walk, which is the object the responder chain would have found; the
    // `panel.swift` precedent for calling AppKit's own plumbing directly when
    // a pumped probe cannot depend on its scheduling.
    rulers.menuItem.target = editor
    defer { rulers.menuItem.target = nil }
    let palette = CommandPalette.commands()
    try require(!palette.isEmpty, "The command palette walked an empty menu")
    guard let paletteRow = palette.first(where: { $0.action == rulers.menuItem.action }) else {
        throw DoctoredCoreError(
            message: "View > Rulers has no command-palette row; the palette has \(palette.count) "
                + "rows and its View ones are "
                + "\(palette.filter { $0.path.hasPrefix("View") }.map(\.title)); key window "
                + "\(String(describing: NSApp.keyWindow)); main window "
                + "\(String(describing: NSApp.mainWindow)); rulers item enabled "
                + "\(rulers.menuItem.isEnabled)")
    }
    try require(paletteRow.shortcut == "⌥⇧⌘K",
        "The palette shows \(paletteRow.shortcut), expected ⌥⇧⌘K")
    try require((ShortcutMenu.stored().bindings.overrides[rulers.identity] ?? nil) == chord,
        "The file does not hold the assignment")

    // A chord on a DYNAMIC menu's customizable row survives the rebuild
    // `menuNeedsUpdate` runs — and AppKit runs one while searching for a key
    // equivalent, so an item recreated there would drop the binding the first
    // time any chord in the app was pressed.
    guard let windowMenu = menu.items.first(where: { $0.title == "Window" })?.submenu,
          let workspaces = windowMenu.items.compactMap({ $0.submenu })
              .first(where: { $0.title == "Workspaces" }) else {
        throw DoctoredCoreError(message: "The Window menu has no Workspaces submenu")
    }
    guard let menuDelegate = workspaces.delegate else {
        throw DoctoredCoreError(message: "The Workspaces submenu has no delegate")
    }
    let saveChord = ShortcutChord(key: "y", modifiers: [.command, .control, .option])
    let save = try shortcutItem(ShortcutMenu.customizableItems(in: menu), "saveWorkspace:")
    try ShortcutMenu.assign(saveChord, to: save.identity, in: menu)
    // A row the rebuild has to ADD, so a menu that did not rebuild fails the
    // gate rather than passing it by doing nothing.
    _ = try WorkspaceLibrary.save(
        layoutB(anchor: CGRect(x: 0, y: 0, width: 900, height: 900), name: "Rebuilt"),
        replace: true)
    defer { try? WorkspaceLibrary.delete(name: "Rebuilt") }
    menuDelegate.menuNeedsUpdate?(workspaces)
    try require(workspaces.items.contains { $0.title == "Rebuilt" },
        "The Workspaces menu did not rebuild its rows: \(workspaces.items.map(\.title))")
    let afterRebuild = try shortcutItem(
        ShortcutMenu.customizableItems(in: menu), "saveWorkspace:")
    try require(afterRebuild.chord == saveChord,
        "A Workspaces rebuild dropped Save Workspace…'s chord: "
            + "\(afterRebuild.chord?.display ?? "—"), expected \(saveChord.display)")

    // Reset All restores the launch-time snapshot for EVERY row.
    try ShortcutMenu.resetAll(in: menu)
    let afterReset = ShortcutMenu.customizableItems(in: menu)
    var drifted: [String] = []
    for item in afterReset where item.chord != item.defaultChord {
        drifted.append("\(item.path): \(item.chord?.display ?? "—")")
    }
    try require(drifted.isEmpty, "Reset All left \(drifted)")
    try require(ShortcutMenu.stored().bindings.overrides.isEmpty,
        "Reset All left overrides in the file")

    // The sheet lists exactly the customizable set.
    let sheet = ShortcutsSheetController(menu: menu)
    try require(sheet.rows.count == afterReset.count,
        "The sheet lists \(sheet.rows.count) rows, the walk has \(afterReset.count)")

    // LAST: the assignment pass 2 reads back off disk.
    try ShortcutMenu.assign(chord, to: rulers.identity, in: menu)
    try require((ShortcutMenu.stored().bindings.overrides[rulers.identity] ?? nil) == chord,
        "The final assignment did not reach the file")

    return ["name": "shortcut-edit-ui", "passed": true, "customizable": items.count,
            "sheet_rows": sheet.rows.count, "collision": collision, "no_command_refusal": bare,
            "palette_shortcut": paletteRow.shortcut, "assigned": chord.json,
            "recorder_intercepted": intercepted, "recorded_chord": recorded?.json ?? [:],
            "always_enabled_holder": holder.path, "always_enabled_refusal": held,
            "save_workspace_chord_after_rebuild": afterRebuild.chord?.json ?? [:],
            "workspaces_rows_after_rebuild": workspaces.items.map(\.title)]
}

/// shortcut-relaunch-ui (pass 2 only).
///
/// A SECOND process over the same private shortcut directory: the menu built
/// at launch already carries the assignment pass 1 stored, and pressing the
/// chord fires the command.
func shortcutRelaunchUI() throws -> [String: Any] {
    let previous = NSApp.mainMenu
    let (menu, delegate) = installMainMenu()
    defer { withExtendedLifetime(delegate) { NSApp.mainMenu = previous } }
    let (document, editor, _) = try openByteDocument()
    defer { document.close(); pump() }

    let chord = ShortcutChord(key: "k", modifiers: [.command, .option, .shift])
    let rulers = try shortcutItem(ShortcutMenu.customizableItems(in: menu), "toggleRulers:")
    try require(ShortcutChord(item: rulers.menuItem) == chord,
        "After relaunch View > Rulers is "
            + "\(ShortcutChord(item: rulers.menuItem)?.display ?? "—"), expected ⌥⇧⌘K")

    let before = ToolOptionsStore.shared.view.rulers
    defer { ToolOptionsStore.shared.view.rulers = before }
    guard let window = editor.view.window else {
        throw DoctoredCoreError(message: "Owned editor window missing")
    }
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    pump()
    // `performKeyEquivalent` fires only an ENABLED item, and enablement for a
    // nil-target item is resolved through the KEY window's responder chain —
    // which an `.accessory` probe that never becomes the active app does not
    // have. The item is pointed at the real editor, which is the object the
    // responder chain would have found; the chord, the matching and the
    // command that runs are all AppKit's own.
    rulers.menuItem.target = editor
    defer { rulers.menuItem.target = nil }
    // `charactersIgnoringModifiers` is "k", not "K": with the Command key
    // held macOS drops the Shift transformation too, which is why every menu
    // in this app (⇧⌘S, ⇧⌘E, …) spells its key equivalent lower-case with
    // Shift in the MASK, and why AppKit's own matching is case-sensitive
    // against that. Verified against `NSMenu.performKeyEquivalent` directly —
    // which is also where the Shift leniency this chord's letter avoids was
    // measured (see the case's header).
    guard let press = NSEvent.keyEvent(
        with: .keyDown, location: .zero, modifierFlags: [.command, .option, .shift],
        timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
        context: nil, characters: "k", charactersIgnoringModifiers: "k", isARepeat: false,
        keyCode: 40) else {
        throw DoctoredCoreError(message: "Owned key event unavailable")
    }
    let fired = menu.performKeyEquivalent(with: press)
    pump()
    let after = ToolOptionsStore.shared.view.rulers
    try require(fired,
        "⌥⇧⌘K matched no menu item after relaunch; the item's key equivalent is "
            + "\"\(rulers.menuItem.keyEquivalent)\" mask "
            + "\(rulers.menuItem.keyEquivalentModifierMask.rawValue), enabled "
            + "\(rulers.menuItem.isEnabled), target "
            + "\(String(describing: rulers.menuItem.target)), key window "
            + "\(String(describing: NSApp.keyWindow))")
    try require(after != before, "⌥⇧⌘K fired but the rulers preference did not flip")

    return ["name": "shortcut-relaunch-ui", "passed": true, "chord": chord.json,
            "rulers_before": before, "rulers_after": after, "performed": fired]
}

// ---------------------------------------------------------------------------
// shortcut-shift-rule-ui (Phase 12E-1b)
// ---------------------------------------------------------------------------

/// The probe target a measured press fires: an `NSObject` whose one action
/// appends the sender's title, so a measurement is "which items fired, in
/// what order" rather than a side effect somebody has to interpret.
private final class ShiftRuleTarget: NSObject {
    var fired: [String] = []
    @objc func probeAction(_ sender: Any?) {
        fired.append((sender as? NSMenuItem)?.title ?? "?")
    }
}

/// The two EVENT SHAPES every press below is measured with — because the
/// first 12E-1b table turned out to be a property of the SHAPE and not of
/// AppKit's rule.
private enum ShiftRuleShape: String {
    /// What `NSEvent.keyEvent(…)` was handed in the first measurement: Shift
    /// in `modifierFlags`, `characters` upper case, and
    /// `charactersIgnoringModifiers` LOWER case. No keyboard delivers this —
    /// `charactersIgnoringModifiers` ignores every modifier EXCEPT Shift.
    case synthesized
    /// What the window server actually delivers, obtained by translating a
    /// `CGEvent` for the same key code and flags through `NSEvent(cgEvent:)`,
    /// which runs the SAME keyboard-layout translation a hardware press runs:
    /// `characters` lower case (the Command-key quirk) and
    /// `charactersIgnoringModifiers` UPPER case.
    case hardware
}

/// The window server's own translation of `keyCode` under `flags`. Used both
/// as the hardware-shaped press and to learn which LETTER this machine's
/// layout puts on that key, so nothing here assumes a US layout.
private func shiftRuleTranslated(_ keyCode: CGKeyCode, shifted: Bool) throws -> NSEvent {
    guard let source = CGEventSource(stateID: .privateState),
          let cg = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: true)
    else {
        throw DoctoredCoreError(message: "Owned CGEvent unavailable")
    }
    cg.flags = shifted ? [.maskShift, .maskCommand] : [.maskCommand]
    guard let event = NSEvent(cgEvent: cg) else {
        throw DoctoredCoreError(message: "Owned key event unavailable")
    }
    return event
}

/// The press for one shape, with the lower-case key this layout puts on
/// `keyCode` — which is what the measured menu spells its items with.
private func shiftRulePress(
    _ shape: ShiftRuleShape,
    shifted: Bool,
    keyCode: CGKeyCode
) throws -> (event: NSEvent, key: String) {
    let plain = try shiftRuleTranslated(keyCode, shifted: false)
    let key = (plain.charactersIgnoringModifiers ?? "").lowercased()
    switch shape {
    case .hardware:
        return (try shiftRuleTranslated(keyCode, shifted: shifted), key)
    case .synthesized:
        guard let press = NSEvent.keyEvent(
            with: .keyDown, location: .zero,
            modifierFlags: shifted ? [.command, .shift] : [.command],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: 0,
            context: nil, characters: shifted ? key.uppercased() : key,
            charactersIgnoringModifiers: key,
            isARepeat: false, keyCode: UInt16(keyCode)) else {
            throw DoctoredCoreError(message: "Owned key event unavailable")
        }
        return (press, key)
    }
}

/// One measured press: a PRIVATE `NSMenu` built to order, the event this file
/// owns, and the result as `{fired, returned}`.
private func shiftRuleMeasure(
    _ layout: [(submenu: String, items: [(title: String, shift: Bool)])],
    shifted: Bool,
    shape: ShiftRuleShape
) throws -> (fired: [String], returned: Bool, json: [String: Any]) {
    let target = ShiftRuleTarget()
    // The app's own spelling: a LOWER-CASE key with Shift in the MASK.
    let (press, key) = try shiftRulePress(shape, shifted: shifted, keyCode: 15)
    // NEVER `NSApp.mainMenu`: this menu exists for the measurement alone, so
    // nothing here can disturb the app's own shortcuts or be disturbed by
    // them.
    let root = NSMenu()
    for group in layout {
        let holder = NSMenuItem(title: group.submenu, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: group.submenu)
        for entry in group.items {
            let item = NSMenuItem(title: entry.title,
                                  action: #selector(ShiftRuleTarget.probeAction(_:)),
                                  keyEquivalent: key)
            item.keyEquivalentModifierMask = entry.shift ? [.command, .shift] : [.command]
            // An EXPLICIT target: enablement for a nil-target item is
            // resolved through the KEY window's responder chain, which an
            // `.accessory` probe does not have (the 12C-2 concession).
            item.target = target
            submenu.addItem(item)
        }
        holder.submenu = submenu
        root.addItem(holder)
    }
    let returned = root.performKeyEquivalent(with: press)
    pump()
    return (target.fired, returned,
            ["fired": target.fired, "returned": returned,
             "shape": shape.rawValue,
             "press": (shifted ? "⇧⌘" : "⌘") + key.uppercased(),
             "characters": press.characters ?? "",
             "characters_ignoring_modifiers": press.charactersIgnoringModifiers ?? "",
             "layout": layout.map { ["submenu": $0.submenu,
                                     "items": $0.items.map { ["title": $0.title,
                                                              "shift": $0.shift] }] }])
}

/// shortcut-shift-rule-ui.
///
/// **MEASURES first, then asserts what was MEASURED — in BOTH event shapes,
/// because the first pass measured only one of them and the one it measured
/// is not the one a keyboard delivers.** The history matters, so it is
/// recorded here rather than tidied away.
///
/// 12D-2 measured ONE item alone (an item at ⌘R fires on a ⇧⌘R press) and
/// INFERRED that "whichever comes first in the walk swallows the other".
/// 12E-1b set out to overturn that inference, on the grounds that the shipped
/// Undo ⌘Z / Redo ⇧⌘Z, Save ⌘S / Save As ⇧⌘S and Export ⌘E / Merge Down ⇧⌘E
/// pairs all work — and the first run's table appeared to confirm 12D-2 and
/// even to make the shipped pairs broken. That table was an ARTIFACT of the
/// event it was made with: `NSEvent.keyEvent(…)` was handed a LOWER-CASE
/// `charactersIgnoringModifiers` with Shift in the flags, and
/// `charactersIgnoringModifiers` ignores every modifier EXCEPT Shift, so no
/// hardware press has that shape. Each press is therefore measured twice —
/// once in that `synthesized` shape and once in the `hardware` shape the
/// window server itself produces (`CGEvent` → `NSEvent(cgEvent:)`, the same
/// layout translation a real press runs; `characters` comes back lower case
/// because Command is held, `charactersIgnoringModifiers` upper case).
///
/// | # | Layout | Press | synthesized | hardware |
/// | --- | --- | --- | --- | --- |
/// | M1 | `Plain` (⌘R) alone | ⇧⌘R | `Plain` | **nothing** |
/// | M2 | `Plain` in A, `Shifted` in B | ⇧⌘R | `Plain` | **`Shifted`** |
/// | M3 | `Shifted` in A, `Plain` in B | ⇧⌘R | `Shifted` | `Shifted` |
/// | M4a | one submenu, `Plain` first | ⇧⌘R | `Plain` | **`Shifted`** |
/// | M4b | one submenu, `Shifted` first | ⇧⌘R | `Shifted` | `Shifted` |
/// | M5 | `Plain` in A, `Shifted` in B | ⌘R | `Plain` | `Plain` |
/// | M6 | `Shifted` (⇧⌘R) alone | ⌘R | `Shifted` | **nothing** |
/// | M7 | the app's OWN Edit menu, Undo (⌘Z) then Redo (⇧⌘Z) | ⇧⌘Z | `Undo` | **`Redo`** |
///
/// **The rule, on the shape a keyboard actually delivers: the chord matches
/// EXACTLY.** A ⇧⌘R press does not fire a ⌘R item (M1) and a ⌘R press does
/// not fire a ⇧⌘R item (M6); where both exist the one whose mask matches
/// fires, wherever it sits in the walk (M2 and M4a, which are the two the
/// synthesized shape got backwards). There is NO Shift leniency and there is
/// NO shipped defect: M7 fires Edit ▸ **Redo** on a hardware-shaped ⇧⌘Z, so
/// the shipped ⌘Z/⇧⌘Z, ⌘S/⇧⌘S and ⌘E/⇧⌘E pairs work exactly as they appear
/// to. 12D-2's carried nit is CLOSED by measurement: the Shift leniency it
/// inferred is a property of hand-built `NSEvent.keyEvent` presses only.
///
/// **What did NOT change.** `ShortcutMenu.conflict` is untouched — it still
/// compares chords EXACTLY, which is precisely what the hardware column
/// says AppKit does, so ⌘R and ⇧⌘R are two chords to both of them.
///
/// This case asserts the MEASURED values in both columns, so it is a
/// regression test both for what AppKit does and for the shape that misled
/// the first pass.
func shortcutShiftRuleUI() throws -> [String: Any] {
    let plain = (title: "Plain", shift: false)
    let shift = (title: "Shifted", shift: true)
    let layouts: [(String, [(submenu: String, items: [(title: String, shift: Bool)])], Bool)] = [
        ("M1", [(submenu: "A", items: [plain])], true),
        ("M2", [(submenu: "A", items: [plain]), (submenu: "B", items: [shift])], true),
        ("M3", [(submenu: "A", items: [shift]), (submenu: "B", items: [plain])], true),
        ("M4a", [(submenu: "A", items: [plain, shift])], true),
        ("M4b", [(submenu: "A", items: [shift, plain])], true),
        ("M5", [(submenu: "A", items: [plain]), (submenu: "B", items: [shift])], false),
        ("M6", [(submenu: "A", items: [shift])], false),
    ]
    // MEASURE everything first, in both shapes, and record it — then assert.
    var measured: [String: (synthesized: [String], hardware: [String])] = [:]
    var measurements: [String: Any] = [:]
    for (label, layout, shifted) in layouts {
        let synthesized = try shiftRuleMeasure(layout, shifted: shifted, shape: .synthesized)
        let hardware = try shiftRuleMeasure(layout, shifted: shifted, shape: .hardware)
        measured[label] = (synthesized.fired, hardware.fired)
        measurements[label] = ["synthesized": synthesized.json, "hardware": hardware.json]
        trace("SHIFT \(label): synthesized \(synthesized.fired) returned "
            + "\(synthesized.returned) | hardware \(hardware.fired) returned "
            + "\(hardware.returned)")
    }
    let m7 = try shiftRuleShippedMenu()
    measured["M7"] = (m7.synthesized, m7.hardware)
    measurements["M7"] = m7.json
    trace("SHIFT M7: \(m7.json)")

    // The assertions, AFTER every measurement is recorded, and against the
    // MEASURED values of BOTH shapes.
    let expected: [(String, [String], [String], String)] = [
        ("M1", ["Plain"], [], "⇧⌘R on a lone ⌘R item"),
        ("M2", ["Plain"], ["Shifted"], "⇧⌘R with Plain first"),
        ("M3", ["Shifted"], ["Shifted"], "⇧⌘R with Shifted first"),
        ("M4a", ["Plain"], ["Shifted"], "⇧⌘R in one submenu, Plain first"),
        ("M4b", ["Shifted"], ["Shifted"], "⇧⌘R in one submenu, Shifted first"),
        ("M5", ["Plain"], ["Plain"], "⌘R with both present"),
        ("M6", ["Shifted"], [], "⌘R on a lone ⇧⌘R item"),
        ("M7", ["Undo"], ["Redo"], "⇧⌘Z on the app's own Edit menu"),
    ]
    for (label, synthesized, hardware, what) in expected {
        let got = measured[label] ?? ([], [])
        try require(got.synthesized == synthesized,
            "\(label): \(what), synthesized shape, fired \(got.synthesized), measured "
                + "\(synthesized)")
        try require(got.hardware == hardware,
            "\(label): \(what), HARDWARE shape, fired \(got.hardware), measured \(hardware)")
    }

    // …and the one thing that did NOT move: `ShortcutMenu.conflict` compares
    // chords exactly, which is what the hardware column shows AppKit doing.
    let a = ShortcutChord(key: "r", modifiers: [.command])
    let b = ShortcutChord(key: "r", modifiers: [.command, .shift])
    try require(a != b, "⌘R and ⇧⌘R must not compare equal as chords")
    measurements["conflict_predicate"] = "exact; unchanged by 12E-1b, and the hardware "
        + "column says exact is right"

    return ["name": "shortcut-shift-rule-ui", "passed": true,
            "measurements": measurements,
            "expectation": "HELD on the hardware-shaped event: an exact chord match wins "
                + "in both directions (M1 and M6 fire nothing, M2 and M4a fire Shifted). "
                + "The first pass's opposite table came from a synthesized event whose "
                + "charactersIgnoringModifiers was lower case under Shift, a shape no "
                + "keyboard delivers.",
            "rule": "on the shape the window server delivers (characters lower case under "
                + "Command, charactersIgnoringModifiers UPPER case under Shift) a menu "
                + "key equivalent matches the chord EXACTLY; the Shift leniency and the "
                + "first-in-the-walk ordering are artifacts of hand-built "
                + "NSEvent.keyEvent presses. M7 fires Edit ▸ Redo, so the shipped "
                + "⌘Z/⇧⌘Z pair is NOT defective",
            "owner": "closed by measurement: ShortcutMenu.conflict's exact comparison "
                + "matches AppKit's own, and nothing is carried to 12E-2"]
}

/// M7: the app's OWN main menu, Edit ▸ Undo (⌘Z) and Edit ▸ Redo (⇧⌘Z), with
/// both items pointed at a probe target so ENABLEMENT cannot confound the
/// matching question (a nil-target item needs a key window an `.accessory`
/// probe lacks — the 12C-2 concession), pressed ⇧⌘Z in BOTH event shapes.
/// Nothing about the menu, the chords or the matching is this file's; only
/// the target is.
private func shiftRuleShippedMenu() throws
    -> (synthesized: [String], hardware: [String], json: [String: Any])
{
    let previous = NSApp.mainMenu
    let (menu, delegate) = installMainMenu()
    defer { withExtendedLifetime(delegate) { NSApp.mainMenu = previous } }
    guard let edit = menu.items.first(where: { $0.title == "Edit" })?.submenu,
          let undo = edit.items.first(where: { $0.title == "Undo" }),
          let redo = edit.items.first(where: { $0.title == "Redo" }) else {
        throw DoctoredCoreError(message: "The Edit menu has no Undo/Redo pair")
    }
    let saved = [undo, redo].map { ($0, $0.target, $0.action) }
    defer { for (item, owner, action) in saved { item.target = owner; item.action = action } }

    var fired: [ShiftRuleShape: [String]] = [:]
    var shapes: [String: Any] = [:]
    for shape in [ShiftRuleShape.synthesized, .hardware] {
        let target = ShiftRuleTarget()
        for item in [undo, redo] {
            item.target = target
            item.action = #selector(ShiftRuleTarget.probeAction(_:))
        }
        // Key code 6 is the key this build's Undo/Redo pair is spelled with;
        // the press is translated for the CURRENT layout like every other, so
        // a layout that puts another letter there is reported rather than
        // measured as a silent non-match.
        let (press, key) = try shiftRulePress(shape, shifted: true, keyCode: 6)
        try require(key == undo.keyEquivalent,
            "This keyboard layout puts \"\(key)\" on key code 6, not Undo's "
                + "\"\(undo.keyEquivalent)\"; M7 cannot be measured on it")
        let returned = menu.performKeyEquivalent(with: press)
        pump()
        fired[shape] = target.fired
        shapes[shape.rawValue] = [
            "press": "⇧⌘Z", "fired": target.fired, "returned": returned,
            "characters": press.characters ?? "",
            "characters_ignoring_modifiers": press.charactersIgnoringModifiers ?? "",
        ]
    }
    shapes["undo"] = ["key": undo.keyEquivalent,
                      "mask": undo.keyEquivalentModifierMask.rawValue]
    shapes["redo"] = ["key": redo.keyEquivalent,
                      "mask": redo.keyEquivalentModifierMask.rawValue]
    return (fired[.synthesized] ?? [], fired[.hardware] ?? [], shapes)
}

/// workspace-frame-fit-ui.
///
/// `WindowFrameFit.fitted` on synthetic displays, with no window: a frame
/// inside the usable area comes back exactly, and every other one lands
/// wholly inside the display it overlaps most (or the fallback when it
/// overlaps none). Every result is idempotent, and is either the unchanged
/// input or inside its target.
func workspaceFrameFitUI() throws -> [String: Any] {
    // A 1440 x 900 laptop with a 25 pt menu bar and an 80 pt Dock, and a
    // 1920 x 1080 display to its right with its own menu bar.
    let laptop = CGRect(x: 0, y: 80, width: 1440, height: 795)
    let external = CGRect(x: 1440, y: 0, width: 1920, height: 1055)
    let pair = [laptop, external]
    let left = CGRect(x: 0, y: 0, width: 1000, height: 800)
    let lowered = CGRect(x: 1000, y: 100, width: 1000, height: 800)
    let level = CGRect(x: 1000, y: 0, width: 1000, height: 800)
    let halfPoint = CGRect(x: 0.5, y: 23.5, width: 1439, height: 800.5)
    struct Case {
        let name: String
        let frame: CGRect
        let screens: [CGRect]
        let fallback: CGRect
        /// nil: kept exactly.
        let target: CGRect?
        var size: CGSize? = nil
        var exact: CGRect? = nil
    }
    let cases = [
        Case(name: "inside", frame: CGRect(x: 100, y: 200, width: 800, height: 600),
             screens: pair, fallback: laptop, target: nil),
        Case(name: "straddling-covered", frame: CGRect(x: 1000, y: 200, width: 900, height: 500),
             screens: pair, fallback: laptop, target: nil),
        Case(name: "under-dock", frame: CGRect(x: 100, y: 79, width: 800, height: 600),
             screens: pair, fallback: laptop, target: laptop,
             exact: CGRect(x: 100, y: 80, width: 800, height: 600)),
        Case(name: "far-x", frame: CGRect(x: 60000, y: 200, width: 800, height: 600),
             screens: pair, fallback: laptop, target: laptop,
             size: CGSize(width: 800, height: 600)),
        Case(name: "far-y", frame: CGRect(x: 100, y: -3000, width: 800, height: 600),
             screens: pair, fallback: laptop, target: laptop,
             size: CGSize(width: 800, height: 600)),
        Case(name: "huge", frame: CGRect(x: 0, y: 0, width: 9000, height: 3000),
             screens: pair, fallback: laptop, target: external, exact: external),
        Case(name: "seventy-thirty", frame: CGRect(x: 300, y: 0, width: 1000, height: 500),
             screens: [left, lowered], fallback: lowered, target: left,
             exact: CGRect(x: 0, y: 0, width: 1000, height: 500)),
        Case(name: "equal-first", frame: CGRect(x: 500, y: 600, width: 1000, height: 400),
             screens: [left, level], fallback: level, target: left),
        Case(name: "equal-first-swapped", frame: CGRect(x: 500, y: 600, width: 1000, height: 400),
             screens: [level, left], fallback: left, target: level),
        Case(name: "half-point", frame: CGRect(x: 100, y: 30, width: 800, height: 900),
             screens: [halfPoint], fallback: halfPoint, target: halfPoint,
             exact: CGRect(x: 100, y: 24, width: 800, height: 800)),
        Case(name: "no-screens", frame: CGRect(x: 60000, y: -3000, width: 800, height: 600),
             screens: [], fallback: laptop, target: nil),
    ]
    var results: [[String: Any]] = []
    for item in cases {
        let fitted = WindowFrameFit.fitted(item.frame, visibleFrames: item.screens,
                                           fallback: item.fallback)
        let again = WindowFrameFit.fitted(fitted, visibleFrames: item.screens,
                                          fallback: item.fallback)
        try require(again == fitted,
            "\(item.name): fitting twice gave \(NSStringFromRect(again)), once "
                + NSStringFromRect(fitted))
        if let target = item.target {
            try require(target.contains(fitted),
                "\(item.name): \(NSStringFromRect(fitted)) is not inside "
                    + NSStringFromRect(target))
            try require(fitted.minX == fitted.minX.rounded() && fitted.minY == fitted.minY.rounded()
                        && fitted.width == fitted.width.rounded()
                        && fitted.height == fitted.height.rounded(),
                "\(item.name): \(NSStringFromRect(fitted)) is not on whole points")
        } else {
            try require(fitted == item.frame,
                "\(item.name): a frame to keep became \(NSStringFromRect(fitted))")
        }
        if let size = item.size {
            try require(fitted.size == size,
                "\(item.name): the size became \(NSStringFromSize(fitted.size)), expected "
                    + NSStringFromSize(size))
        }
        if let exact = item.exact {
            try require(fitted == exact,
                "\(item.name): \(NSStringFromRect(fitted)), expected \(NSStringFromRect(exact))")
        }
        results.append(["case": item.name, "frame": NSStringFromRect(item.frame),
                        "fitted": NSStringFromRect(fitted)])
    }
    return ["name": "workspace-frame-fit-ui", "passed": true, "results": results]
}
