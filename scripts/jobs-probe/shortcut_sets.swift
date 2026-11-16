import AppKit

// "shortcut sets" (#20): named shortcut sets (`ShortcutSets`) on the runner's private
// DR_SHORTCUTS_DIR. With no default file on disk, New Set, switching to the default set and back,
// and deleting the active set all work and leave no default file. Then: the default set lists
// first and is active; a new set takes the edits (menu chord and tool key) while the default
// set's file is untouched; switching swaps the menu chord, the tool key and an open rail's
// tooltip; a fresh install reads the active set (the relaunch); Reset All keeps the set's name;
// Duplicate, Rename (case-only too) and the refusals by sentence; deleting the active set, or its
// file, falls back to the default set; sixteen files list, a seventeenth is refused, and at
// sixteen the active set renames while an onto-existing rename changes no byte. Every set the
// case made, the pointer, the default set and `ToolKeys.shared` are put back on every exit.

/// A main menu as the app builds it, with NSApp's Window and Help menus put back
/// (`shortcut_defaults.swift`). The delegate must outlive the menu's use.
func shortcutProbeMenu() -> (menu: NSMenu, delegate: AppDelegate) {
    let (windows, help) = (NSApp.windowsMenu, NSApp.helpMenu)
    let delegate = AppDelegate()
    let menu = delegate.buildMainMenu()
    NSApp.windowsMenu = windows
    NSApp.helpMenu = help
    return (menu, delegate)
}

/// The leaf of `menu` whose identity is `identity`, hidden ones included.
func shortcutProbeItem(_ identity: String, in menu: NSMenu) -> NSMenuItem? {
    for item in menu.items {
        if let submenu = item.submenu {
            if let found = shortcutProbeItem(identity, in: submenu) { return found }
            continue
        }
        if !item.isHidden, ShortcutMenu.identity(of: item) == identity { return item }
    }
    return nil
}

/// The default set's entry, if one lists.
func shortcutProbeDefaultEntry() -> PreferenceLibraryEntry<ShortcutBindings>? {
    (try? ShortcutLibrary.listing())?.entries.first { $0.item.name == ShortcutBindings.libraryName }
}

/// Deletes every file but the default set's, by file so it works when the listing does not,
/// clears the pointer, puts the default set back as it was (`start`, or no file) and reloads
/// `ToolKeys.shared`.
func restoreShortcutLibrary(defaultSet start: ShortcutBindings?) {
    let keep = ShortcutLibrary.url(ShortcutBindings.libraryName).lastPathComponent
    let files = (try? FileManager.default.contentsOfDirectory(atPath: ShortcutLibrary.directory.path)) ?? []
    for file in files where file.hasSuffix(".json") && file != keep {
        try? ShortcutLibrary.delete(file: file)
    }
    try? ShortcutSets.Active.set(name: nil)
    if let start {
        _ = try? ShortcutLibrary.save(start, replace: true)
    } else if shortcutProbeDefaultEntry() != nil {
        try? ShortcutLibrary.delete(name: ShortcutBindings.libraryName)
    }
    ToolKeys.shared.load(ShortcutMenu.stored().bindings)
}

/// The refusal `body` throws, which must contain `sentence`.
func requireShortcutRefusal(
    _ tag: String, _ what: String, _ sentence: String, _ body: () throws -> Void
) throws {
    do {
        try body()
    } catch let error as DoctoredCoreError {
        try require(error.message.contains(sentence),
                    "\(tag) \(what) refused with “\(error.message)”, not “\(sentence)”")
        return
    }
    throw ProbeFailure(description: "\(tag) \(what) was accepted")
}

