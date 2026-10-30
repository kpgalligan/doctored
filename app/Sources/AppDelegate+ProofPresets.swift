import AppKit

// View ▸ Proof Setup is a submenu (`Custom…` keeps `showProofSetup:` tag 0 and the Working CMYK rows
// `applyWorkingProof:` tags 0–4, built once; preset rows
// carry the `ProofPreset` value; a Delete Broken File group), rebuilt in `menuNeedsUpdate` with one
// `listing()`.

/// View ▸ Proof Setup: Custom…, then one row per saved proof
/// preset, then — when the library holds files that are not presets — a
/// Delete Broken File group.
///
/// Rebuilt in `menuNeedsUpdate` with ONE `listing()` read, on the Workspaces
/// precedent: the library is a folder other instances can write. Each
/// preset row carries the `ProofPreset` VALUE in `representedObject` (so
/// `ShortcutMenu.customizableItems` skips it) and a nil-target
/// `applyProofPresetFromMenu(_:)`; its checkmark comes from
/// `ProofPreset.matches` in validation, which does no I/O.
extension AppDelegate {
    /// The STATIC rows — Custom…, the Working CMYK rows and the separator — built once and never
    /// recreated: `Custom…` keeps `showProofSetup:` and tag 0, the identity a
    /// user's chord for the old flat `Proof Setup…` item is stored under, and
    /// a recreated item would drop it (the Workspaces lost-chord rule).
    private static let staticProofRows = 2 + CmykProof.rows.count

    func proofSetupMenu() -> NSMenu {
        let menu = NSMenu(title: "Proof Setup")
        menu.delegate = self
        rebuildProofSetup(menu)
        return menu
    }

    func rebuildProofSetup(_ menu: NSMenu) {
        if menu.numberOfItems < Self.staticProofRows {
            menu.removeAllItems()
            menu.addItem(item("Custom…", #selector(EditorViewController.showProofSetup(_:))))
            // #1 phase 5: Working CMYK and the four Working plates, tags 0–4
            // (EditorViewController.applyWorkingProof, CmykProof.swift).
            for (tag, row) in CmykProof.rows.enumerated() {
                let entry = item(row.title, #selector(EditorViewController.applyWorkingProof(_:)))
                entry.tag = tag
                menu.addItem(entry)
            }
            menu.addItem(.separator())
        }
        while menu.numberOfItems > Self.staticProofRows {
            menu.removeItem(at: Self.staticProofRows)
        }
        let listing: PreferenceLibraryListing<ProofPreset>
        do {
            listing = try ProofPresetLibrary.listing()
        } catch {
            // A library the app cannot read is SAID, not shown as empty.
            menu.addItem(item(error.localizedDescription, nil))
            return
        }
        if listing.entries.isEmpty {
            menu.addItem(item("No Saved Proof Presets", nil))
        } else {
            for preset in listing.entries.map(\.item) {
                let entry = item(preset.name,
                                 #selector(EditorViewController.applyProofPresetFromMenu(_:)))
                entry.representedObject = preset
                menu.addItem(entry)
            }
        }
        guard !listing.broken.isEmpty else { return }
        menu.addItem(.separator())
        let files = NSMenu(title: "Delete Broken File")
        for broken in listing.broken {
            let entry = item("\(broken.file) — \(broken.reason)",
                             #selector(EditorViewController.deleteProofPresetFile(_:)))
            entry.representedObject = broken.file
            files.addItem(entry)
        }
        menu.addItem(submenuItem(files))
    }
}
