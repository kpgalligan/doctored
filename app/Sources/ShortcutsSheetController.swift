import AppKit

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
/// the single `"Shortcuts"` item, written with `replace: true`.
final class ShortcutsSheetController: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private let menu: NSMenu
    /// The rows currently listed — the whole customizable set, filtered by
    /// the search field.
    private(set) var rows: [ShortcutMenu.Item] = []
    private var all: [ShortcutMenu.Item] = []

    private let search = NSSearchField()
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
        table.reloadData()
        updateRecorder()
    }

    private var selected: ShortcutMenu.Item? {
        rows.indices.contains(table.selectedRow) ? rows[table.selectedRow] : nil
    }

    private func updateRecorder() {
        recorder.show(selected?.chord)
    }

    // MARK: - Presentation

    /// The tab's content: the hint, the search field, the table, the recorder row, the broken
    /// files and the message line.
    func makeContent() -> NSView {
        search.placeholderString = "Search commands"
        search.target = self
        search.action = #selector(searchChanged(_:))
        search.widthAnchor.constraint(equalToConstant: 420).isActive = true
        search.setAccessibilityIdentifier("shortcut-search")

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
        scroll.heightAnchor.constraint(equalToConstant: 320).isActive = true
        scroll.widthAnchor.constraint(equalToConstant: 420).isActive = true

        recorder.translatesAutoresizingMaskIntoConstraints = false
        recorder.onRecord = { [weak self] chord in self?.record(chord) }
        // Nothing to bind a chord TO until a command is selected, so until
        // then the field falls through and ⌘W / ⌘Q / ⌘, reach the menu.
        recorder.canRecord = { [weak self] in self?.selected != nil }

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
            + "refused naming it. Edits apply immediately.")
        hint.font = DS.sans(12)
        hint.textColor = DS.textMuted
        hint.preferredMaxLayoutWidth = 420

        let content = NSStackView(views: [hint, search, scroll, editRow, broken.view, message])
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
    /// than four shortcut sets, an unreadable folder) names no file: it
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
    /// re-applied to the menu, then the rows and the section follow.
    private func libraryChanged() {
        message.stringValue = ""
        ShortcutMenu.apply(ShortcutMenu.stored().bindings, to: menu)
        reload()
        refreshBrokenFiles()
    }

    // MARK: - Editing

    private func record(_ chord: ShortcutChord?) {
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
        guard rows.indices.contains(row) else {
            NSSound.beep()
            return
        }
        do {
            try ShortcutMenu.reset(rows[row].identity, in: menu)
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
            message.stringValue = ""
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
        reload()
    }

    // MARK: - Table

    func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    func tableView(
        _ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int
    ) -> NSView? {
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
