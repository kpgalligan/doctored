import AppKit

/// Nonmodal discovery and failure status; recovery errors must not lock the
/// photograph behind an automatic autosave alert or repeatedly interrupt edits.
final class RecoveryWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate {
    private weak var coordinator: RecoveryCoordinator?
    private let table = NSTableView()
    private let status = NSTextField(wrappingLabelWithString: "")
    private let recoverButton = NSButton(title: "Recover Copy", target: nil, action: nil)
    private let discardButton = NSButton(title: "Discard…", target: nil, action: nil)
    private var entries: [RecoveryStore.Candidate] = []

    init(coordinator: RecoveryCoordinator) {
        self.coordinator = coordinator
        let window = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 640, height: 360),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        super.init(window: window)
        window.title = "Recover Unsaved Work"
        window.isReleasedWhenClosed = false
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("recovery"))
        column.title = "Recovery Copies"
        column.width = 590
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 44
        table.dataSource = self
        table.delegate = self
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 190).isActive = true
        status.font = DS.sans(12)
        status.textColor = DS.textMuted
        recoverButton.target = self; recoverButton.action = #selector(recover(_:))
        discardButton.target = self; discardButton.action = #selector(discard(_:))
        let checkpoint = NSButton(title: "Save Recovery Now", target: self, action: #selector(checkpoint(_:)))
        let refresh = NSButton(title: "Refresh", target: self, action: #selector(refresh(_:)))
        let row = NSStackView(views: [refresh, checkpoint, NSView(), discardButton, recoverButton])
        row.orientation = .horizontal
        row.spacing = 8
        let stack = NSStackView(views: [status, scroll, row])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.edgeInsets = NSEdgeInsets(top: 18, left: 18, bottom: 18, right: 18)
        scroll.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -36).isActive = true
        row.widthAnchor.constraint(equalTo: scroll.widthAnchor).isActive = true
        status.widthAnchor.constraint(equalTo: scroll.widthAnchor).isActive = true
        window.contentView = stack
        window.center()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("RecoveryWindowController does not support NSCoder") }

    func reload() {
        guard let coordinator else { return }
        let selectedID = entries.indices.contains(table.selectedRow) ? entries[table.selectedRow].id : nil
        do {
            entries = try coordinator.available()
            let notices = coordinator.offNotices
            status.stringValue = notices.isEmpty
                ? "Recovery copies preserve unsaved native editing state. Recover opens a new copy; original files stay unchanged."
                : "These open documents are not protected against a crash:\n" + notices.joined(separator: "\n")
        } catch { entries = []; status.stringValue = error.localizedDescription }
        table.reloadData()
        if let selectedID, let row = entries.firstIndex(where: { $0.id == selectedID }) {
            table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }
        updateButtons()
    }

    func numberOfRows(in tableView: NSTableView) -> Int { entries.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard entries.indices.contains(row) else { return nil }
        let entry = entries[row]
        let title = entry.manifest?.title ?? "Unfinished or damaged recovery"
        let detail = entry.manifest.map { DateFormatter.localizedString(from: $0.current.captured, dateStyle: .medium, timeStyle: .short) }
            ?? entry.failure ?? "Cannot read this recovery copy."
        let label = NSTextField(wrappingLabelWithString: "\(title)\n\(detail)")
        label.font = DS.sans(12)
        return label
    }

    func tableViewSelectionDidChange(_ notification: Notification) { updateButtons() }

    private func updateButtons() {
        let valid = entries.indices.contains(table.selectedRow)
        recoverButton.isEnabled = valid && entries[table.selectedRow].manifest != nil
        discardButton.isEnabled = valid
    }

    @objc private func refresh(_ sender: Any?) { reload() }

    @objc private func checkpoint(_ sender: Any?) {
        guard let document = ActionPlayer.frontDocument() else { NSSound.beep(); return }
        do { try coordinator?.checkpoint(document, wait: true); reload() }
        catch { NSApp.presentError(error) }
    }

    @objc private func recover(_ sender: Any?) {
        guard entries.indices.contains(table.selectedRow) else { return }
        do {
            let result = try coordinator?.recover(id: entries[table.selectedRow].id, show: true)
            reload()
            if let warning = result?.1 { status.stringValue = warning; window?.orderFront(nil) }
        } catch { NSApp.presentError(error) }
    }

    @objc private func discard(_ sender: Any?) {
        guard entries.indices.contains(table.selectedRow) else { return }
        let id = entries[table.selectedRow].id
        let alert = NSAlert()
        alert.messageText = "Discard this recovery copy?"
        alert.informativeText = "Its unsaved changes and recovered session snapshots will be removed. Original files are unchanged."
        alert.addButton(withTitle: "Discard Recovery")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        do { try coordinator?.discard(id: id); reload() }
        catch { NSApp.presentError(error) }
    }
}
