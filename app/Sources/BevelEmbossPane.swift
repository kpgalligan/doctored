import AppKit

/// Layer Style > Bevel & Emboss — the relief pane: structure (style,
/// technique — Smooth, Chisel Hard, Chisel Soft (#578) —, depth, direction,
/// size, soften), the light (angle and altitude, both
/// following the document's global light while Use Global Light is on, the
/// DropShadowPane pattern), and the highlight and shadow halves, each with
/// its own blend mode, colour and opacity; then the Contour and Texture
/// sub-panels as sections of the pane. The Texture section is its
/// checkbox, the Pattern Overlay's rows without Angle (`PatternFillRows`:
/// Pattern, Scale, Link with Layer, Snap to Origin), Depth −1000…+1000 % and
/// Invert. Every control writes through the binding; the sheet previews
/// through the projection.
final class BevelEmbossPane: NSObject, LayerStylePane {
    let view: NSView
    private let rows: [PaneRow]

    /// `layerOrigin` and `layerOffset` are the Pattern Overlay pane's (`PatternFillRows`): the
    /// layer content's top left and the layer's offset, for the texture's Snap to Origin.
    init(binding: EffectBinding<BevelEmbossEffect>, context: LayerStylePaneContext,
         layerOrigin: @escaping () -> CGPoint?,
         layerOffset: @escaping () -> (x: Int, y: Int)?) {
        // -- Structure --
        let style = PaneRows.popup(
            "Style:",
            titles: ["Outer Bevel", "Inner Bevel", "Emboss", "Pillow Emboss"],
            values: BevelEmbossEffect.styles,
            get: { binding.read().style },
            set: { value in binding.update { $0.style = value } })
        // Smooth is nil and writes no key (#578).
        let technique = PaneRows.popup(
            "Technique:",
            titles: ["Smooth", "Chisel Hard", "Chisel Soft"],
            values: BevelEmbossEffect.techniques,
            get: { binding.read().technique ?? "smooth" },
            set: { value in binding.update { $0.technique = value == "smooth" ? nil : value } })
        // Depth runs 1…1000 %; the model holds 1.0 = 100 %.
        let depth = PaneRows.slider(
            "Depth:", min: 1, max: 1000, integer: true,
            format: { String(format: "%.0f %%", $0) },
            get: { binding.read().depth * 100 },
            set: { value in binding.update { $0.depth = value / 100 } })
        let direction = PaneRows.popup(
            "Direction:",
            titles: ["Up", "Down"],
            values: BevelEmbossEffect.directions,
            get: { binding.read().direction },
            set: { value in binding.update { $0.direction = value } })
        let size = PaneRows.pixels(
            "Size:", max: 250,
            get: { binding.read().size },
            set: { value in binding.update { $0.size = value } })
        let soften = PaneRows.pixels(
            "Soften:", max: 16,
            get: { binding.read().soften },
            set: { value in binding.update { $0.soften = value } })

        // -- Shading -- With Use Global Light on, Angle and Altitude show
        // and edit the document's light (what the core actually reads);
        // off, the effect's own.
        let angle = PaneRows.angle(
            "Angle:",
            get: {
                let fx = binding.read()
                return fx.useGlobalLight ? context.globalLight.read().angle : fx.angle
            },
            set: { value in
                if binding.read().useGlobalLight {
                    context.globalLight.update { $0.angle = value }
                } else {
                    binding.update { $0.angle = value }
                }
            })
        let altitude = PaneRows.slider(
            "Altitude:", min: 0, max: 90, integer: true,
            format: { String(format: "%.0f°", $0) },
            get: {
                let fx = binding.read()
                return fx.useGlobalLight ? context.globalLight.read().altitude : fx.altitude
            },
            set: { value in
                if binding.read().useGlobalLight {
                    context.globalLight.update { $0.altitude = value }
                } else {
                    binding.update { $0.altitude = value }
                }
            })
        let useGlobalLight = PaneRows.checkbox(
            "Use Global Light",
            get: { binding.read().useGlobalLight },
            set: { on in
                // Turning it OFF keeps the light being shown: the effect
                // adopts the global angle and altitude rather than jumping
                // back to whatever it held before.
                let shown = context.globalLight.read()
                binding.update { fx in
                    fx.useGlobalLight = on
                    if !on {
                        fx.angle = shown.angle
                        fx.altitude = shown.altitude
                    }
                }
                angle.reload()
                altitude.reload()
            })

        // -- Highlight and shadow --
        let highlightBlend = PaneRows.blendPopup(
            "Highlight Mode:", modes: context.blendModes,
            get: { binding.read().highlightBlend },
            set: { value in binding.update { $0.highlightBlend = value } })
        let highlightColor = PaneRows.slotColor(
            "Highlight Color:",
            get: {
                let fx = binding.read()
                return StyleSlotColor(hex: fx.highlightColor, linear: fx.highlightLinear)
            },
            set: { value in
                binding.update {
                    $0.highlightColor = value.hex
                    $0.highlightLinear = value.linear
                }
            })
        let highlightOpacity = PaneRows.percent(
            "Highlight Opacity:",
            get: { binding.read().highlightOpacity },
            set: { value in binding.update { $0.highlightOpacity = value } })
        let shadowBlend = PaneRows.blendPopup(
            "Shadow Mode:", modes: context.blendModes,
            get: { binding.read().shadowBlend },
            set: { value in binding.update { $0.shadowBlend = value } })
        let shadowColor = PaneRows.slotColor(
            "Shadow Color:",
            get: {
                let fx = binding.read()
                return StyleSlotColor(hex: fx.shadowColor, linear: fx.shadowLinear)
            },
            set: { value in
                binding.update {
                    $0.shadowColor = value.hex
                    $0.shadowLinear = value.linear
                }
            })
        let shadowOpacity = PaneRows.percent(
            "Shadow Opacity:",
            get: { binding.read().shadowOpacity },
            set: { value in binding.update { $0.shadowOpacity = value } })

        // -- Gloss Contour, and the Contour sub-panel: a section of
        // the pane (its checkbox, the contour and its Range).
        let glossContour = PaneRows.contour(
            "Gloss Contour:",
            get: { binding.read().glossContour },
            set: { value in binding.update { $0.glossContour = value } },
            antiAliased: { binding.read().glossAntiAliased },
            setAntiAliased: { value in binding.update { $0.glossAntiAliased = value } })
        let contourEnabled = PaneRows.checkbox(
            "Contour",
            get: { binding.read().contourEnabled ?? false },
            set: { on in binding.update { $0.contourEnabled = on ? true : nil } })
        let contour = PaneRows.contour(
            get: { binding.read().contour },
            set: { value in binding.update { $0.contour = value } },
            antiAliased: { binding.read().contourAntiAliased },
            setAntiAliased: { value in binding.update { $0.contourAntiAliased = value } })
        let contourRange = PaneRows.range(
            get: { binding.read().contourRange },
            set: { value in binding.update { $0.contourRange = value } })

        // -- The Texture sub-panel: the texture is written
        // whole once the checkbox is first turned on, the defaults until
        // edited; switched off it is kept for the sheet.
        let textureEnabled = PaneRows.checkbox(
            "Texture",
            get: { binding.read().textureEnabled ?? false },
            set: { on in
                binding.update {
                    $0.textureEnabled = on ? true : nil
                    if on, $0.texture == nil { $0.texture = BevelTexture() }
                }
            })
        let texture = EffectBinding<BevelTexture>(
            get: { binding.read().texture ?? BevelTexture() },
            set: { value in binding.update { $0.texture = value } })
        let textureFill = PatternFillRows.make(
            binding: EffectBinding<PatternFill>(
                get: { texture.read().fill },
                set: { value in texture.update { $0.fill = value } }),
            layerOrigin: layerOrigin, layerOffset: layerOffset, showsAngle: false)
        // The texture Depth runs −1000…+1000 %; the model holds 1.0 = +100 %.
        let textureDepth = PaneRows.slider(
            "Texture Depth:", min: -1000, max: 1000, integer: true,
            format: { String(format: "%+.0f %%", $0) },
            get: { texture.read().depth * 100 },
            set: { value in texture.update { $0.depth = value / 100 } })
        let invert = PaneRows.checkbox(
            "Invert",
            get: { texture.read().invert },
            set: { value in texture.update { $0.invert = value } })

        rows = [
            style, technique, depth, direction, size, soften,
            angle, altitude, useGlobalLight, glossContour,
            highlightBlend, highlightColor, highlightOpacity,
            shadowBlend, shadowColor, shadowOpacity,
            contourEnabled, contour, contourRange,
            textureEnabled,
        ] + textureFill + [textureDepth, invert]
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
