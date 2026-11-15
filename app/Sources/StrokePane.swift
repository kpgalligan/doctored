import AppKit

/// Layer Style > Stroke: Size, Position, Blend Mode, Opacity and the fill
/// — a colour swatch, (Fill Type: Gradient) every stop of the gradient in
/// the shared stop editor (`PaneGradientStopsRow`), Reverse, Style (Shape
/// Burst included), Align with Layer, Dither, Smoothness, Angle and Scale,
/// or (Fill Type: Pattern) the Pattern
/// Overlay's own rows (`PatternFillRows`: Pattern, Angle, Scale, Link with
/// Layer, Snap to Origin) over the stroke's `patternFill`, in the pane's
/// order. Every control is a `PaneRows` row bound straight to the effect
/// (the sheet re-previews on each write). The rows the current fill type
/// does not use are hidden rather than disabled, so the pane reads as one
/// fill at a time.
final class StrokePane: NSObject, LayerStylePane {
    let view: NSView
    private let binding: EffectBinding<StrokeEffect>
    private let rows: [PaneRow]
    private let fillRows: FillRowsSwitch

    /// `layerOrigin` and `layerOffset` are the Pattern Overlay pane's (`PatternFillRows`): the
    /// layer content's top left and the layer's offset, for Snap to Origin.
    init(binding: EffectBinding<StrokeEffect>, context: LayerStylePaneContext,
         layerOrigin: @escaping () -> CGPoint?,
         layerOffset: @escaping () -> (x: Int, y: Int)?) {
        self.binding = binding
        // The switch exists before the rows so the Fill Type popup's closure
        // can reach it; it learns the grid once the grid is built below.
        let fillRows = FillRowsSwitch()
        self.fillRows = fillRows

        let size = PaneRows.pixels(
            "Size:", max: 250,
            get: { binding.read().size },
            set: { value in binding.update { $0.size = value } })
        let position = PaneRows.popup(
            "Position:", titles: ["Outside", "Inside", "Center"],
            values: StrokeEffect.positions,
            get: { binding.read().position },
            set: { value in binding.update { $0.position = value } })
        let blend = PaneRows.blendPopup(
            modes: context.blendModes,
            get: { binding.read().blend },
            set: { value in binding.update { $0.blend = value } })
        let opacity = PaneRows.percent(
            "Opacity:",
            get: { binding.read().opacity },
            set: { value in binding.update { $0.opacity = value } })
        let fillType = PaneRows.popup(
            "Fill Type:", titles: ["Color", "Gradient", "Pattern"], values: StrokeEffect.fillTypes,
            get: { binding.read().fillType },
            set: { value in
                binding.update {
                    $0.fillType = value
                    // A Pattern fill is written out whole, the defaults until edited.
                    if value == "pattern", $0.patternFill == nil { $0.patternFill = PatternFill() }
                }
                fillRows.apply(binding.read())
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

        // Gradient fill.
        let reverse = PaneRows.checkbox(
            "Reverse",
            get: { binding.read().gradient.reverse },
            set: { on in binding.update { $0.gradient.reverse = on } })
        let style = PaneRows.popup(
            "Style:",
            titles: ["Linear", "Radial", "Angle", "Reflected", "Diamond", "Shape Burst"],
            values: GradientFill.strokeStyles,
            get: { binding.read().gradient.style },
            set: { value in binding.update { $0.gradient.style = value } })
        let alignWithLayer = PaneRows.checkbox(
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
        // Every stop, in the shared stop editor; a preset's Smoothness lands
        // in the Smoothness row.
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
        // The schema's 0.1…1.5 scale as a 10…150 % readout.
        let scale = PaneRows.slider(
            "Scale:", min: 10, max: 150, integer: true,
            format: { String(format: "%.0f %%", $0) },
            get: { binding.read().gradient.scale * 100 },
            set: { value in binding.update { $0.gradient.scale = value / 100 } })

        // Pattern fill: the overlay's rows over the stroke's `patternFill`.
        let pattern = PatternFillRows.make(
            binding: EffectBinding<PatternFill>(
                get: { binding.read().patternFill ?? PatternFill() },
                set: { value in binding.update { $0.patternFill = value } }),
            layerOrigin: layerOrigin, layerOffset: layerOffset)

        let common: [PaneRow] = [size, position, blend, opacity, fillType]
        let gradient: [PaneRow] = [
            stops, reverse, style, alignWithLayer, dither, smoothness, angle, scale,
        ]
        let solid: [PaneRow] = [color]
        let all = common + solid + gradient + pattern
        rows = all
        let grid = PaneRows.grid(all)
        // The grid indices of each fill type's rows, from the row array.
        let index = { (row: PaneRow) in all.firstIndex { $0 === row } }
        fillRows.grid = grid
        fillRows.colorRows = solid.compactMap(index)
        fillRows.gradientRows = gradient.compactMap(index)
        fillRows.patternRows = pattern.compactMap(index)
        fillRows.apply(binding.read())

        let stack = NSStackView(views: [grid])
        stack.orientation = .vertical
        stack.alignment = .leading
        view = stack
        super.init()
    }

    func reload() {
        rows.forEach { $0.reload() }
        fillRows.apply(binding.read())
    }
}

/// Shows the grid rows of the current fill type (Color, Gradient or Pattern)
/// and hides the others'.
private final class FillRowsSwitch {
    var grid: NSGridView?
    var colorRows: [Int] = []
    var gradientRows: [Int] = []
    var patternRows: [Int] = []

    func apply(_ fx: StrokeEffect) {
        guard let grid = grid else { return }
        let gradient = fx.fillType == "gradient"
        let pattern = fx.fillType == "pattern"
        for index in colorRows where index < grid.numberOfRows {
            grid.row(at: index).isHidden = gradient || pattern
        }
        for index in gradientRows where index < grid.numberOfRows {
            grid.row(at: index).isHidden = !gradient
        }
        for index in patternRows where index < grid.numberOfRows {
            grid.row(at: index).isHidden = !pattern
        }
    }
}
