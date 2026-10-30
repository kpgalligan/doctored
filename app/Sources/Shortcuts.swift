import AppKit

// `ShortcutMenu`'s identity is the SELECTOR (+`#tag`), never the title, and the DEFAULTS are a
// launch-time snapshot of what `AppDelegate.buildMainMenu()` produced, so the file holds only the
// user's overrides and a user's choice survives a change of defaults (a default an override now
// holds is withheld, `apply`; `designs/v1-shortcuts-fade.md`); `makeMainMenu()` = build →
// `installStored` applied to the LIVE items, which is why the command palette needs no change. A
// chord must contain ⌘ or be F1–F19, conflicts are checked against the WHOLE walk (app menu, system
// selectors and hidden alternates included) and refused naming the holder, and items with a
// `representedObject` — the rebuilt Workspaces and Open Recent lists — are never offered. The
// shortcut and workspace libraries are both `PreferenceLibrary` codecs (`DR_SHORTCUTS_DIR` 4 × 32
// KiB, `DR_WORKSPACES_DIR` 32 × 4 KiB); their writes are PREFERENCES, outside document Undo and
// never Action steps.

extension Notification.Name {
    static let shortcutLibraryDidChange = Notification.Name("ShortcutLibraryDidChange")
}

/// Customizable menu shortcuts (Phase 12C-2): the chord, the stored override
/// set, its `PreferenceLibrary` codec, and the menu walk that applies it.
///
/// **Identity is the SELECTOR, never the title.** A binding keyed on
/// "Rotate 90° Clockwise" is orphaned the day that item is renamed or
/// localised; `NSStringFromSelector(action)` plus the item's `tag` (which is
/// how every tag-indexed menu in this app tells its items apart) survives
/// both. The file therefore holds a map of identity → chord and nothing else.
///
/// **The defaults are a launch-time SNAPSHOT of the menu this build made.**
/// Nothing in the file describes a default, so a build that moves a default
/// (v1 moved them, `designs/v1-shortcuts-fade.md`) leaves every
/// override standing — `resetAll` restores exactly what
/// `AppDelegate.buildMainMenu()` produced, an override always wins, and a new
/// default that an override already holds elsewhere is withheld (`apply`).
struct ShortcutChord: Equatable, Hashable {
    /// Always lower-cased; Shift is carried in `modifiers`, never in the
    /// letter's case, so "R" + no shift and "r" + shift cannot both exist.
    let key: String
    let modifiers: NSEvent.ModifierFlags

    /// The four a menu may carry. Everything else in a mask (function-key,
    /// numeric-pad, caps lock) is display state, not a chord.
    static let allModifiers: NSEvent.ModifierFlags = [.command, .option, .control, .shift]

    /// The names the file and the sheet speak, in the order they are drawn.
    static let modifierNames: [(name: String, flag: NSEvent.ModifierFlags)] = [
        ("control", .control), ("option", .option), ("shift", .shift), ("command", .command),
    ]

    init(key: String, modifiers: NSEvent.ModifierFlags) {
        var flags = modifiers.intersection(Self.allModifiers)
        var text = key
        if text.count == 1, let character = text.first, character.isUppercase {
            flags.insert(.shift)
            text = text.lowercased()
        }
        self.key = text
        self.modifiers = flags
    }

    /// The chord a menu item currently carries, or nil when it has none.
    init?(item: NSMenuItem) {
        guard !item.keyEquivalent.isEmpty else { return nil }
        self.init(key: item.keyEquivalent, modifiers: item.keyEquivalentModifierMask)
    }

    /// The chord a key-down event spells. Modifier-only events have no
    /// character and answer nil.
    init?(event: NSEvent) {
        guard let characters = event.charactersIgnoringModifiers, characters.count == 1 else {
            return nil
        }
        self.init(key: characters, modifiers: event.modifierFlags)
    }

    init(json: [String: Any]) throws {
        guard Set(json.keys) == ["key", "modifiers"], let key = json["key"] as? String,
              key.count == 1, let names = json["modifiers"] as? [String] else {
            throw DoctoredCoreError(
                message: "A shortcut is {key: one character, modifiers: [control|option|shift|command]}.")
        }
        var flags: NSEvent.ModifierFlags = []
        for name in names {
            guard let match = Self.modifierNames.first(where: { $0.name == name }) else {
                throw DoctoredCoreError(message: "“\(name)” is not a shortcut modifier.")
            }
            flags.insert(match.flag)
        }
        self.init(key: key, modifiers: flags)
    }

