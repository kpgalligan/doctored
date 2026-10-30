import AppKit

/// Edit ▸ Stroke…: Width, Color, Location, Mode, Opacity and
/// Preserve Transparency.
///
/// Like `FillSheetController`, the sheet only collects a `StrokeSettings`;
/// its caller captured the edit target and the selection when it opened it
/// and commits through `applyStroke`. There is no live preview. The colour
/// well opens on the foreground colour and is not persisted; the other
/// values are (`ToolOptionsStore.strokeDialog`). On a coverage target the
/// Mode popup omits the four HSL modes and Preserve Transparency is
/// disabled, as in Fill….
final class StrokeSheetController: NSViewController {
    private let target: PaintTarget
    private let initialColor: NSColor
    private let options: StrokeDialogOptions
    private let onApply: (StrokeSettings) -> Void

    private let widthField = NSTextField(string: "1")
    private let colorWell = NSColorWell(frame: .zero)
    private let locationRadios = StrokeLocation.allCases.map {
        NSButton(radioButtonWithTitle: $0.title, target: nil, action: nil)
    }
    private let modePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let opacityField = NSTextField(string: "100")
    private let preserveBox = NSButton(
        checkboxWithTitle: "Preserve Transparency", target: nil, action: nil)
    private let status = NSTextField(labelWithString: "")

    init(
        target: PaintTarget, color: NSColor, options: StrokeDialogOptions,
        onApply: @escaping (StrokeSettings) -> Void
    ) {
        self.target = target
        self.initialColor = color
        self.options = options
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("StrokeSheetController does not support NSCoder")
    }

    override func loadView() {
        let widthFormatter = NumberFormatter()
        widthFormatter.numberStyle = .decimal
        widthFormatter.allowsFloats = true
        widthFormatter.maximumFractionDigits = 1
        widthField.formatter = widthFormatter
        widthField.doubleValue = min(
            max(options.width, StrokeSettings.widthRange.lowerBound),
            StrokeSettings.widthRange.upperBound)
        widthField.widthAnchor.constraint(equalToConstant: 74).isActive = true
        DSField.style(widthField)
        let px = NSTextField(labelWithString: "px")
        px.font = DS.mono(13)
        px.textColor = DS.textMuted
        let widthRow = NSStackView(views: [widthField, px])
        widthRow.orientation = .horizontal
        widthRow.spacing = 8

        colorWell.color = initialColor
        colorWell.widthAnchor.constraint(equalToConstant: 54).isActive = true
        colorWell.heightAnchor.constraint(equalToConstant: 22).isActive = true

        let selected = StrokeLocation(rawValue: options.location) ?? .center
        for (location, radio) in zip(StrokeLocation.allCases, locationRadios) {
            radio.target = self
            radio.action = #selector(locationChanged(_:))
            radio.font = DS.sans(13)
            radio.state = location == selected ? .on : .off
        }
        let locationRow = NSStackView(views: locationRadios)
        locationRow.orientation = .horizontal
        locationRow.spacing = 14

        ChannelMathControls.fillBlendModes(modePopup, singlePlane: target.isCoverage)
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
        preserveBox.isEnabled = !target.isCoverage
        preserveBox.state = options.preserveTransparency && !target.isCoverage ? .on : .off

        status.font = DS.sans(12)
        status.textColor = DS.textMuted

        let grid = NSGridView(views: [
            [fieldLabel("Width:"), widthRow],
            [fieldLabel("Color:"), colorWell],
            [fieldLabel("Location:"), locationRow],
            [fieldLabel("Mode:"), modePopup],
            [fieldLabel("Opacity:"), opacityRow],
            [NSGridCell.emptyContentView, preserveBox],
            [NSGridCell.emptyContentView, status],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Stroke",
            hint: "Strokes the selection's edge — or, with nothing selected, the edge of the "
                + "layer's own pixels — with one colour, through the blend mode at the "
                + "opacity: Inside, centred on, or Outside the edge. The canvas edge counts "
                + "as an edge, so Select All strokes a frame.",
            content: grid,
            buttonRow: makeButtonRow(
                cancel: cancelButton, apply: applyButton,
                leading: [sheetFootnote("one undo step")]))
    }

    // MARK: - Actions

    @objc private func locationChanged(_ sender: Any?) {
        guard let sender = sender as? NSButton else { return }
        for radio in locationRadios { radio.state = radio === sender ? .on : .off }
    }

    private var selectedLocation: StrokeLocation {
        guard let index = locationRadios.firstIndex(where: { $0.state == .on }) else {
            return .center
        }
        return StrokeLocation(rawValue: index) ?? .center
    }

    @objc private func applyClicked(_ sender: Any?) {
        let widthText = widthField.stringValue.trimmingCharacters(in: .whitespaces)
        let width = widthField.doubleValue
        guard !widthText.isEmpty, width.isFinite, StrokeSettings.widthRange.contains(width) else {
            NSSound.beep()
            status.stringValue = "Width must be a number from 1 to 250 (pixels)."
            return
        }
        let text = opacityField.stringValue.trimmingCharacters(in: .whitespaces)
        let opacity = opacityField.doubleValue
        // An empty field is not "0" (FillSheetController's rule).
        guard !text.isEmpty, opacity.isFinite, (0...100).contains(opacity) else {
            NSSound.beep()
            status.stringValue = "Opacity must be a number from 0 to 100."
            return
        }
        let mode = ChannelMathControls.blendMode(modePopup)
        let preserve = preserveBox.isEnabled && preserveBox.state == .on
        let location = selectedLocation
        let settings = StrokeSettings(
            width: width, color: colorWell.color, location: location, mode: mode,
            opacity: opacity, preserveTransparency: preserve)
        ToolOptionsStore.shared.strokeDialog = StrokeDialogOptions(
            width: width, location: location.rawValue,
            blendMode: DrBlendMode.displayName(for: mode), opacity: opacity,
            preserveTransparency: target.isCoverage
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
