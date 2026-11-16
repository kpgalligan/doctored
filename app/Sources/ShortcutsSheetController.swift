import AppKit
import UniformTypeIdentifiers

/// The field that records a chord: it takes first responder, swallows one
/// key-down and reports what it spelled. It is not a text field — a chord is
/// not typed, it is PRESSED, and letting AppKit interpret ⌘R as "select all
/// in a text view, then R" is how a recorder loses the very keystroke it
/// exists to catch.
final class ShortcutRecorderField: NSView {
    /// The chord the user pressed. Modifier-only presses are ignored, so
    /// holding ⌘ before the letter arrives does not report ⌘ alone.
    var onRecord: ((ShortcutChord) -> Void)?

    /// Whether the field may swallow a chord at all. The field takes first
    /// responder as soon as the card opens, and `performKeyEquivalent` runs
    /// AHEAD of `NSApp.mainMenu`, so without this every ⌘ chord — ⌘W, ⌘Q, ⌘,
    /// — was eaten while no table row was selected and the sheet's own window
    /// commands did nothing. The sheet answers "a row is selected"; the
    /// default admits everything, so a standalone field still records.
    var canRecord: () -> Bool = { true }

    private let label = NSTextField(labelWithString: "")
    private var recording = false

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.cornerRadius = DS.controlRadius
        layer?.borderWidth = 1
        label.translatesAutoresizingMaskIntoConstraints = false
        label.font = DS.mono(12)
        label.alignment = .center
        addSubview(label)
        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: centerXAnchor),
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            heightAnchor.constraint(equalToConstant: DS.controlHeight + 4),
            widthAnchor.constraint(greaterThanOrEqualToConstant: 150),
        ])
        refresh()
        setAccessibilityIdentifier("shortcut-recorder")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ShortcutRecorderField does not support NSCoder")
    }

    override var acceptsFirstResponder: Bool { true }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        recording = true
        refresh()
    }

    override func becomeFirstResponder() -> Bool {
        recording = true
        refresh()
        return true
    }

    override func resignFirstResponder() -> Bool {
        recording = false
        refresh()
        return true
    }

    /// A menu key equivalent is resolved AHEAD of the first responder, so a
    /// recorder that only overrode `keyDown` never sees ⌘W, ⌘Q or ⌘, — the
    /// menu runs the command instead, closing or quitting from inside the
    /// sheet that was asking for the chord. The view hierarchy is offered the
    /// event before `NSApp.mainMenu` is, so catching it HERE is what makes
    /// those chords recordable (and refusable by name) at all.
    ///
    /// Only the chords a shortcut may actually be — ⌘ or an F-key, exactly
    /// what `ShortcutChord.validated()` admits — are swallowed, so the card's
    /// own Escape and Return still reach its buttons.
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard recording, canRecord(), let chord = ShortcutChord(event: event),
              chord.modifiers.contains(.command) || chord.isFunctionKey else {
            return super.performKeyEquivalent(with: event)
        }
        onRecord?(chord)
        return true
    }

    override func keyDown(with event: NSEvent) {
        guard recording, canRecord(), let chord = ShortcutChord(event: event) else {
            super.keyDown(with: event)
            return
        }
        onRecord?(chord)
    }

    /// Shows a chord without recording one — how the selected row's current
    /// binding is displayed.
    func show(_ chord: ShortcutChord?) {
        label.stringValue = chord?.display ?? "—"
    }

    private func refresh() {
        layer?.borderColor = (recording ? DS.accent : DS.border).cgColor
        label.textColor = DS.textStrong
        if label.stringValue.isEmpty { label.stringValue = "—" }
    }
}

