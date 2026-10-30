import AppKit

/// HDR Toning's Local Adaptation rows, in the dialog's groups: Edge Glow
/// (Radius, Strength, Smooth Edges), Tone and Detail (Gamma, Exposure, Detail), Advanced (Shadow,
/// Highlight, Vibrance, Saturation), then the Toning Curve — the Curves dialog's
/// `CurveEditorView` over the image's log-luminance histogram (`HdrToningRun.histogram`, loaded
/// once by the sheet). It decides nothing: every edit hands the new values to `onChange`, and
/// `load(_:)` (a chosen preset, phase 4c) sets every row silently.
final class HdrToningLocalAdaptationPane {
    let view: NSView
    /// The Toning Curve's histogram, shown behind the curve.
    var histogram: HistogramBins? {
        didSet { curve.histogram = histogram }
    }
    /// Why the histogram could not be read: the plot says so and this is its tooltip.
    var histogramUnavailableReason: String? {
        didSet { curve.histogramUnavailableReason = histogramUnavailableReason }
    }
    private var values: HdrLocalAdaptation
    private let onChange: (HdrLocalAdaptation) -> Void
    private let curve = CurveEditorView()
    private let radiusRow: CardControlRow, strengthRow: CardControlRow, smoothRow: CardControlRow
    private let gammaRow: CardControlRow, exposureRow: CardControlRow, detailRow: CardControlRow
    private let shadowRow: CardControlRow, highlightRow: CardControlRow
    private let vibranceRow: CardControlRow, saturationRow: CardControlRow

    init(values: HdrLocalAdaptation, onChange: @escaping (HdrLocalAdaptation) -> Void) {
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        view = stack
        self.values = values
        self.onChange = onChange
        let percent: (Double) -> String = { String(format: "%+.0f%%", $0) }
        // The rows are stored (`load`), so their closures are made before `self` is complete and
        // reach it through a weak box filled once it is.
        let box = WeakPaneBox()
        radiusRow = cardSliderRow(label: "Radius:", min: 1, max: 500, value: values.radius,
                                  format: { String(format: "%.0f px", $0) }
        ) { value in box.pane?.set(\.radius, value) }
        strengthRow = cardSliderRow(label: "Strength:", min: 0.1, max: 4, value: values.strength,
                                    format: { String(format: "%.2f", $0) }
        ) { value in box.pane?.set(\.strength, value) }
        smoothRow = cardCheckboxRow(label: "", title: "Smooth Edges", on: values.smoothEdges
        ) { on in box.pane?.set(\.smoothEdges, on) }
        gammaRow = cardSliderRow(label: "Gamma:", min: 0.1, max: 2, value: values.gamma,
                                 format: { String(format: "%.2f", $0) }
        ) { value in box.pane?.set(\.gamma, value) }
        exposureRow = cardSliderRow(label: "Exposure:", min: -5, max: 5, value: values.exposure,
                                    format: { String(format: "%+.2f", $0) }
        ) { value in box.pane?.set(\.exposure, value) }
        detailRow = cardSliderRow(label: "Detail:", min: -100, max: 300, value: values.detail,
                                  format: percent
        ) { value in box.pane?.set(\.detail, value) }
        shadowRow = cardSliderRow(label: "Shadow:", min: -100, max: 100, value: values.shadow,
                                  format: percent
        ) { value in box.pane?.set(\.shadow, value) }
        highlightRow = cardSliderRow(label: "Highlight:", min: -100, max: 100, value: values.highlight,
                                     format: percent
        ) { value in box.pane?.set(\.highlight, value) }
        vibranceRow = cardSliderRow(label: "Vibrance:", min: -100, max: 100, value: values.vibrance,
                                    format: percent
        ) { value in box.pane?.set(\.vibrance, value) }
        saturationRow = cardSliderRow(label: "Saturation:", min: -100, max: 100,
                                      value: values.saturation, format: percent
        ) { value in box.pane?.set(\.saturation, value) }
        let rows: [NSView] = [
            Self.groupTitle("Edge Glow"), radiusRow, strengthRow, smoothRow,
            Self.groupTitle("Tone and Detail"), gammaRow, exposureRow, detailRow,
            Self.groupTitle("Advanced"), shadowRow, highlightRow, vibranceRow, saturationRow,
            Self.groupTitle("Toning Curve and Histogram"),
            curve,
        ]
        box.pane = self
        for row in rows { stack.addArrangedSubview(row) }
        curve.points = values.toningCurve
        curve.setAccessibilityIdentifier("hdr-toning-curve")
        curve.onPointsChanged = { [weak self] points in self?.set(\.toningCurve, points) }
    }

    /// Every row set to `values` WITHOUT calling back (a chosen preset): the stored values first,
    /// so the next edit starts from them.
    func load(_ values: HdrLocalAdaptation) {
        self.values = values
        radiusRow.setValue(values.radius)
        strengthRow.setValue(values.strength)
        smoothRow.setOn(values.smoothEdges)
        gammaRow.setValue(values.gamma)
        exposureRow.setValue(values.exposure)
        detailRow.setValue(values.detail)
        shadowRow.setValue(values.shadow)
        highlightRow.setValue(values.highlight)
        vibranceRow.setValue(values.vibrance)
        saturationRow.setValue(values.saturation)
        curve.points = values.toningCurve
    }

    private func set<Value>(_ key: WritableKeyPath<HdrLocalAdaptation, Value>, _ value: Value) {
        values[keyPath: key] = value
        onChange(values)
    }

    private static func groupTitle(_ text: String) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = DS.sans(12, weight: .semibold)
        label.textColor = DS.textMuted
        return label
    }
}

/// The pane's rows are built before `self` exists; their closures reach it through this box.
private final class WeakPaneBox {
    weak var pane: HdrToningLocalAdaptationPane?
}