    var json: [String: Any] {
        ["key": key,
         "modifiers": Self.modifierNames.filter { modifiers.contains($0.flag) }.map(\.name)]
    }

    /// The chord as the menu itself draws it — "⌥⇧⌘R".
    var display: String { CommandPalette.shortcutText(key: key, modifiers: modifiers) }

    /// The F-keys, which are the one family a menu shortcut may use without
    /// Command: ⌃F is already Repeat Last Filter, and a bare letter belongs to
    /// the canvas's tool keys.
    private static let functionKeys: Set<String> = {
        var keys: Set<String> = []
        for offset in 0..<19 {
            guard let scalar = Unicode.Scalar(0xF704 + offset) else { continue }
            keys.insert(String(Character(scalar)))
        }
        return keys
    }()

    var isFunctionKey: Bool { Self.functionKeys.contains(key) }

    /// A chord a menu may be given. Without Command (or an F-key) it would
    /// shadow typing: a menu key equivalent is resolved AHEAD of the first
    /// responder, which is the hazard Edit ▸ Clear's bare ⌫ documents.
    func validated() throws -> ShortcutChord {
        guard modifiers.contains(.command) || isFunctionKey else {
            throw DoctoredCoreError(
                message: "A shortcut must include ⌘, or be a function key (F1–F19): without one "
                    + "it would be taken from every text field in the app.")
        }
        return self
    }

    static func == (left: ShortcutChord, right: ShortcutChord) -> Bool {
        left.key == right.key && left.modifiers.rawValue == right.modifiers.rawValue
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(key)
        hasher.combine(modifiers.rawValue)
    }
}

/// The user's overrides, and nothing else. A value of `nil` for an identity is
/// DELIBERATE: it means "this command has no shortcut", which is different
/// from "this command was never edited" (its key being absent).
struct ShortcutBindings: Equatable {
    static let schemaVersion = 1
    /// The library holds exactly one item, under this name.
    static let libraryName = "Shortcuts"

    var overrides: [String: ShortcutChord?] = [:]

    var name: String { Self.libraryName }

    var json: [String: Any] {
        var map: [String: Any] = [:]
        for (identity, chord) in overrides {
            map[identity] = chord?.json ?? NSNull()
        }
        return ["schema_version": Self.schemaVersion, "name": Self.libraryName, "overrides": map]
    }

    static func decode(_ object: [String: Any]) throws -> ShortcutBindings {
        guard Set(object.keys) == ["schema_version", "name", "overrides"],
              object["name"] as? String == libraryName,
              let map = object["overrides"] as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid or unsupported shortcut set.")
        }
        guard map.count <= 512 else {
            throw DoctoredCoreError(message: "A shortcut set holds at most 512 overrides.")
        }
        var overrides: [String: ShortcutChord?] = [:]
        for (identity, value) in map {
            guard !identity.isEmpty, identity.count <= 200 else {
                throw DoctoredCoreError(message: "Invalid or unsupported shortcut set.")
            }
            if value is NSNull {
                overrides.updateValue(nil, forKey: identity)
                continue
            }
            guard let chord = value as? [String: Any] else {
                throw DoctoredCoreError(message: "Invalid or unsupported shortcut set.")
            }
            overrides.updateValue(try ShortcutChord(json: chord), forKey: identity)
        }
        // A command renamed since the set was saved (`LegacyNames.shortcutIdentities`) keeps its
        // override under its current identity, unless the set also binds that identity, which
        // wins. A re-save writes the current identity only.
        for (old, current) in LegacyNames.shortcutIdentities {
            guard let chord = overrides.removeValue(forKey: old) else { continue }
            if overrides[current] == nil { overrides.updateValue(chord, forKey: current) }
        }
        return ShortcutBindings(overrides: overrides)
    }
}

/// The shortcut set's codec on the shared `PreferenceLibrary` contract. Four
/// files rather than one so a stray copy is a diagnosed `broken` row instead
/// of a refusal of the whole folder; only `"Shortcuts"` is ever read.
enum ShortcutCodec: PreferenceLibraryCodec {
    typealias Item = ShortcutBindings

