import AppKit

// "tool keys" (#20): the bare tool keys are remappable (`ToolKeys`). Every tool starts on its
// default; a remap reaches `EditorTool(keyCharacter:)`, `EditorTool.tool(forKey:…)` and an open
// rail's tooltip (Reset and Reset All too) and leaves the old letter's other peers alone; a
// removed key answers "" and selects nothing; a reserved, punctuation or two-character key is
// refused by sentence; the set is stored in the "Shortcuts" item beside the menu overrides (each
// half's writes keep the other), survives a fresh read (the relaunch) and a set without tool keys
// keeps its bytes. The library is the runner's private DR_SHORTCUTS_DIR; the process-wide
// `ToolKeys.shared` and the library are put back on every exit, because other cases read the
// default letters.

func verifyToolKeys() throws {
    let tag = "[tool keys]"
    let keys = ToolKeys.shared
    let start = ShortcutMenu.stored().bindings
    defer {
        _ = try? ShortcutLibrary.save(start, replace: true)
        keys.load(start)
    }
    let rotate = ShortcutChord(key: "m", modifiers: [.command, .control])
    try ShortcutLibrary.save(ShortcutBindings(overrides: ["rotateCW:": rotate]), replace: true)
    keys.load(ShortcutMenu.stored().bindings)

    // Defaults.
    let off = EditorTool.allCases.filter { $0.keyCharacter != $0.defaultKeyCharacter }
    try require(off.isEmpty, "\(tag) tools off their default before any remap: \(off)")

    // A rail, its tooltips built on the defaults (the Brush leads its slot; selecting the Move
    // tool rebuilds them). A remap must reach them through the library's change notification,
    // which `save` posts before it returns.
    let rail = ToolRailView(groups: EditorTool.railGroups)
    rail.setSelectedTool(.move)
    func brushTips() -> [String] {
        rail.toolTipOwners.map { $0 as String }.filter { $0.hasPrefix("Brush (") }
    }
    try require(brushTips() == ["Brush (B)"], "\(tag) the rail starts on \(brushTips())")

    // A remap: Brush to K. The Pencil keeps B alone.
    try keys.assign("k", to: .brush)
    try require(EditorTool.brush.keyCharacter == "k" && EditorTool.brush.keyLabel == "K",
                "\(tag) Brush reads \(EditorTool.brush.keyCharacter)")
    try require(brushTips() == ["Brush (K)"],
                "\(tag) the rail's tooltip after the remap reads \(brushTips())")
    try require(EditorTool(keyCharacter: "k") == .brush,
                "\(tag) K selects \(String(describing: EditorTool(keyCharacter: "k")))")
    try require(EditorTool.tool(forKey: "b", current: .move, last: nil) == .pencil
                && EditorTool.tool(forKey: "b", current: .pencil, last: nil) == .pencil,
                "\(tag) B selects the Pencil alone")
    try require(EditorTool.tool(forKey: "k", current: .move, last: nil) == .brush,
                "\(tag) a K press resolves to the Brush")
    // The shared keys added since the defaults were set still cycle.
    try require(EditorTool.tool(forKey: "e", current: .eraser, last: nil) == .magicEraser
                && EditorTool.tool(forKey: "e", current: .magicEraser, last: nil) == .backgroundEraser
                && EditorTool.tool(forKey: "a", current: .pathSelect, last: nil) == .directSelect,
                "\(tag) E and A cycle their peers")

    // A removed key.
    try keys.assign(nil, to: .hand)
    try require(EditorTool.hand.keyCharacter == "" && EditorTool.hand.keyLabel == nil,
                "\(tag) the Hand's key reads “\(EditorTool.hand.keyCharacter)”")
    try require(EditorTool(keyCharacter: "h") == nil
                && EditorTool.tool(forKey: "h", current: .move, last: nil) == nil
                && EditorTool(keyCharacter: "") == nil,
                "\(tag) H and the empty key select nothing")

    // Refusals by sentence, the set unchanged.
    let refusals: [(String, String)] = [
        ("q", "canvas's own key"), ("x", "canvas's own key"), (",", "one letter or digit"),
        ("kk", "one letter or digit"),
    ]
    for (key, sentence) in refusals {
        do {
            try keys.assign(key, to: .zoom)
            throw ProbeFailure(description: "\(tag) “\(key)” was accepted")
        } catch let error as DoctoredCoreError {
            try require(error.message.contains(sentence),
                        "\(tag) “\(key)” refused with “\(error.message)”")
        }
    }
    try require(EditorTool.zoom.keyCharacter == "z", "\(tag) a refusal left Zoom on its key")

    // The relaunch: a fresh read of the library into a fresh set.
    let stored = ShortcutMenu.stored()
    try require(stored.broken.isEmpty, "\(tag) the library reads clean: \(stored.broken)")
    let fresh = ToolKeys()
    fresh.load(stored.bindings)
    try require(fresh.key(for: .brush) == "k" && fresh.key(for: .hand) == ""
                && fresh.key(for: .pencil) == "b",
                "\(tag) a fresh read gives K, none and B")
    try require(stored.bindings.overrides["rotateCW:"] == .some(rotate),
                "\(tag) the menu override saved before the tool keys is still there")
    // The menu half's own write keeps the tool keys.
    try ShortcutMenu.resetAll(in: NSMenu())
    let afterMenuReset = ShortcutMenu.stored().bindings
    try require(afterMenuReset.toolKeys["brush"] == .some("k")
                && afterMenuReset.overrides.isEmpty,
                "\(tag) Reset All of the menu keeps the tool keys")

    // A set without tool keys keeps its bytes; one with them reads them back.
    let plain = ShortcutBindings(overrides: ["rotateCW:": rotate])
    try require(plain.json["tool_keys"] == nil, "\(tag) no tool keys, no tool_keys key")
    let options: JSONSerialization.WritingOptions = [.sortedKeys, .withoutEscapingSlashes]
    let plainBytes = try JSONSerialization.data(withJSONObject: plain.json, options: options)
    let reread = try ShortcutBindings.decode(plain.json)
    let rereadBytes = try JSONSerialization.data(withJSONObject: reread.json, options: options)
    try require(rereadBytes == plainBytes,
                "\(tag) a set without tool keys re-encodes to the same bytes")
    var object = plain.json
    object["tool_keys"] = ["brush": "k", "hand": NSNull(), "aLaterBuildsTool": "7"]
    let decoded = try ShortcutBindings.decode(object)
    try require(decoded.toolKeys["brush"] == .some("k") && decoded.toolKeys["hand"] == .some(nil)
                && decoded.toolKeys["aLaterBuildsTool"] == .some("7"),
                "\(tag) tool_keys decode, an unknown tool's key kept: \(decoded.toolKeys)")
    let malformed: [Any] = [["brush": "q"], ["brush": 5], ["": "k"], "k"]
    for bad in malformed {
        object["tool_keys"] = bad
        do {
            _ = try ShortcutBindings.decode(object)
            throw ProbeFailure(description: "\(tag) tool_keys \(bad) decoded")
        } catch is DoctoredCoreError {}
    }

    // Reset puts the default back.
    try keys.reset(.brush)
    try require(brushTips() == ["Brush (B)"],
                "\(tag) the rail's tooltip after Reset reads \(brushTips())")
    try keys.assign("7", to: .brush)
    try keys.resetAll()
    try require(EditorTool.brush.keyCharacter == "b" && EditorTool.hand.keyCharacter == "h",
                "\(tag) reset and Reset All restore B and H")
    try require(brushTips() == ["Brush (B)"],
                "\(tag) the rail's tooltip after Reset All reads \(brushTips())")
    print("PASS: \(tag) a remap reaches the key lookup, the cycle and the rail's tooltip, a "
          + "removed key selects nothing, reserved and malformed keys are refused, the set "
          + "survives a fresh read beside the menu overrides, and a set without tool keys keeps "
          + "its bytes")
}
