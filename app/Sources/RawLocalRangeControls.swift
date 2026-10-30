import AppKit

/// The range rows of the Local Masks sidebar: a luminance band and a sampled colour, each with an
/// on/off tick. `RawLocalControls` hosts the view, puts `rangeJSON` in every update it sends and
/// forwards the Sample Colour click here.
///
/// A sampled colour is a source point until the development resolves it into a Lab colour
/// (`RawLocalEvaluation`). The point stays in every update these rows emit until the state comes
/// back holding the colour, so a slider moved in between does not drop the sample.
final class RawLocalRangeControls {
    let view = NSStackView()
    var onChange: (() -> Void)?
    /// Colour was ticked with no colour to use yet: the host selects the Sample Colour tool.
    var onSampleTool: (() -> Void)?
    /// The readout of a pending sample point (nil: "Sampling…", Develop's, which resolves the
    /// point at its next development). The Develop Filter's masks name the point instead: the
    /// core resolves it only when the filter runs.
    var pendingReadout: (([Double]) -> String)?
    /// Tick, Low, High, Low falloff, High falloff, Invert.
    private var luminanceRows: [CardControlRow] = []
    /// Tick, Amount, Falloff, Invert.
    private var colorRows: [CardControlRow] = []
    private let readoutLabel = fieldLabel("Sampled Lab:")
    private let readout = NSTextField(labelWithString: "")
    private var lab: [Double]?
    private var pendingSample: [Double]?
    /// Colour is ticked and waiting for its first sample; a state without a colour must not
    /// untick it meanwhile.
    private var awaitingSample = false
    private var enabled = false

    /// Starting values of a component the local does not have yet.
    private static let amount = 10.0, falloff = 10.0

    init() {
        view.orientation = .vertical; view.alignment = .leading; view.spacing = 10
        luminanceRows.append(tick("Luminance:", "Limit to a luminance range", "luminance"))
        luminanceRows.append(slider("Low:", "low", 0...1, 0, "%.2f") { [weak self] in self?.orderBounds(movedLow: true) })
        luminanceRows.append(slider("High:", "high", 0...1, 1, "%.2f") { [weak self] in self?.orderBounds(movedLow: false) })
        luminanceRows.append(slider("Low falloff:", "low-falloff", 0...1, 0, "%.2f"))
        luminanceRows.append(slider("High falloff:", "high-falloff", 0...1, 0, "%.2f"))
        luminanceRows.append(tick("Invert:", "Invert the luminance range", "luminance-invert"))
        colorRows.append(tick("Colour:", "Limit to a sampled colour", "color") { [weak self] in self?.colorTicked() })
        readoutLabel.alignment = .right
        readoutLabel.widthAnchor.constraint(equalToConstant: 106).isActive = true
        readout.font = DS.mono(12)
        readout.setAccessibilityIdentifier("raw-local-range-lab")
        let sampled = NSStackView(views: [readoutLabel, readout])
        sampled.orientation = .horizontal; sampled.alignment = .centerY; sampled.spacing = 12
        view.addArrangedSubview(sampled)
        colorRows.append(slider("Amount ΔE:", "amount", 0...200, Self.amount, "%.1f"))
        colorRows.append(slider("Falloff ΔE:", "color-falloff", 0...200, Self.falloff, "%.1f"))
        colorRows.append(tick("Invert:", "Invert the colour range", "color-invert"))
        refresh()
    }

    /// Shows one local's range (nil: none). Silent: no change is reported.
    func fill(_ range: RawLocalRange?) {
        lab = range?.color?.lab
        pendingSample = range?.color?.sample
        let luminance = range?.luminance
        luminanceRows[0].setOn(luminance != nil)
        luminanceRows[1].setValue(luminance?.low ?? 0)
        luminanceRows[2].setValue(luminance?.high ?? 1)
        luminanceRows[3].setValue(luminance?.lowFalloff ?? 0)
        luminanceRows[4].setValue(luminance?.highFalloff ?? 0)
        luminanceRows[5].setOn(luminance?.invert ?? false)
        let color = range?.color
        if color != nil { awaitingSample = false }
        colorRows[0].setOn(color != nil || awaitingSample)
        // While Colour waits for its first sample the state has no colour yet, so its Amount,
        // Falloff and Invert are the user's own and a frame arriving meanwhile must not reset them.
        if !awaitingSample {
            colorRows[1].setValue(color?.amount ?? Self.amount)
            colorRows[2].setValue(color?.falloff ?? Self.falloff)
            colorRows[3].setOn(color?.invert ?? false)
        }
        refresh()
    }

