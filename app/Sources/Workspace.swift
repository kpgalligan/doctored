import AppKit

// A `WorkspaceLayout` carries window/panel/chrome/tool and deliberately NOT the document's zoom or
// scroll (that is per document, `EditorViewState`); its frame is restored through `WindowFrameFit`:
// a frame that lies inside the usable area (the union of the screens' visible frames) is kept
// exactly, otherwise it is moved, and shrunk if needed, wholly onto one display, and a full-screen
// window keeps its frame.

extension Notification.Name {
    static let workspaceLibraryDidChange = Notification.Name("WorkspaceLibraryDidChange")
}

/// Which right-panel tab a workspace restores. The raw values ARE
/// `EditorViewController.panelTab`'s numbers, and the NAMES are what the file
/// and MCP speak: a layout file that said `"panel": {"tab": 4}` would be
/// unreadable by eye and would silently mean something else the day a tab is
/// inserted.
enum EditorPanelTab: Int, CaseIterable {
    case layers = 0
    case channels
    case assistant
    case info
    case navigator
    /// The Paths panel. APPENDED, so the five values before it keep
    /// the numbers the editor and the saved layouts compare.
    case paths = 5

    var name: String {
        switch self {
        case .layers: return "layers"
        case .channels: return "channels"
        case .assistant: return "assistant"
        case .info: return "info"
        case .navigator: return "navigator"
        case .paths: return "paths"
        }
    }

    /// The tab's title in every panel's tab row.
    var title: String {
        switch self {
        case .layers: return "Layers"
        case .channels: return "Channels"
        case .assistant: return "Assistant"
        case .info: return "Info"
        case .navigator: return "Navigator"
        case .paths: return "Paths"
        }
    }

    /// The tab row's titles, in raw-value order — the one list the six
    /// panels build their `PanelTabsView` from.
    static var titles: [String] { allCases.map(\.title) }

    init?(name: String) {
        guard let match = Self.allCases.first(where: { $0.name == name }) else { return nil }
        self = match
    }
}

/// One saved workspace layout (Phase 12C-2): the FURNITURE of a window.
///
/// Deliberately NOT here: the document's zoom and scroll offset. A layout is
/// window and panel state that applies to whatever document is in front of
/// you; where you are looking inside one picture is per document and belongs
/// to `EditorViewState` (`get_view` / `set_view`). Storing zoom would make
/// restoring a layout jump every open document to one magnification chosen
/// while looking at a different picture.
struct WorkspaceLayout: Equatable {
    static let schemaVersion = 1

    var name: String
    var windowFrame: CGRect
    var panelVisible: Bool
    var panelTab: EditorPanelTab
    var panelWidth: Double
    var rulers: Bool
    var grid: Bool
    var snap: Bool
    var tool: EditorTool

    var json: [String: Any] {
        [
            "schema_version": Self.schemaVersion,
            "name": name,
            "window": ["x": windowFrame.minX, "y": windowFrame.minY,
                       "width": windowFrame.width, "height": windowFrame.height],
            "panel": ["visible": panelVisible, "tab": panelTab.name, "width": panelWidth],
            "view": ["rulers": rulers, "grid": grid, "snap": snap],
            "tool": tool.identifier,
        ]
    }

