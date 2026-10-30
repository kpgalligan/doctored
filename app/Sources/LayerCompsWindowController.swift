import AppKit

// A comp records no pointer back to itself, so the window's ◀ ▶ step from
// `ImageDocument.lastAppliedLayerComp`, which is VIEW bookkeeping — not editing state, never
// persisted, never undone. Layer Comps is a WINDOW rather than a panel tab, on the Snapshots
// precedent.

/// Window ▸ Layer Comps… — the list of a document's comps and the six
/// commands over it, on `SnapshotsWindowController`'s shape.
///
/// A WINDOW rather than a right-panel tab, for the Snapshots precedent: the
/// list is consulted beside the canvas while comps are cycled, and the panel's
/// six tabs are already the per-document inspectors. Every button goes
/// through an `ImageDocument` entry point, which is the one shared writer, so
/// the window registers no undo of its own and holds no model state beyond the
/// table's selection.
final class LayerCompsWindowController: NSWindowController, NSTableViewDataSource,
    NSTableViewDelegate
{
    private weak var imageDocument: ImageDocument?
    private let table = NSTableView()
    private let status = NSTextField(wrappingLabelWithString: "")
    private var comps: [LayerComp] = []
    /// The comp a drag just moved, selected by the next `reload()` wherever
    /// it lands — the Actions window's `pendingStepSelection` pattern.
    private var pendingSelection: String?

    /// The dragged-row pasteboard type, private to this table — the simple
    /// half of the Layers panel's reorder pattern.
    private static let compRowType = NSPasteboard.PasteboardType(
        "com.kgalligan.doctored.layercomprow")

    init(document: ImageDocument) {
        imageDocument = document
        let panel = JobEscapePanel(
            contentRect: NSRect(x: 0, y: 0, width: 470, height: 420),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        super.init(window: panel)
        panel.title = "Layer Comps — \(document.displayName ?? "Image")"
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.jobDocument = { [weak document] in document }
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("comp"))
        column.title = "Layer Comps"
        table.addTableColumn(column)
        table.headerView = nil
        table.rowHeight = 28
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(applyComp(_:))
        table.setAccessibilityIdentifier("layer-comps-table")
        table.registerForDraggedTypes([Self.compRowType])
        table.setDraggingSourceOperationMask(.move, forLocal: true)
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.translatesAutoresizingMaskIntoConstraints = false
        status.font = DS.mono(11)
        status.textColor = DS.textMuted
        let hint = NSTextField(wrappingLabelWithString:
            "A comp records per-layer visibility, position and appearance by layer identity — "
            + "never pixels, the selection or the history, which is what a Snapshot holds. "
            + "V P A shows what each comp captures; a stale entry names a layer that is gone, "
            + "and a locked one a position the layer's lock forbids. "
            + "Drag a comp to reorder it; ◀ ▶ step through the list in this order.")
        hint.font = DS.sans(12)
        hint.textColor = DS.textMuted
        let buttons = NSStackView(views: [
            button("New…", #selector(newComp(_:))),
            button("Apply", #selector(applyComp(_:))),
            button("◀", #selector(applyPrevious(_:))),
            button("▶", #selector(applyNext(_:))),
            button("Update", #selector(updateComp(_:))),
            button("Options…", #selector(showOptions(_:))),
            button("Delete", #selector(deleteComp(_:))),
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
        panel.minSize = NSSize(width: 500, height: 360)
        NotificationCenter.default.addObserver(
            self, selector: #selector(documentChanged(_:)),
            name: .imageDocumentImageDidChange, object: document)
        reload()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("LayerCompsWindowController does not support NSCoder")
    }
    deinit { NotificationCenter.default.removeObserver(self) }

    func numberOfRows(in tableView: NSTableView) -> Int { comps.count }

    func tableView(
        _ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int
    ) -> NSView? {
        guard comps.indices.contains(row) else { return nil }
        let comp = comps[row]
        var badges: [String] = [comp.captures.glyphs]
        if comp.staleCount > 0 { badges.append("stale \(comp.staleCount)") }
        if comp.lockedCount > 0 { badges.append("locked \(comp.lockedCount)") }
        let label = NSTextField(
            labelWithString: "\(comp.name)  ·  \(badges.joined(separator: "  ·  "))")
        label.font = DS.sans(12)
        label.toolTip = comp.comment.isEmpty ? nil : comp.comment
        return label
    }

    private var selectedComp: LayerComp? {
        comps.indices.contains(table.selectedRow) ? comps[table.selectedRow] : nil
    }

    /// Re-reads the list from the core and keeps the selection on the comp
    /// that was selected (or the one a drag just moved), by NAME — an index
    /// moves when one is deleted or reordered.
    @objc private func reload() {
        let name = pendingSelection ?? selectedComp?.name
        pendingSelection = nil
        comps = (try? imageDocument?.requireLayerComps()) ?? []
        table.reloadData()
        if let name, let index = comps.firstIndex(where: { $0.name == name }) {
            table.selectRowIndexes(IndexSet(integer: index), byExtendingSelection: false)
        }
        let stale = comps.reduce(0) { $0 + $1.staleCount }
        let locked = comps.reduce(0) { $0 + $1.lockedCount }
        status.stringValue = "\(comps.count) comps · \(stale) stale entries · \(locked) locked"
    }

    // MARK: Drag reorder

    /// The dragged row's number.
    func tableView(
        _ tableView: NSTableView, pasteboardWriterForRow row: Int
    ) -> NSPasteboardWriting? {
        guard comps.indices.contains(row) else { return nil }
        let item = NSPasteboardItem()
        item.setString(String(row), forType: Self.compRowType)
        return item
    }

    /// The list is flat — there is nothing to drop ONTO — so every drop is an
    /// insertion between rows.
    func tableView(
        _ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int,
        proposedDropOperation dropOperation: NSTableView.DropOperation
    ) -> NSDragOperation {
        guard info.draggingPasteboard.availableType(from: [Self.compRowType]) != nil else {
            return []
        }
        if dropOperation == .on { tableView.setDropRow(row, dropOperation: .above) }
        return .move
    }

    /// Moves the dragged comp so it sits at the insertion point `row` (a
    /// between-rows index, 0…count). The core counts `to` with the comp
    /// already taken out, so an insertion below the source is one less; a
    /// drop onto the comp's own edges touches nothing.
    func tableView(
        _ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int,
        dropOperation: NSTableView.DropOperation
    ) -> Bool {
        guard let document = imageDocument,
              let text = info.draggingPasteboard.string(forType: Self.compRowType),
              let source = Int(text), comps.indices.contains(source)
        else { return false }
        let to = row > source ? row - 1 : row
        guard to != source else { return false }
        let name = comps[source].name
        pendingSelection = name
        var moved = false
        run {
            moved = try document.reorderLayerComp(named: name, to: to)
            return nil
        }
        pendingSelection = nil
        return moved
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

    @objc private func newComp(_ sender: Any?) {
        guard let document = imageDocument, let result = LayerCompOptionsSheetController.runNew()
        else { return }
        run {
            try document.saveLayerComp(
                name: result.name, comment: result.comment, captures: result.captures)
            return nil
        }
    }

    @objc private func applyComp(_ sender: Any?) {
        guard let document = imageDocument, let comp = selectedComp else {
            NSSound.beep()
            return
        }
        run { try document.applyLayerComp(named: comp.name).caveat.map { "Applied with \($0)." } }
    }

    @objc private func applyPrevious(_ sender: Any?) { step(-1) }
    @objc private func applyNext(_ sender: Any?) { step(1) }

    private func step(_ delta: Int) {
        guard let document = imageDocument else {
            NSSound.beep()
            return
        }
        run {
            try document.applyAdjacentLayerComp(step: delta).caveat
                .map { "Applied with \($0)." }
        }
    }

    /// Re-records the selected comp from the document's current state, with
    /// its name, comment and flags untouched.
    @objc private func updateComp(_ sender: Any?) {
        guard let document = imageDocument, let comp = selectedComp else {
            NSSound.beep()
            return
        }
        run {
            try document.updateLayerComp(
                named: comp.name, newName: nil, comment: nil, captures: nil, recapture: true)
            return nil
        }
    }

    @objc private func showOptions(_ sender: Any?) {
        guard let document = imageDocument, let comp = selectedComp,
              let result = LayerCompOptionsSheetController.runOptions(for: comp)
        else {
            if selectedComp == nil { NSSound.beep() }
            return
        }
        run {
            try document.updateLayerComp(
                named: comp.name, newName: result.name, comment: result.comment,
                captures: result.captures, recapture: result.recapture)
            return nil
        }
    }

    @objc private func deleteComp(_ sender: Any?) {
        guard let document = imageDocument, let comp = selectedComp else {
            NSSound.beep()
            return
        }
        run {
            try document.deleteLayerComp(named: comp.name)
            return nil
        }
    }

    /// Every button's body. A body may hand back one sentence — what an apply
    /// could not honour — which replaces the reloaded status line and beeps.
    /// A refusal is the CORE's own sentence (a duplicate name, the 64-comp
    /// limit), shown there too and never swallowed.
    private func run(_ body: () throws -> String?) {
        do {
            let note = try body()
            reload()
            if let note {
                NSSound.beep()
                status.stringValue = note
            }
        } catch {
            NSSound.beep()
            status.stringValue = error.localizedDescription
        }
    }
}

extension EditorViewController {
    @objc func showLayerComps(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        if document.layerCompsWindowController == nil {
            document.layerCompsWindowController = LayerCompsWindowController(document: document)
        }
        document.layerCompsWindowController?.showWindow(sender)
    }
}
