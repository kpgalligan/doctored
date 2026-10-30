import AppKit

extension AppDelegate {
    func toolsMenu() -> NSMenu {
        // Deliberately no key equivalents: the bare tool keys (v/b/e/t) are
        // handled in ImageCanvasView.keyDown so they never steal keystrokes
        // from text editing. Built from the rail's own groups, in rail
        // order, so this menu and the rail can never disagree about the
        // tool set; a planned tool's item is validation-disabled.
        let menu = NSMenu(title: "Tools")
        for group in EditorTool.railGroups {
            for tool in group {
                menu.addItem(item("\(tool.displayName) Tool", tool.action))
            }
        }
        menu.addItem(.separator())
        // ⌘F: the Command Palette (`designs/v1-shortcuts-fade.md`).
        // The Tools menu is the right home for an app utility that is not a tool.
        menu.addItem(item("Command Palette…", #selector(showCommandPalette(_:)), "f"))
        return menu
    }
}
