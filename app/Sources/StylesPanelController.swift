import AppKit
import UniformTypeIdentifiers

/// One click in the Styles panel: the preset and whether Shift was held
/// (Add to the current style instead of replacing it). The `sender` of the
/// nil-targeted `applyStylePreset:` (`EditorViewController+StylePresets`).
final class StylePresetRequest: NSObject {
    let preset: StylePreset
    let add: Bool

    init(preset: StylePreset, add: Bool) {
        self.preset = preset
        self.add = add
        super.init()
    }
}

/// Window ▸ Styles: the Styles panel — a grid of
/// style presets, one section per group (the built-in "Default Styles"
/// first), each a swatch (`StylePresetThumbnail`) with its name as the
/// tooltip. A click applies the preset to the front document's selected
/// layers through the responder chain (`applyStylePreset:`), Shift adds it
/// to their styles; a swatch's menu renames, moves or deletes it, a group
/// header's renames, deletes or exports the group (the built-in group's
/// exports only); the bottom bar holds New Style… (`newStylePreset:`), Import
/// Styles… and Export Styles… (a `.asl`). App-wide like the
/// Actions window: the library is a preference, not a document's.
final class StylesPanelController: NSWindowController, NSCollectionViewDataSource,
    NSCollectionViewDelegateFlowLayout
{
    static let shared = StylesPanelController()

    private let grid = NSCollectionView()
    private let status = NSTextField(labelWithString: "")
    private var sections: [(group: String?, presets: [StylePreset])] = []
    private var hasPositioned = false

    private init() {
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 300, height: 420),
            styleMask: [.titled, .closable, .resizable, .utilityWindow],
            backing: .buffered, defer: false)
        panel.title = "Styles"
        panel.isFloatingPanel = true
        panel.becomesKeyOnlyIfNeeded = true
        panel.hidesOnDeactivate = true
        panel.minSize = NSSize(width: 220, height: 240)
        panel.setFrameAutosaveName("RasterizeStylesPanel")
        panel.isReleasedWhenClosed = false
        super.init(window: panel)
        panel.contentView = makeContent()
        NotificationCenter.default.addObserver(
            self, selector: #selector(libraryDidChange(_:)), name: StylePresetLibrary.didChange,
            object: nil)
        reload()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("StylesPanelController does not support NSCoder")
    }

    func show() {
        guard let window else { return }
        if !hasPositioned {
            hasPositioned = true
            if !window.setFrameUsingName(window.frameAutosaveName) { window.center() }
        }
        reload()
        BackgroundLaunch.present(window, makeKey: false)
    }

    // MARK: - Layout

    private func makeContent() -> NSView {
        let root = NSView()
        let layout = NSCollectionViewFlowLayout()
        layout.itemSize = NSSize(width: 52, height: 52)
        layout.minimumInteritemSpacing = 4
        layout.minimumLineSpacing = 4
        layout.sectionInset = NSEdgeInsets(top: 4, left: 8, bottom: 8, right: 8)
        layout.headerReferenceSize = NSSize(width: 100, height: 22)
        grid.collectionViewLayout = layout
        grid.dataSource = self
        grid.delegate = self
        grid.isSelectable = false
        grid.backgroundColors = [DS.chromeBackground]
        grid.register(StyleSwatchItem.self, forItemWithIdentifier: StyleSwatchItem.identifier)
        grid.register(StyleGroupHeader.self,
                      forSupplementaryViewOfKind: NSCollectionView.elementKindSectionHeader,
                      withIdentifier: StyleGroupHeader.identifier)
        let scroll = NSScrollView()
        scroll.documentView = grid
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        status.font = DS.sans(11)
        status.textColor = DS.textMuted
        status.lineBreakMode = .byTruncatingTail
        let newStyle = NSButton(title: "New Style…", target: nil,
                                action: #selector(EditorViewController.newStylePreset(_:)))
        let importStyles = NSButton(title: "Import Styles…", target: self,
                                    action: #selector(importStyles(_:)))
        let exportStyles = NSButton(title: "Export Styles…", target: self,
                                    action: #selector(exportStyles(_:)))
        for button in [newStyle, importStyles, exportStyles] {
            button.bezelStyle = .rounded
            button.controlSize = .small
            button.font = DS.sans(11)
        }
        let bar = NSStackView(views: [newStyle, importStyles, exportStyles])
        bar.orientation = .horizontal
        bar.spacing = 6
        for view in [scroll, status, bar] {
            view.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(view)
        }
        NSLayoutConstraint.activate([
            scroll.topAnchor.constraint(equalTo: root.topAnchor),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -4),
            status.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 8),
            status.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -8),
            status.bottomAnchor.constraint(equalTo: bar.topAnchor, constant: -4),
            bar.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 8),
            bar.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -8),
        ])
        return root
    }

    // MARK: - Data

    @objc private func libraryDidChange(_ note: Notification) { reload() }

    func reload() {
        do {
            let (presets, broken) = try StylePresetLibrary.all()
            var grouped: [(group: String?, presets: [StylePreset])] = []
            for preset in presets {
                if let last = grouped.last, last.group == preset.group {
                    grouped[grouped.count - 1].presets.append(preset)
                } else {
                    grouped.append((preset.group, [preset]))
                }
            }
            sections = grouped
            if !broken.isEmpty {
                status.stringValue = "\(broken.count) unreadable style file"
                    + (broken.count == 1 ? "" : "s") + " in the library."
            } else {
                status.stringValue = ""
            }
        } catch {
            sections = [(StylePreset.builtinGroup, StylePresetLibrary.builtins)]
            status.stringValue = error.localizedDescription
        }
        grid.reloadData()
    }

    func numberOfSections(in collectionView: NSCollectionView) -> Int { sections.count }

    func collectionView(_ collectionView: NSCollectionView, numberOfItemsInSection section: Int) -> Int {
        sections[section].presets.count
    }

    func collectionView(_ collectionView: NSCollectionView,
                        itemForRepresentedObjectAt indexPath: IndexPath) -> NSCollectionViewItem {
        let item = collectionView.makeItem(withIdentifier: StyleSwatchItem.identifier, for: indexPath)
        if let swatch = item as? StyleSwatchItem {
            let preset = sections[indexPath.section].presets[indexPath.item]
            swatch.show(preset)
            swatch.onClick = { [weak self] add in self?.apply(preset, add: add) }
            swatch.onMenu = { [weak self] in self?.menu(for: preset) }
        }
        return item
    }

    func collectionView(_ collectionView: NSCollectionView,
                        viewForSupplementaryElementOfKind kind: NSCollectionView.SupplementaryElementKind,
                        at indexPath: IndexPath) -> NSView {
        let view = collectionView.makeSupplementaryView(
            ofKind: kind, withIdentifier: StyleGroupHeader.identifier, for: indexPath)
        if let header = view as? StyleGroupHeader {
            let group = sections[indexPath.section].group
            header.label.stringValue = group ?? "Ungrouped"
            header.onMenu = { [weak self] in self?.groupMenu(group) }
        }
        return view
    }

    // MARK: - Actions

    private func apply(_ preset: StylePreset, add: Bool) {
        let request = StylePresetRequest(preset: preset, add: add)
        if !NSApp.sendAction(#selector(EditorViewController.applyStylePreset(_:)), to: nil,
                             from: request) {
            NSSound.beep()
        }
    }

    private func menu(for preset: StylePreset) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        for (title, selector) in [("Rename…", #selector(renamePreset(_:))),
                                  ("Move to Group…", #selector(movePreset(_:))),
                                  ("Delete", #selector(deletePreset(_:)))] {
            let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            item.target = self
            item.representedObject = StylePresetRequest(preset: preset, add: false)
            item.isEnabled = !preset.builtin
            menu.addItem(item)
        }
        return menu
    }

    private func groupMenu(_ group: String?) -> NSMenu? {
        guard let group else { return nil }
        let menu = NSMenu()
        var entries = [("Export Group…", #selector(exportGroup(_:)))]
        if !StylePreset.isBuiltinGroup(group) {
            entries.insert(contentsOf: [("Rename Group…", #selector(renameGroup(_:))),
                                        ("Delete Group…", #selector(deleteGroup(_:)))], at: 0)
        }
        for (title, selector) in entries {
            let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
            item.target = self
            item.representedObject = group
            menu.addItem(item)
        }
        return menu
    }

    private func preset(of sender: Any?) -> StylePreset? {
        ((sender as? NSMenuItem)?.representedObject as? StylePresetRequest)?.preset
    }

    @objc private func renamePreset(_ sender: Any?) {
        guard let preset = preset(of: sender),
              let name = ask("Rename Style", "Style name", preset.name) else { return }
        attempt { _ = try StylePresetLibrary.rename(name: preset.name, group: preset.group,
                                                    to: name, group: preset.group) }
    }

    @objc private func movePreset(_ sender: Any?) {
        guard let preset = preset(of: sender),
              let group = ask("Move to Group", "Group name (empty for no group)", preset.group ?? "")
        else { return }
        attempt { _ = try StylePresetLibrary.rename(name: preset.name, group: preset.group,
                                                    to: preset.name, group: group) }
    }

    @objc private func deletePreset(_ sender: Any?) {
        guard let preset = preset(of: sender) else { return }
        attempt { try StylePresetLibrary.delete(name: preset.name, group: preset.group) }
    }

    @objc private func renameGroup(_ sender: Any?) {
        guard let group = (sender as? NSMenuItem)?.representedObject as? String,
              let newGroup = ask("Rename Group", "Group name", group) else { return }
        attempt {
            for preset in try StylePresetLibrary.all().presets where preset.group == group {
                _ = try StylePresetLibrary.rename(name: preset.name, group: group,
                                                  to: preset.name, group: newGroup)
            }
        }
    }

    @objc private func deleteGroup(_ sender: Any?) {
        guard let group = (sender as? NSMenuItem)?.representedObject as? String else { return }
        let alert = NSAlert()
        alert.messageText = "Delete the group “\(group)” and its styles?"
        alert.informativeText = "A style preset is a preference: deleting one changes no picture."
        alert.addButton(withTitle: "Delete Group")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        attempt {
            for preset in try StylePresetLibrary.all().presets where preset.group == group {
                try StylePresetLibrary.delete(name: preset.name, group: group)
            }
        }
    }

    /// File chooser for a Photoshop style library; the report goes in the
    /// status line, and in an alert when anything was dropped or skipped.
    @objc private func importStyles(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "asl") ?? .data]
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let data = try StylePresetLibrary.readASL(url)
            let result = try StylePresetLibrary.importASL(
                data, group: StylePresetLibrary.defaultGroup(for: url))
            status.stringValue = "Imported \(result.imported.count) style"
                + (result.imported.count == 1 ? "" : "s") + " from \(url.lastPathComponent)."
            let losses = result.dropped + result.skipped
            if !losses.isEmpty {
                let alert = NSAlert()
                alert.messageText = "Some of \(url.lastPathComponent) could not be imported"
                alert.informativeText = losses.prefix(20).joined(separator: "\n")
                    + (losses.count > 20 ? "\n…and \(losses.count - 20) more." : "")
                alert.runModal()
            }
        } catch {
            fail(error)
        }
    }

    /// Every preset of the library, built-ins included, as one `.asl`.
    @objc private func exportStyles(_ sender: Any?) {
        attempt { export(try StylePresetLibrary.all().presets, suggested: "Styles") }
    }

    /// One group's presets as one `.asl` (the built-in group's too).
    @objc private func exportGroup(_ sender: Any?) {
        guard let group = (sender as? NSMenuItem)?.representedObject as? String else { return }
        attempt {
            let presets = try StylePresetLibrary.all().presets.filter { $0.group == group }
            export(presets, suggested: StylePresetLibrary.sanitized(group))
        }
    }

    /// Save panel for a Photoshop style library; the count goes in the status
    /// line, and the presets the core could not write in an alert.
    private func export(_ presets: [StylePreset], suggested: String) {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "asl") ?? .data]
        panel.nameFieldStringValue = (suggested.isEmpty ? "Styles" : suggested) + ".asl"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let result = try StylePresetLibrary.exportASL(presets, to: url)
            status.stringValue = "Exported \(result.exported) style"
                + (result.exported == 1 ? "" : "s") + " to \(url.lastPathComponent)."
            if !result.skipped.isEmpty {
                let alert = NSAlert()
                alert.messageText = "Some styles could not be written to \(url.lastPathComponent)"
                alert.informativeText = result.skipped.prefix(20).joined(separator: "\n")
                    + (result.skipped.count > 20 ? "\n…and \(result.skipped.count - 20) more." : "")
                alert.runModal()
            }
        } catch {
            fail(error)
        }
    }

    private func attempt(_ body: () throws -> Void) {
        do { try body() } catch { fail(error) }
    }

    private func fail(_ error: Error) {
        NSSound.beep()
        status.stringValue = error.localizedDescription
    }

    private func ask(_ title: String, _ placeholder: String, _ value: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 260, height: 24))
        DSField.style(field)
        field.placeholderString = placeholder
        field.stringValue = value
        alert.accessoryView = field
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return field.stringValue
    }
}

