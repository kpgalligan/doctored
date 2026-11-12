import AppKit

// The screens' windows, one table each: `windowKinds` (a screen file's `window`), `cards` (the
// modal cards `run_modal_card` opens) and `panels` (the non-modal windows `show_window` opens and a
// `panel` capture takes). A new window, card or panel is one entry here; plan.swift validates every
// name against these tables and scenes.swift runs them.

/// How a window kind's screens run.
enum Host {
    /// A fresh untitled document of the fixture in its editor window: every step scope, and
    /// `run_modal_card` opens a card from it.
    case editor
    /// A modal card opened from the fixture: every step runs inside it, and `window` is the card.
    case card(CardSpec)
    /// A plain window with no document: only `any` steps, and only its window is captured.
    case plain(open: (Context) throws -> NSWindow)

    /// Whether a screen on this host can take `target`; `hasCard` says a card runs.
    func admits(_ target: CaptureTarget, hasCard: Bool) -> Bool {
        switch (self, target) {
        case (_, .window): return true
        case (.editor, .view), (.editor, .sheet), (.editor, .panel), (.editor, .alert), (.card, .alert): return true
        case (.editor, .card), (.card, .card): return hasCard
        default: return false
        }
    }
}

struct WindowKind {
    /// The fixture kind (`fixtures.json`'s `kind`) its screens open, or nil for none.
    let fixture: String?
    let host: Host
}

/// A modal card: its window's title (the modal driver matches `NSApp.modalWindow` by it), the call
/// that presents it and returns once its modal session ends, and an optional readiness check that
/// may refuse (throw) instead. The runner ends the card with its Cancel button, or `abortModal`.
struct CardSpec {
    let title: String
    let open: (Context) throws -> Void
    let ready: ((NSWindow, Context) throws -> Bool)?

    init(title: String, ready: ((NSWindow, Context) throws -> Bool)? = nil,
         open: @escaping (Context) throws -> Void) {
        self.title = title
        self.open = open
        self.ready = ready
    }
}

/// A non-modal window: how `show_window` opens it (nil when another action opens it), and how a
/// `panel` capture finds it once open.
struct PanelSpec {
    let show: ((Context) throws -> Void)?
    let window: (Context) -> NSWindow?
}

let windowKinds: [String: WindowKind] = [
    "editor": WindowKind(fixture: "photo", host: .editor),
    // A camera file opened in the editor, developed at the default settings (develop_actions.swift).
    "editor_raw": WindowKind(fixture: "raw", host: .editor),
    "develop": WindowKind(fixture: "raw", host: .card(developCard)),
    "welcome": WindowKind(fixture: nil, host: .plain(open: openWelcome)),
]

let cards: [String: CardSpec] = [
    "new_document": CardSpec(title: "New Document") { _ in _ = NewDocumentSheetController.run() },
    // The Export sheet's Metadata policy menu ▸ Manage…, opened on the default policy.
    "metadata_policies": CardSpec(title: "Metadata Policies") { _ in
        _ = MetadataPolicyManageSheet.run(current: .defaults)
    },
    // File ▸ Automate ▸ Merge Panorama…, with its list empty: Browse… is never pressed.
    "merge_panorama": CardSpec(title: MergePanorama.title) { _ in _ = MergePanoramaSheetController.run() },
    // Layer ▸ Redevelop RAW… on the active camera RAW layer (an `editor_raw` screen), captured once
    // its preview is ready; cancelled, so nothing is published.
    "redevelop_raw": CardSpec(title: "Develop RAW", ready: developReady) { context in
        try context.requireEditor().redevelopRaw(nil)
    },
    // Edit ▸ Color Settings….
    "color_settings": CardSpec(title: "Color Settings") { _ in _ = ColorSettingsWindowController.run() },
    // File ▸ Automate ▸ Batch…, which needs an action in the library (`save_action` first): an
    // empty library puts up an alert instead.
    "batch": CardSpec(title: "Batch") { _ in BatchWindowController.present() },
    // The consent card Generative Fill's Generate puts up before the first send to a provider,
    // here OpenAI's (the Provider pop-up's first item).
    "cloud_consent": CardSpec(title: "Send to \(CloudProviders.provider(.openai).displayName)?") { _ in
        _ = CloudConsentCard.present(CloudProviders.provider(.openai))
    },
    // Doctored ▸ Settings… on the Shortcuts tab: the card lists the live main menu's commands, so
    // the app's menu is installed first (`installMainMenu`, actions.swift); without one `run()`
    // only beeps. A `tab` capture selects another tab.
    "settings": CardSpec(title: "Settings") { context in
        try installMainMenu(context)
        SettingsWindowController.run()
    },
]

