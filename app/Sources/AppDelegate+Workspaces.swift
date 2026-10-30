import AppKit

/// Window ▸ Workspaces (Phase 12C-2): Save, Delete ▸ and one row per saved
/// layout.
///
/// The list is rebuilt in `menuNeedsUpdate`, not at launch: the library is a
/// folder other instances of the app can write, so the only honest moment to
/// read it is when the menu is about to be shown. Each row carries its NAME in
/// `representedObject` and a nil-target `restoreWorkspace(_:)`, which is why
/// `ShortcutMenu.customizableItems` skips items with a represented object —
/// a binding on a row that may not exist tomorrow is a binding on nothing.
extension AppDelegate {
    /// How many rows above the separator are STATIC — Save Workspace…, the
    /// Delete Workspace ▸ submenu and the separator itself. Named once
    /// because it is both the "have they been built yet" test and the index
    /// the dynamic rows start at, and a positional literal in three places
    /// is how those two silently drift apart.
    private static let staticWorkspaceRows = 3

    func workspacesMenu() -> NSMenu {
        let menu = NSMenu(title: "Workspaces")
        menu.delegate = self
        rebuildWorkspaces(menu)
        return menu
    }

    /// The three STATIC rows — Save Workspace…, Delete Workspace ▸ and the
    /// separator — are built once and never recreated. `Save Workspace…` is a
    /// customizable command, and this runs on every `menuNeedsUpdate`
    /// (AppKit also runs one while searching for a key equivalent), so a new
    /// item here would hand back an empty key equivalent and silently drop a
    /// chord the user assigned to it. Only the dynamic rows below the
    /// separator are rebuilt.
    private func rebuildWorkspaces(_ menu: NSMenu) {
        if menu.numberOfItems < Self.staticWorkspaceRows {
            menu.removeAllItems()
            menu.addItem(
                item("Save Workspace…", #selector(EditorViewController.saveWorkspace(_:))))
            let delete = NSMenu(title: "Delete Workspace")
            delete.delegate = self
            menu.addItem(submenuItem(delete))
            menu.addItem(.separator())
        }
        while menu.numberOfItems > Self.staticWorkspaceRows {
            menu.removeItem(at: Self.staticWorkspaceRows)
        }
        let (listing, problem) = workspaceListing()
        if let problem {
            // A library the app cannot read is SAID, not shown as empty.
            menu.addItem(item(problem, nil))
        } else if listing.entries.isEmpty {
            // A nil action is a disabled row, which is exactly what "there are
            // none" should look like.
            menu.addItem(item("No Saved Workspaces", nil))
        } else {
            for layout in listing.entries.map(\.item) {
                let entry = item(layout.name, #selector(EditorViewController.restoreWorkspace(_:)))
                entry.representedObject = layout.name
                menu.addItem(entry)
            }
        }
        if let delete = menu.item(at: 1)?.submenu {
            rebuildDeletes(delete, listing: listing, problem: problem)
        }
    }

    /// ONE `listing()` read for both menus. A library-level fault (a busy
    /// lock, an unreadable folder) is an empty listing plus the sentence the
    /// menus show in place of "No Saved Workspaces".
    private func workspaceListing()
        -> (listing: PreferenceLibraryListing<WorkspaceLayout>, problem: String?) {
        do {
            return (try WorkspaceLibrary.listing(), nil)
        } catch {
            return (PreferenceLibraryListing(entries: [], broken: []), error.localizedDescription)
        }
    }

    /// The named rows, then — when the library holds files that are not
    /// layouts — a separator and a "Delete Broken File" group, one row per
    /// file titled "file — reason". Each broken row carries its FILE in
    /// `representedObject` (so `ShortcutMenu.customizableItems` skips it) and
    /// is the UI twin of `delete_workspace {file}`.
    private func rebuildDeletes(_ menu: NSMenu, listing: PreferenceLibraryListing<WorkspaceLayout>,
                                problem: String?) {
        menu.removeAllItems()
        if let problem {
            menu.addItem(item(problem, nil))
        } else if listing.entries.isEmpty {
            menu.addItem(item("No Saved Workspaces", nil))
        } else {
            for layout in listing.entries.map(\.item) {
                let entry = item(layout.name, #selector(EditorViewController.deleteWorkspace(_:)))
                entry.representedObject = layout.name
                menu.addItem(entry)
            }
        }
        guard !listing.broken.isEmpty else { return }
        menu.addItem(.separator())
        let files = NSMenu(title: "Delete Broken File")
        for broken in listing.broken {
            let entry = item("\(broken.file) — \(broken.reason)",
                             #selector(EditorViewController.deleteWorkspaceFile(_:)))
            entry.representedObject = broken.file
            files.addItem(entry)
        }
        menu.addItem(submenuItem(files))
    }
}

extension AppDelegate: NSMenuDelegate {
    /// `AppDelegate` holds no stored state (a probe builds its own menu from
    /// `AppDelegate().makeMainMenu()`), so the two dynamic menus are told
    /// apart by their titles rather than by a remembered reference.
    func menuNeedsUpdate(_ menu: NSMenu) {
        switch menu.title {
        case "Workspaces":
            rebuildWorkspaces(menu)
        case "Delete Workspace":
            let (listing, problem) = workspaceListing()
            rebuildDeletes(menu, listing: listing, problem: problem)
        case "Proof Setup":
            rebuildProofSetup(menu)
        default:
            break
        }
    }
}
