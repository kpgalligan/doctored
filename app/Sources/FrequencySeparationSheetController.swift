import AppKit

/// Filters ▸ Other ▸ Frequency Separation…: the low layer's blur radius. Apply hands it to
/// `onApply` (`EditorViewController.commitFrequencySeparation`).
final class FrequencySeparationSheetController: NSViewController {
    private let onApply: (Double) -> Void
    private let radiusSlider = NSSlider(
        value: 4, minValue: FrequencySeparationParameters.radiusRange.lowerBound,
        maxValue: FrequencySeparationParameters.radiusRange.upperBound, target: nil, action: nil)
    private let radiusField = NSTextField(string: "4.0")

    init(onApply: @escaping (Double) -> Void) {
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FrequencySeparationSheetController does not support NSCoder")
    }

    override func loadView() {
        radiusSlider.isContinuous = true
        radiusSlider.target = self
        radiusSlider.action = #selector(sliderChanged(_:))
        radiusSlider.widthAnchor.constraint(equalToConstant: 170).isActive = true
        radiusField.target = self
        radiusField.action = #selector(fieldChanged(_:))
        radiusField.widthAnchor.constraint(equalToConstant: 56).isActive = true
        DSField.style(radiusField)
        let row = NSStackView(views: [radiusSlider, radiusField])
        row.orientation = .horizontal
        row.spacing = 10

        let grid = NSGridView(views: [[fieldLabel("Radius (px):"), row]])
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Frequency separation",
            hint: "Splits the layer into a blurred Low layer and a High detail layer above it "
                + "(Linear Light, clipped), which together recompose it within one level where "
                + "it is opaque. Retouch tone on Low and texture on High. The selection is ignored.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    private var radius: Double {
        let range = FrequencySeparationParameters.radiusRange
        return min(max(radiusSlider.doubleValue, range.lowerBound), range.upperBound)
    }

    @objc private func sliderChanged(_ sender: Any?) {
        radiusField.stringValue = String(format: "%.1f", radius)
    }

    @objc private func fieldChanged(_ sender: Any?) {
        radiusSlider.doubleValue = radiusField.doubleValue
        radiusField.stringValue = String(format: "%.1f", radius)
    }

    @objc private func applyClicked(_ sender: Any?) {
        // A typed value counts even when Return was never pressed.
        fieldChanged(sender)
        let chosen = radius
        dismiss(self)
        onApply(chosen)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