let panels: [String: PanelSpec] = [
    "history": PanelSpec(show: { context in
        let document = try context.requireDocument()
        let controller = document.historyWindowController ?? HistoryWindowController(document: document)
        document.historyWindowController = controller
        // Window ▸ History… orders the panel front; it must not come over the desktop.
        try showBehind(controller.window) { document.showHistoryPanel() }
    }, window: { $0.document?.historyWindowController?.window }),
    // Opened by `open_command_palette` (actions.swift), which types its query.
    "command_palette": PanelSpec(show: nil, window: { _ in CommandPaletteWindowController.shared.window }),
    // Window ▸ Snapshots…, widened to its own minimum width: it is built narrower, which cuts
    // its row of six buttons.
    "snapshots": PanelSpec(show: { context in
        let editor = try context.requireEditor()
        let document = try context.requireDocument()
        let controller = document.snapshotsWindowController ?? SnapshotsWindowController(document: document)
        document.snapshotsWindowController = controller
        guard let window = controller.window else { throw ProbeFailure(description: "no Snapshots window") }
        try showBehind(window) { editor.showSnapshots(nil) }
        var frame = window.frame
        frame.size.width = max(frame.width, window.minSize.width)
        window.setFrame(frame, display: true)
    }, window: { $0.document?.snapshotsWindowController?.window }),
    // File ▸ Recover Unsaved Work…, with its first copy chosen as a person would before
    // Recover Copy (the buttons that act on a copy are disabled until one is). The coordinator
    // is never started, so it lists only the copies `seed_recovery_copy` left.
    "recovery": PanelSpec(show: { context in
        RecoveryCoordinator.shared.showRecoveryWindow(nil)
        guard let window = recoveryWindow(), let content = window.contentView else {
            throw ProbeFailure(description: "no Recover Unsaved Work window")
        }
        let tables = descendants(content).compactMap { $0 as? NSTableView }
        guard tables.count == 1, tables[0].numberOfRows > 0 else {
            throw ProbeFailure(description: "the recovery list is empty (seed_recovery_copy first)")
        }
        tables[0].selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
    }, window: { _ in recoveryWindow() }),
    // Window ▸ Layer Comps…, widened to its own minimum width: it is built narrower, which cuts
    // its row of seven buttons.
    "layer_comps": PanelSpec(show: { context in
        let editor = try context.requireEditor()
        let document = try context.requireDocument()
        let controller = document.layerCompsWindowController ?? LayerCompsWindowController(document: document)
        document.layerCompsWindowController = controller
        guard let window = controller.window else { throw ProbeFailure(description: "no Layer Comps window") }
        try showBehind(window) { editor.showLayerComps(nil) }
        var frame = window.frame
        frame.size.width = max(frame.width, window.minSize.width)
        window.setFrame(frame, display: true)
    }, window: { $0.document?.layerCompsWindowController?.window }),
    // Window ▸ Styles. The panel is app-wide and floats; it is shown at the normal level, so it
    // stays behind every other app's windows, and its level is put back at teardown. It is
    // widened to hold its button bar with a margin: built at 300 pt, the bar ends flush with its
    // right edge.
    "styles": PanelSpec(show: { context in
        let controller = StylesPanelController.shared
        guard let window = controller.window else { throw ProbeFailure(description: "no Styles panel") }
        let level = window.level, hides = window.hidesOnDeactivate
        context.restorers.append {
            window.level = level
            window.hidesOnDeactivate = hides
        }
        // The probe is never active, so a panel that hides on deactivation would never show.
        window.hidesOnDeactivate = false
        window.level = .normal
        try showBehind(window) { controller.show() }
        guard let content = window.contentView, let bar = descendants(content).compactMap({ $0 as? NSStackView })
                .first(where: { buttons($0).contains { $0.title == "Export Styles…" } }) else {
            throw ProbeFailure(description: "the Styles panel has no button bar")
        }
        let wanted = bar.fittingSize.width + 2 * bar.frame.minX
        var frame = window.frame
        frame.size.width = max(frame.width, ceil(wanted))
        window.setFrame(frame, display: true)
    }, window: { _ in StylesPanelController.shared.window }),
    // File ▸ Automate ▸ Actions…: the app-wide library window, on the run's private Actions
    // folder (`save_action` writes into it).
    "actions": PanelSpec(show: { _ in
        let controller = ActionsWindowController.shared
        try showBehind(controller.window) { controller.show() }
    }, window: { _ in ActionsWindowController.shared.window }),
    // The options bar's More panel, a child window of the editor's; opened by `open_overflow`
    // (retouch_actions.swift).
    "overflow": PanelSpec(show: nil, window: { context in
        context.window?.childWindows?.first { $0 is OverflowPanel && $0.isVisible }
    }),
]

