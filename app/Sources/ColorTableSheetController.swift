import AppKit
import UniformTypeIdentifiers

/// Image ▸ Mode ▸ Color Table… (#1 Indexed phase 1c): the table as a 16-column swatch grid, a
/// Color well editing the selected entry, the Transparent index (None or an entry; the document's
/// table only), and Load… / Save… of Adobe Color Table (`.act`) files through the core
/// (`IndexedColorTable.readAct` / `writeAct`). OK asks `refusal` first (the core's plan: a table
/// shorter than the image's largest index needs) and shows its sentence here with a beep; otherwise
/// it hands the table to `commit`. The same dialog edits the Indexed Color sheet's Custom palette
/// (`.custom`: no transparent index, any table of 2…256 entries). The well and the swatches show
/// each entry's codes in the document's RGB colour space (`tableSpace(for:)`), so a P3-tagged
/// document shows its true colours (#1 Indexed phase 2a).
final class ColorTableSheetController: NSViewController {
    enum Purpose { case document, custom }

    private var table: IndexedColorTable
    private let purpose: Purpose
    private let space: NSColorSpace
    private let refusal: (IndexedColorTable) -> String?
    private let commit: (IndexedColorTable) -> Void

    private let swatches = ColorTableSwatchView()
    private let colorWell = NSColorWell()
    private let transparentPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let note = NSTextField(wrappingLabelWithString: "")

    init(table: IndexedColorTable, purpose: Purpose, space: NSColorSpace,
         refusal: @escaping (IndexedColorTable) -> String?,
         commit: @escaping (IndexedColorTable) -> Void) {
        self.table = table
        self.purpose = purpose
        self.space = space
        self.refusal = refusal
        self.commit = commit
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ColorTableSheetController does not support NSCoder")
    }

    /// The space the table's codes are shown in: the document's drawing space (`nsColorSpace`, the
    /// authored/sampled rule) when it is an RGB space, else sRGB (and sRGB with no document). Never
    /// `NSColorSpace(cgColorSpace:)`, which is non-nil for a LUT profile whose conversions fail.
    static func tableSpace(for doc: RasterDocument?) -> NSColorSpace {
        guard let space = doc?.nsColorSpace, space.colorSpaceModel == .rgb else { return .sRGB }
        return space
    }

    override func loadView() {
        swatches.space = space
        swatches.onSelect = { [weak self] index in self?.selectEntry(index) }
        swatches.setAccessibilityIdentifier("color-table-swatches")
        colorWell.target = self
        colorWell.action = #selector(colorChanged(_:))
        colorWell.setAccessibilityIdentifier("color-table-color")
        transparentPopup.font = DS.sans(13)
        transparentPopup.target = self
        transparentPopup.action = #selector(transparentChanged(_:))
        transparentPopup.setAccessibilityIdentifier("color-table-transparent")
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        note.setAccessibilityIdentifier("color-table-note")

        let load = NSButton(title: "Load…", target: self, action: #selector(loadClicked(_:)))
        let save = NSButton(title: "Save…", target: self, action: #selector(saveClicked(_:)))
        load.setAccessibilityIdentifier("color-table-load")
        save.setAccessibilityIdentifier("color-table-save")
        let files = NSStackView(views: [load, save])
        files.spacing = 8

        var rows: [[NSView]] = [[fieldLabel("Color:"), colorWell]]
        if purpose == .document { rows.append([fieldLabel("Transparent:"), transparentPopup]) }
        rows.append([NSGridCell.emptyContentView, files])
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let content = NSStackView(views: [swatches, grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancelButton.setAccessibilityIdentifier("color-table-cancel")
        let okButton = StickerButton(title: "OK", style: .primary, target: self, action: #selector(okClicked(_:)))
        okButton.setAccessibilityIdentifier("color-table-ok")
        reloadTable(selecting: 0)
        let hint = purpose == .document
            ? "Edits the document's palette: every pixel keeps its "
                + "index and takes its entry's new colour. Load and Save read and write Adobe "
                + "Color Table (.act) files."
            : "The Custom palette's colours: each pixel takes its nearest entry. Load reads an "
                + "Adobe Color Table (.act) file."
        view = makeSheetView(
            title: purpose == .document ? "Color Table" : "Custom Palette",
            hint: hint,
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: okButton,
                                     leading: purpose == .document ? [sheetFootnote("one undo step")] : []))
    }

    /// Shows `table` and selects `index` (clamped).
    private func reloadTable(selecting index: Int) {
        swatches.colors = table.colors
        swatches.transparentIndex = purpose == .document ? table.transparentIndex : nil
        transparentPopup.removeAllItems()
        transparentPopup.addItem(withTitle: "None")
        transparentPopup.addItems(withTitles: table.colors.indices.map { "\($0)" })
        transparentPopup.selectItem(at: table.transparentIndex.map { $0 + 1 } ?? 0)
        selectEntry(min(max(index, 0), table.colors.count - 1))
        note.stringValue = "\(table.colors.count) colours."
    }

    private func selectEntry(_ index: Int) {
        guard table.colors.indices.contains(index) else { return }
        swatches.selected = index
        colorWell.color = Self.color(table.colors[index], in: space)
    }

    /// An entry's codes as a colour in `space`, and back (each component rounded to its 8-bit
    /// code), so an unchanged entry round-trips exactly.
    static func color(_ codes: [UInt8], in space: NSColorSpace) -> NSColor {
        let unit = { (index: Int) in CGFloat(codes.count == 3 ? codes[index] : 0) / 255 }
        let components = [unit(0), unit(1), unit(2), 1]
        return NSColor(colorSpace: space, components: components, count: 4)
    }

    /// nil when the conversion into `space` refuses (`ColorProfile.converted`, LUT-aware).
    private static func codes(_ color: NSColor, in space: NSColorSpace) -> [UInt8]? {
        guard let converted = ColorProfile.converted(color, into: space),
              converted.numberOfComponents >= 3 else { return nil }
        let code = { (value: CGFloat) in UInt8((min(max(value, 0), 1) * 255).rounded()) }
        return [code(converted.redComponent), code(converted.greenComponent),
                code(converted.blueComponent)]
    }

    @objc private func colorChanged(_ sender: Any?) {
        let index = swatches.selected
        guard table.colors.indices.contains(index) else { return }
        guard let codes = Self.codes(colorWell.color, in: space) else {
            NSSound.beep()
            colorWell.color = Self.color(table.colors[index], in: space)
            return
        }
        table.colors[index] = codes
        swatches.colors = table.colors
    }

    @objc private func transparentChanged(_ sender: Any?) {
        let item = transparentPopup.indexOfSelectedItem
        table.transparentIndex = item <= 0 ? nil : item - 1
        swatches.transparentIndex = table.transparentIndex
    }

    @objc private func loadClicked(_ sender: Any?) {
        guard let window = view.window else { NSSound.beep(); return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "act") ?? .data]
        panel.allowsMultipleSelection = false
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.load(url)
        }
    }

    private func load(_ url: URL) {
        do {
            var loaded = try IndexedColorTable.readAct(url: url)
            // The Custom palette has no transparent entry: a transparent conversion appends one.
            if purpose == .custom { loaded.transparentIndex = nil }
            table = loaded
            reloadTable(selecting: 0)
        } catch {
            NSSound.beep()
            note.stringValue = error.localizedDescription
        }
    }

    @objc private func saveClicked(_ sender: Any?) {
        guard let window = view.window else { NSSound.beep(); return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "act") ?? .data]
        panel.nameFieldStringValue = "Color Table.act"
        let table = table
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            do {
                try table.writeAct(to: url)
            } catch {
                NSSound.beep()
                self?.note.stringValue = error.localizedDescription
            }
        }
    }

