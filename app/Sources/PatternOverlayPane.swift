import AppKit

/// Layer Style > Pattern Overlay: the layer's shape filled with a tiled pattern above
/// its pixels, below a Gradient and a Color Overlay, at the overlay's OWN blend mode and opacity.
/// The rows: Blend Mode, Opacity, the Pattern popup (the four presets with their
/// swatches, plus "Custom: <name>" while the effect holds a custom tile, e.g. from a PSD or MCP),
/// Angle (turning the tile about its origin), Scale 1–1000 %, Link with Layer, and Snap to Origin. A linked tile starts at the canvas
/// origin plus the phase plus the layer's moves since it was made (Photoshop's fxrp; the core
/// keeps where the layer was made as the effect's `anchor`) and then moves with the layer. Snap
/// to Origin puts the phase back: linked, at the layer content's top left with the anchor at the
/// layer's offset now, so the tile starts there; unlinked, at the canvas origin.
final class PatternOverlayPane: NSObject, LayerStylePane {
    let view: NSView
    private let rows: [PaneRow]

    /// `layerOrigin` answers the layer content's top left in canvas px (nil when it has none);
    /// `layerOffset` the layer's offset (nil when unknown).
    init(binding: EffectBinding<PatternOverlayEffect>, context: LayerStylePaneContext,
         layerOrigin: @escaping () -> CGPoint?,
         layerOffset: @escaping () -> (x: Int, y: Int)?) {
        let blend = PaneRows.blendPopup(
            modes: context.blendModes,
            get: { binding.read().blend },
            set: { value in binding.update { $0.blend = value } })
        let opacity = PaneRows.percent(
            "Opacity:",
            get: { binding.read().opacity },
            set: { value in binding.update { $0.opacity = value } })
        let fill = EffectBinding<PatternFill>(
            get: { binding.read().fill },
            set: { value in binding.update { $0.fill = value } })
        let fillRows = PatternFillRows.make(
            binding: fill, layerOrigin: layerOrigin, layerOffset: layerOffset)
        let note = PaneRows.note(
            "Tiles the pattern over the layer's shape above its pixels, with its own blend mode. "
                + "Linked, the pattern moves with the layer; unlinked, it stays on the canvas grid.")
        rows = [blend, opacity] + fillRows + [note]
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

/// The pattern fill's rows, shared by the Pattern Overlay's pane, a Stroke of
/// Fill Type Pattern and the bevel's Texture (`showsAngle` false: the texture
/// has no Angle): the Pattern popup, Angle, Scale 1–1000 %, Link with Layer and Snap to
/// Origin. `layerOrigin` answers the layer content's top left in canvas px (nil when it has
/// none); `layerOffset` the layer's offset (nil when unknown).
enum PatternFillRows {
    static func make(
        binding: EffectBinding<PatternFill>,
        layerOrigin: @escaping () -> CGPoint?,
        layerOffset: @escaping () -> (x: Int, y: Int)?,
        showsAngle: Bool = true
    ) -> [PaneRow] {
        let held = binding.read().pattern.flatMap { $0.isCustom ? $0 : nil }
        let pattern = patternPopup(held: held, binding: binding)
        let angle = PaneRows.angle(
            "Angle:",
            get: { binding.read().angle },
            set: { value in binding.update { $0.angle = value } })
        let scale = PaneRows.slider(
            "Scale:", min: 1, max: 1000, integer: true,
            format: { String(format: "%.0f %%", $0) },
            get: { binding.read().scale * 100 },
            set: { value in binding.update { $0.scale = value / 100 } })
        let link = PaneRows.checkbox(
            "Link with Layer",
            get: { binding.read().linkWithLayer },
            set: { value in
                binding.update {
                    $0.linkWithLayer = value
                    // Linking anew anchors where the layer was made, on Apply.
                    $0.anchor = nil
                }
            })
        let snap = PaneButtonRow(title: "Snap to Origin") {
            binding.update { fx in
                if fx.linkWithLayer {
                    let origin = layerOrigin() ?? .zero
                    fx.phase = [Double(origin.x), Double(origin.y)]
                    // Anchored at the offset NOW, so the tile starts at the content's
                    // corner even on a layer moved since it was made.
                    fx.anchor = layerOffset().map { [$0.x, $0.y] }
                } else {
                    fx.phase = [0, 0]
                }
            }
        }
        return showsAngle ? [pattern, angle, scale, link, snap] : [pattern, scale, link, snap]
    }

    /// The presets (`RasterDocument.patternPresets`) as `preset:<id>` items with their swatches,
    /// and the held custom tile as one `custom` item.
    private static func patternPopup(held: PatternRef?, binding: EffectBinding<PatternFill>)
        -> PanePopupRow
    {
        let menu = NSMenu()
        for preset in RasterDocument.patternPresets() {
            let item = NSMenuItem(title: preset.name, action: nil, keyEquivalent: "")
            item.image = preset.image
            item.representedObject = "preset:" + preset.id
            menu.addItem(item)
        }
        if let held {
            let item = NSMenuItem(
                title: "Custom: " + (held.name ?? "Pattern"), action: nil, keyEquivalent: "")
            item.representedObject = "custom"
            menu.addItem(item)
        }
        return PanePopupRow(
            label: "Pattern:", menu: menu,
            get: {
                let current = binding.read().pattern
                if current?.isCustom == true { return "custom" }
                return "preset:" + (current?.preset ?? "checkerboard")
            },
            set: { value in
                binding.update { fx in
                    if value == "custom" {
                        fx.pattern = held
                    } else {
                        fx.pattern = PatternRef(preset: String(value.dropFirst("preset:".count)))
                    }
                }
            })
    }
}

/// A push button in the control columns (the label column stays empty).
final class PaneButtonRow: NSObject, PaneRow {
    private let button: NSButton
    private let action: () -> Void

    var views: [NSView] { [NSView(), button] }
    var span: PaneRowSpan { .controls }

    init(title: String, action: @escaping () -> Void) {
        self.action = action
        button = NSButton(title: title, target: nil, action: nil)
        super.init()
        button.bezelStyle = .rounded
        button.font = DS.sans(13)
        button.target = self
        button.action = #selector(pressed(_:))
    }

    func reload() {}

    @objc private func pressed(_ sender: Any?) {
        action()
    }
}
