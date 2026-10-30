import AppKit

/// What a new editor window reopens with: the frame and the right
/// panel's visibility and tab of the editor window the user last moved,
/// resized, left or closed. `EditorWindowController` restores it in `init`
/// and records it on move, end of live resize, resign-key and close — never
/// while full screen. The tool is not remembered.
///
/// An app-wide VIEW PREFERENCE in `UserDefaults`, like the rest of the view
/// chrome, so it has no MCP twin (app/CLAUDE.md's exemption). It is never
/// read or written under automation (`AutomationAlerts.isEnabled`: every
/// drive and probe sets `DR_BACKGROUND=1` or `DR_AUTOMATION_ALERTS=record`),
/// so a test window keeps the canvas-sized default frame and leaves the
/// user's memory alone.
struct EditorWindowMemory: Codable, Equatable {
    var frame: CGRect
    var panelVisible: Bool
    /// `EditorPanelTab.name`.
    var panelTab: String

    static let defaultsKey = "editorWindowMemory"

    /// The cascade step between windows that would otherwise open on top of
    /// one another, AppKit's own 22-point title-bar offset.
    static let cascadeStep: CGFloat = 22

    static var isEnabled: Bool { !AutomationAlerts.isEnabled }

    /// The stored memory, or nil when there is none or it cannot be read or
    /// names an empty or non-finite frame.
    static func load(from defaults: UserDefaults = .standard) -> EditorWindowMemory? {
        guard let data = defaults.data(forKey: defaultsKey),
              let memory = try? JSONDecoder().decode(EditorWindowMemory.self, from: data)
        else { return nil }
        let f = memory.frame
        guard [f.minX, f.minY, f.width, f.height].allSatisfy(\.isFinite),
              f.width > 0, f.height > 0
        else { return nil }
        return memory
    }

    func save(to defaults: UserDefaults = .standard) {
        guard let data = try? JSONEncoder().encode(self) else { return }
        defaults.set(data, forKey: Self.defaultsKey)
    }

    /// Where a new window opens for a remembered `frame`: its origin stepped
    /// by (+22, −22) while it equals an open editor window's origin
    /// (`occupied`), then fitted onto the displays (`WindowFrameFit.fitted`,
    /// `fallback` the placement screen's visible frame).
    static func placement(_ frame: CGRect, occupied: [CGRect], visibleFrames: [CGRect],
                          fallback: CGRect) -> CGRect {
        var placed = frame
        // Each step moves the origin to a new point, so at most one step per
        // occupied window.
        for _ in 0..<occupied.count where occupied.contains(where: { $0.origin == placed.origin }) {
            placed.origin.x += cascadeStep
            placed.origin.y -= cascadeStep
        }
        return WindowFrameFit.fitted(placed, visibleFrames: visibleFrames, fallback: fallback)
    }
}