func verifyShortcutSets() throws {
    let tag = "[shortcut sets]"
    let defaultName = ShortcutBindings.libraryName
    let start = shortcutProbeDefaultEntry()?.item
    defer { restoreShortcutLibrary(defaultSet: start) }
    let (menu, delegate) = shortcutProbeMenu()
    defer { withExtendedLifetime(delegate) {} }
    func active() -> String { ShortcutSets.activeName() }

    // No default file on disk ("tool keys" leaves one): the default set loads as empty bindings.
    if shortcutProbeDefaultEntry() != nil { try ShortcutLibrary.delete(name: defaultName) }
    try ShortcutSets.Active.set(name: nil)
    ToolKeys.shared.load(ShortcutMenu.stored().bindings)
    try ShortcutSets.create("Mine", in: menu)
    try require(active() == "Mine", "\(tag) New Set did not make Mine active: \(active())")
    try ShortcutSets.activate(defaultName, in: menu)
    try require(active() == defaultName, "\(tag) the default set is not active: \(active())")
    try ShortcutSets.activate("Mine", in: menu)
    try ShortcutSets.delete("Mine", in: menu)
    try require(active() == defaultName && ShortcutSets.names() == [defaultName]
                && shortcutProbeDefaultEntry() == nil,
                "\(tag) with no default file the sets end \(ShortcutSets.names()), active "
                + "\(active()), default file \(shortcutProbeDefaultEntry() != nil)")

    // The default set on disk, with one override of its own.
    let rotate = ShortcutChord(key: "9", modifiers: [.command, .control, .option])
    try ShortcutLibrary.save(ShortcutBindings(overrides: ["rotateCW:": rotate]), replace: true)
    let defaultURL = ShortcutLibrary.url(defaultName)
    let defaultBytes = try Data(contentsOf: defaultURL)
    let options: JSONSerialization.WritingOptions = [.sortedKeys, .withoutEscapingSlashes]
    let stored = ShortcutMenu.stored().bindings
    let reencoded = try JSONSerialization.data(
        withJSONObject: ShortcutBindings.decode(stored.json).json, options: options)
    try require(ShortcutSets.names() == [defaultName] && active() == defaultName,
                "\(tag) the start lists \(ShortcutSets.names()), active \(active())")
    try require(stored.json["name"] as? String == defaultName && reencoded == defaultBytes,
                "\(tag) the default set's json carries name \(String(describing: stored.json["name"])) "
                + "and re-encodes to the same bytes: \(reencoded == defaultBytes)")

    // A new set takes the edits; the default set's file is untouched.
    guard let rulers = shortcutProbeItem("toggleRulers:", in: menu) else {
        throw ProbeFailure(description: "\(tag) View ▸ Rulers is missing")
    }
    let rulersDefault = ShortcutChord(key: "r", modifiers: [.command])
    let rulersMine = ShortcutChord(key: "k", modifiers: [.command, .option, .shift])
    let rail = ToolRailView(groups: EditorTool.railGroups)
    rail.setSelectedTool(.move)
    func brushTips() -> [String] {
        rail.toolTipOwners.map { $0 as String }.filter { $0.hasPrefix("Brush (") }
    }
    try ShortcutSets.create("Mine", in: menu)
    try ShortcutMenu.assign(rulersMine, to: "toggleRulers:", in: menu)
    try ToolKeys.shared.assign("k", to: .brush)
    let mine = try ShortcutLibrary.named("Mine")
    try require(mine.overrides["toggleRulers:"] == .some(rulersMine)
                && mine.toolKeys["brush"] == .some("k"),
                "\(tag) Mine's file holds \(mine.overrides), \(mine.toolKeys)")
    let afterEdits = try Data(contentsOf: defaultURL)
    try require(afterEdits == defaultBytes, "\(tag) an edit of Mine changed the default set's file")

    // Switching swaps the menu chord, the tool key and the rail's tooltip.
    try ShortcutSets.activate(defaultName, in: menu)
    try require(ShortcutChord(item: rulers) == rulersDefault && EditorTool.brush.keyCharacter == "b"
                && brushTips() == ["Brush (B)"],
                "\(tag) on the default set Rulers is \(rulers.keyEquivalent), Brush "
                + "\(EditorTool.brush.keyCharacter), tooltip \(brushTips())")
    try ShortcutSets.activate("Mine", in: menu)
    try require(ShortcutChord(item: rulers) == rulersMine && EditorTool.brush.keyCharacter == "k"
                && brushTips() == ["Brush (K)"],
                "\(tag) on Mine Rulers is \(rulers.keyEquivalent), Brush "
                + "\(EditorTool.brush.keyCharacter), tooltip \(brushTips())")

    // The relaunch: a fresh menu installed from disk and a fresh ToolKeys.
    let (relaunched, relaunchDelegate) = shortcutProbeMenu()
    defer { withExtendedLifetime(relaunchDelegate) {} }
    ShortcutMenu.installStored(to: relaunched)
    let fresh = ToolKeys()
    fresh.load(ShortcutMenu.stored().bindings)
    try require(shortcutProbeItem("toggleRulers:", in: relaunched).flatMap { ShortcutChord(item: $0) }
                == rulersMine && fresh.key(for: .brush) == "k",
                "\(tag) a fresh install does not read Mine")

    // Reset All keeps the set's name and file.
    try ShortcutMenu.resetAll(in: menu)
    let reset = try ShortcutLibrary.named("Mine")
    let afterReset = try Data(contentsOf: defaultURL)
    try require(active() == "Mine" && reset.overrides.isEmpty && reset.toolKeys["brush"] == .some("k")
                && ShortcutSets.names() == [defaultName, "Mine"] && afterReset == defaultBytes,
                "\(tag) Reset All on Mine left \(ShortcutSets.names()), active \(active()), "
                + "overrides \(reset.overrides)")

    // Duplicate, rename, a case-only rename, and the refusals.
    try ShortcutSets.duplicate(as: "Mine Copy", in: menu)
    let copy = try ShortcutLibrary.named("Mine Copy")
    try require(active() == "Mine Copy" && copy.toolKeys == reset.toolKeys,
                "\(tag) Duplicate made \(active()) holding \(copy.toolKeys)")
    try ShortcutSets.rename("Mine Copy", to: "Travel", in: menu)
    try require(active() == "Travel" && ShortcutSets.names() == [defaultName, "Mine", "Travel"],
                "\(tag) Rename left \(ShortcutSets.names()), active \(active())")
    try ShortcutSets.rename("Travel", to: "TRAVEL", in: menu)
    try require(active() == "TRAVEL" && ShortcutSets.names() == [defaultName, "Mine", "TRAVEL"],
                "\(tag) a case-only rename left \(ShortcutSets.names()), active \(active())")
    try requireShortcutRefusal(tag, "a rename onto Mine", "A shortcut set named “Mine” already exists.") {
        try ShortcutSets.rename("TRAVEL", to: "Mine", in: menu)
    }
    try requireShortcutRefusal(tag, "renaming the default set",
                               "The default shortcut set can be reset but not renamed.") {
        try ShortcutSets.rename(defaultName, to: "Other", in: menu)
    }
    try requireShortcutRefusal(tag, "deleting the default set",
                               "The default shortcut set can be reset but not deleted.") {
        try ShortcutSets.delete(defaultName, in: menu)
    }
    try requireShortcutRefusal(tag, "a new set named shortcuts", "“Shortcuts” is the default set's name.") {
        try ShortcutSets.create("shortcuts", in: menu)
    }

    // Deleting the active set, or its file, falls back to the default set.
    try ShortcutSets.delete("TRAVEL", in: menu)
    try require(active() == defaultName && ShortcutChord(item: rulers) == rulersDefault
                && brushTips() == ["Brush (B)"] && ShortcutSets.names() == [defaultName, "Mine"],
                "\(tag) deleting the active set left \(ShortcutSets.names()), active \(active())")
    try ShortcutSets.activate("Mine", in: menu)
    let listed = try ShortcutLibrary.listing()
    guard let mineFile = listed.entries.first(where: { $0.item.name == "Mine" })?.file else {
        throw ProbeFailure(description: "\(tag) Mine does not list")
    }
    try ShortcutLibrary.delete(file: mineFile)
    let fallback = ShortcutMenu.stored().bindings
    try require(active() == defaultName && fallback.overrides["rotateCW:"] == .some(rotate),
                "\(tag) deleting the active set's file left \(active()) active")
    ToolKeys.shared.load(fallback)

    // Sixteen files: the default set and fifteen more.
    for index in 1...15 { try ShortcutSets.create("Set \(index)", in: menu) }
    let full = try ShortcutLibrary.listing()
    try require(full.entries.count == 16 && full.broken.isEmpty,
                "\(tag) sixteen sets list \(full.entries.count), broken \(full.broken.count)")
    try requireShortcutRefusal(tag, "a seventeenth set",
                               "The shortcut set library folder holds 16 files; delete a preset or "
                                + "a broken file first.") {
        try ShortcutSets.create("Set 16", in: menu)
    }
    try ToolKeys.shared.assign("k", to: .brush)
    try ShortcutSets.rename("Set 15", to: "Renamed", in: menu)
    let renamed = try ShortcutLibrary.listing()
    let renamedSet = try ShortcutLibrary.named("Renamed")
    try require(renamed.entries.count == 16 && active() == "Renamed"
                && !renamed.entries.contains { $0.item.name == "Set 15" }
                && renamedSet.toolKeys["brush"] == .some("k"),
                "\(tag) a rename at sixteen left \(renamed.entries.map(\.item.name)), active \(active())")
    let set1URL = ShortcutLibrary.url("Set 1")
    let renamedURL = ShortcutLibrary.url("Renamed")
    let (set1Bytes, renamedBytes) = try (Data(contentsOf: set1URL), Data(contentsOf: renamedURL))
    try requireShortcutRefusal(tag, "a rename onto Set 1 at sixteen",
                               "A shortcut set named “Set 1” already exists.") {
        try ShortcutSets.rename("Renamed", to: "Set 1", in: menu)
    }
    let (set1After, renamedAfter) = try (Data(contentsOf: set1URL), Data(contentsOf: renamedURL))
    let afterRefusal = try ShortcutLibrary.listing()
    try require(set1After == set1Bytes && renamedAfter == renamedBytes
                && afterRefusal.entries.count == 16,
                "\(tag) a refused rename at sixteen changed a file")
    print("PASS: \(tag) with no default file the default set switches and takes a delete; a new "
          + "set takes the edits beside an untouched default file; switching swaps Rulers, the "
          + "Brush key and the rail's tooltip; a fresh install reads the active set; Reset All "
          + "keeps the name; Duplicate, Rename and a case-only rename; the refusals by sentence; "
          + "deleting the active set or its file falls back; sixteen files list, a seventeenth "
          + "is refused, and the active set renames at sixteen")
}
