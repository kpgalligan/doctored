import AppKit

/// Layer Style > Outer Glow: a halo around the shape, outside only. Every
/// control is a `PaneRows` row bound straight to the effect (the
/// `DropShadowPane` pattern). A glow has no direction, so there is no Angle
/// or Use Global Light row and the context goes unread; Spread is the
/// fraction of Size that is hard growth (the rest is blur), as in the
/// Drop Shadow pane. The Fill rows (colour or gradient) and Jitter are
/// `GlowFillRows`, shared with the Inner Glow pane.
final class OuterGlowPane: NSObject, LayerStylePane {
    let view: NSView
    private let rows: [PaneRow]
    private let fill: GlowFillRows<OuterGlowEffect>

    init(binding: EffectBinding<OuterGlowEffect>, context: LayerStylePaneContext) {
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
        let spread = PaneRows.percent(
            "Spread:",
            get: { binding.read().spread },
            set: { value in binding.update { $0.spread = value } })
        let size = PaneRows.pixels(
            "Size:", max: 250,
            get: { binding.read().size },
            set: { value in binding.update { $0.size = value } })
        // The default Screen mode lightens what lies beneath, so over a
        // white backdrop the halo composites to white — worth saying once,
        // since "nothing happened" is the first thing a user sees there.
        let note = PaneRows.note(
            "The glow is drawn outside the shape only. Screen (the default) lightens the "
                + "backdrop, so it is invisible over white — pick Normal or a darker mode there.")
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
            + [spread, size, contour, range, fill.jitter, note]
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
