import AppKit

/// The bare keys that select tools: this build's defaults (`EditorTool.defaultKeyCharacter`) under
/// the user's overrides, read through `EditorTool.keyCharacter` by every reader — the canvas's
/// `keyDown`, the rail's tooltips and group menus, the status line.
///
/// **Stored beside the menu chords.** The overrides are the optional `tool_keys` map of the
/// active set's item (`ShortcutBindings`, `ShortcutSets`), identifier → key, or null for a key the
/// user removed, so switching sets switches the tool keys with the menu chords
/// (`designs/shortcut-sets.md`). Each write rewrites the whole item and keeps the menu half as
/// it is.
///
/// **Which keys a tool may take.** One lowercase letter or digit, other than the canvas's own bare
/// keys (`reservedKeys`). Two tools may share a key: sharing is the cycle rule that already lets
/// E, J or M step through a slot, so there is no conflict to refuse. Punctuation stays the canvas's
/// (the brush-size brackets, the preset `,` `.`, the Magnetic Lasso's option keys).
///
/// A preference: no document Undo, no Action step, and no MCP twin — the view-preference
/// exemption every shortcut shares (app/CLAUDE.md).
final class ToolKeys {
    static let shared = ToolKeys()

    /// The canvas's own bare keys: Quick Mask, the screen modes, the colour swap and the default
    /// colours.
    static let reservedKeys: Set<String> = ["q", "f", "x", "d"]

    /// identifier → key, or nil for a key the user removed. Empty means every default.
    private(set) var overrides: [String: String?] = [:]

    /// The key `tool` answers to: its override, else its default; "" when removed.
    func key(for tool: EditorTool) -> String {
        if let override = overrides[tool.identifier] { return override ?? "" }
        return tool.defaultKeyCharacter
    }

    /// The set read from a library item.
    func load(_ bindings: ShortcutBindings) {
        overrides = bindings.toolKeys
    }

    /// `key`, lowercased, when a tool may take it; otherwise the sentence saying why not.
    static func validated(_ key: String) throws -> String {
        let lower = key.lowercased()
        let letters: ClosedRange<Unicode.Scalar> = "a"..."z"
        let digits: ClosedRange<Unicode.Scalar> = "0"..."9"
        guard lower.unicodeScalars.count == 1, let scalar = lower.unicodeScalars.first,
              letters.contains(scalar) || digits.contains(scalar) else {
            throw DoctoredCoreError(
                message: "A tool key is one letter or digit; “\(key)” is not.")
        }
        guard !reservedKeys.contains(lower) else {
            throw DoctoredCoreError(
                message: "“\(lower.uppercased())” is the canvas's own key (Q Quick Mask, F screen "
                    + "modes, X swap colours, D default colours), so no tool may take it.")
        }
        return lower
    }

    /// Gives `tool` the key `key`, or removes its key when `key` is nil, and stores the set.
    func assign(_ key: String?, to tool: EditorTool) throws {
        let value = try key.map(Self.validated)
        var bindings = ShortcutMenu.stored().bindings
        if value == tool.defaultKeyCharacter {
            bindings.toolKeys.removeValue(forKey: tool.identifier)
        } else {
            bindings.toolKeys.updateValue(value, forKey: tool.identifier)
        }
        try store(bindings)
    }

    /// Puts `tool`'s default key back.
    func reset(_ tool: EditorTool) throws {
        var bindings = ShortcutMenu.stored().bindings
        bindings.toolKeys.removeValue(forKey: tool.identifier)
        try store(bindings)
    }

    /// Puts every tool's default key back; the menu chords are kept.
    func resetAll() throws {
        var bindings = ShortcutMenu.stored().bindings
        bindings.toolKeys = [:]
        try store(bindings)
    }

    /// The new set is current BEFORE `save`, because `save` posts `.shortcutLibraryDidChange`
    /// before it returns and its observers (the rail's tooltips) read `key(for:)` inside that
    /// post; a failed save puts the previous set back, and posts nothing.
    private func store(_ bindings: ShortcutBindings) throws {
        let previous = overrides
        overrides = bindings.toolKeys
        do {
            try ShortcutLibrary.save(bindings, replace: true)
        } catch {
            overrides = previous
            throw error
        }
    }
}
