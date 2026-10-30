import AppKit

// "default shortcuts" (`designs/v1-shortcuts-fade.md`): the menus this build makes, walked
// whole — the app menu, the system selectors and the hidden twins included — carry exactly the
// audit's chords, no chord is held twice, the bare tool letters are `expectedToolLetters`, and a stored
// override that takes a default chord withholds that default until the override goes. No file is
// written: the withhold rule runs through `ShortcutMenu.apply` alone.

/// Every chorded item of `AppDelegate.buildMainMenu()`, as "identity chord" (the identity is
/// `ShortcutMenu.identity(of:)`, the chord `CommandPalette.shortcutText`'s spelling). The audit
/// table in `designs/v1-shortcuts-fade.md` is the source; a change to a default fails here.
private let expectedDefaultChords: Set<String> = [
    // Doctored (the app menu)
    "showSettings: ⌘,", "showSettings: ⌘K", "hide: ⌘H", "hideOtherApplications: ⌥⌘H",
    "terminate: ⌘Q",
    // File
    "newDocument: ⌘N", "newFromClipboard: ⌥⌘N", "openDocument: ⌘O", "showActions: ⌥F9",
    "performClose: ⌘W", "saveDocument: ⌘S", "saveDocumentAs: ⇧⌘S", "revertDocumentToSaved: F12",
    "showExportWorkspace: ⌥⇧⌘W", "quickExport: ⌥⌘E", "showFileInfo: ⌥⇧⌘I",
    "runPageLayout: ⇧⌘P", "printDocument: ⌘P",
    // Edit
    "undo: ⌘Z", "redo: ⇧⌘Z", "showFade: ⇧⌘F", "cut: ⌘X", "copy: ⌘C", "copyMerged: ⇧⌘C",
    "paste: ⌘V", "pasteInPlace: ⇧⌘V", "pasteInto: ⌥⇧⌘V", "clearSelection: ⌫",
    "fillSelection: ⇧F5", "fillSelectionFromDeleteKey: ⇧⌫", "fillWithForeground: ⌥⌫",
    "fillWithBackground: ⌘⌫", "fillWithForegroundPreserving: ⌥⇧⌫",
    "fillWithBackgroundPreserving: ⇧⌘⌫", "showColorSettings: ⇧⌘K",
    // Image
    "resizeImage: ⌥⌘I", "showCanvasSize: ⌥⌘C", "showAdjustments: ⌥⌘A", "showLevels: ⌘L",
    "showCurves: ⌘M", "showAdjustmentSheet:#2 ⌘U", "showAdjustmentSheet:#3 ⌘B",
    "showAdjustmentSheet:#4 ⌥⇧⌘B", "applyInvert: ⌘I", "applyDesaturate: ⇧⌘U", "autoTone: ⇧⌘L",
    "autoContrast: ⌥⇧⌘L", "autoColor: ⇧⌘B",
    // Layer
    "newLayer: ⇧⌘N", "groupLayers: ⌘G", "ungroupLayers: ⇧⌘G", "layerViaCopy: ⌘J",
    "layerViaCut: ⇧⌘J", "freeTransform: ⌘T", "freeTransformCopy: ⌥⌘T", "transformAgain: ⇧⌘T",
    "transformAgainWithCopy: ⌥⇧⌘T", "toggleClippingMask: ⌥⌘G", "mergeDown: ⌘E",
    "mergeVisible: ⇧⌘E", "stampVisible: ⌥⇧⌘E", "bringToFront: ⇧⌘]", "bringForward: ⌘]",
    "sendBackward: ⌘[", "sendToBack: ⇧⌘[", "lockAll: ⌘/",
    // Select
    "selectAll: ⌘A", "deselect: ⌘D", "invertSelection: ⇧⌘I", "reselect: ⇧⌘D",
    "refineSelection: ⌥⌘R", "featherSelection: ⇧F6",
    // Filters
    "repeatLastFilter: ⌃F", "showLiquify: ⇧⌘X", "showDevelopFilter: ⇧⌘A",
    "showLensCorrection: ⇧⌘R",
    // Tools
    "showCommandPalette: ⌘F",
    // View
    "zoomInAction: ⌘+", "zoomInAction: ⌘=", "zoomOutAction: ⌘-", "zoomActualAction: ⌘1",
    "zoomActualAction: ⌥⌘0", "zoomFitAction: ⌘0", "resetViewRotation: ⌃⌥⌘R",
    "toggleProofColors: ⌘Y", "toggleGamutWarning: ⇧⌘Y", "toggleShowClipping: ⌥⌘J",
    "toggleLayersPanel: F7", "showChannels: ⌃⌘C", "showAssistant: ⌃⌘A", "showInfo: F8",
    "showNavigator: ⌃⌘N", "toggleRulers: ⌘R", "toggleGuides: ⌘;", "toggleLockGuides: ⌥⌘;",
    "toggleGrid: ⌘'", "toggleSnap: ⇧⌘;",
    // Window
    "minimizeWindow: ⌃⌘M",
]