/// One swatch: the preset's thumbnail, its name as the tooltip; a click
/// reports whether Shift was held, a right click asks for the menu.
final class StyleSwatchItem: NSCollectionViewItem {
    static let identifier = NSUserInterfaceItemIdentifier("StyleSwatchItem")

    var onClick: ((Bool) -> Void)?
    var onMenu: (() -> NSMenu?)?

    override init(nibName: NSNib.Name?, bundle: Bundle?) {
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("StyleSwatchItem does not support NSCoder")
    }

    override func loadView() {
        let swatch = StyleSwatchView()
        swatch.item = self
        view = swatch
    }

    func show(_ preset: StylePreset) {
        guard let swatch = view as? StyleSwatchView else { return }
        swatch.image = StylePresetThumbnail.image(for: preset.style)
        swatch.toolTip = preset.name
        swatch.setAccessibilityLabel(preset.name)
        swatch.needsDisplay = true
    }
}

final class StyleSwatchView: NSView {
    weak var item: StyleSwatchItem?
    var image: NSImage?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("StyleSwatchView does not support NSCoder")
    }

    override func draw(_ dirtyRect: NSRect) {
        let frame = bounds.insetBy(dx: 2, dy: 2)
        DS.border.setStroke()
        NSBezierPath(roundedRect: frame, xRadius: 4, yRadius: 4).stroke()
        image?.draw(in: frame.insetBy(dx: 1, dy: 1))
    }

    override func mouseDown(with event: NSEvent) {
        // Shift comes from the click itself (`NSApp.currentEvent` is this
        // event while it is being handled).
        let shift = (NSApp.currentEvent ?? event).modifierFlags.contains(.shift)
        item?.onClick?(shift)
    }

    override func menu(for event: NSEvent) -> NSMenu? { item?.onMenu?() }
}

/// A group's section header: its name, and the group menu on a right click.
final class StyleGroupHeader: NSView, NSCollectionViewElement {
    static let identifier = NSUserInterfaceItemIdentifier("StyleGroupHeader")

    let label = NSTextField(labelWithString: "")
    var onMenu: (() -> NSMenu?)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        label.font = DS.sans(11, weight: .semibold)
        label.textColor = DS.textMuted
        label.translatesAutoresizingMaskIntoConstraints = false
        addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("StyleGroupHeader does not support NSCoder")
    }

    override func menu(for event: NSEvent) -> NSMenu? { onMenu?() }
}
