import AppKit

/// Edit ▸ Fill…: Contents, Mode, Opacity and Preserve Transparency.
///
/// The sheet only collects a `FillSettings`; its caller captured the edit
/// target when it opened it and commits through `applyFill`, so the fill
/// lands on the layer, mask, plane or channel the user saw named. There is
/// no live preview. On a coverage target (a mask, a colour plane, an alpha
/// channel) the Mode popup omits the four HSL modes, which need colour, and
/// Preserve Transparency is disabled, because a plane has no transparency
/// of its own to keep. In Quick Mask the sheet fills the Quick Mask,
/// a coverage target like the others, and its hint says so.
///
/// Contents Pattern (a layer target only: `FillContents.offered`) shows a
/// Pattern popup of the four built-ins and then the user's library
/// (`PatternChoice.all`, re-read when the sheet opens); Apply embeds the
/// chosen tile in the `FillSettings`.
///
/// Paths panel ▸ Fill Path… opens the same sheet with its own title and hint
/// and without Pattern (`fill_path` fills one colour;
/// EditorViewController+PathSelect.swift).
final class FillSheetController: NSViewController {
    private let coverage: Bool
    private let quickMask: Bool
    private let sheetTitle: String
    private let sheetHint: String?
    /// Whether the Mode popup lists Behind and Clear (a layer target in a
    /// colour mode that offers them, `PaintOnlyBlend`); choosing either turns
    /// Preserve Transparency off, since both change the layer's alpha.
    private let paintOnly: Bool
    private let options: FillDialogOptions
    private let onApply: (FillSettings) -> Void
    private let offered: [FillContents]
    private let patterns: [PatternChoice]

    private let contentsPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let patternPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var patternRow: NSGridRow?
    private let colorWell = NSColorWell(frame: .zero)
    private let modePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let opacityField = NSTextField(string: "100")
    private let preserveBox = NSButton(
        checkboxWithTitle: "Preserve Transparency", target: nil, action: nil)
    private let status = NSTextField(labelWithString: "")

    /// `pattern` false leaves Pattern out of Contents on any target; `title`
    /// and `hint` replace Edit ▸ Fill…'s own (nil keeps its hint).
    init(
        coverage: Bool, quickMask: Bool, options: FillDialogOptions, paintOnly: Bool = false,
        pattern: Bool = true, title: String = "Fill", hint: String? = nil,
        onApply: @escaping (FillSettings) -> Void
    ) {
        self.coverage = coverage
        self.quickMask = quickMask
        self.paintOnly = paintOnly && !coverage && !quickMask
        self.options = options
        self.onApply = onApply
        sheetTitle = title
        sheetHint = hint
        offered = FillContents.offered(coverage: coverage || quickMask || !pattern)
        patterns = coverage || quickMask || !pattern ? [] : PatternChoice.all()
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FillSheetController does not support NSCoder")
    }

