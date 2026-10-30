import AppKit
import UniformTypeIdentifiers

/// File ▸ Automate ▸ Merge Panorama… (MergePanorama.swift): the one
/// dialog — Layout (the six), Source Files (Use
/// Files or Folder, the list, Browse…, Remove, Add Open Files) and the four
/// boxes — on the shared `makeSheetView` / `makeButtonRow` builders.
///
/// A `ModalCardWindowController` rather than a document-modal sheet, for
/// `NewDocumentSheetController`'s reason: it runs with no document open.
///
/// The dialog's rules: Content-Aware Fill Transparent Areas is disabled
/// and cleared while Blend Images Together is off; Collage and Reposition
/// disable and clear the two lens boxes and clear (not disable) the fill. OK
/// needs at least two files. It decides nothing about the merge.
final class MergePanoramaSheetController: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private let layouts = MergePanorama.layouts
    private lazy var layoutButtons: [NSButton] = layouts.enumerated().map { position, layout in
        let button = NSButton(radioButtonWithTitle: layout.title, target: nil, action: nil)
        button.tag = position
        return button
    }
    private let use = NSPopUpButton(frame: .zero, pullsDown: false)
    private let table = NSTableView()
    private var files: [URL] = []
    private let blend = NSButton(checkboxWithTitle: "Blend Images Together", target: nil,
                                 action: nil)
    private let vignette = NSButton(checkboxWithTitle: "Vignette Removal", target: nil, action: nil)
    private let distortion = NSButton(checkboxWithTitle: "Geometric Distortion Correction",
                                      target: nil, action: nil)
    private let fill = NSButton(checkboxWithTitle: "Content-Aware Fill Transparent Areas",
                                target: nil, action: nil)
    private var remove: NSButton?
    private var ok: NSButton?
    private var modal: ModalCardWindowController?
    private var result: (urls: [URL], options: MergePanorama.Options)?

    private enum Source: Int { case files, folder }

    /// Runs the dialog and answers the files and options, or nil when the
    /// user cancelled.
    static func run() -> (urls: [URL], options: MergePanorama.Options)? {
        MergePanoramaSheetController().present()
    }

    private func present() -> (urls: [URL], options: MergePanorama.Options)? {
        for button in layoutButtons {
            // One shared action is what makes sibling radio buttons exclusive.
            button.target = self
            button.action = #selector(layoutChanged(_:))
            button.font = DS.sans(13)
            let name = layouts.indices.contains(button.tag)
                ? layouts[button.tag].projectionName : "layout"
            button.setAccessibilityIdentifier("merge-panorama-layout-\(name)")
        }
        layoutButtons.first?.state = .on
        let radios = NSStackView(views: layoutButtons)
        radios.orientation = .vertical
        radios.alignment = .leading
        radios.spacing = 6

        use.addItems(withTitles: ["Files", "Folder"])
        use.font = DS.sans(13)
        use.setAccessibilityIdentifier("merge-panorama-use")
        let useRow = NSStackView(views: [fieldLabel("Use:"), use])
        useRow.orientation = .horizontal
        useRow.spacing = 8

        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("file"))
        column.title = "Source Files"
        column.width = 250
        table.addTableColumn(column)
        table.headerView = nil
        table.dataSource = self
        table.delegate = self
        table.allowsMultipleSelection = true
        table.allowsEmptySelection = true
        table.rowHeight = 20
        table.setAccessibilityIdentifier("merge-panorama-files")
        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        scroll.widthAnchor.constraint(equalToConstant: 262).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 150).isActive = true

        let browse = StickerButton(title: "Browse…", style: .secondary, target: self,
                                   action: #selector(browseClicked(_:)))
        browse.setAccessibilityIdentifier("merge-panorama-browse")
        let remove = StickerButton(title: "Remove", style: .secondary, target: self,
                                   action: #selector(removeClicked(_:)))
        remove.setAccessibilityIdentifier("merge-panorama-remove")
        self.remove = remove
        let addOpen = StickerButton(title: "Add Open Files", style: .secondary, target: self,
                                    action: #selector(addOpenClicked(_:)))
        addOpen.setAccessibilityIdentifier("merge-panorama-add-open")
        let actions = NSStackView(views: [browse, remove, addOpen])
        actions.orientation = .vertical
        actions.alignment = .leading
        actions.spacing = 6
        let list = NSStackView(views: [scroll, actions])
        list.orientation = .horizontal
        list.alignment = .top
        list.spacing = 10
        let sources = NSStackView(views: [useRow, list])
        sources.orientation = .vertical
        sources.alignment = .leading
        sources.spacing = 8

        for (box, identifier, on) in [(blend, "merge-panorama-blend", true),
                                      (vignette, "merge-panorama-vignette", false),
                                      (distortion, "merge-panorama-distortion", false),
                                      (fill, "merge-panorama-fill", false)] {
            box.font = DS.sans(13)
            box.state = on ? .on : .off
            box.target = self
            box.action = #selector(boxChanged(_:))
            box.setAccessibilityIdentifier(identifier)
        }
        let boxes = NSStackView(views: [blend, vignette, distortion, fill])
        boxes.orientation = .vertical
        boxes.alignment = .leading
        boxes.spacing = 6

        let grid = NSGridView(views: [[fieldLabel("Layout:"), radios],
                                      [fieldLabel("Source Files:"), sources],
                                      [NSGridCell.emptyContentView, boxes]])
        grid.columnSpacing = 12
        grid.rowSpacing = 14
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        for row in 0..<grid.numberOfRows {
            grid.cell(atColumnIndex: 0, rowIndex: row).yPlacement = .top
            grid.cell(atColumnIndex: 1, rowIndex: row).yPlacement = .top
        }

        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let ok = StickerButton(title: "OK", style: .primary, target: self,
                               action: #selector(okClicked(_:)))
        ok.setAccessibilityIdentifier("merge-panorama-ok")
        self.ok = ok
        refresh()
        modal = ModalCardWindowController(
            title: MergePanorama.title,
            card: makeSheetView(
                title: MergePanorama.title,
                hint: "Two or more overlapping photos become one new panorama document: each "
                    + "file a layer, aligned to the middle file, then blended. A file that does "
                    + "not overlap the middle one is kept, hidden. The list's order is the "
                    + "layers' order, bottom first.",
                content: grid, buttonRow: makeButtonRow(cancel: cancel, apply: ok),
                width: 520))
        guard modal?.runModal() == .OK else { return nil }
        return result
    }

    // MARK: - State

    private var chosenLayout: AutoAlign.Registration {
        let chosen = layoutButtons.first { $0.state == .on }?.tag ?? 0
        return layouts.indices.contains(chosen) ? layouts[chosen] : .auto
    }

    private var takesLens: Bool {
        chosenLayout != .collage && chosenLayout != .translation
    }

    /// Enables what the current choices allow (the type comment's rules).
    private func refresh() {
        vignette.isEnabled = takesLens
        distortion.isEnabled = takesLens
        if !takesLens {
            vignette.state = .off
            distortion.state = .off
        }
        fill.isEnabled = blend.state == .on
        if blend.state != .on { fill.state = .off }
        remove?.isEnabled = !table.selectedRowIndexes.isEmpty
        ok?.isEnabled = files.count >= MergePanorama.minimumFiles
    }

    /// Adds `urls` after the list's own files, skipping any already listed.
    private func append(_ urls: [URL]) {
        var known = Set(files.map(\.standardizedFileURL))
        for url in urls where !known.contains(url.standardizedFileURL) {
            files.append(url)
            known.insert(url.standardizedFileURL)
        }
        table.reloadData()
        refresh()
    }

    // MARK: - Actions

    /// The radios' shared action: sharing it is what makes them exclusive.
    /// Collage and Reposition clear the fill as well as the lens boxes.
    @objc private func layoutChanged(_ sender: Any?) {
        if !takesLens { fill.state = .off }
        refresh()
    }

    @objc private func boxChanged(_ sender: Any?) {
        refresh()
    }

    @objc private func browseClicked(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.title = MergePanorama.title
        panel.prompt = "Choose"
        if Source(rawValue: use.indexOfSelectedItem) == .folder {
            panel.message = "Choose a folder of overlapping photos."
            panel.canChooseFiles = false
            panel.canChooseDirectories = true
            panel.allowsMultipleSelection = false
            guard panel.runModal() == .OK, let folder = panel.url else { return }
            files = BatchRun.imageURLs(in: folder)
            table.reloadData()
            refresh()
        } else {
            panel.message = "Choose two or more overlapping photos."
            panel.canChooseFiles = true
            panel.canChooseDirectories = false
            panel.allowsMultipleSelection = true
            panel.allowedContentTypes = ImageDocument.readableTypes.compactMap(UTType.init)
            guard panel.runModal() == .OK else { return }
            // Natural file-name order ("frame 2" before "frame 10"), as Finder lists them.
            append(panel.urls.sorted {
                $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent)
                    == .orderedAscending
            })
        }
    }

    @objc private func removeClicked(_ sender: Any?) {
        let selected = table.selectedRowIndexes
        guard !selected.isEmpty else {
            NSSound.beep()
            return
        }
        files = files.enumerated().filter { !selected.contains($0.offset) }.map(\.element)
        table.deselectAll(nil)
        table.reloadData()
        refresh()
    }

    @objc private func addOpenClicked(_ sender: Any?) {
        let open = MergePanorama.openFileURLs()
        guard !open.isEmpty else {
            NSSound.beep()
            return
        }
        append(open)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        result = nil
        modal?.end(.cancel)
    }

    @objc private func okClicked(_ sender: Any?) {
        guard files.count >= MergePanorama.minimumFiles else {
            NSSound.beep()
            refresh()
            return
        }
        var options = MergePanorama.Options()
        options.layout = chosenLayout
        options.blend = blend.state == .on
        options.vignette = vignette.isEnabled && vignette.state == .on
        options.distortion = distortion.isEnabled && distortion.state == .on
        options.fill = fill.isEnabled && fill.state == .on
        result = (urls: files, options: options)
        modal?.end(.OK)
    }

    // MARK: - The list

    func numberOfRows(in tableView: NSTableView) -> Int { files.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int)
        -> NSView?
    {
        guard files.indices.contains(row) else { return nil }
        let label = NSTextField(labelWithString: files[row].lastPathComponent)
        label.font = DS.sans(12)
        label.textColor = DS.textStrong
        label.lineBreakMode = .byTruncatingMiddle
        label.toolTip = files[row].path
        return label
    }

    func tableViewSelectionDidChange(_ notification: Notification) {
        refresh()
    }
}
