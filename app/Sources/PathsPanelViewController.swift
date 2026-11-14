import AppKit

/// One row of the Paths panel: the active layer's vector mask, or a path of
/// the document's list.
enum PathRow: Equatable {
    case vectorMask(layerID: UInt64, layerName: String, info: VectorMaskInfo)
    case path(PathEntry)

    /// What selecting the row makes the document's current path.
    var target: PathTarget {
        switch self {
        case .vectorMask(let layerID, _, _): return .vectorMask(layerID: layerID)
        case .path(let entry): return .path(entry.id)
        }
    }

    var shape: PathShape {
        switch self {
        case .vectorMask(_, _, let info): return info.shape
        case .path(let entry): return entry.shape
        }
    }

    var title: String {
        switch self {
        case .vectorMask(_, let layerName, _): return "\(layerName) Vector Mask"
        case .path(let entry): return entry.name
        }
    }

    /// The mono meta line: what the row is, and how big.
    var subtitle: String {
        let count = shape.anchorCount
        let anchors = count == 1 ? "1 anchor" : "\(count) anchors"
        switch self {
        case .vectorMask(_, _, let info):
            let state = info.enabled ? (info.shape.isEmpty ? "reveals all" : anchors) : "disabled"
            return "vector mask · \(state)" + (info.linked ? "" : " · unlinked")
        case .path(let entry):
            return (entry.kind == .work ? "work path · " : "path · ") + anchors
                + (entry.clipping.map(Self.clippingNote) ?? "")
        }
    }

    /// The clipping path's part of the subtitle, its flatness when it has one.
    private static func clippingNote(_ clipping: PathClipping) -> String {
        guard clipping.flatness > 0 else { return " · clipping path" }
        let flatness = clipping.flatness.formatted(.number.precision(.fractionLength(0...4)))
        return " · clipping path, flatness \(flatness) px"
    }

    var isVectorMask: Bool {
        if case .vectorMask = self { return true }
        return false
    }
}

/// The Paths tab of the right panel: the active layer's vector mask
/// (while it has one), the work path, then every saved path.
///
/// Selecting a row makes it the document's CURRENT PATH — the one the canvas
/// outlines and the Pen edits — and clicking the empty part of the list
/// deselects, which hides the outline. That is view state, like the Channels
/// panel's row: no undo step, no dirty flag. Everything that changes the
/// document goes out through the same nil-target actions the Select and Layer
/// menus send (`EditorViewController+Paths.swift`, `+VectorMasks.swift`), so
/// the footer, the row menu and the menu bar share one set of handlers; the
/// MCP twins are in `AgentServer+Paths.swift`.
final class PathsPanelViewController: NSViewController {
    weak var document: ImageDocument?

    /// Tab switches, mirroring the other five panels.
    var onShowLayers: (() -> Void)?
    var onShowChannels: (() -> Void)?
    var onShowAssistant: (() -> Void)?
    var onShowInfo: (() -> Void)?
    var onShowNavigator: (() -> Void)?

    /// A row was selected, or the selection was cleared (nil): the editor
    /// makes it the document's current path and refreshes the canvas.
    var onSelectTarget: ((PathTarget?) -> Void)?

    /// A path row's inline rename committed: the path's stable id and the
    /// new name. The id, not the row: the field editor outlives the row it
    /// was opened on.
    var onRenamePath: ((UInt64, String) -> Void)?

    private let tableView = CanvasKeyTableView()
    private let tableScroll = NSScrollView()
    private let rowMenu = NSMenu()
    private(set) var rows: [PathRow] = []
    private var fillButton: NSButton!
    private var strokeButton: NSButton!
    private var selectionButton: NSButton!
    private var workPathButton: NSButton!
    private var maskButton: NSButton!
    private var deleteButton: NSButton!
    private let countLabel = NSTextField(labelWithString: "")

    /// Guards programmatic (re)selection so it never round-trips back out as
    /// a user selection — the Layers panel's pattern.
    private var isReloading = false
    /// A change that arrived while the tab was hidden; consumed by
    /// `setPanelVisible`.
    private var needsReload = false
    /// Whether the editor has a selection to trace — pushed in, because the
    /// footer's buttons are nil-target actions AppKit never validates.
    private var hasSelection = false

