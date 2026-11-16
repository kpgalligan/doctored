import AppKit

/// Named shortcut sets (`designs/shortcut-sets.md`): each set is one item of the shortcut
/// `PreferenceLibrary` (`ShortcutBindings`, its menu overrides and tool keys together), and the
/// ACTIVE set's name lives in the library's hidden `.default` file
/// (`PreferenceLibraryDefault<ShortcutCodec>`). The default set, `"Shortcuts"`, is active exactly
/// when `.default` is absent or names no listed set, so a deleted set falls back to it, and a fresh
/// folder (no file at all until the first edit) is the default set with no overrides.
///
/// Every edit writes the active set (`ShortcutMenu.stored()` reads it). Switching applies the set
/// through the full `ShortcutMenu.apply` and `ToolKeys.load`. A preference: no document Undo, no
/// Action step and no MCP twin (app/CLAUDE.md, "View preferences").
enum ShortcutSets {
    typealias Active = PreferenceLibraryDefault<ShortcutCodec>

    /// The default set's name.
    static var defaultName: String { ShortcutBindings.libraryName }

    private static func isDefault(_ name: String) -> Bool {
        ShortcutLibrary.key(name) == ShortcutLibrary.key(defaultName)
    }

    /// The default set first, on disk or not, then the listed sets in the library's order. A
    /// listing fault answers the default set alone.
    static func names() -> [String] {
        guard let listing = try? ShortcutLibrary.listing() else { return [defaultName] }
        return [defaultName] + listing.entries.map(\.item.name).filter { !isDefault($0) }
    }

    /// The active set's name, as the listed entry spells it.
    static func activeName() -> String {
        (try? Active.name()) ?? ShortcutBindings.libraryName
    }

    /// A set's stored bindings. The default set is its listed entry, or empty bindings while no
    /// default file lists (never `ShortcutLibrary.named`, which throws then).
    private static func bindings(of name: String) throws -> ShortcutBindings {
        guard isDefault(name) else { return try ShortcutLibrary.named(name) }
        let listing = try? ShortcutLibrary.listing()
        return listing?.entries.first { isDefault($0.item.name) }?.item ?? ShortcutBindings()
    }

    /// Makes `name` the active set. The menu and `ToolKeys` take the set FIRST and the pointer is
    /// written after, because the write posts `.shortcutLibraryDidChange` and the rail's tooltips
    /// read `ToolKeys` inside that post (the `ToolKeys.store` rule). A failed write puts the
    /// previous set back on both and rethrows.
    static func activate(_ name: String, in menu: NSMenu) throws {
        let normalized = try ShortcutLibrary.normalizedName(name)
        let target = try bindings(of: normalized)
        let previous = ShortcutMenu.stored().bindings
        ToolKeys.shared.load(target)
        ShortcutMenu.apply(target, to: menu)
        do {
            try Active.set(name: isDefault(normalized) ? nil : normalized)
        } catch {
            ToolKeys.shared.load(previous)
            ShortcutMenu.apply(previous, to: menu)
            throw error
        }
    }

    private static func refuseDefaultName(_ name: String) throws {
        guard !isDefault(name) else {
            throw DoctoredCoreError(message: "“\(defaultName)” is the default set's name.")
        }
    }

    private static func alreadyExists(_ name: String) -> DoctoredCoreError {
        DoctoredCoreError(message: "A shortcut set named “\(name)” already exists.")
    }

    /// New Set…: an empty set under `name`, made active.
    static func create(_ name: String, in menu: NSMenu) throws {
        let normalized = try ShortcutLibrary.normalizedName(name)
        try refuseDefaultName(normalized)
        try ShortcutLibrary.save(ShortcutBindings(name: normalized), replace: false)
        try activate(normalized, in: menu)
    }

    /// Duplicate Set…: the active set's bindings under `name`, made active.
    static func duplicate(as name: String, in menu: NSMenu) throws {
        let normalized = try ShortcutLibrary.normalizedName(name)
        try refuseDefaultName(normalized)
        var copy = ShortcutMenu.stored().bindings
        copy.name = normalized
        try ShortcutLibrary.save(copy, replace: false)
        try activate(normalized, in: menu)
    }

