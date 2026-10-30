import AppKit

extension AppDelegate {
    func helpMenu() -> NSMenu {
        let menu = NSMenu(title: "Help")
        menu.addItem(item("Doctored Help", #selector(NSApplication.showHelp(_:))))
        menu.addItem(item("Memory Usage…", #selector(showResourceUsage(_:))))
        return menu
    }
}
