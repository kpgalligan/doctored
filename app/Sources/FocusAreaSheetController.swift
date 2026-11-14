import AppKit

/// Select ▸ Focus Area…: Auto, the In-Focus Range and Soften Edge. Apply hands the parameters to
/// `onApply` (`EditorViewController.commitFocusArea`); the selection itself is the preview.
final class FocusAreaSheetController: NSViewController {
    private let onApply: (FocusAreaParameters) -> Void
    private let autoCheckbox = NSButton(checkboxWithTitle: "Auto", target: nil, action: nil)
    private let rangeSlider = NSSlider(
        value: 50, minValue: FocusAreaParameters.rangeRange.lowerBound,
        maxValue: FocusAreaParameters.rangeRange.upperBound, target: nil, action: nil)
    private let rangeField = NSTextField(string: "50")
    private let softenCheckbox = NSButton(checkboxWithTitle: "Soften Edge", target: nil, action: nil)

    init(onApply: @escaping (FocusAreaParameters) -> Void) {
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FocusAreaSheetController does not support NSCoder")
    }

    override func loadView() {
        for checkbox in [autoCheckbox, softenCheckbox] {
            checkbox.font = DS.sans(13)
            checkbox.state = .on
        }
        autoCheckbox.target = self
        autoCheckbox.action = #selector(autoChanged(_:))
        rangeSlider.isContinuous = true
        rangeSlider.target = self
        rangeSlider.action = #selector(sliderChanged(_:))
        rangeField.target = self
        rangeField.action = #selector(fieldChanged(_:))
        let row = ChannelMathControls.opacityRow(slider: rangeSlider, field: rangeField)
        syncEnabled()

        let grid = NSGridView(views: [
            [NSGridCell.emptyContentView, autoCheckbox],
            [fieldLabel("In-Focus Range:"), row],
            [NSGridCell.emptyContentView, softenCheckbox],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Focus area",
            hint: "Selects the in-focus part of the picture from its local sharpness. Auto picks "
                + "the cut; a lower In-Focus Range selects more. Flat areas count only when a "
                + "sharp area encloses them.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    private func syncEnabled() {
        let manual = autoCheckbox.state != .on
        rangeSlider.isEnabled = manual
        rangeField.isEnabled = manual
    }

    @objc private func autoChanged(_ sender: Any?) { syncEnabled() }

    @objc private func sliderChanged(_ sender: Any?) {
        ChannelMathControls.syncField(rangeField, to: rangeSlider)
    }

    @objc private func fieldChanged(_ sender: Any?) {
        ChannelMathControls.syncSlider(rangeSlider, to: rangeField)
    }

    @objc private func applyClicked(_ sender: Any?) {
        let range = FocusAreaParameters.rangeRange
        let parameters = FocusAreaParameters(
            range: autoCheckbox.state == .on
                ? nil : min(max(rangeSlider.doubleValue, range.lowerBound), range.upperBound),
            softenEdge: softenCheckbox.state == .on)
        dismiss(self)
        onApply(parameters)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