    override func loadView() {
        contentsPopup.addItems(withTitles: offered.map(\.title))
        let saved = FillContents(rawValue: options.contents)
        contentsPopup.selectItem(at: offered.firstIndex(where: { $0 == saved }) ?? 0)
        contentsPopup.target = self
        contentsPopup.action = #selector(contentsChanged(_:))
        contentsPopup.font = DS.sans(12)
        contentsPopup.setAccessibilityIdentifier("fill-contents")
        modePopup.setAccessibilityIdentifier("fill-mode")
        opacityField.setAccessibilityIdentifier("fill-opacity")

        patternPopup.font = DS.sans(12)
        patternPopup.setAccessibilityIdentifier("fill-pattern")
        for choice in patterns {
            let item = NSMenuItem(title: choice.name, action: nil, keyEquivalent: "")
            item.image = choice.image
            item.representedObject = choice.key
            patternPopup.menu?.addItem(item)
        }
        if let key = options.pattern, let index = patterns.firstIndex(where: { $0.key == key }) {
            patternPopup.selectItem(at: index)
        }

        colorWell.color = TextLayer.color(fromHex: options.customColor) ?? .black
        colorWell.widthAnchor.constraint(equalToConstant: 54).isActive = true
        colorWell.heightAnchor.constraint(equalToConstant: 22).isActive = true
        let contentsRow = NSStackView(views: [contentsPopup, colorWell])
        contentsRow.orientation = .horizontal
        contentsRow.spacing = 8

        ChannelMathControls.fillBlendModes(modePopup, singlePlane: coverage, paintOnly: paintOnly)
        if let mode = DrBlendMode.paintBlendModes.first(where: {
            $0.1.caseInsensitiveCompare(options.blendMode) == .orderedSame
        })?.0 {
            // A saved HSL mode is absent from a coverage target's menu (and a
            // saved Behind or Clear wherever they are not offered), so the
            // popup keeps the Normal the builder selected.
            modePopup.selectItem(withTag: Int(mode.rawValue))
        }
        modePopup.target = self
        modePopup.action = #selector(modeChanged(_:))

        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.allowsFloats = true
        formatter.maximumFractionDigits = 1
        opacityField.formatter = formatter
        opacityField.doubleValue = min(max(options.opacity, 0), 100)
        opacityField.widthAnchor.constraint(equalToConstant: 74).isActive = true
        DSField.style(opacityField)
        let percent = NSTextField(labelWithString: "%")
        percent.font = DS.mono(13)
        percent.textColor = DS.textMuted
        let opacityRow = NSStackView(views: [opacityField, percent])
        opacityRow.orientation = .horizontal
        opacityRow.spacing = 8

        preserveBox.font = DS.sans(13)
        preserveBox.isEnabled = !coverage
        preserveBox.state = options.preserveTransparency && !coverage ? .on : .off
        modeChanged(nil)

        status.font = DS.sans(12)
        status.textColor = DS.textMuted

        let grid = NSGridView(views: [
            [fieldLabel("Contents:"), contentsRow],
            [fieldLabel("Pattern:"), patternPopup],
            [fieldLabel("Mode:"), modePopup],
            [fieldLabel("Opacity:"), opacityRow],
            [NSGridCell.emptyContentView, preserveBox],
            [NSGridCell.emptyContentView, status],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        patternRow = grid.row(at: 1)
        updateColorWell()

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.setAccessibilityIdentifier("fill-apply")
        let contentsWords: String = patterns.isEmpty
            ? "with one colour" : "with one colour or a pattern"
        let hint: String = sheetHint ?? (quickMask
            ? "Fills the whole Quick Mask with the colour's gray, through the blend "
                + "mode at the opacity: white adds to the selection, black removes it."
            : "Fills the selection — or the whole target when nothing is selected — "
                + contentsWords + ", through the blend mode at the opacity. "
                + "Preserve Transparency keeps the layer's alpha as it is.")
        view = makeSheetView(
            title: sheetTitle,
            hint: hint,
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    // MARK: - Actions

    @objc private func contentsChanged(_ sender: Any?) {
        updateColorWell()
    }

    /// Behind and Clear change the layer's alpha, so either disables and
    /// clears Preserve Transparency; any other mode enables it again.
    @objc private func modeChanged(_ sender: Any?) {
        let changesAlpha = DrBlendMode.isPaintOnly(ChannelMathControls.blendMode(modePopup))
        if changesAlpha { preserveBox.state = .off }
        preserveBox.isEnabled = !coverage && !changesAlpha
    }

    /// The well is shown only for Color…, the one choice it names; the
    /// Pattern row only for Pattern.
    private func updateColorWell() {
        colorWell.isHidden = selectedContents != .color
        if colorWell.isHidden { colorWell.deactivate() }
        patternRow?.isHidden = selectedContents != .pattern
    }

    private var selectedContents: FillContents {
        let index = contentsPopup.indexOfSelectedItem
        return offered.indices.contains(index) ? offered[index] : .foreground
    }

    /// The chosen Pattern row, nil when there is none to choose.
    private var selectedPattern: PatternChoice? {
        let index = patternPopup.indexOfSelectedItem
        return patterns.indices.contains(index) ? patterns[index] : nil
    }

    @objc private func applyClicked(_ sender: Any?) {
        let text = opacityField.stringValue.trimmingCharacters(in: .whitespaces)
        let opacity = opacityField.doubleValue
        // An empty field is not "0": the formatter would read it as one and
        // the fill would silently change nothing.
        guard !text.isEmpty, opacity.isFinite, (0...100).contains(opacity) else {
            NSSound.beep()
            status.stringValue = "Opacity must be a number from 0 to 100."
            return
        }
        let mode = ChannelMathControls.blendMode(modePopup)
        let preserve = preserveBox.isEnabled && preserveBox.state == .on
        let custom = colorWell.color
        let pattern = selectedContents == .pattern ? selectedPattern : nil
        if selectedContents == .pattern, pattern == nil {
            NSSound.beep()
            status.stringValue = "Choose a pattern."
            return
        }
        var settings = FillSettings(
            contents: selectedContents, customColor: custom, mode: mode, opacity: opacity,
            preserveTransparency: preserve)
        settings.pattern = pattern?.ref
        ToolOptionsStore.shared.fillDialog = FillDialogOptions(
            contents: selectedContents.rawValue, customColor: TextLayer.hex(custom),
            blendMode: DrBlendMode.displayName(for: mode), opacity: opacity,
            preserveTransparency: coverage
                ? options.preserveTransparency : preserve,
            pattern: pattern?.key ?? options.pattern)
        colorWell.deactivate()
        dismiss(self)
        onApply(settings)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        colorWell.deactivate()
        dismiss(self)
    }
}