    @objc private func okClicked(_ sender: Any?) {
        if let reason = refusal(table) {
            NSSound.beep()
            note.stringValue = reason
            return
        }
        let table = table
        dismiss(self)
        commit(table)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}

/// The Color Table's swatch grid: 16 entries a row, the selected one outlined, the transparent
/// one crossed. A click selects an entry (`onSelect`).
final class ColorTableSwatchView: NSView {
    static let columns = 16
    static let cell: CGFloat = 22

    var colors: [[UInt8]] = [] {
        didSet {
            invalidateIntrinsicContentSize()
            needsDisplay = true
        }
    }
    /// The space the codes are drawn in (`ColorTableSheetController.tableSpace(for:)`).
    var space: NSColorSpace = .sRGB { didSet { needsDisplay = true } }
    var transparentIndex: Int? { didSet { needsDisplay = true } }
    var selected = 0 { didSet { needsDisplay = true } }
    var onSelect: ((Int) -> Void)?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ColorTableSwatchView does not support NSCoder")
    }

    override var isFlipped: Bool { true }

    override var intrinsicContentSize: NSSize {
        let rows = max(1, (colors.count + Self.columns - 1) / Self.columns)
        return NSSize(width: CGFloat(Self.columns) * Self.cell, height: CGFloat(rows) * Self.cell)
    }

    private func rect(_ index: Int) -> NSRect {
        NSRect(x: CGFloat(index % Self.columns) * Self.cell, y: CGFloat(index / Self.columns) * Self.cell,
               width: Self.cell, height: Self.cell)
    }

    override func draw(_ dirtyRect: NSRect) {
        for (index, codes) in colors.enumerated() where codes.count == 3 {
            let box = rect(index).insetBy(dx: 1, dy: 1)
            ColorTableSheetController.color(codes, in: space).setFill()
            box.fill()
            if index == transparentIndex {
                let cross = NSBezierPath()
                cross.move(to: NSPoint(x: box.minX, y: box.maxY))
                cross.line(to: NSPoint(x: box.maxX, y: box.minY))
                DS.textStrong.setStroke()
                cross.lineWidth = 1.5
                cross.stroke()
            }
            if index == selected {
                DS.textStrong.setStroke()
                let outline = NSBezierPath(rect: box.insetBy(dx: -0.5, dy: -0.5))
                outline.lineWidth = 2
                outline.stroke()
            }
        }
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let column = Int(point.x / Self.cell), row = Int(point.y / Self.cell)
        guard (0..<Self.columns).contains(column), row >= 0 else { return }
        let index = row * Self.columns + column
        guard colors.indices.contains(index) else { return }
        onSelect?(index)
    }
}
