import Foundation

/// Filter ▸ Sharpen ▸ Smart Sharpen's Remove choice: the blur the
/// sharpen undoes (`sampling_smart_sharpen.rs`). The raw value is the core's.
enum SmartSharpenRemove: String, CaseIterable {
    case gaussian, lens, motion

    var title: String {
        switch self {
        case .gaussian: return "Gaussian Blur"
        case .lens: return "Lens Blur"
        case .motion: return "Motion Blur"
        }
    }
}

/// One Smart Sharpen setting, in the core's units and with its defaults: the
/// sheet's values, an `apply_filter` / recorded step's `params`, and an export
/// request's Smart Sharpen output sharpening (`OutputSharpen`). Ranges
/// belong to the core, which refuses by name.
struct SmartSharpenSettings: Equatable {
    var amount = 200.0, radius = 1.0, noise = 10.0, remove = SmartSharpenRemove.lens, angle = 0.0
    var shadowFade = 0.0, shadowTonalWidth = 50.0, shadowRadius = 1
    var highlightFade = 0.0, highlightTonalWidth = 50.0, highlightRadius = 1

    /// `sampling_smart_sharpen::SMART_SHARPEN_KEYS`, in the same order.
    static let keys: [String] = [
        "amount", "radius", "noise", "remove", "angle",
        "shadow_fade", "shadow_tonal_width", "shadow_radius",
        "highlight_fade", "highlight_tonal_width", "highlight_radius",
    ]

    /// Every key, written explicitly, as a recorded step writes every argument.
    var params: [String: Any] {
        [
            "amount": ActionArgs.number(amount), "radius": ActionArgs.number(radius),
            "noise": ActionArgs.number(noise), "remove": remove.rawValue,
            "angle": ActionArgs.number(angle),
            "shadow_fade": ActionArgs.number(shadowFade),
            "shadow_tonal_width": ActionArgs.number(shadowTonalWidth),
            "shadow_radius": shadowRadius,
            "highlight_fade": ActionArgs.number(highlightFade),
            "highlight_tonal_width": ActionArgs.number(highlightTonalWidth),
            "highlight_radius": highlightRadius,
        ]
    }

    init() {}

    /// The setting `params` names, each absent key at its default. An unknown
    /// key, a non-numeric value, a fractional radius or an unknown removal
    /// throws; a number out of range is the core's to refuse.
    init(params: [String: Any]) throws {
        if let key = params.keys.sorted().first(where: { !Self.keys.contains($0) }) {
            throw DoctoredCoreError(message: "Smart Sharpen has no setting \(key).")
        }
        func number(_ key: String, _ fallback: Double) throws -> Double {
            guard let value = params[key] else { return fallback }
            guard let number = ActionJSON.number(value)?.doubleValue, number.isFinite else {
                throw DoctoredCoreError(message: "Smart Sharpen needs a numeric \(key).")
            }
            return number
        }
        func integer(_ key: String, _ fallback: Int) throws -> Int {
            let value = try number(key, Double(fallback))
            guard value == value.rounded(), abs(value) <= Double(Int32.max) else {
                throw DoctoredCoreError(message: "Smart Sharpen needs a whole-pixel \(key).")
            }
            return Int(value)
        }
        amount = try number("amount", amount)
        radius = try number("radius", radius)
        noise = try number("noise", noise)
        if let value = params["remove"] {
            guard let name = value as? String, let choice = SmartSharpenRemove(rawValue: name) else {
                throw DoctoredCoreError(message: "Smart Sharpen's remove must be gaussian, lens or motion.")
            }
            remove = choice
        }
        angle = try number("angle", angle)
        shadowFade = try number("shadow_fade", shadowFade)
        shadowTonalWidth = try number("shadow_tonal_width", shadowTonalWidth)
        shadowRadius = try integer("shadow_radius", shadowRadius)
        highlightFade = try number("highlight_fade", highlightFade)
        highlightTonalWidth = try number("highlight_tonal_width", highlightTonalWidth)
        highlightRadius = try integer("highlight_radius", highlightRadius)
    }
}
