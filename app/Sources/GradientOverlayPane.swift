import AppKit

/// Layer Style > Gradient Overlay — every parameter of the effect in the
/// dialog's order: blend mode and opacity, the gradient's stops in the shared
/// stop editor (`PaneGradientStopsRow`: every stop, its midpoints and a
/// library preset, whose Smoothness lands in the Smoothness row) and Reverse,
/// Style with Align with Layer, Dither, Smoothness, Angle, Scale. The angle
/// is the gradient's own — a Gradient Overlay never reads the
/// global light, so there is no Use Global Light row here.
final class GradientOverlayPane: NSObject, LayerStylePane {
    let view: NSView
    private let rows: [PaneRow]

    init(binding: EffectBinding<GradientOverlayEffect>, context: LayerStylePaneContext) {
        let blend = PaneRows.blendPopup(
            modes: context.blendModes,
            get: { binding.read().blend },
            set: { value in binding.update { $0.blend = value } })
        let opacity = PaneRows.percent(
            "Opacity:",
            get: { binding.read().opacity },
            set: { value in binding.update { $0.opacity = value } })
        let reverse = PaneRows.checkbox(
            "Reverse",
            get: { binding.read().gradient.reverse },
            set: { on in binding.update { $0.gradient.reverse = on } })
        let style = PaneRows.popup(
            "Style:",
            titles: ["Linear", "Radial", "Angle", "Reflected", "Diamond"],
            values: GradientFill.styles,
            get: { binding.read().gradient.style },
            set: { value in binding.update { $0.gradient.style = value } })
        let align = PaneRows.checkbox(
            "Align with Layer",
            get: { binding.read().gradient.alignWithLayer },
            set: { on in binding.update { $0.gradient.alignWithLayer = on } })
        let dither = PaneRows.checkbox(
            "Dither",
            get: { binding.read().gradient.dither ?? false },
            set: { on in binding.update { $0.gradient.dither = on ? true : nil } })
        // Smoothness: 0…100 %, stored as a fraction and
        // nil at 100 % (no key).
        let smoothness = PaneRows.slider(
            "Smoothness:", min: 0, max: 100, integer: true,
            format: { String(format: "%.0f %%", $0) },
            get: { (binding.read().gradient.smoothness ?? 1) * 100 },
            set: { value in
                binding.update { $0.gradient.smoothness = value >= 100 ? nil : value / 100 }
            })
        let stops = PaneRows.gradientStops(
            get: { binding.read().gradient },
            setStops: { value in binding.update { $0.gradient.stops = value } },
            smoothness: (
                get: { binding.read().gradient.smoothness ?? 1 },
                set: { value in
                    binding.update { $0.gradient.smoothness = value >= 1 ? nil : value }
                    smoothness.reload()
                }))
        let angle = PaneRows.angle(
            "Angle:",
            get: { binding.read().gradient.angle },
            set: { value in binding.update { $0.gradient.angle = value } })
        // Scale is stored as a 0.1…1.5 factor and shown as 10…150 %
        // (whole percents, like the opacity rows).
        let scale = PaneRows.slider(
            "Scale:", min: 10, max: 150, integer: true,
            format: { String(format: "%.0f %%", $0) },
            get: { binding.read().gradient.scale * 100 },
            set: { value in binding.update { $0.gradient.scale = value / 100 } })
        rows = [blend, opacity, stops, reverse, style, align, dither, smoothness, angle, scale]
        let stack = NSStackView(views: [PaneRows.grid(rows)])
        stack.orientation = .vertical
        stack.alignment = .leading
        view = stack
        super.init()
    }

    func reload() {
        rows.forEach { $0.reload() }
    }
}
