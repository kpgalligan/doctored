import AppKit

/// The Vibrance dialog: the chroma-weighted Vibrance slider and the flat
/// Saturation slider that rides with it in the same dialog. Both are drawn
/// as ±100 and stored as the core's unit floats — the phase's
/// numeric convention, where a percent-facing slider divides by 100 on
/// write and the readout shows what the parameter holds.
///
/// Every parameter, range and default comes from `AdjustmentSchema`; the
/// lifecycle — the captured handles, the debounced in-context preview, one
/// undo step on Apply and nothing on Cancel — is `AdjustmentSheet`'s, in all
/// three modes (Image ▸ Adjustments, Layer ▸ New Adjustment Layer, and
/// Adjustment Options… on an existing layer).
final class VibranceSheet: AdjustmentSheet {
    /// The chroma-weighted gain, [-1, 1] as the meta stores it.
    private var vibrance: Double = 0
    /// The flat Saturation slider. It is NOT `hue_saturation`'s master
    /// saturation: it scales each pixel's distance
    /// from a ProPhoto-luminance anchor in the document's linear light (#2,
    /// `adjust_color`), so the two dialogs differ at the same number.
    private var saturation: Double = 0

    override func loadState() {
        vibrance = clamped(initial.number("vibrance", default: 0), -1...1)
        saturation = clamped(initial.number("saturation", default: 0), -1...1)
    }

    override func makeContent() -> NSView {
        AdjustmentSheet.grid([
            sliderRow(
                "Vibrance:", range: -100...100, value: vibrance * 100, format: percentText
            ) { [weak self] value in self?.vibrance = value.rounded() / 100 },
            sliderRow(
                "Saturation:", range: -100...100, value: saturation * 100, format: percentText
            ) { [weak self] value in self?.saturation = value.rounded() / 100 },
        ])
    }

    override func currentParams() -> [String: Any] {
        ["vibrance": vibrance, "saturation": saturation]
    }

    /// The two facts the sliders cannot show. A positive Vibrance gain is
    /// held back on reds and skin — a face keeps its colour while the shirt
    /// behind it lifts — and Saturation works on the document's linear
    /// values (#2, `adjust_color`).
    override var footnote: String { "reds and skin held back · saturation in linear light" }
}

/// The ±100 readout for a slider whose stored value is that over
/// 100. Whole numbers only, because what the row writes is
/// `rounded() / 100`: the readout and the stored parameter are then never
/// two different numbers.
private let percentText: (Double) -> String = { String(format: "%.0f%%", $0) }

/// Prefill defence. A meta may hold anything a host wrote, and a slider
/// silently clamps its own position while this dialog's stored state would
/// not — so both are built from the one clamped value here.
private func clamped(_ value: Double, _ range: ClosedRange<Double>) -> Double {
    min(max(value, range.lowerBound), range.upperBound)
}