    /// Rename Set…. A case-only rename rewrites the set's own file. Otherwise the new name is
    /// saved and the old one deleted; at the library's bound (where a new name's `save` refuses)
    /// the old file goes FIRST and comes back if the new one cannot be saved. The pointer follows
    /// an active set.
    static func rename(_ name: String, to newName: String, in menu: NSMenu) throws {
        let old = try ShortcutLibrary.normalizedName(name)
        let new = try ShortcutLibrary.normalizedName(newName)
        guard !isDefault(old) else {
            throw DoctoredCoreError(message: "The default shortcut set can be reset but not renamed.")
        }
        try refuseDefaultName(new)
        let listing = try ShortcutLibrary.listing()
        let key = ShortcutLibrary.key
        guard var item = listing.entries.first(where: { key($0.item.name) == key(old) })?.item else {
            throw DoctoredCoreError(message: "No shortcut set named “\(old)”.")
        }
        let caseOnly = key(old) == key(new)
        if !caseOnly, listing.entries.contains(where: { key($0.item.name) == key(new) }) {
            throw alreadyExists(new)
        }
        let previous = item
        let wasActive = key(activeName()) == key(old)
        item.name = new
        if caseOnly {
            try ShortcutLibrary.save(item, replace: true)
        } else if listing.entries.count + listing.broken.count < ShortcutCodec.maximumEntries {
            try ShortcutLibrary.save(item, replace: false)
            if wasActive { try Active.set(name: new) }
            try ShortcutLibrary.delete(name: old)
            return
        } else {
            // A plain library delete, so `.default` keeps the old name until the new one lists.
            try ShortcutLibrary.delete(name: old)
            do {
                try ShortcutLibrary.save(item, replace: false)
            } catch {
                _ = try? ShortcutLibrary.save(previous, replace: false)
                throw error
            }
        }
        if wasActive { try Active.set(name: new) }
    }

    /// Delete Set. The default set can be reset but not deleted; deleting the active set makes
    /// the default set active first.
    static func delete(_ name: String, in menu: NSMenu) throws {
        let normalized = try ShortcutLibrary.normalizedName(name)
        guard !isDefault(normalized) else {
            throw DoctoredCoreError(message: "The default shortcut set can be reset but not deleted.")
        }
        if ShortcutLibrary.key(activeName()) == ShortcutLibrary.key(normalized) {
            try activate(defaultName, in: menu)
        }
        try Active.delete(name: normalized)
    }

    /// Import Shortcuts File…: the `.kys` file's mapped entries as a NEW set named after the
    /// file (" 2", " 3"… while that name is taken), made active. Nothing is written over an
    /// existing set, and a file of which nothing maps is refused.
    static func importKys(
        contentsOf url: URL, in menu: NSMenu
    ) throws -> (name: String, report: KysImportReport) {
        let entries = try KysImport.entries(contentsOf: url)
        let base = try ShortcutLibrary.normalizedName(url.deletingPathExtension().lastPathComponent)
        // At most the default set and sixteen others are taken, so one of the first eighteen
        // spellings is free.
        let taken = Set(names().map(ShortcutLibrary.key))
        var name = base
        var suffix = 2
        while taken.contains(ShortcutLibrary.key(name)) {
            guard suffix <= ShortcutCodec.maximumEntries + 2 else { throw alreadyExists(name) }
            name = try ShortcutLibrary.normalizedName("\(base) \(suffix)")
            suffix += 1
        }
        let (bindings, report) = KysImport.bindings(entries, name: name, menu: menu)
        guard !report.applied.isEmpty else {
            throw DoctoredCoreError(
                message: "Nothing in “\(url.lastPathComponent)” maps onto a command or tool here.")
        }
        try ShortcutLibrary.save(bindings, replace: false)
        try activate(name, in: menu)
        return (name, report)
    }
}