/// Doctored ▸ Settings… (⌘,), its Shortcuts tab — every customizable menu
/// command with its key equivalent, editable with conflict detection and
/// reset. `SettingsWindowController` hosts `makeContent()` in its card.
///
/// Every edit is applied and STORED immediately — there is no Apply button,
/// because a shortcut sheet's whole point is trying the chord. The store is
/// the ACTIVE set's item (`ShortcutSets`), written with `replace: true`. The
/// set popup beside the search field switches sets, makes, duplicates,
/// renames and deletes one, and imports a `.kys` file into a new one.
///
/// After the commands come the tools ("Tools ▸ Brush"), one row per tool the
/// rail offers, each with its bare key (`ToolKeys`): table row `rows.count + i`
/// is `toolRows[i]`.
final class ShortcutsSheetController: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private let menu: NSMenu
    /// The command rows currently listed — the whole customizable set,
    /// filtered by the search field.
    private(set) var rows: [ShortcutMenu.Item] = []
    private var all: [ShortcutMenu.Item] = []
    /// The tool rows currently listed, in rail order, filtered by the search field.
    private(set) var toolRows: [EditorTool] = []

    private let search = NSSearchField()
    private let setPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let table = NSTableView()
    private let recorder = ShortcutRecorderField()
    private let message = NSTextField(wrappingLabelWithString: "")
    private var brokenSection: BrokenLibraryFilesSection?
    /// Rows appeared or disappeared (the broken-files section): the host refits its window.
    var onResize: (() -> Void)?

    init(menu: NSMenu) {
        self.menu = menu
        super.init()
        reload()
    }

    // MARK: - Rows

    /// Re-walks the LIVE menu, so every row shows what the menu items
    /// actually carry after the last edit.
    private func reload() {
        all = ShortcutMenu.customizableItems(in: menu)
        let needle = search.stringValue.trimmingCharacters(in: .whitespaces).lowercased()
        rows = needle.isEmpty ? all : all.filter { $0.path.lowercased().contains(needle) }
        let tools = EditorTool.railGroups.flatMap { $0 }.filter { !$0.planned }
        toolRows = needle.isEmpty ? tools
            : tools.filter { Self.toolPath($0).lowercased().contains(needle) }
        table.reloadData()
        updateRecorder()
    }

    private var selected: ShortcutMenu.Item? {
        rows.indices.contains(table.selectedRow) ? rows[table.selectedRow] : nil
    }

    /// The selected tool row's tool, or nil on a command row or none.
    private var selectedTool: EditorTool? {
        let index = table.selectedRow - rows.count
        return toolRows.indices.contains(index) ? toolRows[index] : nil
    }

    private func updateRecorder() {
        if let tool = selectedTool {
            recorder.show(tool.keyCharacter.isEmpty ? nil
                          : ShortcutChord(key: tool.keyCharacter, modifiers: []))
        } else {
            recorder.show(selected?.chord)
        }
    }

    // MARK: - Presentation

    /// The tab's content: the hint, the search field beside the set popup, the table, the
    /// recorder row, the broken files and the message line.
    func makeContent() -> NSView {
        search.placeholderString = "Search commands"
        search.target = self
        search.action = #selector(searchChanged(_:))
        search.widthAnchor.constraint(equalToConstant: 250).isActive = true
        search.setAccessibilityIdentifier("shortcut-search")
        setPopup.translatesAutoresizingMaskIntoConstraints = false
        setPopup.widthAnchor.constraint(equalToConstant: 162).isActive = true
        setPopup.font = DS.sans(12)
        setPopup.target = self
        setPopup.action = #selector(setChosen(_:))
        setPopup.autoenablesItems = false
        setPopup.setAccessibilityIdentifier("shortcut-set-popup")
        rebuildSetPopup()
        let searchRow = NSStackView(views: [search, setPopup])
        searchRow.orientation = .horizontal
        searchRow.alignment = .centerY
        searchRow.spacing = 8

        let command = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("command"))
        command.title = "Command"
        command.width = 250
        let chord = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("chord"))
        chord.title = "Shortcut"
        chord.width = 100
        let reset = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("reset"))
        reset.title = ""
        reset.width = 60
        table.addTableColumn(command)
        table.addTableColumn(chord)
        table.addTableColumn(reset)
        table.rowHeight = 22
        table.usesAlternatingRowBackgroundColors = true
        table.dataSource = self
        table.delegate = self
        table.setAccessibilityIdentifier("shortcut-table")

        let scroll = NSScrollView()
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        // The tallest tab sets the Settings card's height, which stays within 700 pt
        // (jobs-probe "settings card height").
        scroll.heightAnchor.constraint(equalToConstant: 290).isActive = true
        scroll.widthAnchor.constraint(equalToConstant: 420).isActive = true

        recorder.translatesAutoresizingMaskIntoConstraints = false
        recorder.onRecord = { [weak self] chord in self?.record(chord) }
        // Nothing to bind a chord TO until a command is selected, so until
        // then the field falls through and ⌘W / ⌘Q / ⌘, reach the menu.
        recorder.canRecord = { [weak self] in self?.selected != nil || self?.selectedTool != nil }

        let clear = StickerButton(title: "None", style: .secondary, target: self,
                                  action: #selector(clearShortcut(_:)))
        let resetAll = StickerButton(title: "Reset All", style: .secondary, target: self,
                                     action: #selector(resetAll(_:)))
        resetAll.setAccessibilityIdentifier("shortcut-reset-all")
        let editRow = NSStackView(views: [recorder, clear, resetAll])
        editRow.orientation = .horizontal
        editRow.alignment = .centerY
        editRow.spacing = 8

        message.font = DS.sans(11)
        message.textColor = DS.warningText
        message.preferredMaxLayoutWidth = 420
        message.setAccessibilityIdentifier("shortcut-message")
        // The library's bad files, each named with Delete File, as the
        // preference-library managers list theirs. There is no repair, and no
        // MCP twin: shortcuts are an app-wide preference.
        let broken = BrokenLibraryFilesSection(
            title: "Files that are not shortcut sets (none of their shortcuts were applied):",
            identifierPrefix: "shortcut",
            delete: { try ShortcutLibrary.delete(file: $0) }, repair: nil,
            changed: { [weak self] in self?.libraryChanged() },
            report: { [weak self] in self?.message.stringValue = $0 })
        brokenSection = broken

        let hint = NSTextField(wrappingLabelWithString:
            "Select a command, click the recorder and press the chord. A shortcut must "
            + "include ⌘ or be a function key, and a chord another command already holds is "
            + "refused naming it. A tool (after the commands) takes one bare letter or digit; "
            + "tools sharing a key cycle in rail order. Edits apply immediately.")
        hint.font = DS.sans(12)
        hint.textColor = DS.textMuted
        hint.preferredMaxLayoutWidth = 420

        let content = NSStackView(views: [hint, searchRow, scroll, editRow, broken.view, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        reload()
        refreshBrokenFiles()
        return content
    }

    /// Lists the library's bad files in the broken-files section. Only a row
    /// naming a real `.json` file in the library folder is listed — those are
    /// the ones Delete File can remove. A failure of the whole listing (more
    /// than sixteen shortcut sets, an unreadable folder) names no file: it
    /// applies no stored shortcut at all and is said in the message line.
    /// Asked when the sheet opens and after a delete, not per keystroke.
    private func refreshBrokenFiles() {
        let broken = ShortcutMenu.stored().broken
        let isFile: (PreferenceLibraryBroken) -> Bool = { row in
            guard let file = try? ShortcutLibrary.validatedFileName(row.file) else { return false }
            return FileManager.default.fileExists(atPath: ShortcutLibrary.fileURL(file).path)
        }
        if let whole = broken.first(where: { !isFile($0) }) {
            message.stringValue = "\(whole.reason) No stored shortcuts were applied."
        }
        if brokenSection?.update(broken.filter(isFile)) == true { onResize?() }
    }

    /// A bad file was deleted: the stored set may read again, so it is
    /// re-applied to the menu and the tool keys, then the rows and the
    /// section follow.
    private func libraryChanged() {
        message.stringValue = ""
        let bindings = ShortcutMenu.stored().bindings
        ToolKeys.shared.load(bindings)
        ShortcutMenu.apply(bindings, to: menu)
        rebuildSetPopup()
        reload()
        refreshBrokenFiles()
        Self.refreshEditorStatus()
    }

    // MARK: - Sets

    /// The popup's operation rows, after the set names (tag 0).
    private enum SetOperation: Int {
        case create = 1, duplicate, rename, delete, importFile
    }

    /// The set names with the active one selected, then the operations. Rename and Delete are
    /// off on the default set, which can be reset but not renamed or deleted.
    private func rebuildSetPopup() {
        setPopup.removeAllItems()
        let active = ShortcutLibrary.key(ShortcutSets.activeName())
        let onDefault = active == ShortcutLibrary.key(ShortcutSets.defaultName)
        var selected: NSMenuItem?
        for name in ShortcutSets.names() {
            let item = NSMenuItem(title: name, action: nil, keyEquivalent: "")
            item.representedObject = name
            setPopup.menu?.addItem(item)
            if ShortcutLibrary.key(name) == active { selected = item }
        }
        setPopup.menu?.addItem(.separator())
        let operations: [(String, SetOperation)] = [
            ("New Set…", .create), ("Duplicate Set…", .duplicate), ("Rename Set…", .rename),
            ("Delete Set", .delete), ("Import Shortcuts File…", .importFile),
        ]
        for (title, operation) in operations {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.tag = operation.rawValue
            item.isEnabled = !(onDefault && (operation == .rename || operation == .delete))
            setPopup.menu?.addItem(item)
        }
        setPopup.select(selected ?? setPopup.itemArray.first)
    }

    @objc private func setChosen(_ sender: Any?) {
        guard let item = setPopup.selectedItem else { return }
        let active = ShortcutSets.activeName()
        do {
            var note = ""
            if let name = item.representedObject as? String {
                try ShortcutSets.activate(name, in: menu)
            } else {
                switch SetOperation(rawValue: item.tag) {
                case .create:
                    if let name = askName(title: "New Shortcut Set", initial: "") {
                        try ShortcutSets.create(name, in: menu)
                    }
                case .duplicate:
                    if let name = askName(title: "Duplicate Shortcut Set", initial: active + " Copy") {
                        try ShortcutSets.duplicate(as: name, in: menu)
                    }
                case .rename:
                    if let name = askName(title: "Rename Shortcut Set", initial: active) {
                        try ShortcutSets.rename(active, to: name, in: menu)
                    }
                case .delete:
                    try ShortcutSets.delete(active, in: menu)
                case .importFile:
                    note = try importShortcutsFile() ?? ""
                case nil:
                    break
                }
            }
            message.stringValue = note
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
        rebuildSetPopup()
        reload()
        refreshBrokenFiles()
        Self.refreshEditorStatus()
    }

    /// A set name, or nil when cancelled.
    private func askName(title: String, initial: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = "A shortcut set is a preference: it is saved for the whole app, "
            + "and switching or editing one changes no document and adds no undo step."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24))
        DSField.style(field)
        field.stringValue = initial
        field.placeholderString = "Set name"
        field.setAccessibilityIdentifier("shortcut-set-name")
        alert.accessoryView = field
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return field.stringValue
    }

    /// Import Shortcuts File…: the chosen `.kys` file becomes a new, active set, and its
    /// report is shown. Answers the message line's one-sentence count, or nil when cancelled.
    private func importShortcutsFile() throws -> String? {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "kys") ?? .data]
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return nil }
        let (name, report) = try ShortcutSets.importKys(contentsOf: url, in: menu)
        let summary = "Imported \(report.applied.count) of the entries in "
            + "\(url.lastPathComponent) into “\(name)”; \(report.unmapped.count) not mapped, "
            + "\(report.refused.count) refused."
        func section(_ title: String, _ lines: [String]) -> String {
            "\(title) (\(lines.count))\n" + (lines.isEmpty ? "—" : lines.joined(separator: "\n"))
        }
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 460, height: 300))
        text.isEditable = false
        text.font = DS.mono(11)
        text.string = [section("Applied", report.applied), section("Not mapped", report.unmapped),
                       section("Refused", report.refused)].joined(separator: "\n\n")
        let scroll = NSScrollView(frame: text.frame)
        scroll.hasVerticalScroller = true
        scroll.documentView = text
        let alert = NSAlert()
        alert.messageText = "Imported “\(name)”"
        alert.informativeText = summary
        alert.accessoryView = scroll
        alert.addButton(withTitle: "Done")
        alert.runModal()
        return summary
    }

    // MARK: - Editing

    private func record(_ chord: ShortcutChord?) {
        if let tool = selectedTool {
            recordToolKey(chord, for: tool)
            return
        }
        guard let item = selected else {
            message.stringValue = "Select a command first."
            NSSound.beep()
            return
        }
        do {
            try ShortcutMenu.assign(chord, to: item.identity, in: menu)
            message.stringValue = ""
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
        reload()
    }

    @objc private func searchChanged(_ sender: Any?) {
        reload()
    }

    @objc private func clearShortcut(_ sender: Any?) {
        record(nil)
    }

    @objc private func resetRow(_ sender: Any?) {
        let row = (sender as? NSButton)?.tag ?? -1
        let toolIndex = row - rows.count
        guard rows.indices.contains(row) || toolRows.indices.contains(toolIndex) else {
            NSSound.beep()
            return
        }
        do {
            if rows.indices.contains(row) {
                try ShortcutMenu.reset(rows[row].identity, in: menu)
            } else {
                try ToolKeys.shared.reset(toolRows[toolIndex])
                Self.refreshEditorStatus()
            }
            message.stringValue = ""
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
        reload()
    }

    @objc private func resetAll(_ sender: Any?) {
        do {
            try ShortcutMenu.resetAll(in: menu)
            try ToolKeys.shared.resetAll()
            Self.refreshEditorStatus()
            message.stringValue = ""
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
        reload()
    }

    // MARK: - Tool keys

    /// "Tools ▸ Brush": a tool row's first column, and what the search field matches.
    private static func toolPath(_ tool: EditorTool) -> String { "Tools ▸ \(tool.displayName)" }

    /// A bare key for a tool row; nil (None) removes the tool's key. A chord with a modifier is
    /// refused: a tool key is pressed alone.
    private func recordToolKey(_ chord: ShortcutChord?, for tool: EditorTool) {
        do {
            if let chord, !chord.modifiers.isEmpty {
                throw DoctoredCoreError(
                    message: "A tool key is pressed alone — without ⌘, ⌥, ⌃ or ⇧.")
            }
            try ToolKeys.shared.assign(chord?.key, to: tool)
            Self.refreshEditorStatus()
            message.stringValue = ""
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
        reload()
    }

    /// Every open editor's status line names the current tool's key, so it is redrawn.
    private static func refreshEditorStatus() {
        for document in NSDocumentController.shared.documents.compactMap({ $0 as? ImageDocument }) {
            for editor in document.editors where editor.isViewLoaded { editor.updateStatus() }
        }
    }

    private func toolCell(_ tool: EditorTool, column: String?, row: Int) -> NSView {
        let isDefault = tool.keyCharacter == tool.defaultKeyCharacter
        switch column {
        case "command":
            let label = NSTextField(labelWithString: Self.toolPath(tool))
            label.font = DS.sans(12)
            label.lineBreakMode = .byTruncatingHead
            return label
        case "chord":
            let label = NSTextField(labelWithString: tool.keyLabel ?? "—")
            label.font = DS.mono(12)
            label.textColor = isDefault ? DS.textMuted : DS.textStrong
            return label
        default:
            let button = NSButton(title: "Reset", target: self, action: #selector(resetRow(_:)))
            button.bezelStyle = .inline
            button.font = DS.sans(11)
            button.tag = row
            button.isEnabled = !isDefault
            return button
        }
    }

    // MARK: - Table

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count + toolRows.count }

    func tableView(
        _ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int
    ) -> NSView? {
        if toolRows.indices.contains(row - rows.count) {
            return toolCell(toolRows[row - rows.count], column: tableColumn?.identifier.rawValue,
                            row: row)
        }
        guard rows.indices.contains(row) else { return nil }
        let item = rows[row]
        switch tableColumn?.identifier.rawValue {
        case "command":
            let label = NSTextField(labelWithString: item.path)
            label.font = DS.sans(12)
            label.lineBreakMode = .byTruncatingHead
            label.toolTip = item.path
            return label
        case "chord":
            let label = NSTextField(labelWithString: item.chord?.display ?? "—")
            label.font = DS.mono(12)
            label.textColor = item.chord == item.defaultChord ? DS.textMuted : DS.textStrong
            return label
        default:
            let button = NSButton(title: "Reset", target: self, action: #selector(resetRow(_:)))
            button.bezelStyle = .inline
            button.font = DS.sans(11)
            button.tag = row
            button.isEnabled = item.chord != item.defaultChord
            return button
        }
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        updateRecorder()
    }
}
