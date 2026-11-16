import AppKit

// "kys import" (#20): `KysImport` over hand-written `.kys` text (a user-saved export: `modified="1"`,
// a removed binding, a removed tool key). Static commands map by `id` (one under a foreign name),
// dynamic ones by name, tools by `key` (one under a foreign name); unknown ids, names and tool
// keys, Burn, a type-2 tool and a `<taskspace>` are reported as not mapped; ⌘Q and ⌘, (fixed
// chords), a chord taken twice, a second shortcut, a bare letter and an unreadable key are
// refused by sentence; Rulers at ⌥⌘J is refused because Show Clipping holds that default, while a
// Group/Ungroup swap is kept. `importKys` writes a NEW active set named from the file (" 2" the
// second time) and changes no other set's bytes; whole files are refused by sentence; and the
// mapping tables name only customizable commands and real tools, none twice. The library is
// the runner's private DR_SHORTCUTS_DIR and is put back on every exit.

private let probeKysText = """
<?xml version="1.0" encoding="UTF-8"?>
<photoshop-keyboard-shortcuts version="4" filename="Probe" modified="1">
	<command kind="static" id="10" name="Neu...">
		<shortcut>Opt+Shift+Cmd+N</shortcut>
	</command>
	<command kind="dynamic" name="Liquify">
		<shortcut>Control+Cmd+X</shortcut>
	</command>
	<command kind="static" id="99999" name="Some Unknown">
		<shortcut>Opt+Cmd+U</shortcut>
	</command>
	<command kind="dynamic" name="Vanishing Point">
		<shortcut>Opt+Cmd+V</shortcut>
	</command>
	<command kind="static" id="1036" name="Feather...">
	</command>
	<command kind="static" id="30" name="Save">
		<shortcut>Cmd+Q</shortcut>
	</command>
	<command kind="static" id="1004" name="Zoom In">
		<shortcut>Cmd++</shortcut>
		<shortcut>Cmd+=</shortcut>
	</command>
	<command kind="static" id="1055" name="Info">
		<shortcut>Cmd+,</shortcut>
	</command>
	<command kind="static" id="1137" name="File Info...">
		<shortcut>Control+Cmd+1</shortcut>
	</command>
	<command kind="static" id="2101" name="Print...">
		<shortcut>Control+Cmd+1</shortcut>
	</command>
	<command kind="static" id="1170" name="Actions">
		<shortcut>K</shortcut>
	</command>
	<command kind="static" id="1192" name="Fit on Screen">
		<shortcut>Cmd+Up Arrow</shortcut>
	</command>
	<command kind="static" id="1190" name="100%">
		<shortcut>F9</shortcut>
	</command>
	<command kind="static" id="1002" name="Rulers">
		<shortcut>Opt+Cmd+J</shortcut>
	</command>
	<command kind="static" id="2958" name="Group Layers">
		<shortcut>Shift+Cmd+G</shortcut>
	</command>
	<command kind="static" id="2959" name="Ungroup Layers">
		<shortcut>Cmd+G</shortcut>
	</command>
	<tool name="Pinsel" type="1" key="1886286946">K</tool>
	<tool name="Hand Tool" type="1" key="1751215716"></tool>
	<tool name="Move Tool" type="1" key="1819113074">V</tool>
	<tool name="Zoom Tool" type="1" key="2054123373">Q</tool>
	<tool name="Burn Tool" type="1" key="1651864174">O</tool>
	<tool name="Default Foreground/Background Colors" type="2">D</tool>
	<taskspace name="Select and Mask">
		<taskspace-tool name="Brush Tool" type="1" key="1886286946">B</taskspace-tool>
	</taskspace>
</photoshop-keyboard-shortcuts>
"""

/// The message of the `DoctoredCoreError` `body` throws, or nil when it throws none.
private func kysRefusal(_ body: () throws -> Void) -> String? {
    do {
        try body()
        return nil
    } catch let error as DoctoredCoreError {
        return error.message
    } catch {
        return "\(error)"
    }
}

