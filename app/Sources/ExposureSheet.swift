import AppKit

/// The Exposure dialog: exposure in stops, an offset, and a REVERSED
/// log-feel gamma slider.
///
/// Every parameter, range and default comes from `AdjustmentSchema`; the
/// lifecycle — the captured handles, the debounced in-context preview, one
/// undo step on Apply and nothing on Cancel — is `AdjustmentSheet`'s, in all
/// three modes (Image ▸ Adjustments, Layer ▸ New Adjustment Layer, and
/// Adjustment Options… on an existing layer).
///
/// The gamma slider is the one control in the app drawn against its own
/// axis — 9.99 at the LEFT, 0.01 at the right. Gamma is applied last as the
/// reciprocal power, so above 1 it brightens — the same direction as the
/// Levels dialog's midtone gamma. The slider runs 9.99 to 0.01, so moving
/// the handle right darkens: the handle's value rises as gamma falls, and a
/// smaller gamma is a larger exponent 1/gamma. The exponent is measured.
/// The caption and the footnote state the direction.
final class ExposureSheet: AdjustmentSheet {
    /// The control values, read from `initial` in `loadState()`: the
    /// layer's own values, a loaded preset's, or the schema's defaults.
    private var stops = 0.0
    private var offset = 0.0
    private var gamma = 1.0

    override func loadState() {
        stops = number("exposure").value
        offset = number("offset").value
        gamma = number("gamma").value
    }

    override func makeContent() -> NSView {
        let exposureRow = number("exposure")
        let offsetRow = number("offset")
        let gammaRow = number("gamma")
        let gammaRange = gammaRow.range

        let grid = Self.grid([
            // The precisions: stops to two places, the offset to
            // four (it moves the black point, where a ten-thousandth of the
            // range is still visible), gamma to two.
            sliderRow(
                "Exposure:", range: exposureRow.range, value: stops,
                format: { String(format: "%+.2f", $0) },
                onChange: { [weak self] value in self?.stops = Self.snapped(value, to: 100) }),
            sliderRow(
                "Offset:", range: offsetRow.range, value: offset,
                format: { String(format: "%+.4f", $0) },
                onChange: { [weak self] value in self?.offset = Self.snapped(value, to: 10000) }),
            sliderRow(
                "Gamma:", range: Self.sliderRange(forGamma: gammaRange),
                value: Self.sliderValue(forGamma: gamma),
                format: { String(format: "%.2f", Self.gamma(atSlider: $0, in: gammaRange)) },
                onChange: { [weak self] value in
                    self?.gamma = Self.gamma(atSlider: value, in: gammaRange)
                }),
        ])

        let caption = NSTextField(
            wrappingLabelWithString:
                "The stops scale the pixels in linear light and the offset lifts or drops "
                + "the black point; gamma is applied last as the reciprocal power, "
                + "so above 1 it brightens — the same direction as the Levels dialog's "
                + "midtone gamma. The slider runs 9.99 to 0.01, so moving the handle "
                + "right darkens. "
                + colorEvaluationCaption)
        caption.font = DS.sans(12)
        caption.textColor = DS.textMuted
        caption.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2

        let content = NSStackView(views: [grid, caption])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12
        return content
    }

    override func currentParams() -> [String: Any] {
        ["exposure": stops, "offset": offset, "gamma": gamma]
    }

    /// The convention worth stating beside the buttons; the caption above
    /// carries the rest (a 10 pt footnote shares its line with the two
    /// buttons and has room for about thirty characters).
    override var footnote: String { "gamma > 1 brightens · one undo" }

    // MARK: - The reversed, log-feel gamma axis

    /// What the gamma handle carries: −log10(gamma). The logarithm is the
    /// Levels dialog's log feel (a constant RATIO per handle step, so 0.5
    /// and 2 sit the same distance from 1), and the negation is what
    /// reverses the axis, so 9.99 sits at the left.
    /// Since gamma is the reciprocal power, the value rising as the
    /// handle moves right lowers gamma, raises the exponent 1/gamma and
    /// darkens the picture.
    private static func sliderValue(forGamma gamma: Double) -> Double {
        -log10(max(gamma, 0.0001))
    }

    /// The gamma a handle position names: snapped to the two decimals the
    /// readout shows and clamped into the schema's range, so the value
    /// written can never be one the core would refuse.
    private static func gamma(atSlider value: Double, in range: ClosedRange<Double>) -> Double {
        min(max(snapped(pow(10, -value), to: 100), range.lowerBound), range.upperBound)
    }

    /// `value` rounded to the resolution its readout shows. The meta must
    /// not carry precision the dialog cannot display: a re-opened layer
    /// would otherwise show numbers that are not quite the ones that made
    /// its pixels. Values the sheet only READS are left exactly as they
    /// are, so re-committing an untouched layer stays a no-op.
    private static func snapped(_ value: Double, to scale: Double) -> Double {
        (value * scale).rounded() / scale
    }

    /// The handle's own range, derived from the schema's gamma range with
    /// the ends swapped — the reversal is spelled exactly once, here.
    private static func sliderRange(forGamma range: ClosedRange<Double>) -> ClosedRange<Double> {
        sliderValue(forGamma: range.upperBound)...sliderValue(forGamma: range.lowerBound)
    }

    // MARK: - Schema reads

    /// The value a control opens on and the range it moves in, both from
    /// `AdjustmentSchema`'s row for `key` — so a slider's endpoints and the
    /// MCP validator's refusals are literally the same numbers, and a meta
    /// from a newer build (or a hand-written one) cannot put a handle off
    /// its own slider. The fallback is unreachable: the keys below are that
    /// table's own rows, and it exists only so the dialog still opens if the
    /// table is ever edited out from under it.
    private func number(_ key: String) -> (value: Double, range: ClosedRange<Double>) {
        guard let param = AdjustmentSchema.params(for: op).first(where: { $0.key == key }),
              case .number(let range, let fallback) = param.kind
        else { return (0, 0...1) }
        let raw = initial.number(key, default: fallback)
        return (min(max(raw, range.lowerBound), range.upperBound), range)
    }
}
