import AppKit

/// Image ▸ Adjustments ▸ Brightness/Contrast/Saturation… and its adjustment
/// layer: three −1…1 sliders and Use Legacy.
///
/// Off, the MODERN curve (`legacy: false`): brightness is a power and
/// contrast an S-curve about mid-grey, so neither moves black or white and
/// neither clips. On, the LEGACY offset and straight line the app has always
/// shipped, whose numbers are frozen. An older layer has no
/// `legacy` key and IS legacy, so Adjustment Options… on one opens with the
/// box ticked; a new dialog opens modern, and so does the Preset popup's
/// Default in every mode.
///
/// The destructive legacy apply keeps its own frozen export
/// (`dr_image_adjust`, recorded as `apply_filter "adjust"`); the modern one
/// runs the shared adjustment twin. Both record the flat `adjust` step with
/// `legacy` spelled out.
///
/// On a CMYK document (#1) Saturation is disabled and held at 0: the op runs
/// per plate on `1 − ink`, where a saturation has no meaning
/// (Hue/Saturation is the way).
final class BrightnessContrastSheet: AdjustmentSheet {
    private var brightness = 0.0
    private var contrast = 0.0
    private var saturation = 0.0
    private var legacy = false
    /// False until the first `loadState()`, the one that reads the payload
    /// the sheet opened on.
    private var didLoadOpeningState = false

    override func loadState() {
        let opening = !didLoadOpeningState
        didLoadOpeningState = true
        brightness = clamped(initial.number("brightness", default: 0))
        contrast = clamped(initial.number("contrast", default: 0))
        saturation = clamped(initial.number("saturation", default: 0))
        if initial.params["legacy"] != nil {
            legacy = initial.bool("legacy", default: true)
        } else if opening, case .edit = mode {
            // Only the layer the sheet opened on can be an older one. A
            // later load with no key is the Preset popup's Default (a saved
            // preset always carries the key), and Default is modern.
            legacy = true
        } else {
            legacy = false
        }
    }

    /// A CMYK document's sheet: Saturation disabled and 0.
    private var cmyk: Bool { document.doc?.isCmyk == true }

    override func makeContent() -> NSView {
        if cmyk { saturation = 0 }
        let saturationRow = sliderRow(
            "Saturation:", range: -1...1, value: saturation, format: twoPlaces
        ) { [weak self] value in self?.saturation = snapped(value) }
        for case let slider as NSSlider in saturationRow { slider.isEnabled = !cmyk }
        return AdjustmentSheet.grid([
            sliderRow(
                "Brightness:", range: -1...1, value: brightness, format: twoPlaces
            ) { [weak self] value in self?.brightness = snapped(value) },
            sliderRow(
                "Contrast:", range: -1...1, value: contrast, format: twoPlaces
            ) { [weak self] value in self?.contrast = snapped(value) },
            saturationRow,
            checkboxRow("Use Legacy", value: legacy) { [weak self] on in self?.legacy = on },
        ])
    }

    override func currentParams() -> [String: Any] {
        ["brightness": brightness, "contrast": contrast, "saturation": cmyk ? 0 : saturation,
         "legacy": legacy]
    }

    override func destructiveOperation(
        capture: OperationCapture, params: [String: Any]
    ) -> (RasterImage) -> RasterImage? {
        let values = Self.values(params)
        guard values.legacy else { return super.destructiveOperation(capture: capture, params: params) }
        return {
            $0.adjusted(
                brightness: values.brightness, contrast: values.contrast,
                saturation: values.saturation)
        }
    }

    override func destructiveRecord(params: [String: Any], target: String) -> [ActionStep] {
        let values = Self.values(params)
        return .filter(
            "adjust",
            [
                "brightness": ActionArgs.number(values.brightness),
                "contrast": ActionArgs.number(values.contrast),
                "saturation": ActionArgs.number(values.saturation),
                "legacy": values.legacy,
            ],
            target: target)
    }

    private static func values(_ params: [String: Any])
        -> (brightness: Double, contrast: Double, saturation: Double, legacy: Bool)
    {
        let payload = AdjustmentLayerPayload(op: .bcs, params: params)
        return (payload.number("brightness", default: 0), payload.number("contrast", default: 0),
                payload.number("saturation", default: 0), payload.bool("legacy", default: true))
    }
}

/// The readout the old dialog showed, and the value the row stores.
private let twoPlaces: (Double) -> String = { String(format: "%.2f", $0) }

/// A slider value rounded to the readout's two places, so the stored
/// parameter and the number on screen are the same.
private func snapped(_ value: Double) -> Double {
    (value * 100).rounded() / 100
}

/// Prefill defence: a meta may hold anything a host wrote.
private func clamped(_ value: Double) -> Double {
    min(max(value, -1), 1)
}