/// The bare default key of every tool (`EditorTool.keyCharacter`), by case name.
private let expectedToolLetters: [String: String] = [
    "select": "m", "ellipseSelect": "m", "lasso": "l", "freehandLasso": "l",
    "magneticLasso": "l", "wand": "w", "quickSelect": "w", "objectSelect": "w", "subject": "w",
    "move": "v", "brush": "b", "eraser": "e", "fill": "g", "gradient": "g", "text": "t",
    "eyedropper": "i", "colorSampler": "i", "ruler": "i", "crop": "c", "perspectiveCrop": "c",
    "clone": "s", "dodge": "o", "sponge": "o", "blurBrush": "r", "sharpenBrush": "r",
    "smudge": "r", "historyBrush": "y", "artHistoryBrush": "y", "heal": "j", "spotHeal": "j", "patch": "j",
    "redEye": "j", "remove": "j", "contentAwareMove": "j", "shapeRect": "u",
    "shapeEllipse": "u", "shapeLine": "u", "pen": "p", "zoom": "z", "hand": "h",
]

/// Every leaf item with an action under `menu`, depth-first, hidden ones included.
private func shortcutLeaves(_ menu: NSMenu) -> [NSMenuItem] {
    menu.items.flatMap { item -> [NSMenuItem] in
        if let submenu = item.submenu { return shortcutLeaves(submenu) }
        return item.action == nil || item.isSeparatorItem ? [] : [item]
    }
}

private func chordEntry(_ item: NSMenuItem) -> String? {
    guard !item.keyEquivalent.isEmpty else { return nil }
    return ShortcutMenu.identity(of: item) + " " + CommandPalette.shortcutText(for: item)
}

