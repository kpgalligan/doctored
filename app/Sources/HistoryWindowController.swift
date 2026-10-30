import AppKit

// `HistoryWindowController` and MCP expose navigation, budgets and the budget exception (the
// nearest Undo is kept even when it alone exceeds the budget). A row's context menu sets
// the History Brush's source state, marked "✎".

/// An enumerable state panel; choosing a row follows the document's real Undo
/// and Redo registrations, including selections and grouped Actions.
final class HistoryWindowController: NSWindowController, NSTableViewDataSource, NSTableViewDelegate {
    private weak var imageDocument: ImageDocument?
    private let table = NSTableView()
    private let usage = NSTextField(wrappingLabelWithString: "")
    private let warning = NSTextField(wrappingLabelWithString: "")
    private let budget = NSPopUpButton(frame: .zero, pullsDown: false)
    private var refreshing = false

    init(document: ImageDocument) {
        imageDocument = document
        let panel = JobEscapePanel(contentRect: NSRect(x: 0, y: 0, width: 420, height: 410),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        super.init(window: panel)
        panel.title = "History — \(document.displayName ?? "Image")"
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.jobDocument = { [weak document] in document }
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("state"))
        column.title = "Editing States"
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 28
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.action = #selector(restoreSelected(_:))
        // The row menu names the History Brush's source state.
        let rowMenu = NSMenu()
        let sourceItem = NSMenuItem(title: "Set History Brush Source",
            action: #selector(setBrushSource(_:)), keyEquivalent: "")
        sourceItem.target = self
        rowMenu.addItem(sourceItem)
        table.menu = rowMenu
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.translatesAutoresizingMaskIntoConstraints = false
        let hint = NSTextField(wrappingLabelWithString:
            "Choose a state to undo or redo to it. A new edit replaces later states. History lasts for this session.")
        hint.font = DS.sans(12)
        hint.textColor = DS.textMuted
        usage.font = DS.mono(11)
        usage.textColor = DS.textMuted
        warning.font = DS.sans(12)
        warning.textColor = DS.warningText
        // Tag = 1-based preset index; 0 is the Custom item.
        for (index, bytes) in DocumentHistory.budgetPresets.enumerated() {
            budget.addItem(withTitle: Self.presetTitle(bytes))
            budget.lastItem?.tag = index + 1
        }
        budget.target = self
        budget.action = #selector(changeBudget(_:))
        let budgetRow = NSStackView(views: [NSTextField(labelWithString: "History budget"), budget])
        budgetRow.spacing = 10
        let stack = NSStackView(views: [hint, scroll, usage, warning, budgetRow])
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
            scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 140),
            hint.widthAnchor.constraint(equalTo: stack.widthAnchor),
            usage.widthAnchor.constraint(equalTo: stack.widthAnchor),
            warning.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])
        panel.minSize = NSSize(width: 380, height: 300)
        NotificationCenter.default.addObserver(self, selector: #selector(reload),
            name: .imageDocumentHistoryDidChange, object: document)
        reload()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("HistoryWindowController does not support NSCoder") }
    deinit { NotificationCenter.default.removeObserver(self) }

    func numberOfRows(in tableView: NSTableView) -> Int { imageDocument?.history.entries.count ?? 0 }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let document = imageDocument, document.history.entries.indices.contains(row) else { return nil }
        let entry = document.history.entries[row]
        let saved = entry.state.document === document.historySavedDocument ? " · Saved" : ""
        // "✎" marks the state the History Brush paints from.
        let source = entry.id == document.history.brushSource()?.id ? "✎ " : ""
        let label = NSTextField(labelWithString: "\(row == document.history.cursor ? "● " : "")\(source)\(entry.name)\(saved)")
        label.font = DS.sans(12)
        return label
    }

    @objc private func reload() {
        guard let document = imageDocument else { return }
        refreshing = true
        defer { refreshing = false }
        _ = document.history.report()
        table.reloadData()
        table.selectRowIndexes(IndexSet(integer: document.history.cursor), byExtendingSelection: false)
        let history = document.history
        let format: (UInt64) -> String = {
            ByteCountFormatter.string(fromByteCount: Int64(min($0, UInt64(Int64.max))), countStyle: .memory)
        }
        usage.stringValue = "\(format(history.incrementalBytes)) additional / \(format(history.budgetBytes)) · \(history.evictedCount) older states removed"
        if let error = history.accountingError {
            warning.stringValue = "History memory could not be measured: \(error)"
        } else if history.isOverBudget, history.entries.count > 2 {
            warning.stringValue = "These retained states exceed the incremental budget after navigation. The next edit, save or budget change trims them."
        } else if history.isOverBudget {
            warning.stringValue = "One reversible step alone exceeds the budget. It is retained so this edit can still be undone or redone."
        } else { warning.stringValue = "" }
        warning.isHidden = warning.stringValue.isEmpty
        for item in budget.itemArray where item.tag == 0 { budget.menu?.removeItem(item) }
        if let index = DocumentHistory.budgetPresets.firstIndex(of: history.budgetBytes) {
            budget.selectItem(withTag: index + 1)
        } else {
            budget.addItem(withTitle: "Custom (\(format(history.budgetBytes)))")
            budget.lastItem?.tag = 0
            budget.selectItem(withTag: 0)
        }
    }

    @objc private func restoreSelected(_ sender: Any?) {
        guard !refreshing, let document = imageDocument,
              document.history.entries.indices.contains(table.selectedRow) else { return }
        // ⌘Z's rule: choosing another state while this document's
        // UI-started job runs cancels the job instead of navigating. The
        // current row stays a no-op; an agent's or a draining job navigates.
        if table.selectedRow != document.history.cursor, DocumentJobs.cancelUserJob(on: document) {
            reload()
            return
        }
        do { try document.navigateHistory(toID: document.history.entries[table.selectedRow].id) }
        catch { presentError(error); reload() }
    }

    /// The row menu's item: the clicked state becomes the History Brush's
    /// source. Nothing in the document changes, so there is no undo step.
    @objc private func setBrushSource(_ sender: Any?) {
        guard let document = imageDocument,
              document.history.entries.indices.contains(table.clickedRow) else {
            NSSound.beep()
            return
        }
        document.history.brushSourceID = document.history.entries[table.clickedRow].id
        document.history.notify()
    }

    @objc private func changeBudget(_ sender: Any?) {
        let presets = DocumentHistory.budgetPresets
        guard let document = imageDocument, let selected = budget.selectedItem,
              presets.indices.contains(selected.tag - 1) else { return }
        do { try document.setHistoryBudget(presets[selected.tag - 1]) }
        catch { presentError(error); reload() }
    }

    /// Whole GiB as "N GiB", else whole MiB as "N MiB", else the formatter's string.
    private static func presetTitle(_ bytes: UInt64) -> String {
        let mib: UInt64 = 1_048_576
        if bytes % (1024 * mib) == 0 { return "\(bytes / (1024 * mib)) GiB" }
        if bytes % mib == 0 { return "\(bytes / mib) MiB" }
        return ByteCountFormatter.string(fromByteCount: Int64(min(bytes, UInt64(Int64.max))), countStyle: .memory)
    }
}
