import AppKit

/// What the Outer and Inner Glow share for their Fill: the colour
/// swatch, or the gradient's stops, and Jitter.
protocol GlowFillEffect {
    var color: String { get set }
    var linear: [Double]? { get set }
    var fillType: String? { get set }
    var gradient: GradientFill? { get set }
    var jitter: Double? { get set }
}

extension OuterGlowEffect: GlowFillEffect {}
extension InnerGlowEffect: GlowFillEffect {}

/// Layer Style > Outer Glow / Inner Glow's fill rows: the Fill popup
/// (colour or gradient), the colour, the gradient's stops in the shared stop
/// editor (`PaneGradientStopsRow`: every stop, a library preset), and Jitter.
/// The editor shows no midpoints and a preset brings no Smoothness: a glow's
/// gradient keeps its stops alone, the core resetting every other key and
/// every stop's midpoints. The rows the current fill does not use are hidden
/// (the `StrokePane` pattern). Jitter only scatters a gradient but stays
/// visible in both fills, so a colour glow's stored Jitter can be seen and
/// cleared. Choosing Gradient on a glow that has none seeds Foreground to
/// Transparent: the glow's colour at 100 % to the same colour at 0 %, which
/// draws the colour glow.
final class GlowFillRows<E: GlowFillEffect> {
    let rows: [PaneRow]
    let jitter: PaneRow
    private let binding: EffectBinding<E>
    private let toggle: GlowFillSwitch

    init(binding: EffectBinding<E>) {
        self.binding = binding
        // The switch exists before the rows so the Fill popup's closure can
        // reach it; it learns the grid in `attach`.
        let toggle = GlowFillSwitch()
        self.toggle = toggle
        let fill = PaneRows.popup(
            "Fill:", titles: ["Color", "Gradient"], values: OuterGlowEffect.fillTypes,
            get: { binding.read().fillType ?? "color" },
            set: { value in
                binding.update { fx in
                    if value == "gradient" {
                        fx.fillType = value
                        if fx.gradient == nil { fx.gradient = glowSeedGradient(fx.color) }
                    } else {
                        fx.fillType = nil
                    }
                }
                toggle.apply(binding.read())
                // A gradient just seeded from the colour shows its stops.
                toggle.stops?.reload()
            })
        let color = PaneRows.slotColor(
            "Color:",
            get: {
                let fx = binding.read()
                return StyleSlotColor(hex: fx.color, linear: fx.linear)
            },
            set: { value in
                binding.update {
                    $0.color = value.hex
                    $0.linear = value.linear
                }
            })
        let stops = PaneRows.gradientStops(
            get: {
                var shown = GradientFill()
                shown.stops = glowStops(binding.read())
                return shown
            },
            setStops: { value in
                binding.update { fx in
                    var gradient = fx.gradient ?? glowSeedGradient(fx.color)
                    gradient.stops = value
                    fx.gradient = gradient
                }
            },
            showsMidpoints: false)
        toggle.stops = stops
        rows = [fill, color, stops]
        jitter = PaneRows.noise(
            "Jitter:",
            get: { binding.read().jitter },
            set: { value in binding.update { $0.jitter = value } })
    }

    /// The grid holding these rows: `rows` are the grid indices of `self.rows`
    /// in order (`jitter` is never hidden).
    func attach(_ grid: NSGridView, rows: [Int]) {
        toggle.grid = grid
        if rows.count == self.rows.count {
            toggle.colorRows = [rows[1]]
            toggle.gradientRows = [rows[2]]
        }
        apply()
    }

    /// Shows the rows of the current fill (after a reload).
    func apply() {
        toggle.apply(binding.read())
    }
}

/// The glow's stops as the sheet shows them: its gradient's, or the seed's
/// when it has none (or fewer than two, which only a hand-built value has).
private func glowStops<E: GlowFillEffect>(_ fx: E) -> [GradientStop] {
    if let stops = fx.gradient?.stops, stops.count >= 2 { return stops }
    return glowSeedGradient(fx.color).stops
}

/// Foreground to Transparent in the glow's colour.
private func glowSeedGradient(_ color: String) -> GradientFill {
    var fill = GradientFill()
    fill.stops = [
        GradientStop(position: 0, color: color, opacity: 1),
        GradientStop(position: 1, color: color, opacity: 0),
    ]
    return fill
}

/// Shows the grid rows of the current fill and hides the others'.
private final class GlowFillSwitch {
    weak var grid: NSGridView?
    var colorRows: [Int] = []
    var gradientRows: [Int] = []
    /// The stops row, reloaded when Fill: Gradient seeds a gradient.
    weak var stops: PaneGradientStopsRow?

    func apply<E: GlowFillEffect>(_ fx: E) {
        guard let grid = grid else { return }
        let gradient = fx.fillType == "gradient"
        for index in colorRows where index < grid.numberOfRows {
            grid.row(at: index).isHidden = gradient
        }
        for index in gradientRows where index < grid.numberOfRows {
            grid.row(at: index).isHidden = !gradient
        }
    }
}
