import AppKit

/// What the Outer and Inner Glow share for their Fill: the colour
/// swatch, or the gradient button's end stops, and Jitter.
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
/// (colour or gradient), the colour, the gradient's first and last stop
/// colours and opacities, the note shown for a gradient with more than two
/// stops, and Jitter. The rows the current fill does not use are hidden
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
        // The sheet edits the two end stops; a gradient with more stops (set
        // over MCP or pasted) keeps its middle stops.
        let startColor = PaneRows.color(
            "Start Color:",
            get: { glowStops(binding.read()).first?.color ?? "#000000" },
            set: { value in binding.update { glowEditStop(&$0, last: false) { $0.color = value } } })
        let endColor = PaneRows.color(
            "End Color:",
            get: { glowStops(binding.read()).last?.color ?? "#000000" },
            set: { value in binding.update { glowEditStop(&$0, last: true) { $0.color = value } } })
        let startOpacity = PaneRows.percent(
            "Start Opacity:",
            get: { glowStops(binding.read()).first?.opacity ?? 1 },
            set: { value in
                binding.update { glowEditStop(&$0, last: false) { $0.opacity = value } }
            })
        let endOpacity = PaneRows.percent(
            "End Opacity:",
            get: { glowStops(binding.read()).last?.opacity ?? 0 },
            set: { value in
                binding.update { glowEditStop(&$0, last: true) { $0.opacity = value } }
            })
        let stopsNote = PaneRows.note(
            "Only the first and last stops are edited here; the gradient's middle stops keep "
                + "their colours and positions.")
        rows = [fill, color, startColor, endColor, startOpacity, endOpacity, stopsNote]
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
            toggle.gradientRows = Array(rows[2...5])
            toggle.stopsNoteRow = rows[6]
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

/// Edits the first (or last) stop, seeding the gradient first when the glow
/// has none or fewer than two stops.
private func glowEditStop<E: GlowFillEffect>(
    _ fx: inout E, last: Bool, _ edit: (inout GradientStop) -> Void
) {
    var fill = fx.gradient ?? glowSeedGradient(fx.color)
    if fill.stops.count < 2 { fill.stops = glowSeedGradient(fx.color).stops }
    let index = last ? fill.stops.count - 1 : 0
    edit(&fill.stops[index])
    fx.gradient = fill
}

/// Shows the grid rows of the current fill and hides the others'; the stops
/// note only appears for a gradient with more than two stops.
private final class GlowFillSwitch {
    weak var grid: NSGridView?
    var colorRows: [Int] = []
    var gradientRows: [Int] = []
    var stopsNoteRow: Int?

    func apply<E: GlowFillEffect>(_ fx: E) {
        guard let grid = grid else { return }
        let gradient = fx.fillType == "gradient"
        for index in colorRows where index < grid.numberOfRows {
            grid.row(at: index).isHidden = gradient
        }
        for index in gradientRows where index < grid.numberOfRows {
            grid.row(at: index).isHidden = !gradient
        }
        if let index = stopsNoteRow, index < grid.numberOfRows {
            grid.row(at: index).isHidden = !gradient || (fx.gradient?.stops.count ?? 2) <= 2
        }
    }
}
