import AppKit

/// Layer Style > Inner Glow: a glow inside the shape, hugging the contour
/// (Source: Edge) or rising from the interior (Source: Center), with its own
/// blend mode (Screen by default), opacity and colour. Size is the glow's
/// extent in px; Choke is the fraction of it that stays hard (the core
/// dilates the outside — or erodes the shape, for Center — by size × choke
/// and blurs the rest). No light direction, so the context is unused. The
/// Fill rows (colour or gradient) and Jitter are `GlowFillRows`,
/// shared with the Outer Glow pane.
final class InnerGlowPane: NSObject, LayerStylePane {
    let view: NSView
    private let rows: [PaneRow]
    private let fill: GlowFillRows<InnerGlowEffect>

    init(binding: EffectBinding<InnerGlowEffect>, context: LayerStylePaneContext) {
        let blend = PaneRows.blendPopup(
            modes: context.blendModes,
            get: { binding.read().blend },
            set: { value in binding.update { $0.blend = value } })
        let opacity = PaneRows.percent(
            "Opacity:",
            get: { binding.read().opacity },
            set: { value in binding.update { $0.opacity = value } })
        let fill = GlowFillRows(binding: binding)
        self.fill = fill
        let source = PaneRows.popup(
            "Source:", titles: ["Edge", "Center"], values: InnerGlowEffect.sources,
            get: { binding.read().source },
            set: { value in binding.update { $0.source = value } })
        let choke = PaneRows.percent(
            "Choke:",
            get: { binding.read().choke },
            set: { value in binding.update { $0.choke = value } })
        let size = PaneRows.pixels(
            "Size:", max: 250,
            get: { binding.read().size },
            set: { value in binding.update { $0.size = value } })
        let contour = PaneRows.contour(
            get: { binding.read().contour },
            set: { value in binding.update { $0.contour = value } },
            antiAliased: { binding.read().antiAliased },
            setAntiAliased: { value in binding.update { $0.antiAliased = value } })
        let range = PaneRows.range(
            get: { binding.read().range },
            set: { value in binding.update { $0.range = value } })
        let noise = PaneRows.noise(
            get: { binding.read().noise },
            set: { value in binding.update { $0.noise = value } })
        let all: [PaneRow] = [blend, opacity, noise] + fill.rows
            + [source, choke, size, contour, range, fill.jitter]
        rows = all
        let grid = PaneRows.grid(all)
        let first = 3
        fill.attach(grid, rows: Array(first..<(first + fill.rows.count)))
        let stack = NSStackView(views: [grid])
        stack.orientation = .vertical
        stack.alignment = .leading
        view = stack
        super.init()
    }

    func reload() {
        rows.forEach { $0.reload() }
        fill.apply()
    }
}
