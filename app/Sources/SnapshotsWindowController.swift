import AppKit
import UniformTypeIdentifiers

/// Window > Snapshots. Session captures are deliberately distinct from their
/// portable Save/Load files, with the persistence policy visible beside them.
final class SnapshotsWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate {
    private weak var imageDocument: ImageDocument?
    private let table = NSTableView()
    private let status = NSTextField(wrappingLabelWithString: "")

    init(document: ImageDocument) {
        imageDocument = document
        let panel = JobEscapePanel(contentRect: NSRect(x: 0, y: 0, width: 470, height: 420),
                            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        super.init(window: panel)
        panel.title = "Snapshots — \(document.displayName ?? "Image")"
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.jobDocument = { [weak document] in document }
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("name"))
        column.title = "Session Snapshots"
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 28
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(restore(_:))
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.translatesAutoresizingMaskIntoConstraints = false
        status.font = DS.mono(11)
        status.textColor = DS.textMuted
        let hint = NSTextField(wrappingLabelWithString:
            "Snapshots hold the full image, selection and edit target. Save a .drsnapshot file to keep one after closing. Ordinary .doctored saves do not embed this list.")
        hint.font = DS.sans(12)
        hint.textColor = DS.textMuted
        let buttons = NSStackView(views: [
            button("New…", #selector(create(_:))), button("Restore", #selector(restore(_:))),
            button("Rename…", #selector(rename(_:))), button("Remove", #selector(remove(_:))),
            button("Save…", #selector(save(_:))), button("Load…", #selector(loadSnapshot(_:))),
        ])
        buttons.spacing = 6
        let stack = NSStackView(views: [hint, scroll, status, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        let content = NSView()
        content.addSubview(stack)
        panel.contentView = content
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 16),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -16),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 16),
            stack.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -16),
            scroll.widthAnchor.constraint(equalTo: stack.widthAnchor),
            scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 160),
            hint.widthAnchor.constraint(equalTo: stack.widthAnchor),
            status.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])
        panel.minSize = NSSize(width: 500, height: 340)
        NotificationCenter.default.addObserver(self, selector: #selector(reload),
                                               name: .imageDocumentSnapshotsDidChange, object: document)
        NotificationCenter.default.addObserver(self, selector: #selector(documentChanged(_:)),
                                               name: .imageDocumentImageDidChange, object: document)
        reload()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("SnapshotsWindowController does not support NSCoder") }
    deinit { NotificationCenter.default.removeObserver(self) }

    func numberOfRows(in tableView: NSTableView) -> Int { imageDocument?.namedSnapshots.entries.count ?? 0 }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let entries = imageDocument?.namedSnapshots.entries, entries.indices.contains(row) else { return nil }
        let entry = entries[row]
        let label = NSTextField(labelWithString: "\(entry.name)  ·  \(entry.state.document.width) × \(entry.state.document.height)")
        label.font = DS.sans(12)
        return label
    }

    private var selectedID: String? {
        guard let entries = imageDocument?.namedSnapshots.entries, entries.indices.contains(table.selectedRow) else { return nil }
        return entries[table.selectedRow].id
    }

    @objc private func reload() {
        let id = selectedID
        table.reloadData()
        if let id, let index = imageDocument?.namedSnapshots.entries.firstIndex(where: { $0.id == id }) {
            table.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
        }
        guard let report = try? imageDocument?.namedSnapshots.report(),
              let used = report["incremental_bytes"] as? UInt64,
              let budget = report["budget_bytes"] as? UInt64 else { return }
        status.stringValue = "\(used / 1_048_576) MiB additional / \(budget / 1_048_576) MiB budget"
    }

    @objc private func documentChanged(_ note: Notification) {
        guard note.userInfo?["isLive"] as? Bool != true else { return }
        reload()
    }

    private func button(_ title: String, _ action: Selector) -> NSButton {
        let button = NSButton(title: title, target: self, action: action)
        button.bezelStyle = .rounded
        return button
    }

    private func name(_ title: String, current: String = "Snapshot") -> String? {
        let alert = NSAlert()
        alert.messageText = title
        let field = NSTextField(string: current)
        field.frame = NSRect(x: 0, y: 0, width: 300, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        return alert.runModal() == .alertFirstButtonReturn ? field.stringValue : nil
    }

    @objc private func create(_ sender: Any?) {
        guard let name = name("New Snapshot") else { return }
        run { _ = try self.imageDocument?.namedSnapshots.create(name: name) }
    }

    @objc private func restore(_ sender: Any?) {
        guard let id = selectedID else { NSSound.beep(); return }
        run { try self.imageDocument?.namedSnapshots.restore(id: id) }
    }

    @objc private func rename(_ sender: Any?) {
        guard let id = selectedID,
              let entry = imageDocument?.namedSnapshots.entries.first(where: { $0.id == id }),
              let name = name("Rename Snapshot", current: entry.name) else { return }
        run { try self.imageDocument?.namedSnapshots.rename(id: id, name: name) }
    }

    @objc private func remove(_ sender: Any?) {
        guard let id = selectedID else { NSSound.beep(); return }
        run { try self.imageDocument?.namedSnapshots.delete(id: id) }
    }

    @objc private func save(_ sender: Any?) {
        guard let id = selectedID else { NSSound.beep(); return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(exportedAs: "com.kgalligan.doctored-snapshot")]
        panel.nameFieldStringValue = "Snapshot." + SideFileExtension.snapshot.current
        guard panel.runModal() == .OK, let url = panel.url else { return }
        run { try self.imageDocument?.namedSnapshots.save(id: id, to: url) }
    }

    @objc private func loadSnapshot(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(exportedAs: "com.kgalligan.doctored-snapshot")]
        panel.canChooseDirectories = false
        panel.treatsFilePackagesAsDirectories = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        run { _ = try self.imageDocument?.namedSnapshots.load(from: url) }
    }

    private func run(_ body: () throws -> Void) {
        do { try body(); reload() } catch { NSSound.beep(); NSApp.presentError(error) }
    }
}

extension EditorViewController {
    @objc func showSnapshots(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        if document.snapshotsWindowController == nil {
            document.snapshotsWindowController = SnapshotsWindowController(document: document)
        }
        document.snapshotsWindowController?.showWindow(sender)
    }
}