    /// Strict, like every other library codec: an unexpected key, a missing
    /// field or an unknown name THROWS, and the throw becomes one
    /// `invalid_schema` row rather than a half-read layout.
    static func decode(_ object: [String: Any]) throws -> WorkspaceLayout {
        func refuse() -> Error {
            DoctoredCoreError(message: "Invalid or unsupported workspace.")
        }
        guard Set(object.keys) == ["schema_version", "name", "window", "panel", "view", "tool"],
              let name = object["name"] as? String,
              let window = object["window"] as? [String: Any],
              let panel = object["panel"] as? [String: Any],
              let view = object["view"] as? [String: Any],
              let toolName = object["tool"] as? String,
              let tool = EditorTool.named(toolName) else {
            throw refuse()
        }
        guard Set(window.keys) == ["x", "y", "width", "height"],
              Set(panel.keys) == ["visible", "tab", "width"],
              Set(view.keys) == ["rulers", "grid", "snap"] else {
            throw refuse()
        }
        func number(_ container: [String: Any], _ key: String) throws -> Double {
            guard let value = (container[key] as? NSNumber)?.doubleValue, value.isFinite else {
                throw refuse()
            }
            return value
        }
        func flag(_ container: [String: Any], _ key: String) throws -> Bool {
            guard let value = container[key] as? Bool else { throw refuse() }
            return value
        }
        guard let tabName = panel["tab"] as? String, let tab = EditorPanelTab(name: tabName) else {
            throw refuse()
        }
        let width = try number(panel, "width")
        guard DS.panelWidthRange.contains(CGFloat(width)) else { throw refuse() }
        let frame = CGRect(x: try number(window, "x"), y: try number(window, "y"),
                           width: try number(window, "width"),
                           height: try number(window, "height"))
        guard frame.width > 0, frame.height > 0, frame.width <= 100_000,
              frame.height <= 100_000 else {
            throw refuse()
        }
        return WorkspaceLayout(
            name: name, windowFrame: frame, panelVisible: try flag(panel, "visible"),
            panelTab: tab, panelWidth: width, rulers: try flag(view, "rulers"),
            grid: try flag(view, "grid"), snap: try flag(view, "snap"), tool: tool)
    }
}

/// Where a restored workspace's window frame lands. Pure:
/// no AppKit types, so the probe drives it on synthetic displays.
enum WindowFrameFit {
    /// A frame that lies inside the usable area (the union of the screens'
    /// visible frames) is kept exactly. Otherwise it is moved, and shrunk if
    /// needed, wholly onto one display.
    ///
    /// The display is the visible frame the frame overlaps most (the earlier
    /// one on a tie), or `fallback` when it overlaps none. No screens at all
    /// keeps the frame.
    static func fitted(_ frame: CGRect, visibleFrames: [CGRect], fallback: CGRect) -> CGRect {
        guard !visibleFrames.isEmpty else { return frame }
        func area(_ rect: CGRect) -> CGFloat {
            rect.isNull ? 0 : rect.width * rect.height
        }
        let overlaps = visibleFrames.map { area(frame.intersection($0)) }
        if overlaps.reduce(0, +) >= area(frame) - 0.5 { return frame }
        var target = fallback
        var best: CGFloat = 0
        for (index, overlap) in overlaps.enumerated() where overlap > best {
            best = overlap
            target = visibleFrames[index]
        }
        // Whole points inside the target, even on a half-point visible frame.
        let minX = target.minX.rounded(.up), minY = target.minY.rounded(.up)
        let maxX = target.maxX.rounded(.down), maxY = target.maxY.rounded(.down)
        let width = min(frame.width.rounded(.down), max(maxX - minX, 0))
        let height = min(frame.height.rounded(.down), max(maxY - minY, 0))
        let x = min(max(frame.minX.rounded(), minX), maxX - width)
        let y = min(max(frame.minY.rounded(), minY), maxY - height)
        return CGRect(x: x, y: y, width: width, height: height)
    }
}

/// The workspace library's codec on the shared `PreferenceLibrary` contract —
/// the same folder rules, lock, per-entry diagnosis and delete-by-file every
/// other preference library has. Its writes are PREFERENCES: outside document
/// Undo and never Action steps.
enum WorkspaceCodec: PreferenceLibraryCodec {
    typealias Item = WorkspaceLayout

    static let noun = "workspace"
    static let directoryEnvironment = "DR_WORKSPACES_DIR"
    static let directoryName = "Doctored/Workspaces"
    static let maximumEntries = 32
    static let maximumFileBytes = 4096
    static let schemaVersions = WorkspaceLayout.schemaVersion...WorkspaceLayout.schemaVersion
    static let didChange = Notification.Name.workspaceLibraryDidChange

    static func name(of item: WorkspaceLayout) -> String { item.name }
    static func json(of item: WorkspaceLayout) -> [String: Any] { item.json }

    static func decode(_ object: [String: Any], schemaVersion: Int) throws -> WorkspaceLayout {
        try WorkspaceLayout.decode(object)
    }
}

typealias WorkspaceLibrary = PreferenceLibrary<WorkspaceCodec>