    static let noun = "shortcut set"
    static let directoryEnvironment = "DR_SHORTCUTS_DIR"
    static let directoryName = "Doctored/Shortcuts"
    static let maximumEntries = 4
    static let maximumFileBytes = 32 * 1024
    static let schemaVersions = ShortcutBindings.schemaVersion...ShortcutBindings.schemaVersion
    static let didChange = Notification.Name.shortcutLibraryDidChange

    static func name(of item: ShortcutBindings) -> String { item.name }
    static func json(of item: ShortcutBindings) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> ShortcutBindings {
        try ShortcutBindings.decode(object)
    }
}

typealias ShortcutLibrary = PreferenceLibrary<ShortcutCodec>

/// The menu walk: which items may be rebound, what they are called, what they
/// were, and the four operations the sheet runs.
enum ShortcutMenu {
    /// One customizable command.
    struct Item {
        let identity: String
        /// "View ▸ Rulers" — for the sheet's first column and every refusal.
        let path: String
        let menuItem: NSMenuItem
        let defaultChord: ShortcutChord?
        var chord: ShortcutChord?
    }

    /// The commands macOS itself owns. Rebinding ⌘Q or ⌘W is a foot-gun with
    /// no upside, and the Settings item's own ⌘, is the shortcut a user would
    /// need to get back to this sheet.
    static let systemSelectors: Set<String> = [
        "hide:", "hideOtherApplications:", "unhideAllApplications:", "terminate:",
        "orderFrontStandardAboutPanel:", "showSettings:", "minimizeWindow:",
        "performZoom:", "arrangeInFront:", "showHelp:", "undo:", "redo:", "performClose:",
    ]

    /// Menu nesting the walk follows — `CommandPalette`'s bound, for its
    /// reason.
    private static let maximumDepth = 6

    /// The launch-time defaults, keyed by identity. `nil` for a command that
    /// ships without a shortcut, which is most of them.
    private static var defaults: [String: ShortcutChord?] = [:]
    private static var didSnapshot = false

    /// The chord each menu item carried when the walk FIRST saw it — the
    /// builder's chord, before any `apply` could withhold it. It covers the
    /// items `defaults` does not (the app menu, the system selectors, hidden
    /// twins) and an item of a menu built after the snapshot, so a withheld
    /// chord can always come back. Weak keys: a rebuilt menu's rows go away.
    private final class BuilderChord {
        let chord: ShortcutChord?
        init(_ chord: ShortcutChord?) { self.chord = chord }
    }
    private static let builderChords = NSMapTable<NSMenuItem, BuilderChord>.weakToStrongObjects()

    /// `item`'s builder chord, recorded on first sight.
    private static func builderChord(of item: NSMenuItem) -> ShortcutChord? {
        if let known = builderChords.object(forKey: item) { return known.chord }
        let chord = ShortcutChord(item: item)
        builderChords.setObject(BuilderChord(chord), forKey: item)
        return chord
    }

    static func identity(of item: NSMenuItem) -> String {
        guard let action = item.action else { return "" }
        return NSStringFromSelector(action) + (item.tag != 0 ? "#\(item.tag)" : "")
    }

    /// Every command the sheet offers, in menu order.
    ///
    /// Five exclusions, each for its own reason: the APP menu (macOS owns its
    /// shape), the `systemSelectors`, submenu PARENTS and separators (no
    /// command), HIDDEN and ALTERNATE items (the hidden ⌘= Zoom In is one
    /// command's second spelling and keeps its default), and anything
    /// carrying a `representedObject` — the Workspaces and Open Recent lists
    /// are rebuilt from the library on every menu update, so a binding on one
    /// of their rows would name a row that may not exist tomorrow.
    static func customizableItems(in menu: NSMenu) -> [Item] {
        var out: [Item] = []
        // The app menu is the first top-level item; everything under it is
        // macOS's own.
        for top in menu.items.dropFirst() {
            guard let submenu = top.submenu else { continue }
            collect(submenu, path: [top.title], into: &out, depth: 1)
        }
        return out
    }

