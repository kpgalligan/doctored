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
final class FillSheetController: NSViewController {
    private let coverage: Bool
    private let quickMask: Bool
    private let options: FillDialogOptions
    private let onApply: (FillSettings) -> Void

    private let contentsPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let colorWell = NSColorWell(frame: .zero)
    private let modePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let opacityField = NSTextField(string: "100")
    private let preserveBox = NSButton(
        checkboxWithTitle: "Preserve Transparency", target: nil, action: nil)
    private let status = NSTextField(labelWithString: "")

    init(
        coverage: Bool, quickMask: Bool, options: FillDialogOptions,
        onApply: @escaping (FillSettings) -> Void
    ) {
        self.coverage = coverage
        self.quickMask = quickMask
        self.options = options
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FillSheetController does not support NSCoder")
    }

    override func loadView() {
        contentsPopup.addItems(withTitles: FillContents.allCases.map(\.title))
        contentsPopup.selectItem(at: FillContents(rawValue: options.contents)?.rawValue ?? 0)
        contentsPopup.target = self
        contentsPopup.action = #selector(contentsChanged(_:))
        contentsPopup.font = DS.sans(12)

        colorWell.color = TextLayer.color(fromHex: options.customColor) ?? .black
        colorWell.widthAnchor.constraint(equalToConstant: 54).isActive = true
        colorWell.heightAnchor.constraint(equalToConstant: 22).isActive = true
        let contentsRow = NSStackView(views: [contentsPopup, colorWell])
        contentsRow.orientation = .horizontal
        contentsRow.spacing = 8

        ChannelMathControls.fillBlendModes(modePopup, singlePlane: coverage)
        if let mode = DrBlendMode.allBlendModes.first(where: {
            $0.1.caseInsensitiveCompare(options.blendMode) == .orderedSame
        })?.0 {
            // A saved HSL mode is absent from a coverage target's menu, so
            // the popup keeps the Normal the builder selected.
            modePopup.selectItem(withTag: Int(mode.rawValue))
        }

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

        status.font = DS.sans(12)
        status.textColor = DS.textMuted

        let grid = NSGridView(views: [
            [fieldLabel("Contents:"), contentsRow],
            [fieldLabel("Mode:"), modePopup],
            [fieldLabel("Opacity:"), opacityRow],
            [NSGridCell.emptyContentView, preserveBox],
            [NSGridCell.emptyContentView, status],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        updateColorWell()

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Fill",
            hint: quickMask
                ? "Fills the whole Quick Mask with the colour's gray, through the blend "
                    + "mode at the opacity: white adds to the selection, black removes it."
                : "Fills the selection — or the whole target when nothing is selected — "
                    + "with one colour, through the blend mode at the opacity. "
                    + "Preserve Transparency keeps the layer's alpha as it is.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    // MARK: - Actions

    @objc private func contentsChanged(_ sender: Any?) {
        updateColorWell()
    }

    /// The well is shown only for Color…, the one choice it names.
    private func updateColorWell() {
        colorWell.isHidden = selectedContents != .color
        if colorWell.isHidden { colorWell.deactivate() }
    }

    private var selectedContents: FillContents {
        FillContents(rawValue: contentsPopup.indexOfSelectedItem) ?? .foreground
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
        let settings = FillSettings(
            contents: selectedContents, customColor: custom, mode: mode, opacity: opacity,
            preserveTransparency: preserve)
        ToolOptionsStore.shared.fillDialog = FillDialogOptions(
            contents: selectedContents.rawValue, customColor: TextLayer.hex(custom),
            blendMode: DrBlendMode.displayName(for: mode), opacity: opacity,
            preserveTransparency: coverage
                ? options.preserveTransparency : preserve)
        colorWell.deactivate()
        dismiss(self)
        onApply(settings)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        colorWell.deactivate()
        dismiss(self)
    }
}
