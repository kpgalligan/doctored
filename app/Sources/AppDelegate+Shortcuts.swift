import AppKit

/// The main menu's two-step construction (Phase 12C-2) and the Settings item
/// that opens the Settings card (Shortcuts and Models tabs).
///
/// `buildMainMenu()` stays the ONE statement of what this build's menus are;
/// `makeMainMenu()` is that menu with the user's stored overrides applied on
/// top. The split is what keeps a user's choice through a change of defaults
/// (`designs/v1-shortcuts-fade.md`): the snapshot taken here is what `Reset
/// All` restores, and the file holds only what the user changed.
extension AppDelegate {
    /// What `applicationWillFinishLaunching` installs — and what a probe can
    /// build for itself, since `AppDelegate` keeps no stored state.
    func makeMainMenu() -> NSMenu {
        let menu = buildMainMenu()
        ShortcutMenu.installStored(to: menu)
        return menu
    }

    /// Doctored ▸ Settings… (⌘,, and ⌘K through its hidden twin).
    @objc func showSettings(_ sender: Any?) {
        SettingsWindowController.run()
    }
}