    private static func collect(
        _ menu: NSMenu, path: [String], into out: inout [Item], depth: Int
    ) {
        guard depth < maximumDepth else { return }
        for item in menu.items {
            if item.isSeparatorItem || item.isHidden || item.isAlternate { continue }
            if let submenu = item.submenu {
                collect(submenu, path: path + [item.title], into: &out, depth: depth + 1)
                continue
            }
            guard let action = item.action, !item.title.isEmpty,
                  item.representedObject == nil,
                  !systemSelectors.contains(NSStringFromSelector(action)) else { continue }
            let key = identity(of: item)
            let chord = ShortcutChord(item: item)
            let fallback = builderChord(of: item)
            out.append(Item(identity: key, path: (path + [item.title]).joined(separator: " ▸ "),
                            menuItem: item, defaultChord: defaults[key] ?? fallback, chord: chord))
        }
    }

    /// Records what this build's own menu builder produced, ONCE and BEFORE
    /// anything is applied — a second snapshot taken over an applied override
    /// would enshrine the override as the default.
    static func snapshotDefaults(of menu: NSMenu) {
        guard !didSnapshot else { return }
        didSnapshot = true
        for item in customizableItems(in: menu) {
            defaults.updateValue(ShortcutChord(item: item.menuItem), forKey: item.identity)
        }
    }

    /// The stored set, plus whatever the library could not read. A broken file
    /// applies NOTHING: a half-read override set is worse than the defaults.
    static func stored() -> (bindings: ShortcutBindings, broken: [PreferenceLibraryBroken]) {
        do {
            let listing = try ShortcutLibrary.listing()
            let item = listing.entries.first { $0.item.name == ShortcutBindings.libraryName }
            return (item?.item ?? ShortcutBindings(), listing.broken)
        } catch {
            return (ShortcutBindings(),
                    [PreferenceLibraryBroken(file: ShortcutBindings.libraryName,
                                             fault: .unreadable,
                                             reason: error.localizedDescription,
                                             repairable: false)])
        }
    }

    /// Establishes every customizable item's chord: its override when it has
    /// one, its snapshot default otherwise. Full rather than incremental, so
    /// removing an override is the same operation as adding one. Returns the
    /// identities the file names that this menu does not have.
    ///
    /// **Overrides always win.** A DEFAULT chord — a customizable item's
    /// snapshot default, or the builder chord of any other item (the app
    /// menu, the system selectors, hidden twins) — that an override assigns
    /// to a different identity is WITHHELD: the item gets no chord, so one
    /// chord never fires two commands. That is how a user who bound, say,
    /// ⌘E to Export before v1 moved ⌘E to Merge Down keeps their choice. The
    /// withheld default comes back on a later `apply` once no override holds
    /// it, since every call recomputes from the defaults.
    @discardableResult
    static func apply(_ bindings: ShortcutBindings, to menu: NSMenu) -> [String] {
        var held: [ShortcutChord: Set<String>] = [:]
        for (identity, chord) in bindings.overrides {
            guard let chord else { continue }
            held[chord, default: []].insert(identity)
        }
        func withheld(_ chord: ShortcutChord?, from identity: String) -> Bool {
            guard let chord, let holders = held[chord] else { return false }
            return !holders.subtracting([identity]).isEmpty
        }
        var seen: Set<String> = []
        var customizable: Set<ObjectIdentifier> = []
        for item in customizableItems(in: menu) {
            seen.insert(item.identity)
            customizable.insert(ObjectIdentifier(item.menuItem))
            let chord: ShortcutChord?
            if let override = bindings.overrides[item.identity] {
                chord = override
            } else if withheld(item.defaultChord, from: item.identity) {
                chord = nil
            } else {
                chord = item.defaultChord
            }
            set(chord, on: item.menuItem)
        }
        // Every other item keeps its builder chord unless an override holds it.
        func walk(_ menu: NSMenu, _ depth: Int) {
            guard depth < maximumDepth else { return }
            for item in menu.items {
                if item.isSeparatorItem { continue }
                if let submenu = item.submenu {
                    walk(submenu, depth + 1)
                    continue
                }
                guard item.action != nil,
                      !customizable.contains(ObjectIdentifier(item)) else { continue }
                let builder = builderChord(of: item)
                let chord = withheld(builder, from: identity(of: item)) ? nil : builder
                if ShortcutChord(item: item) != chord { set(chord, on: item) }
            }
        }
        walk(menu, 0)
        return bindings.overrides.keys.filter { !seen.contains($0) }.sorted()
    }

    private static func set(_ chord: ShortcutChord?, on item: NSMenuItem) {
        item.keyEquivalent = chord?.key ?? ""
        item.keyEquivalentModifierMask = chord?.modifiers ?? []
    }