func verifyKysImport() throws {
    let tag = "[kys import]"
    let defaultName = ShortcutBindings.libraryName
    let start = shortcutProbeDefaultEntry()?.item
    defer { restoreShortcutLibrary(defaultSet: start) }
    let (menu, delegate) = shortcutProbeMenu()
    defer { withExtendedLifetime(delegate) {} }

    // The holders' paths, read from the menu as built, before anything is applied.
    let quit = ShortcutChord(key: "q", modifiers: [.command])
    let comma = ShortcutChord(key: ",", modifiers: [.command])
    let clipping = ShortcutChord(key: "j", modifiers: [.command, .option])
    let controlOne = ShortcutChord(key: "1", modifiers: [.control, .command])
    let items = ShortcutMenu.customizableItems(in: menu)
    guard let quitPath = ShortcutMenu.conflict(quit, excluding: "saveDocument:", in: menu)?.path,
          let settingsPath = ShortcutMenu.conflict(comma, excluding: "showInfo:", in: menu)?.path,
          let fileInfoPath = items.first(where: { $0.identity == "showFileInfo:" })?.path,
          let clippingPath = items.first(where: { $0.identity == "toggleShowClipping:" })?.path,
          let f9Scalar = Unicode.Scalar(0xF704 + 8) else {
        throw ProbeFailure(description: "\(tag) a holder's menu item is missing")
    }
    let bareLetter = kysRefusal { _ = try ShortcutChord(key: "k", modifiers: []).validated() } ?? ""
    let reservedQ = kysRefusal { _ = try ToolKeys.validated("Q") } ?? ""

    // The mapping.
    let entries = try KysImport.entries(Data(probeKysText.utf8))
    let (bindings, report) = KysImport.bindings(entries, name: "Probe", menu: menu)
    let expectedOverrides: [String: ShortcutChord?] = [
        "newDocument:": ShortcutChord(key: "n", modifiers: [.option, .shift, .command]),
        "showLiquify:": ShortcutChord(key: "x", modifiers: [.control, .command]),
        "featherSelection:": nil,
        "showFileInfo:": controlOne,
        "zoomActualAction:": ShortcutChord(key: String(Character(f9Scalar)), modifiers: []),
        "groupLayers:": ShortcutChord(key: "g", modifiers: [.shift, .command]),
        "ungroupLayers:": ShortcutChord(key: "g", modifiers: [.command]),
    ]
    let expectedToolKeys: [String: String?] = ["brush": "k", "hand": nil]
    let expectedApplied = [
        "Neu...", "Liquify", "Feather...", "Zoom In", "File Info...", "100%", "Group Layers",
        "Ungroup Layers", "Pinsel", "Hand Tool", "Move Tool",
    ]
    let expectedUnmapped = [
        "Some Unknown", "Vanishing Point", "Burn Tool", "Default Foreground/Background Colors",
        "Select and Mask",
    ]
    let expectedRefused = [
        "Save: \(quit.display) is already \(quitPath).",
        "Zoom In: “Cmd+=” is a second shortcut; a command takes one here.",
        "Info: \(comma.display) is already \(settingsPath).",
        "Print...: \(controlOne.display) is already \(fileInfoPath).",
        "Actions: \(bareLetter)",
        "Fit on Screen: “Cmd+Up Arrow” is not a shortcut this import reads.",
        "Zoom Tool: \(reservedQ)",
        "Rulers: \(clipping.display) is already \(clippingPath).",
    ]
    try require(bindings.name == "Probe" && bindings.overrides == expectedOverrides,
                "\(tag) the overrides are \(bindings.overrides)")
    try require(bindings.toolKeys == expectedToolKeys, "\(tag) the tool keys are \(bindings.toolKeys)")
    try require(report.applied == expectedApplied, "\(tag) applied \(report.applied)")
    try require(report.unmapped == expectedUnmapped, "\(tag) not mapped \(report.unmapped)")
    try require(report.refused == expectedRefused, "\(tag) refused \(report.refused)")

    // Applied, Show Clipping keeps its default and the swap holds.
    ShortcutMenu.apply(bindings, to: menu)
    func chord(_ identity: String) -> ShortcutChord? {
        shortcutProbeItem(identity, in: menu).flatMap { ShortcutChord(item: $0) }
    }
    try require(chord("toggleShowClipping:") == clipping
                && chord("toggleRulers:") == ShortcutChord(key: "r", modifiers: [.command])
                && chord("groupLayers:") == ShortcutChord(key: "g", modifiers: [.shift, .command])
                && chord("ungroupLayers:") == ShortcutChord(key: "g", modifiers: [.command]),
                "\(tag) after apply Show Clipping is \(String(describing: chord("toggleShowClipping:")))")

    // importKys: a new active set named from the file, " 2" the second time, no other bytes.
    try ShortcutSets.Active.set(name: nil)
    try ShortcutLibrary.save(ShortcutBindings(), replace: true)
    let defaultURL = ShortcutLibrary.url(defaultName)
    let defaultBytes = try Data(contentsOf: defaultURL)
    let file = directory.appendingPathComponent("Probe Keys.kys")
    try Data(probeKysText.utf8).write(to: file)
    let first = try ShortcutSets.importKys(contentsOf: file, in: menu)
    let firstSet = try ShortcutLibrary.named("Probe Keys")
    try require(first.name == "Probe Keys" && ShortcutSets.activeName() == "Probe Keys"
                && first.report == report && firstSet.overrides == expectedOverrides
                && firstSet.toolKeys == expectedToolKeys && EditorTool.brush.keyCharacter == "k",
                "\(tag) the first import made \(first.name), active \(ShortcutSets.activeName())")
    let firstURL = ShortcutLibrary.url("Probe Keys")
    let firstBytes = try Data(contentsOf: firstURL)
    let second = try ShortcutSets.importKys(contentsOf: file, in: menu)
    let (defaultAfter, firstAfter) = try (Data(contentsOf: defaultURL), Data(contentsOf: firstURL))
    try require(second.name == "Probe Keys 2" && ShortcutSets.activeName() == "Probe Keys 2"
                && defaultAfter == defaultBytes && firstAfter == firstBytes
                && ShortcutSets.names() == [defaultName, "Probe Keys", "Probe Keys 2"],
                "\(tag) the second import made \(second.name), sets \(ShortcutSets.names())")

    // Whole-file refusals.
    let notKys = "This is not a keyboard shortcut (.kys) file."
    let wholeFiles: [(String, Data, String)] = [
        ("not XML", Data("hello".utf8), notKys),
        ("a wrong root", Data("<?xml version=\"1.0\"?><keyboard version=\"4\"/>".utf8), notKys),
        ("version 3",
         Data(probeKysText.replacingOccurrences(of: "version=\"4\"", with: "version=\"3\"").utf8),
         "This .kys file is version “3”; this import reads version 4."),
        ("over 1 MiB", Data(count: KysImport.maximumBytes + 1), "This .kys file is larger than 1 MiB."),
    ]
    for (what, data, sentence) in wholeFiles {
        let message = kysRefusal { _ = try KysImport.entries(data) }
        try require(message == sentence, "\(tag) \(what) answered \(message ?? "no refusal")")
    }
    let unmappedOnly = directory.appendingPathComponent("Unmapped.kys")
    try Data("""
        <?xml version="1.0" encoding="UTF-8"?>
        <photoshop-keyboard-shortcuts version="4">
        <tool name="Burn Tool" type="1" key="1651864174">O</tool>
        </photoshop-keyboard-shortcuts>
        """.utf8).write(to: unmappedOnly)
    let nothing = kysRefusal { _ = try ShortcutSets.importKys(contentsOf: unmappedOnly, in: menu) }
    try require(nothing == "Nothing in “Unmapped.kys” maps onto a command or tool here."
                && ShortcutSets.names().count == 3,
                "\(tag) a file of which nothing maps answered \(nothing ?? "no refusal")")

    // The tables.
    let identities = Set(items.map(\.identity))
    let commands = Array(KysImport.staticCommands.values) + Array(KysImport.dynamicCommands.values)
    let tools = Array(KysImport.tools.values)
    let toolIdentifiers = Set(EditorTool.allCases.map(\.identifier))
    let strayCommands = commands.filter { !identities.contains($0) }.sorted()
    let strayTools = tools.filter { !toolIdentifiers.contains($0) }.sorted()
    try require(strayCommands.isEmpty && strayTools.isEmpty,
                "\(tag) the tables name \(strayCommands) and \(strayTools)")
    try require(Set(commands).count == commands.count && Set(tools).count == tools.count,
                "\(tag) a table value repeats")
    print("PASS: \(tag) \(report.applied.count) entries applied, \(report.unmapped.count) not "
          + "mapped and \(report.refused.count) refused by sentence (Rulers at Show Clipping's "
          + "default among them, the Group/Ungroup swap kept); importKys writes a new active set "
          + "and a second one, changing no other bytes; whole files refused; \(commands.count) "
          + "commands and \(tools.count) tools in the tables")
}
