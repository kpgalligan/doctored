import AppKit

extension AppDelegate {
    func appMenu() -> NSMenu {
        let menu = NSMenu(title: "Doctored")
        menu.addItem(
            item("About Doctored", #selector(NSApplication.orderFrontStandardAboutPanel(_:))))
        // Enabled only in a launch with an updater (AppUpdater); no key equivalent.
        menu.addItem(item("Check for Updates…", #selector(checkForUpdates(_:))))
        menu.addItem(.separator())
        // The app menu's standard slot for preferences, on macOS's own ⌘,.
        menu.addItem(item("Settings…", #selector(showSettings(_:)), ","))
        // Hidden twin on ⌘K, the ⌘= Zoom In idiom.
        let settingsK = item("Settings…", #selector(showSettings(_:)), "k")
        settingsK.isHidden = true
        settingsK.allowsKeyEquivalentWhenHidden = true
        menu.addItem(settingsK)
        menu.addItem(item("Working Memory…", #selector(showWorkingMemory(_:))))
        menu.addItem(.separator())
        menu.addItem(item("Hide Doctored", #selector(NSApplication.hide(_:)), "h"))
        menu.addItem(
            item(
                "Hide Others", #selector(NSApplication.hideOtherApplications(_:)), "h",
                [.command, .option]))
        menu.addItem(item("Show All", #selector(NSApplication.unhideAllApplications(_:))))
        menu.addItem(.separator())
        menu.addItem(item("Quit Doctored", #selector(NSApplication.terminate(_:)), "q"))
        return menu
    }
}