/// The Recover Unsaved Work panel: the coordinator keeps its controller private.
func recoveryWindow() -> NSWindow? {
    NSApp.windows.first { $0.title == "Recover Unsaved Work" }
}

/// Runs `show`, which orders `window` front, with the window transparent, then orders it behind
/// every other app's windows: the probe never comes over the desktop.
func showBehind(_ window: NSWindow?, _ show: () -> Void) throws {
    guard let window else { throw ProbeFailure(description: "the window was not built") }
    window.alphaValue = 0
    show()
    window.orderBack(nil)
    window.alphaValue = 1
}

// ---- welcome

/// The probe is no app bundle, so AppKit would give it a generic icon: Doctored's own comes from
/// the asset catalog, before any window copies it (the welcome window, every NSAlert). run.py
/// starts the probe in the repository.
func installAppIcon() throws {
    let icon = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        .appendingPathComponent("app/Assets.xcassets/AppIcon.appiconset/icon_512x512-2x.png")
    guard let image = NSImage(contentsOf: icon) else { throw ProbeFailure(description: "no app icon at \(icon.path)") }
    image.size = NSSize(width: 512, height: 512)
    NSApp.applicationIconImage = image
}

/// Doctored's welcome window, which shows the app icon `installAppIcon` set.
func openWelcome(_ context: Context) throws -> NSWindow {
    let controller = WelcomeWindowController.shared
    // Under DR_BACKGROUND this orders the window behind (BackgroundLaunch.show).
    controller.show()
    guard let window = controller.window else { throw ProbeFailure(description: "the welcome window was not built") }
    return window
}

// ---- develop

/// The Develop RAW card for the screen's camera file, captured once its preview is ready
/// (`developReady`, develop_actions.swift).
let developCard = CardSpec(title: "Develop RAW", ready: developReady, open: { context in
    guard let url = context.screen.fixture?.path else { throw ProbeFailure(description: "no camera file") }
    let snapshot = try RawSourceSnapshot.capture(url, maximumBytes: 256 << 20)
    let descriptor = try RetainedRawDecoder.capture(
        url: snapshot.url, filename: url.lastPathComponent, sha256: snapshot.sha256)
    guard let probe = RawImage.inspect(url: snapshot.url) else {
        throw ProbeFailure(description: "\(url.lastPathComponent) is not a camera file the app reads")
    }
    _ = RawDevelopWindowController.runImport(probe, descriptor: descriptor, snapshot: snapshot)
})