    /// What `makeMainMenu()` runs: snapshot, then apply what is on disk.
    static func installStored(to menu: NSMenu) {
        snapshotDefaults(of: menu)
        apply(stored().bindings, to: menu)
    }

    /// Who already holds `chord`, if anyone. The WHOLE menu is walked,
    /// including the app menu, the system selectors and hidden alternates: a
    /// chord that collides with ⌘Q collides whether or not this sheet offers
    /// ⌘Q for editing.
    ///
    /// **The comparison is EXACT, and a measurement says that is RIGHT**
    /// (12E-1b). 12D-2 measured one item alone — an item at ⌘R appeared to
    /// fire on a ⇧⌘R press — and inferred that either half of an `X` / `⇧X`
    /// pair would swallow the other. 12E-1b measured the two-item case in
    /// BOTH event shapes (`scripts/native-paint-probe/workspace.swift`,
    /// `shortcut-shift-rule-ui`, M1–M7) and the inference did NOT survive the
    /// second one:
    ///
    /// * the leniency appears only for a press hand-built with
    ///   `NSEvent.keyEvent` carrying a LOWER-CASE
    ///   `charactersIgnoringModifiers` under Shift — a shape no keyboard
    ///   delivers, since that property ignores every modifier EXCEPT Shift;
    /// * on the shape the window server itself produces (`CGEvent` →
    ///   `NSEvent(cgEvent:)`, the translation a real press runs) a key
    ///   equivalent matches the chord EXACTLY: a ⇧⌘R press does not fire a
    ///   ⌘R item (M1), a ⌘R press does not fire a ⇧⌘R item (M6), and where
    ///   both exist the matching one fires wherever it sits in the walk
    ///   (M2/M4a);
    /// * on the app's OWN main menu a hardware-shaped ⇧⌘Z press matches Edit
    ///   ▸ Redo (M7), so the shipped ⌘Z/⇧⌘Z, ⌘S/⇧⌘S and ⌘E/⇧⌘E pairs are not
    ///   shadowed.
    ///
    /// So ⌘R and ⇧⌘R are two chords to AppKit exactly as they are to this
    /// predicate, and nothing here needs to change. The table is in
    /// `archive/phase-12e-layer-ops-status.md`.
    static func conflict(
        _ chord: ShortcutChord, excluding identity: String, in menu: NSMenu
    ) -> (path: String, item: NSMenuItem)? {
        var found: (path: String, item: NSMenuItem)?
        func walk(_ menu: NSMenu, _ path: [String], _ depth: Int) {
            guard depth < maximumDepth, found == nil else { return }
            for item in menu.items {
                if found != nil { return }
                if item.isSeparatorItem { continue }
                if let submenu = item.submenu {
                    walk(submenu, path + [item.title], depth + 1)
                    continue
                }
                guard item.action != nil, ShortcutChord(item: item) == chord,
                      ShortcutMenu.identity(of: item) != identity else { continue }
                found = ((path + [item.title]).joined(separator: " ▸ "), item)
                return
            }
        }
        walk(menu, [], 0)
        return found
    }

    /// The sheet's one write: validate, refuse a collision by NAME, apply to
    /// the live menu items, and store the whole override set.
    static func assign(_ chord: ShortcutChord?, to identity: String, in menu: NSMenu) throws {
        var bindings = stored().bindings
        if let chord {
            let valid = try chord.validated()
            if let holder = conflict(valid, excluding: identity, in: menu) {
                throw DoctoredCoreError(
                    message: "\(valid.display) is already \(holder.path).")
            }
            bindings.overrides.updateValue(valid, forKey: identity)
        } else {
            bindings.overrides.updateValue(nil, forKey: identity)
        }
        apply(bindings, to: menu)
        try ShortcutLibrary.save(bindings, replace: true)
    }

    /// Drops one override, putting the launch-time default back.
    static func reset(_ identity: String, in menu: NSMenu) throws {
        var bindings = stored().bindings
        bindings.overrides.removeValue(forKey: identity)
        apply(bindings, to: menu)
        try ShortcutLibrary.save(bindings, replace: true)
    }

    /// Drops every override. The snapshot is what comes back, so this restores
    /// exactly the menu this build ships.
    static func resetAll(in menu: NSMenu) throws {
        let bindings = ShortcutBindings()
        apply(bindings, to: menu)
        try ShortcutLibrary.save(bindings, replace: true)
    }
}
