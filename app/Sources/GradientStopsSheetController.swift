import AppKit

/// The Gradient tool's stops sheet, opened by the options bar's Edit…
/// button: the shared stop editor (`GradientStopsPane`) over the ramp the
/// tool paints, with a button that goes back to the two rail swatches, and
/// below it the ramp's Smoothness in percent.
///
/// Apply writes `ToolOptionsStore.shared.gradient.stops`, the stored ramp a
/// drag resolves through `GradientRamp`; "Use Foreground → Background" stores
/// nil. Both store the Smoothness field as a fraction clamped to 0…1 (a
/// non-finite entry leaves the stored value alone); Cancel stores nothing.
/// The sheet opens on the stored stops, or on the two swatches when
/// there are none. Reverse, Transparency, Opacity and the shape stay on the
/// bar: they apply to whatever ramp is stored. A tool option, not a document
/// edit: no undo step.
final class GradientStopsSheetController: NSViewController {
    private let onApply: () -> Void
    private let swatches: [GradientStop]
    private var pane: GradientStopsPane?
    private let smoothnessField = NSTextField(string: "")

    init(foreground: NSColor, background: NSColor, onApply: @escaping () -> Void) {
        self.onApply = onApply
        swatches = [
            GradientStop(
                position: 0, color: LayerStyleColor.hex(foreground),
                opacity: Double(foreground.alphaComponent)),
            GradientStop(
                position: 1, color: LayerStyleColor.hex(background),
                opacity: Double(background.alphaComponent)),
        ]
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("GradientStopsSheetController does not support NSCoder")
    }

    override func loadView() {
        var fill = GradientFill()
        fill.stops = ToolOptionsStore.shared.gradient.stops ?? swatches
        let pane = GradientStopsPane(fill: fill)
        self.pane = pane

        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.maximumFractionDigits = 1
        smoothnessField.formatter = formatter
        smoothnessField.alignment = .right
        smoothnessField.doubleValue = ToolOptionsStore.shared.gradient.smoothness * 100
        smoothnessField.identifier = NSUserInterfaceItemIdentifier("gradient-stops-smoothness")
        DSField.style(smoothnessField)
        smoothnessField.widthAnchor.constraint(equalToConstant: 62).isActive = true
        let percent = NSTextField(labelWithString: "%")
        percent.font = DS.sans(13)
        percent.textColor = DS.textMuted
        let smoothnessRow = NSStackView(views: [fieldLabel("Smoothness:"), smoothnessField, percent])
        smoothnessRow.orientation = .horizontal
        smoothnessRow.alignment = .centerY
        smoothnessRow.spacing = 6
        let content = NSStackView(views: [pane, smoothnessRow])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let reset = StickerButton(
            title: "Use Foreground → Background", style: .secondary,
            target: self, action: #selector(resetClicked(_:)))
        reset.identifier = NSUserInterfaceItemIdentifier("gradient-stops-reset")
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("gradient-stops-apply")
        view = makeSheetView(
            title: "Gradient Stops",
            hint: "Click the ramp to add a stop, drag one to move it, press ⌫ or drag it away "
                + "to remove it, drag a diamond to move a midpoint. Each stop has its own "
                + "colour and opacity.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton, leading: [reset]))
    }

    @objc private func applyClicked(_ sender: Any?) {
        if let stops = pane?.fill.stops, stops.count >= GradientEditorView.minStops {
            ToolOptionsStore.shared.gradient.stops = stops
        }
        storeSmoothness()
        dismiss(self)
        onApply()
    }

    /// Back to the two rail swatches: the tool follows them again.
    @objc private func resetClicked(_ sender: Any?) {
        ToolOptionsStore.shared.gradient.stops = nil
        storeSmoothness()
        dismiss(self)
        onApply()
    }

    /// The Smoothness field, 0 to 100 %, stored as a fraction. An empty field leaves the
    /// stored value as it is (`doubleValue` would read it as 0 %).
    private func storeSmoothness() {
        let typed = smoothnessField.stringValue.trimmingCharacters(in: .whitespaces)
        guard !typed.isEmpty else { return }
        let smoothness = smoothnessField.doubleValue / 100
        guard smoothness.isFinite else { return }
        ToolOptionsStore.shared.gradient.smoothness = min(max(smoothness, 0), 1)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
