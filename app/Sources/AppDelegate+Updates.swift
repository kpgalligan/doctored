import AppKit

/// Doctored ▸ Check for Updates… (AppUpdater). On AppDelegate because it must work with no
/// document open (the File ▸ Automate reason). In every build, so the menu tree is one tree;
/// enabled only when this launch has an updater. No MCP mirror (AppUpdater's header).
extension AppDelegate {
    @objc func checkForUpdates(_ sender: Any?) { AppUpdater.checkForUpdates(sender) }
    func validateUpdatesItem(_ item: NSMenuItem) -> Bool { AppUpdater.canCheckForUpdates }
}
