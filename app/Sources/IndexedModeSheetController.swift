import AppKit

/// Image ▸ Mode ▸ Indexed Color… (#1 Indexed phases 1a to 3d): the dialog — Palette
/// (Exact, System (Mac OS), System (Windows), Web, Uniform, Adaptive (median cut), Custom… with its
/// Table… (`ColorTableSheetController`, `.custom`), Previous (the last conversion's palette,
/// `IndexedPalettes`, disabled when there is none); Uniform by default; both sent to the core as
/// `palette: "custom"` with a `table`; Local and Master Perceptual and Selective listed disabled for
/// good, unpublished and unmatched by phase 3d's take), Colors and Forced (Uniform and Adaptive;
/// since phase 3c Custom… with its Forced
/// Table…, sent as `forced_table`; Colors shows the table's count for Custom and Previous, and
/// returning to a counted palette restores the user's own value), Transparency, Matte (None,
/// Foreground Color, Background Color, White,
/// Black, 50% Gray, Netscape Gray, resolved to the document's codes) and Dither (None, Diffusion
/// with Amount and Preserve Exact Colors, since phase 3c Pattern and Noise, disabled for Exact).
/// Apply asks the core's plan of a typed (8-bit Grayscale)
/// source first: a palette refusal beeps and shows its sentence here; the conversion, its
/// "Flatten layers?" question and its one undo step are `EditorViewController+IndexedMode.swift`'s.
final class IndexedModeSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var document: ImageDocument?

    /// The palettes in menu order, with their wire names. Local and Master Perceptual and Selective
    /// are listed disabled for good: Adobe has not published them and no candidate matched phase
    /// 3d's Photoshop take (`unpublishedPalettes`).
    private static let palettes: [(title: String, name: String)] = [
        ("Exact", "exact"), ("System (Mac OS)", "system_mac"), ("System (Windows)", "system_windows"),
        ("Web", "web"), ("Uniform", "uniform"), ("Local (Perceptual)", "local_perceptual"),
        ("Local (Selective)", "local_selective"), ("Adaptive (median cut)", "adaptive"),
        ("Master (Perceptual)", "master_perceptual"), ("Master (Selective)", "master_selective"),
        ("Custom…", "custom"), ("Previous", "previous"),
    ]
    private static let unpublishedPalettes: Set<String> = [
        "local_perceptual", "local_selective", "master_perceptual", "master_selective",
    ]
    private static let forcedColors: [(title: String, name: String)] = [
        ("None", "none"), ("Black and White", "black_white"), ("Primaries", "primaries"), ("Web", "web"),
        ("Custom…", "custom"),
    ]
    /// The dithers in menu order, with their wire names (phase 3c adds Pattern and Noise).
    private static let dithers: [(title: String, name: String)] = [
        ("None", "none"), ("Diffusion", "diffusion"), ("Pattern", "pattern"), ("Noise", "noise"),
    ]
    /// The mattes in menu order; nil is None, and the two colour wells resolve at Apply.
    private static let mattes: [(title: String, name: String?)] = [
        ("None", nil), ("Foreground Color", "foreground"), ("Background Color", "background"),
        ("White", "white"), ("Black", "black"), ("50% Gray", "gray50"), ("Netscape Gray", "netscape_gray"),
    ]

    private let palettePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let colorsField = NSTextField(string: "256")
    private let forcedPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let transparencyBox = NSButton(checkboxWithTitle: "Transparency", target: nil, action: nil)
    private let mattePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let ditherPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let amountField = NSTextField(string: "75")
    private let preserveBox = NSButton(checkboxWithTitle: "Preserve Exact Colors", target: nil, action: nil)
    private let note = NSTextField(wrappingLabelWithString: "")
    private var applyButton: NSButton!
    private var tableButton: NSButton!
    private var forcedTableButton: NSButton!
    /// The Custom palette's table: Previous when there is one, else black and white.
    private var customTable = IndexedPalettes.previous
        ?? IndexedColorTable(colors: [[0, 0, 0], [255, 255, 255]], transparentIndex: nil)
    /// Forced Custom's colours (phase 3c): black and white until the user edits them.
    private var forcedTable = IndexedColorTable(colors: [[0, 0, 0], [255, 255, 255]],
                                                transparentIndex: nil)
    /// The user's Colors while a counted palette was last selected (Custom and Previous overwrite
    /// the field with their table's count).
    private var countedColors = "256"
    private var wasCounted = true

    init(editor: EditorViewController, document: ImageDocument) {
        self.editor = editor
        self.document = document
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("IndexedModeSheetController does not support NSCoder")
    }

    override func loadView() {
        let popups: [(NSPopUpButton, [String], String)] = [
            (palettePopup, Self.palettes.map(\.title), "indexed-mode-palette"),
            (forcedPopup, Self.forcedColors.map(\.title), "indexed-mode-forced"),
            (mattePopup, Self.mattes.map(\.title), "indexed-mode-matte"),
            (ditherPopup, Self.dithers.map(\.title), "indexed-mode-dither"),
        ]
        for (popup, titles, identifier) in popups {
            popup.addItems(withTitles: titles)
            popup.font = DS.sans(13)
            popup.target = self
            popup.action = #selector(optionsChanged(_:))
            popup.setAccessibilityIdentifier(identifier)
        }
        palettePopup.selectItem(at: Self.palettes.firstIndex { $0.name == "uniform" } ?? 0)
        palettePopup.autoenablesItems = false
        if let previous = Self.palettes.firstIndex(where: { $0.name == "previous" }) {
            palettePopup.item(at: previous)?.isEnabled = IndexedPalettes.previous != nil
        }
        for (index, palette) in Self.palettes.enumerated() where Self.unpublishedPalettes.contains(palette.name) {
            palettePopup.item(at: index)?.isEnabled = false
        }
        tableButton = NSButton(title: "Table…", target: self, action: #selector(tableClicked(_:)))
        tableButton.setAccessibilityIdentifier("indexed-mode-table")
        forcedTableButton = NSButton(title: "Forced Table…", target: self,
                                     action: #selector(forcedTableClicked(_:)))
        forcedTableButton.setAccessibilityIdentifier("indexed-mode-forced-table")
        ditherPopup.autoenablesItems = false
        ditherPopup.selectItem(at: 1)
        for (field, identifier) in [(colorsField, "indexed-mode-colors"), (amountField, "indexed-mode-amount")] {
            field.font = DS.sans(13)
            field.alignment = .right
            field.widthAnchor.constraint(equalToConstant: 64).isActive = true
            field.target = self
            field.action = #selector(optionsChanged(_:))
            field.setAccessibilityIdentifier(identifier)
        }
        for box in [transparencyBox, preserveBox] {
            box.font = DS.sans(13)
            box.target = self
            box.action = #selector(optionsChanged(_:))
        }
        transparencyBox.state = .on
        transparencyBox.setAccessibilityIdentifier("indexed-mode-transparency")
        preserveBox.setAccessibilityIdentifier("indexed-mode-preserve")
        note.font = DS.sans(12)
        note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2
        note.setAccessibilityIdentifier("indexed-mode-note")

        let amountRow = NSStackView(views: [amountField, fieldLabel("%")])
        amountRow.spacing = 4
        let paletteRow = NSStackView(views: [palettePopup, tableButton])
        paletteRow.spacing = 8
        let forcedRow = NSStackView(views: [forcedPopup, forcedTableButton])
        forcedRow.spacing = 8
        let grid = NSGridView(views: [
            [fieldLabel("Palette:"), paletteRow],
            [fieldLabel("Colors:"), colorsField],
            [fieldLabel("Forced:"), forcedRow],
            [NSGridCell.emptyContentView, transparencyBox],
            [fieldLabel("Matte:"), mattePopup],
            [fieldLabel("Dither:"), ditherPopup],
            [fieldLabel("Amount:"), amountRow],
            [NSGridCell.emptyContentView, preserveBox],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let content = NSStackView(views: [grid, note])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancelButton.setAccessibilityIdentifier("indexed-mode-cancel")
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("indexed-mode-apply")
        syncControls()
        view = makeSheetView(
            title: "Indexed Color",
            hint: "Converts the image to one layer of at most 256 palette colours. The palette, "
                + "its transparent index and every pixel's index are kept in the document.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton,
                                     leading: [sheetFootnote("one undo step")]))
    }

    private var paletteName: String {
        let index = palettePopup.indexOfSelectedItem
        return Self.palettes.indices.contains(index) ? Self.palettes[index].name : "uniform"
    }

    private var ditherName: String {
        let index = ditherPopup.indexOfSelectedItem
        return Self.dithers.indices.contains(index) ? Self.dithers[index].name : "diffusion"
    }

    private var diffusion: Bool { ditherName == "diffusion" }

    private var forcedName: String {
        let index = forcedPopup.indexOfSelectedItem
        return Self.forcedColors.indices.contains(index) ? Self.forcedColors[index].name : "none"
    }

    /// The palettes that take Colors and Forced: Uniform and Adaptive.
    private var counted: Bool { ["uniform", "adaptive"].contains(paletteName) }

    /// The table Custom or Previous sends; nil for every other palette.
    private var tableColors: [[UInt8]]? {
        switch paletteName {
        case "custom": return customTable.colors
        case "previous": return IndexedPalettes.previous?.colors
        default: return nil
        }
    }

    private func syncControls() {
        if wasCounted && !counted { countedColors = colorsField.stringValue }
        if !wasCounted && counted { colorsField.stringValue = countedColors }
        wasCounted = counted
        colorsField.isEnabled = counted
        forcedPopup.isEnabled = counted
        if !counted { forcedPopup.selectItem(at: 0) }
        tableButton.isEnabled = paletteName == "custom"
        forcedTableButton.isEnabled = counted && forcedName == "custom"
        if let colors = tableColors { colorsField.stringValue = "\(colors.count)" }
        // Exact gives every pixel its own entry, so the ordered dithers do not apply to it.
        let ordered = paletteName != "exact"
        for (index, dither) in Self.dithers.enumerated() where ["pattern", "noise"].contains(dither.name) {
            ditherPopup.item(at: index)?.isEnabled = ordered
        }
        if !ordered, ["pattern", "noise"].contains(ditherName) {
            ditherPopup.selectItem(at: Self.dithers.firstIndex { $0.name == "none" } ?? 0)
        }
        amountField.isEnabled = diffusion
        preserveBox.isEnabled = diffusion
        note.stringValue = "Uniform, Web and the System palettes are fixed tables; Exact keeps every "
            + "colour when the image holds 256 or fewer; Adaptive is median cut (Heckbert, 1982) "
            + "over the image's colours; Custom maps each "
            + "pixel to its nearest table entry, and Previous reuses the last conversion's palette. "
            + "Pattern and Noise are declared ordered dithers (Photoshop's take: Pattern close on "
            + "the Web palette only, Noise not measurable); Local and Master Perceptual and "
            + "Selective are not available, as their algorithms are unpublished: use Adaptive."
    }

    /// The matte's 8-bit codes in the document's space: the named greys as they are, Foreground
    /// and Background through the document's colour space; nil for None.
    private func matteCodes() -> [Int]? {
        let index = mattePopup.indexOfSelectedItem
        guard Self.mattes.indices.contains(index), let name = Self.mattes[index].name else { return nil }
        if let codes = IndexedDocument.mattes[name] { return codes }
        guard let editor, let doc = document?.doc else { return nil }
        let color = name == "foreground" ? editor.paintColor : editor.backgroundColor
        let space = NSColorSpace(cgColorSpace: doc.colorSpace) ?? .sRGB
        let converted = color.usingColorSpace(space) ?? color.usingColorSpace(.sRGB) ?? color
        let code = { (value: CGFloat) in Int((min(max(value, 0), 1) * 255).rounded()) }
        if converted.numberOfComponents < 3 {
            let white = code(converted.whiteComponent)
            return [white, white, white]
        }
        return [code(converted.redComponent), code(converted.greenComponent), code(converted.blueComponent)]
    }

    /// The core's options for the current settings (`indexed_convert`), without Flatten.
    private func coreOptions() -> [String: Any] {
        var options: [String: Any] = [
            "palette": paletteName, "transparency": transparencyBox.state == .on,
            "dither": ditherName,
        ]
        if let colors = tableColors {
            options["palette"] = "custom"
            options["table"] = colors.map { $0.map { Int($0) } }
        }
        if counted {
            options["colors"] = min(max(colorsField.integerValue, 2), 256)
            options["forced"] = forcedName
            if forcedName == "custom" {
                options["forced_table"] = forcedTable.colors.map { $0.map { Int($0) } }
            }
        }
        if diffusion {
            options["amount"] = min(max(amountField.integerValue, 1), 100)
            options["preserve_exact"] = preserveBox.state == .on
        }
        if let matte = matteCodes() { options["matte"] = matte }
        return options
    }

    @objc private func optionsChanged(_ sender: Any?) {
        syncControls()
    }

    /// Custom's Table…: the Color Table dialog over the Custom palette (any 2…256 entries).
    @objc private func tableClicked(_ sender: Any?) {
        presentAsSheet(ColorTableSheetController(
            table: customTable, purpose: .custom,
            space: ColorTableSheetController.tableSpace(for: document?.doc), refusal: { _ in nil },
            commit: { [weak self] in self?.customTableChosen($0) }))
    }

    private func customTableChosen(_ table: IndexedColorTable) {
        customTable = table
        syncControls()
    }

    /// Forced Custom's Forced Table… (phase 3c): the Color Table dialog over the forced colours
    /// (any 2…256 entries), which lead the Uniform or Adaptive palette in order.
    @objc private func forcedTableClicked(_ sender: Any?) {
        presentAsSheet(ColorTableSheetController(
            table: forcedTable, purpose: .custom,
            space: ColorTableSheetController.tableSpace(for: document?.doc), refusal: { _ in nil },
            commit: { [weak self] in self?.forcedTableChosen($0) }))
    }

    private func forcedTableChosen(_ table: IndexedColorTable) {
        forcedTable = table
        syncControls()
    }

    @objc private func applyClicked(_ sender: Any?) {
        guard let document, let editor, let doc = document.doc else { NSSound.beep(); return }
        let options = coreOptions()
        // A typed source is planned here, so a palette refusal stays in the sheet; a flatten the
        // conversion asks about itself.
        if doc.precision != nil {
            do {
                let plan = try CmykModeConversion.plan(source: doc, target: .indexed, profile: nil,
                                                       options: CmykConversionOptions(), indexed: options)
                if let refusal = plan["refusal"] as? String, plan["flatten_required"] as? Bool != true {
                    NSSound.beep()
                    note.stringValue = refusal
                    return
                }
            } catch {
                NSSound.beep()
                note.stringValue = error.localizedDescription
                return
            }
        }
        dismiss(self)
        editor.commitIndexed(document: document, options: options, flatten: false)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