    func setEnabled(_ value: Bool) {
        enabled = value
        refresh()
    }

    /// Another local was selected: a tick waiting for its first sample belonged to the last one.
    func selectionChanged() { awaitingSample = false }

    /// The Sample Colour click, in oriented source pixels: turns the colour range on at that
    /// point and reports the change.
    func sampled(at point: CGPoint) {
        guard enabled else { return }
        pendingSample = [Double(point.x), Double(point.y)]
        lab = nil; awaitingSample = false
        colorRows[0].setOn(true)
        refresh()
        onChange?()
    }

    /// The update's `range` value: the range object, or NSNull when neither component is on.
    var rangeJSON: Any {
        var range = RawLocalRange()
        if isOn(luminanceRows[0]) {
            range.luminance = RawLuminanceRange(low: value(luminanceRows[1]), high: value(luminanceRows[2]),
                lowFalloff: value(luminanceRows[3]), highFalloff: value(luminanceRows[4]),
                invert: isOn(luminanceRows[5]))
        }
        if isOn(colorRows[0]), lab != nil || pendingSample != nil {
            range.color = RawColorRange(lab: pendingSample == nil ? lab : nil, sample: pendingSample,
                amount: value(colorRows[1]), falloff: value(colorRows[2]), invert: isOn(colorRows[3]))
        }
        if range.luminance == nil, range.color == nil { return NSNull() }
        return range.json
    }

    private func isOn(_ row: CardControlRow) -> Bool { row.checkbox?.state == .on }
    private func value(_ row: CardControlRow) -> Double { row.slider?.doubleValue ?? 0 }

    private func changed() {
        refresh()
        onChange?()
    }
    /// A colour range with no colour yet has nothing to send: ask for the sample instead.
    private func colorTicked() {
        awaitingSample = isOn(colorRows[0]) && lab == nil && pendingSample == nil
        refresh()
        if awaitingSample { onSampleTool?() } else { onChange?() }
    }

    /// Keeps Low at or below High by moving the other slider along.
    private func orderBounds(movedLow: Bool) {
        let low = value(luminanceRows[1]), high = value(luminanceRows[2])
        guard low > high else { return }
        if movedLow { luminanceRows[2].setValue(low) } else { luminanceRows[1].setValue(high) }
    }

    private func refresh() {
        luminanceRows[0].isEnabled = enabled
        for row in luminanceRows.dropFirst() { row.isEnabled = enabled && isOn(luminanceRows[0]) }
        colorRows[0].isEnabled = enabled
        let color = enabled && isOn(colorRows[0])
        for row in colorRows.dropFirst() { row.isEnabled = color }
        readoutLabel.textColor = color ? DS.textStrong : DS.textFaint
        readout.textColor = color ? DS.textMuted : DS.textFaint
        if let pendingSample {
            readout.stringValue = pendingReadout?(pendingSample) ?? "Sampling…"
        } else if let lab, lab.count == 3 {
            readout.stringValue = String(format: "L %.1f  a %.1f  b %.1f", lab[0], lab[1], lab[2])
        } else {
            readout.stringValue = "Click the image with Sample Colour"
        }
    }

    private func tick(_ label: String, _ title: String, _ id: String,
                      _ action: (() -> Void)? = nil) -> CardControlRow {
        let row = cardCheckboxRow(label: label, title: title, on: false) { [weak self] _ in
            if let action { action() } else { self?.changed() }
        }
        row.setAccessibilityIdentifier("raw-local-range-" + id)
        view.addArrangedSubview(row)
        return row
    }
    private func slider(_ label: String, _ id: String, _ range: ClosedRange<Double>, _ value: Double,
                        _ format: String, _ before: (() -> Void)? = nil) -> CardControlRow {
        let row = cardSliderRow(label: label, min: range.lowerBound, max: range.upperBound, value: value,
            format: { String(format: format, $0) }) { [weak self] _ in before?(); self?.changed() }
        row.setAccessibilityIdentifier("raw-local-range-" + id)
        view.addArrangedSubview(row)
        return row
    }
}
