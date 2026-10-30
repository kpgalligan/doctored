import AppKit

// `ScreenKey` is what the canvas reports from `keyDown`'s bare-key block — Tab by keyCode 48
// (Shift+Tab's characters are backtab), `.allPanels` or, with Shift, `.sidePanels`, and bare F
// `.fullScreen`; a key repeat is consumed and ignored. `screenKeyPressed` beeps while a mouse
// button is down (a relayout mid-drag would jump the stroke), then Tab runs `togglePanels(_:)`
// (View ▸ Hide/Show Panels: anything showing hides all three, otherwise shows all three, and a hide
// hands key focus back to the canvas so Tab can return it), Shift+Tab the existing
// `toggleLayersPanel(_:)` and F `window.toggleFullScreen`.
//
// `setToolChromeVisible` closes the options bar's overflow, hides the bar and the rail and slides
// them past the root's edges through `optionsBarTopConstraint` / `toolRailLeadingConstraint` (never
// collapsing their size, so their internal constraints stay satisfiable); the rulers, well and
// panels pin to their edges and follow, and `updatePanelVisibility()` stays the one side-panel
// path. `validateScreenChromeItem` answers both panel items above the session guard (both are
// view-only; `togglePanels:` is in `viewOnlyActions`). Neither Tab nor F is a menu key equivalent:
// a modifier-less equivalent is resolved ahead of the first responder and would steal the key from
// every text field. The state is per window and never persisted (`WorkspaceLayout` stays v1;
// `applyWorkspace` shows the rail and bar again), and there is no MCP twin — window chrome changes
// no document, render or saved output, and an agent must not put the user's window into full
// screen.

/// The canvas's screen keys. The canvas matches Tab by keyCode
/// (Shift+Tab's characters are backtab) and only reports the key.
enum ScreenKey: Equatable {
    /// Tab: the tool rail, the options bar and the side panel together.
    case allPanels
    /// Shift+Tab: the side panel only (View ▸ Hide Layers).
    case sidePanels
    /// F: standard macOS full screen on the editor window.
    case fullScreen
}

/// Tab / Shift+Tab / F and View ▸ Hide/Show Panels. The state is per window
/// and in memory only; there is no MCP twin because window chrome changes
/// no document, render or saved output.
extension EditorViewController {
    /// True when anything Tab hides is showing.
    var anyPanelsVisible: Bool { toolChromeVisible || layersPanelVisible }

    /// View ▸ Hide/Show Panels, and Tab on the canvas: if anything is
    /// showing, hide all three; otherwise show all three.
    @objc func togglePanels(_ sender: Any?) {
        let show = !anyPanelsVisible
        setToolChromeVisible(show)
        layersPanelVisible = show
        updatePanelVisibility()
        // A menu-driven hide from a focused panel control would otherwise
        // leave nothing on screen that takes Tab back.
        if !show, !canvas.hasActiveTextSession, view.window?.firstResponder !== canvas {
            view.window?.makeFirstResponder(canvas)
        }
    }

    /// The ONE writer of `toolChromeVisible`. The bar and the rail slide past
    /// the root's edges rather than collapse, which keeps their internal
    /// constraints satisfiable; the rulers, the well and the panels pin to
    /// their bottom/trailing edges and follow.
    func setToolChromeVisible(_ visible: Bool) {
        toolChromeVisible = visible
        if !visible { optionsBar.closeOverflow() }
        optionsBar.isHidden = !visible
        toolRail.isHidden = !visible
        optionsBarTopConstraint.constant = visible ? 0 : -DS.optionsBarHeight
        toolRailLeadingConstraint.constant = visible ? 0 : -DS.railWidth
    }

    /// Canvas → controller. Refused (beep) while a mouse button is down,
    /// because a relayout mid-drag would jump the stroke.
    func screenKeyPressed(_ key: ScreenKey) {
        guard NSEvent.pressedMouseButtons == 0 else {
            NSSound.beep()
            return
        }
        switch key {
        case .allPanels:
            togglePanels(nil)
        case .sidePanels:
            toggleLayersPanel(nil)
        case .fullScreen:
            view.window?.toggleFullScreen(nil)
        }
    }

    /// Hide/Show Panels and Hide/Show Layers. Both are view-only, so they
    /// answer above the session guard and stay live in text, shape-edit and
    /// transform sessions. nil means "not one of mine".
    func validateScreenChromeItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        switch item.action {
        case #selector(togglePanels(_:)):
            if let menuItem = item as? NSMenuItem {
                menuItem.title = anyPanelsVisible ? "Hide Panels" : "Show Panels"
            }
            return true
        case #selector(toggleLayersPanel(_:)):
            if let menuItem = item as? NSMenuItem {
                menuItem.title = layersPanelVisible ? "Hide Layers" : "Show Layers"
            }
            return true
        default:
            return nil
        }
    }
}