func verifyDefaultShortcuts() throws {
    let tag = "[default shortcuts]"
    // `buildMainMenu` installs its Window and Help menus on NSApp; put them back.
    let (windows, help) = (NSApp.windowsMenu, NSApp.helpMenu)
    let delegate = AppDelegate()
    let menu = delegate.buildMainMenu()
    NSApp.windowsMenu = windows
    NSApp.helpMenu = help
    defer { withExtendedLifetime(delegate) {} }

    let leaves = shortcutLeaves(menu)
    let actual = leaves.compactMap(chordEntry)
    let actualSet = Set(actual)
    let missing = expectedDefaultChords.subtracting(actualSet).sorted()
    let extra = actualSet.subtracting(expectedDefaultChords).sorted()
    try require(missing.isEmpty && extra.isEmpty && actual.count == actualSet.count,
                "\(tag) the default chords differ from the audit table: missing \(missing), "
                + "unexpected \(extra), entries \(actual.count) vs distinct \(actualSet.count)")
    // No chord is held twice, whatever the items' identities.
    var holders: [ShortcutChord: [String]] = [:]
    for item in leaves {
        guard let chord = ShortcutChord(item: item) else { continue }
        holders[chord, default: []].append(ShortcutMenu.identity(of: item))
    }
    let doubled = holders.filter { $0.value.count > 1 }
        .map { "\($0.key.display): \($0.value)" }.sorted()
    try require(doubled.isEmpty, "\(tag) a chord is held twice: \(doubled)")

    // The tool letters.
    var letters: [String: String] = [:]
    for tool in EditorTool.allCases { letters[tool.identifier] = tool.keyCharacter }
    try require(letters == expectedToolLetters,
                "\(tag) the tool letters differ: \(letters.filter { expectedToolLetters[$0.key] != $0.value })")

    // The withhold rule: an override that takes ⌃⌘M leaves Window ▸ Minimize unbound, and
    // dropping it brings Minimize's chord back. `apply` alone: no file is read or written.
    guard let minimize = leaves.first(where: { ShortcutMenu.identity(of: $0) == "minimizeWindow:" }),
          let rotate = leaves.first(where: { ShortcutMenu.identity(of: $0) == "rotateCW:" }) else {
        throw ProbeFailure(description: "\(tag) Minimize or Rotate 90° Clockwise is missing")
    }
    let controlM = ShortcutChord(key: "m", modifiers: [.command, .control])
    try require(ShortcutChord(item: minimize) == controlM && ShortcutChord(item: rotate) == nil,
                "\(tag) the starting chords")
    ShortcutMenu.apply(ShortcutBindings(overrides: ["rotateCW:": controlM]), to: menu)
    try require(ShortcutChord(item: rotate) == controlM,
                "\(tag) the override reaches Rotate 90° Clockwise")
    try require(minimize.keyEquivalent.isEmpty,
                "\(tag) Minimize's default is withheld while an override holds it: "
                + "\(CommandPalette.shortcutText(for: minimize))")
    ShortcutMenu.apply(ShortcutBindings(), to: menu)
    try require(ShortcutChord(item: minimize) == controlM && ShortcutChord(item: rotate) == nil,
                "\(tag) with no override Minimize gets ⌃⌘M back and Rotate has none")
    // A default the override does not hold stays: ⌘M is Curves… throughout.
    let curves = leaves.first { ShortcutMenu.identity(of: $0) == "showCurves:" }
    try require(curves.flatMap { ShortcutChord(item: $0) }
                == ShortcutChord(key: "m", modifiers: [.command]), "\(tag) Curves… keeps ⌘M")

    // A set saved under a renamed command's earlier identity (`LegacyNames.shortcutIdentities`):
    // decoded, each override lands on the command's current item; a set that binds both the
    // earlier and the current identity keeps the current one's chord.
    guard let develop = leaves.first(where: { ShortcutMenu.identity(of: $0) == "showDevelopFilter:" }),
          let panorama = leaves.first(where: { ShortcutMenu.identity(of: $0) == "mergePanorama:" }) else {
        throw ProbeFailure(description: "\(tag) Develop Filter… or Merge Panorama… is missing")
    }
    try require(develop.title == "Develop Filter…" && panorama.title == "Merge Panorama…",
                "\(tag) the renamed items' titles: \(develop.title), \(panorama.title)")
    let developChord = ShortcutChord(key: "d", modifiers: [.command, .control, .option])
    let panoramaChord = ShortcutChord(key: "p", modifiers: [.command, .control, .option])
    let currentChord = ShortcutChord(key: "k", modifiers: [.command, .control, .option])
    func legacySet(_ overrides: [String: Any]) throws -> ShortcutBindings {
        try ShortcutBindings.decode(["schema_version": ShortcutBindings.schemaVersion,
                                     "name": ShortcutBindings.libraryName, "overrides": overrides])
    }
    let legacy = try legacySet(["showCameraRawFilter:": developChord.json,
                                "photomerge:": panoramaChord.json])
    try require(legacy.overrides.keys.sorted() == ["mergePanorama:", "showDevelopFilter:"],
                "\(tag) the earlier identities decode as the current ones: \(legacy.overrides.keys.sorted())")
    ShortcutMenu.apply(legacy, to: menu)
    try require(ShortcutChord(item: develop) == developChord,
                "\(tag) a showCameraRawFilter: override reaches Develop Filter…: "
                + "\(CommandPalette.shortcutText(for: develop))")
    try require(ShortcutChord(item: panorama) == panoramaChord,
                "\(tag) a photomerge: override reaches Merge Panorama…: "
                + "\(CommandPalette.shortcutText(for: panorama))")
    let both = try legacySet(["showCameraRawFilter:": developChord.json,
                              "showDevelopFilter:": currentChord.json])
    try require(both.overrides.count == 1, "\(tag) both identities decode as one: \(both.overrides.keys.sorted())")
    ShortcutMenu.apply(both, to: menu)
    try require(ShortcutChord(item: develop) == currentChord,
                "\(tag) with both identities bound the current one's chord wins: "
                + "\(CommandPalette.shortcutText(for: develop))")
    ShortcutMenu.apply(ShortcutBindings(), to: menu)
    try require(ShortcutChord(item: develop) == ShortcutChord(key: "a", modifiers: [.command, .shift])
                && ShortcutChord(item: panorama) == nil,
                "\(tag) with no override Develop Filter… gets ⇧⌘A back and Merge Panorama… has none")
    print("PASS: \(tag) \(actual.count) chords match the audit, none doubled, "
          + "\(letters.count) tool letters, the withhold rule, and overrides saved under "
          + "showCameraRawFilter: and photomerge: land on Develop Filter… and Merge Panorama… "
          + "(the current identity wins when both are set)")
}