    init() {
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PathsPanelViewController does not support NSCoder")
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    // MARK: - View construction

    override func loadView() {
        let root = NSView(frame: NSRect(x: 0, y: 0, width: DS.panelWidth, height: 400))
        root.wantsLayer = true
        root.layer?.backgroundColor = DS.chromeBackground.cgColor

        let tab = PanelTabsView(
            titles: EditorPanelTab.titles, activeIndex: EditorPanelTab.paths.rawValue
        ) { [weak self] index in
            switch EditorPanelTab(rawValue: index) {
            case .layers?: self?.onShowLayers?()
            case .channels?: self?.onShowChannels?()
            case .assistant?: self?.onShowAssistant?()
            case .info?: self?.onShowInfo?()
            case .navigator?: self?.onShowNavigator?()
            case .paths?, nil: break
            }
        }
        tab.translatesAutoresizingMaskIntoConstraints = false

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("path"))
        column.width = DS.panelWidth - 20
        column.resizingMask = .autoresizingMask
        tableView.addTableColumn(column)
        tableView.headerView = nil
        // Keyboard focus stays on the canvas, as in the Layers and Channels
        // panels: rows select by click, renaming focuses its own field.
        tableView.refusesFirstResponder = true
        tableView.rowHeight = DS.layerRow
        tableView.allowsMultipleSelection = false
        // No row selected is a real state: no current path, nothing outlined.
        tableView.allowsEmptySelection = true
        tableView.dataSource = self
        tableView.delegate = self
        tableView.target = self
        tableView.doubleAction = #selector(rowDoubleClicked(_:))
        rowMenu.delegate = self
        tableView.menu = rowMenu
        tableView.backgroundColor = .clear
        tableView.selectionHighlightStyle = .regular
        tableView.style = .plain

        tableScroll.translatesAutoresizingMaskIntoConstraints = false
        tableScroll.documentView = tableView
        tableScroll.hasVerticalScroller = true
        tableScroll.autohidesScrollers = true
        tableScroll.drawsBackground = false

        fillButton = GhostButton(
            symbol: "circle.fill", fallback: "●", caption: nil,
            tooltip: "Fill Path with Foreground Color",
            action: #selector(EditorViewController.fillCurrentPath(_:)))
        strokeButton = GhostButton(
            symbol: "circle", fallback: "○", caption: nil, tooltip: "Stroke Path with Brush",
            action: #selector(EditorViewController.strokeCurrentPath(_:)))
        selectionButton = GhostButton(
            symbol: "circle.dashed", fallback: "◌", caption: nil,
            tooltip: "Make Selection from Path…",
            action: #selector(EditorViewController.makeSelectionFromPath(_:)))
        workPathButton = GhostButton(
            symbol: "scribble", fallback: "∿", caption: nil,
            tooltip: "Make Work Path from Selection…",
            action: #selector(EditorViewController.makeWorkPath(_:)))
        maskButton = GhostButton(
            symbol: "circle.rectangle.filled.pattern.diagonalline", fallback: "◩", caption: nil,
            tooltip: "Add Vector Mask from Path",
            action: #selector(EditorViewController.addVectorMaskFromCurrentPath(_:)))
        deleteButton = GhostButton(
            symbol: "trash", fallback: "✕", caption: nil, tooltip: "Delete",
            action: #selector(EditorViewController.deleteCurrentPath(_:)))

        countLabel.translatesAutoresizingMaskIntoConstraints = false
        countLabel.font = DS.mono(10)
        countLabel.textColor = DS.textFaint
        countLabel.alignment = .right

        let footerSeparator = NSView()
        footerSeparator.translatesAutoresizingMaskIntoConstraints = false
        footerSeparator.wantsLayer = true

        let footerSpacer = NSView()
        let footer = NSStackView(views: [
            fillButton, strokeButton, selectionButton, workPathButton, maskButton, deleteButton,
            footerSpacer, countLabel,
        ])
        footer.translatesAutoresizingMaskIntoConstraints = false
        footer.orientation = .horizontal
        footer.spacing = 2
        footer.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 12)

        root.addSubview(tab)
        root.addSubview(tableScroll)
        root.addSubview(footerSeparator)
        root.addSubview(footer)

        NSLayoutConstraint.activate([
            tab.topAnchor.constraint(equalTo: root.topAnchor),
            tab.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            tab.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            tab.heightAnchor.constraint(equalToConstant: DS.tabHeight),

            tableScroll.topAnchor.constraint(equalTo: tab.bottomAnchor, constant: 10),
            tableScroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            tableScroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),

            footerSeparator.topAnchor.constraint(equalTo: tableScroll.bottomAnchor),
            footerSeparator.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            footerSeparator.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            footerSeparator.heightAnchor.constraint(equalToConstant: 1),

            footer.topAnchor.constraint(equalTo: footerSeparator.bottomAnchor),
            footer.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            footer.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            footer.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            footer.heightAnchor.constraint(equalToConstant: 36),
        ])

        view = root
        footerSeparator.layer?.backgroundColor = DS.border.cgColor
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        NotificationCenter.default.addObserver(
            self, selector: #selector(documentDidChange(_:)),
            name: .imageDocumentImageDidChange, object: document)
        reload()
    }

    /// The window closed: stop observing the document.
    func releaseForWindowClose() {
        NotificationCenter.default.removeObserver(
            self, name: .imageDocumentImageDidChange, object: nil)
    }

    @objc private func documentDidChange(_ note: Notification) {
        // A live tick changes no path and no mask record.
        let isLive = (note.userInfo?["isLive"] as? Bool) ?? document?.isLiveEditing ?? false
        guard !isLive else { return }
        reloadWhenVisible()
    }

    /// Called by the editor whenever this tab is shown or hidden; a reload
    /// that was deferred while hidden happens here.
    func setPanelVisible(_ visible: Bool) {
        guard visible, needsReload else { return }
        needsReload = false
        reload()
    }

    /// The active layer is not a document change and posts no notification,
    /// yet the vector-mask row is computed from it — so the editor calls this.
    /// It is also what the editor calls after it changes the current path.
    func activeLayerChanged() {
        reloadWhenVisible()
    }

    private func reloadWhenVisible() {
        guard isViewLoaded else { return }
        guard !view.isHidden else {
            needsReload = true
            return
        }
        reload()
    }

    /// The editor's answer to "is there a selection to trace?", mirrored in
    /// from `updateStatus` — the term `validatePathsItem` applies to Select ▸
    /// Make Work Path…, so the footer and the menu item agree.
    func setSelectionState(hasSelection: Bool) {
        guard self.hasSelection != hasSelection else { return }
        self.hasSelection = hasSelection
        guard isViewLoaded else { return }
        updateButtonStates()
    }

    // MARK: - Reload

    /// The rows of `doc` with `activeLayer` active: its vector mask, then the
    /// path list in the core's order (the work path first).
    static func rows(of doc: RasterDocument, activeLayer: Int) -> [PathRow] {
        var rows: [PathRow] = []
        if let info = doc.vectorMask(activeLayer), let layer = doc.layerInfo(activeLayer) {
            rows.append(.vectorMask(layerID: layer.id, layerName: layer.name, info: info))
        }
        rows.append(contentsOf: doc.paths.entries.map(PathRow.path))
        return rows
    }

    func reload() {
        guard isViewLoaded else { return }
        let activeLayer = document?.activeLayerIndex ?? 0
        rows = document?.doc.map { Self.rows(of: $0, activeLayer: activeLayer) } ?? []
        isReloading = true
        tableView.reloadData()
        selectCurrentRow()
        isReloading = false
        let count = rows.filter { !$0.isVectorMask }.count
        countLabel.stringValue = count == 1 ? "1 path" : "\(count) paths"
        updateButtonStates()
    }

    /// Mirrors the document's current path into the row selection.
    private func selectCurrentRow() {
        guard let target = document?.currentPath,
              let row = rows.firstIndex(where: { $0.target == target })
        else {
            tableView.deselectAll(nil)
            return
        }
        tableView.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
    }

    /// The selected row, or nil.
    var selectedRow: PathRow? {
        let row = tableView.selectedRow
        return row >= 0 && row < rows.count ? rows[row] : nil
    }

    private func updateButtonStates() {
        let row = selectedRow
        let drawn = row.map { !$0.shape.isEmpty } ?? false
        let hasDocument = document?.doc != nil
        fillButton.isEnabled = hasDocument && drawn
        strokeButton.isEnabled = hasDocument && drawn
        selectionButton.isEnabled = hasDocument && drawn
        workPathButton.isEnabled = hasDocument && hasSelection
        // A path row becomes the active layer's vector mask; a vector-mask
        // row already is one.
        maskButton.isEnabled = hasDocument && drawn && row?.isVectorMask == false
        deleteButton.isEnabled = hasDocument && row != nil
    }

    // MARK: - Row actions

    @objc private func rowDoubleClicked(_ sender: Any?) {
        beginRename(row: tableView.clickedRow)
    }

    @objc private func renameClickedPath(_ sender: Any?) {
        beginRename(row: tableView.clickedRow >= 0 ? tableView.clickedRow : tableView.selectedRow)
    }

    private func beginRename(row: Int) {
        // Only a path carries a name of its own; the vector-mask row is named
        // by its layer.
        guard row >= 0, row < rows.count, case .path = rows[row] else {
            NSSound.beep()
            return
        }
        tableView.scrollRowToVisible(row)
        (tableView.view(atColumn: 0, row: row, makeIfNecessary: true) as? PathCellView)?
            .beginRename()
    }
}

// MARK: - Table data source / delegate

extension PathsPanelViewController: NSTableViewDataSource, NSTableViewDelegate, NSMenuDelegate {
    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(
        _ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int
    ) -> NSView? {
        guard let doc = document?.doc, row < rows.count else { return nil }
        let pathRow = rows[row]
        let cell = PathCellView(frame: .zero)
        cell.configure(
            row: pathRow, canvas: CGSize(width: doc.width, height: doc.height))
        if case .path(let entry) = pathRow {
            let id = entry.id
            cell.onRename = { [weak self] name in self?.onRenamePath?(id, name) }
        }
        return cell
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        LayerRowView(frame: .zero)
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        guard !isReloading else { return }
        updateButtonStates()
        onSelectTarget?(selectedRow?.target)
    }

    /// The row menu, built for the row under the cursor and SELECTING it
    /// first — so the menu, the footer and the menu bar act on the same row.
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let row = tableView.clickedRow
        func add(_ title: String, _ action: Selector, target: AnyObject? = nil) {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.target = target
            menu.addItem(item)
        }
        if row >= 0, row < rows.count {
            if tableView.selectedRow != row {
                tableView.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
            }
            if case .path(let entry) = rows[row] {
                add(
                    entry.kind == .work ? "Save Path…" : "Rename",
                    #selector(renameClickedPath(_:)), target: self)
                add("Duplicate Path", #selector(EditorViewController.duplicateCurrentPath(_:)))
                add("Delete Path", #selector(EditorViewController.deleteCurrentPath(_:)))
                add("Clipping Path", #selector(EditorViewController.toggleClippingPath(_:)))
                add(
                    "Clipping Path Flatness…",
                    #selector(EditorViewController.editClippingPathFlatness(_:)))
            } else {
                add(
                    "Delete Vector Mask",
                    #selector(EditorViewController.deleteVectorMask(_:)))
            }
            menu.addItem(.separator())
            add("Make Selection…", #selector(EditorViewController.makeSelectionFromPath(_:)))
            add("Fill Path", #selector(EditorViewController.fillCurrentPath(_:)))
            add("Stroke Path", #selector(EditorViewController.strokeCurrentPath(_:)))
            if case .path = rows[row] {
                add(
                    "Add Vector Mask",
                    #selector(EditorViewController.addVectorMaskFromCurrentPath(_:)))
            }
            menu.addItem(.separator())
        }
        add("Make Work Path…", #selector(EditorViewController.makeWorkPath(_:)))
    }
}

// MARK: - PathCellView

/// One row of the paths table: the shape drawn small in a framed well, then
/// the row's name over a mono meta line — the Channels row without its eye.
final class PathCellView: NSView, NSTextFieldDelegate {
    private let thumb = PathThumbnailView()
    private let nameField = NSTextField(string: "")
    private let metaLabel = NSTextField(labelWithString: "")
    private var committedName = ""

    /// Only path rows are renameable; the vector-mask row leaves this nil.
    var onRename: ((String) -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)

        thumb.translatesAutoresizingMaskIntoConstraints = false
        thumb.wantsLayer = true
        thumb.layer?.cornerRadius = DS.controlRadius
        thumb.layer?.borderWidth = 1
        thumb.layer?.borderColor = DS.border.cgColor
        thumb.layer?.masksToBounds = true

        nameField.translatesAutoresizingMaskIntoConstraints = false
        nameField.isBordered = false
        nameField.drawsBackground = false
        nameField.usesSingleLineMode = true
        nameField.lineBreakMode = .byTruncatingTail
        nameField.font = DS.sans(13)
        nameField.textColor = DS.textStrong
        // Editable only for the length of a rename, so a click on the name
        // selects the row like a click anywhere else in it.
        nameField.isEditable = false
        nameField.isSelectable = false
        nameField.delegate = self

        metaLabel.translatesAutoresizingMaskIntoConstraints = false
        metaLabel.font = DS.mono(10)
        metaLabel.textColor = DS.textFaint
        metaLabel.lineBreakMode = .byTruncatingTail

        addSubview(thumb)
        addSubview(nameField)
        addSubview(metaLabel)

        NSLayoutConstraint.activate([
            thumb.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            thumb.centerYAnchor.constraint(equalTo: centerYAnchor),
            thumb.widthAnchor.constraint(equalToConstant: LayerCellView.thumbSide),
            thumb.heightAnchor.constraint(equalToConstant: LayerCellView.thumbSide),

            nameField.leadingAnchor.constraint(equalTo: thumb.trailingAnchor, constant: 9),
            nameField.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            nameField.bottomAnchor.constraint(equalTo: centerYAnchor, constant: 1),

            metaLabel.leadingAnchor.constraint(equalTo: nameField.leadingAnchor),
            metaLabel.trailingAnchor.constraint(equalTo: nameField.trailingAnchor),
            metaLabel.topAnchor.constraint(equalTo: centerYAnchor, constant: 2),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PathCellView does not support NSCoder")
    }

    func configure(row: PathRow, canvas: CGSize) {
        committedName = row.title
        nameField.stringValue = row.title
        metaLabel.stringValue = row.subtitle
        thumb.shape = row.shape
        thumb.canvas = canvas
        thumb.toolTip = row.isVectorMask
            ? "The active layer's vector mask. Select it to edit its shape with the Pen."
            : "Select to outline this path on the canvas and edit it with the Pen."
    }

    func beginRename() {
        nameField.isEditable = true
        guard window?.makeFirstResponder(nameField) == true else {
            nameField.isEditable = false
            return
        }
        nameField.currentEditor()?.selectAll(nil)
    }

    /// Commits the inline rename, reverting to the committed name when the
    /// field holds nothing usable. The core refuses a taken name with its own
    /// sentence, which the editor presents; the panel's reload then puts the
    /// stored name back.
    func controlTextDidEndEditing(_ obj: Notification) {
        nameField.isEditable = false
        let name = nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if name.isEmpty || name == committedName {
            nameField.stringValue = committedName
        } else {
            onRename?(name)
        }
    }
}

/// A path drawn small: its outline scaled to fit the well, over the canvas's
/// own rectangle so a small path in a corner reads as one.
final class PathThumbnailView: NSView {
    var shape = PathShape() { didSet { needsDisplay = true } }
    var canvas = CGSize.zero { didSet { needsDisplay = true } }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        DS.hoverFill.setFill()
        bounds.fill()
        guard !shape.isEmpty else { return }
        // The frame the thumbnail shows: the canvas, grown to hold a path
        // that leaves it.
        var frame = CGRect(origin: .zero, size: canvas)
        for subpath in shape.subpaths {
            for anchor in subpath.anchors {
                frame = frame.union(CGRect(origin: anchor.point, size: .zero))
            }
        }
        guard frame.width > 0, frame.height > 0 else { return }
        let inset = bounds.insetBy(dx: 4, dy: 4)
        let scale = min(inset.width / frame.width, inset.height / frame.height)
        let offset = CGPoint(
            x: inset.midX - frame.midX * scale, y: inset.midY - frame.midY * scale)
        func place(_ point: CGPoint) -> CGPoint {
            CGPoint(x: point.x * scale + offset.x, y: point.y * scale + offset.y)
        }
        let outline = NSBezierPath()
        for subpath in shape.subpaths {
            guard let first = subpath.anchors.first else { continue }
            outline.move(to: place(first.point))
            let count = subpath.anchors.count
            let segments = subpath.closed ? count : count - 1
            for index in 0..<max(segments, 0) {
                let from = subpath.anchors[index]
                let to = subpath.anchors[(index + 1) % count]
                outline.curve(
                    to: place(to.point), controlPoint1: place(from.handleOut),
                    controlPoint2: place(to.handleIn))
            }
            if subpath.closed { outline.close() }
        }
        outline.windingRule = shape.fillRule == .evenOdd ? .evenOdd : .nonZero
        DS.textMuted.withAlphaComponent(0.35).setFill()
        outline.fill()
        DS.textStrong.setStroke()
        outline.lineWidth = 1
        outline.stroke()
    }
}
