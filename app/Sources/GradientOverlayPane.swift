import AppKit

/// Layer Style > Gradient Overlay — every parameter of the effect in the
/// dialog's order: blend mode and opacity, the gradient's end colours and
/// Reverse, Style with Align with Layer, Angle, Scale. The two swatches
/// edit the first and last stop; a style with more stops (set over MCP or
/// pasted) keeps its inner stops untouched and says so in a note. Dither
/// follows Align with Layer, then Smoothness. The angle
/// is the gradient's own — a Gradient Overlay never reads the
/// global light, so there is no Use Global Light row here.
final class GradientOverlayPane: NSObject, LayerStylePane {
    let view: NSView
    private let rows: [PaneRow]
    private let grid: NSGridView
    /// The grid row of the more-than-two-stops note, collapsed while the
    /// swatches describe the whole gradient.
    private let stopsNoteRow: Int
    private let binding: EffectBinding<GradientOverlayEffect>

    init(binding: EffectBinding<GradientOverlayEffect>, context: LayerStylePaneContext) {
        self.binding = binding
        let blend = PaneRows.blendPopup(
            modes: context.blendModes,
            get: { binding.read().blend },
            set: { value in binding.update { $0.blend = value } })
        let opacity = PaneRows.percent(
            "Opacity:",
            get: { binding.read().opacity },
            set: { value in binding.update { $0.opacity = value } })
        // The stops are position-sorted by the core, so first/last are the
        // gradient's two ends; a stop list shorter than two (never produced
        // by the core, but a pasted payload is only checked on Apply) is
        // left alone rather than indexed.
        let start = PaneRows.color(
            "Start Color:",
            get: { binding.read().gradient.stops.first?.color ?? "#000000" },
            set: { value in
                binding.update { fx in
                    guard !fx.gradient.stops.isEmpty else { return }
                    fx.gradient.stops[0].color = value
                }
            })
        let end = PaneRows.color(
            "End Color:",
            get: { binding.read().gradient.stops.last?.color ?? "#ffffff" },
            set: { value in
                binding.update { fx in
                    guard let last = fx.gradient.stops.indices.last else { return }
                    fx.gradient.stops[last].color = value
                }
            })
        let stopsNote = PaneRows.note(
            "This gradient has more than two stops; the swatches edit its first and last, "
                + "the inner stops stay as set.")
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
        rows = [
            blend, opacity, start, end, stopsNote, reverse, style, align, dither, smoothness,
            angle, scale,
        ]
        stopsNoteRow = 4
        grid = PaneRows.grid(rows)
        let stack = NSStackView(views: [grid])
        stack.orientation = .vertical
        stack.alignment = .leading
        view = stack
        super.init()
        updateStopsNote()
    }

    func reload() {
        rows.forEach { $0.reload() }
        updateStopsNote()
    }

    private func updateStopsNote() {
        grid.row(at: stopsNoteRow).isHidden = binding.read().gradient.stops.count <= 2
    }
}
