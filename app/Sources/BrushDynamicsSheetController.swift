import AppKit

/// The Brush's, Pencil's and Eraser's Dynamics… sheet (`designs/brush-dynamics.md`): the
/// size and opacity responses (Control, Minimum, Jitter and a Curve… popover over the
/// shared `CurveEditorView`), Scatter (Amount, Both Axes, Count, Count Jitter) and, for the
/// Brush and the Eraser (`shapes`), the tip shape's Angle (Control, Jitter) and Roundness
/// (Control, Minimum, Jitter), the Texture (a pattern popup of None, the built-ins and the
/// library, which embeds the chosen tile; Scale and Depth in percent) and the Dual Brush (a
/// checkbox, then the second tip's Size in pixels, Hardness, Spacing, Angle, Roundness, Scatter,
/// Both Axes, Count and Count Jitter), and for the Brush (`colour`) the colour dynamics
/// (Foreground/Background Mix, Hue, Saturation and Brightness Jitter, in percent). It edits a
/// copy of the tool's options; Apply hands the result to `onApply`
/// (`EditorViewController.showBrushDynamicsSheet`). Tool options only: no undo step, no
/// Action. Percent fields show the model's 0…1 values (scatter: diameters) × 100.
final class BrushDynamicsSheetController: NSViewController, NSPopoverDelegate {
    private var options: PaintToolOptions
    /// Whether the tool takes a tip shape: false for the Pencil, whose tip is round.
    private let shapes: Bool
    /// Whether the tool takes colour dynamics: the Brush alone (`takesColourDynamics`).
    private let colour: Bool
    private let onApply: (PaintToolOptions) -> Void
    private static let controls: [(BrushDynamics.Control, String)] = [
        (.off, "Off"), (.pressure, "Pen Pressure"), (.tilt, "Pen Tilt")]
    private static let angleControls: [(BrushDynamics.TipShape.AngleControl, String)] = [
        (.off, "Off"), (.rotation, "Pen Rotation"), (.tilt, "Pen Tilt"), (.direction, "Direction")]

    private let sizeControl = NSPopUpButton()
    private let opacityControl = NSPopUpButton()
    private let sizeMinimum = NSTextField(string: "")
    private let sizeJitter = NSTextField(string: "")
    private let opacityMinimum = NSTextField(string: "")
    private let opacityJitter = NSTextField(string: "")
    private let scatter = NSTextField(string: "")
    private let bothAxes = NSButton(checkboxWithTitle: "Both axes", target: nil, action: nil)
    private let count = NSTextField(string: "")
    private let countJitter = NSTextField(string: "")
    private let angleControl = NSPopUpButton()
    private let angleJitter = NSTextField(string: "")
    private let roundnessControl = NSPopUpButton()
    private let roundnessMinimum = NSTextField(string: "")
    private let roundnessJitter = NSTextField(string: "")
    private let texture = NSPopUpButton()
    private let textureScale = NSTextField(string: "")
    private let textureDepth = NSTextField(string: "")
    private let dualEnabled = NSButton(checkboxWithTitle: "On", target: nil, action: nil)
    private let dualSize = NSTextField(string: "")
    private let dualHardness = NSTextField(string: "")
    private let dualSpacing = NSTextField(string: "")
    private let dualAngle = NSTextField(string: "")
    private let dualRoundness = NSTextField(string: "")
    private let dualScatter = NSTextField(string: "")
    private let dualBothAxes = NSButton(checkboxWithTitle: "Both axes", target: nil, action: nil)
    private let dualCount = NSTextField(string: "")
    private let dualCountJitter = NSTextField(string: "")
    private let colourMix = NSTextField(string: "")
    private let colourHue = NSTextField(string: "")
    private let colourSaturation = NSTextField(string: "")
    private let colourBrightness = NSTextField(string: "")
    /// The Texture popup's patterns by row: nil (None), the options' own when no picker row
    /// holds it, then every built-in and library pattern.
    private var texturePatterns: [PatternRef?] = []
    private var popover: NSPopover?

    init(options: PaintToolOptions, shapes: Bool, colour: Bool = false,
         onApply: @escaping (PaintToolOptions) -> Void) {
        self.options = options
        self.shapes = shapes
        self.colour = colour
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("BrushDynamicsSheetController does not support NSCoder")
    }

    override func loadView() {
        for popup in [sizeControl, opacityControl, roundnessControl] {
            popup.addItems(withTitles: Self.controls.map { $0.1 })
        }
        angleControl.addItems(withTitles: Self.angleControls.map { $0.1 })
        let fields: [(NSTextField, String)] = [
            (sizeMinimum, "brush-dynamics-size-minimum"), (sizeJitter, "brush-dynamics-size-jitter"),
            (opacityMinimum, "brush-dynamics-opacity-minimum"),
            (opacityJitter, "brush-dynamics-opacity-jitter"), (scatter, "brush-dynamics-scatter"),
            (count, "brush-dynamics-count"), (countJitter, "brush-dynamics-count-jitter"),
            (angleJitter, "brush-dynamics-angle-jitter"),
            (roundnessMinimum, "brush-dynamics-roundness-minimum"),
            (roundnessJitter, "brush-dynamics-roundness-jitter"),
            (textureScale, "brush-dynamics-texture-scale"),
            (textureDepth, "brush-dynamics-texture-depth"),
            (dualSize, "brush-dynamics-dual-size"), (dualHardness, "brush-dynamics-dual-hardness"),
            (dualSpacing, "brush-dynamics-dual-spacing"), (dualAngle, "brush-dynamics-dual-angle"),
            (dualRoundness, "brush-dynamics-dual-roundness"),
            (dualScatter, "brush-dynamics-dual-scatter"), (dualCount, "brush-dynamics-dual-count"),
            (dualCountJitter, "brush-dynamics-dual-count-jitter"),
            (colourMix, "brush-dynamics-colour-mix"), (colourHue, "brush-dynamics-colour-hue"),
            (colourSaturation, "brush-dynamics-colour-saturation"),
            (colourBrightness, "brush-dynamics-colour-brightness")]
        for (field, id) in fields {
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 56).isActive = true
            field.setAccessibilityIdentifier(id)
        }
        dualEnabled.setAccessibilityIdentifier("brush-dynamics-dual")
        dualBothAxes.setAccessibilityIdentifier("brush-dynamics-dual-both-axes")
        sizeControl.setAccessibilityIdentifier("brush-dynamics-size-control")
        opacityControl.setAccessibilityIdentifier("brush-dynamics-opacity-control")
        angleControl.setAccessibilityIdentifier("brush-dynamics-angle-control")
        roundnessControl.setAccessibilityIdentifier("brush-dynamics-roundness-control")
        texture.setAccessibilityIdentifier("brush-dynamics-texture")
        let sizeCurve = curveButton(#selector(editSizeCurve(_:)))
        let opacityCurve = curveButton(#selector(editOpacityCurve(_:)))
        func row(_ views: NSView...) -> NSStackView {
            let row = NSStackView(views: views)
            row.orientation = .horizontal
            row.spacing = 8
            return row
        }
        var rows: [[NSView]] = [
            [fieldLabel("Size:"), row(sizeControl, sizeCurve)],
            [fieldLabel("Minimum (%):"), row(sizeMinimum, fieldLabel("Jitter (%):"), sizeJitter)],
            [fieldLabel("Opacity:"), row(opacityControl, opacityCurve)],
            [fieldLabel("Minimum (%):"), row(opacityMinimum, fieldLabel("Jitter (%):"), opacityJitter)],
            [fieldLabel("Scatter (%):"), row(scatter, bothAxes)],
            [fieldLabel("Count:"), row(count, fieldLabel("Jitter (%):"), countJitter)],
        ]
        // The Pencil's tip is round: it shows no Angle or Roundness rows.
        if shapes {
            rows += [
                [fieldLabel("Angle:"), row(angleControl, fieldLabel("Jitter (%):"), angleJitter)],
                [fieldLabel("Roundness:"), row(roundnessControl)],
                [fieldLabel("Minimum (%):"),
                 row(roundnessMinimum, fieldLabel("Jitter (%):"), roundnessJitter)],
                // The texture, which the Pencil's two-valued dab does not take either.
                [fieldLabel("Texture:"), row(texture)],
                [fieldLabel("Scale (%):"), row(textureScale, fieldLabel("Depth (%):"), textureDepth)],
                // The dual brush, which the Pencil's two-valued dab does not take either.
                [fieldLabel("Dual brush:"), row(dualEnabled, fieldLabel("Size (px):"), dualSize)],
                [fieldLabel("Hardness (%):"),
                 row(dualHardness, fieldLabel("Spacing (%):"), dualSpacing)],
                [fieldLabel("Angle (°):"),
                 row(dualAngle, fieldLabel("Roundness (%):"), dualRoundness)],
                [fieldLabel("Scatter (%):"), row(dualScatter, dualBothAxes)],
                [fieldLabel("Count:"), row(dualCount, fieldLabel("Jitter (%):"), dualCountJitter)],
            ]
        }
        // Colour dynamics: the Brush's alone, whose one ink is the per-dab one's start.
        if colour {
            rows += [
                [fieldLabel("Fg/Bg mix (%):"),
                 row(colourMix, fieldLabel("Hue jitter (%):"), colourHue)],
                [fieldLabel("Saturation (%):"),
                 row(colourSaturation, fieldLabel("Brightness (%):"), colourBrightness)],
            ]
        }
        let grid = NSGridView(views: rows)
        grid.columnSpacing = 12
        grid.rowSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let reset = NSButton(title: "Reset", target: self, action: #selector(resetClicked(_:)))
        reset.bezelStyle = .rounded
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        view = makeSheetView(
            title: "Brush dynamics",
            hint: "Size and opacity can follow pen pressure or tilt through a curve, down to a "
                + "minimum; jitter shrinks each dab at random, and scatter spreads the dabs "
                + "across the stroke, in percent of the brush size."
                + (shapes ? " The tip's angle can follow the pen's rotation, its tilt or the "
                    + "direction of travel, and its roundness pressure or tilt. A texture holds "
                    + "paint back where its pattern is dark, by up to Depth, on the canvas's own "
                    + "grid at Scale. A dual brush walks a second tip of its own size, spacing "
                    + "and scatter along the stroke, and paints only where both tips reach "
                    + "(an airbrush's held deposits lay the first tip alone)." : "")
                + (colour ? " Colour moves each dab's ink toward the background colour by up to "
                    + "the mix, turns its hue by up to ±180° × the hue jitter and lowers its "
                    + "saturation and brightness by up to their jitter; Grayscale, Duotone and Lab "
                    + "take the mix and brightness, and a mask, a Quick Mask stroke, a mode other "
                    + "than Normal and a CMYK, Indexed or Bitmap document paint the one colour."
                    : "")
                + " A mouse reads pressure, tilt and rotation as off. Each stroke draws a new "
                + "random sequence and records the dabs it laid, so a replay paints the same "
                + "pixels.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton, leading: [reset]))
        reload()
    }

    private func curveButton(_ action: Selector) -> NSButton {
        let button = NSButton(title: "Curve…", target: self, action: action)
        button.bezelStyle = .rounded
        return button
    }

    private var dynamics: BrushDynamics { options.dynamics ?? BrushDynamics() }

    /// The controls from `options`.
    private func reload() {
        let d = dynamics
        func index(_ control: BrushDynamics.Control) -> Int {
            Self.controls.firstIndex { $0.0 == control } ?? 0
        }
        sizeControl.selectItem(at: index(options.sizeControl))
        opacityControl.selectItem(at: index(options.opacityControl))
        func percent(_ value: Double) -> String { String(format: "%g", (value * 100).rounded()) }
        sizeMinimum.stringValue = percent(d.size.minimum)
        sizeJitter.stringValue = percent(d.size.jitter)
        opacityMinimum.stringValue = percent(d.opacity.minimum)
        opacityJitter.stringValue = percent(d.opacity.jitter)
        scatter.stringValue = percent(d.scatter)
        bothAxes.state = d.bothAxes ? .on : .off
        count.stringValue = "\(d.count)"
        countJitter.stringValue = percent(d.countJitter)
        let shape = d.shape ?? BrushDynamics.TipShape()
        angleControl.selectItem(at: Self.angleControls.firstIndex { $0.0 == shape.angleControl } ?? 0)
        angleJitter.stringValue = percent(shape.angleJitter)
        roundnessControl.selectItem(at: index(shape.roundnessControl))
        roundnessMinimum.stringValue = percent(shape.roundnessMinimum)
        roundnessJitter.stringValue = percent(shape.roundnessJitter)
        loadTextureRows(d.texture?.pattern)
        texture.selectItem(at: texturePatterns.firstIndex { $0 == d.texture?.pattern } ?? 0)
        textureScale.stringValue = percent(d.texture?.scale ?? 1)
        textureDepth.stringValue = percent(d.texture?.depth ?? 1)
        // Off, the rows show a second tip half the brush's size at the defaults.
        dualEnabled.state = d.dual != nil ? .on : .off
        let dual = d.dual ?? BrushDynamics.Dual(size: min(max((options.size / 2).rounded(),
            BrushDynamics.Dual.sizeRange.lowerBound), BrushDynamics.Dual.sizeRange.upperBound))
        dualSize.stringValue = String(format: "%g", dual.size)
        dualHardness.stringValue = percent(dual.hardness)
        dualSpacing.stringValue = String(format: "%g", dual.spacing)
        dualAngle.stringValue = String(format: "%g", dual.angleDegrees)
        dualRoundness.stringValue = percent(dual.roundness)
        dualScatter.stringValue = percent(dual.scatter)
        dualBothAxes.state = dual.bothAxes ? .on : .off
        dualCount.stringValue = "\(dual.count)"
        dualCountJitter.stringValue = percent(dual.countJitter)
        let tint = d.color ?? BrushDynamics.ColorDynamics()
        colourMix.stringValue = percent(tint.mix)
        colourHue.stringValue = percent(tint.hue)
        colourSaturation.stringValue = percent(tint.saturation)
        colourBrightness.stringValue = percent(tint.brightness)
    }

    /// The Texture popup's rows (`texturePatterns`): None, `current` when no picker row holds
    /// it (a tile an MCP preset brought), then the built-ins and the library, each a library
    /// pattern's tile without its native samples (the core reads the raster alone). Items are
    /// added to the menu directly, so two rows of one name both stay.
    private func loadTextureRows(_ current: PatternRef?) {
        let choices = PatternChoice.all()
        var rows: [(String, NSImage?, PatternRef?)] = [("None", nil, nil)]
        if let current, !choices.contains(where: { $0.ref.withoutNative == current }) {
            rows.append((current.name ?? current.preset ?? "Custom", nil, current))
        }
        for choice in choices { rows.append((choice.name, choice.image, choice.ref.withoutNative)) }
        texturePatterns = rows.map { $0.2 }
        texture.removeAllItems()
        for (title, image, _) in rows {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.image = image
            texture.menu?.addItem(item)
        }
    }

    /// `options` from the controls, each value clamped to its range.
    private func read() {
        var d = dynamics
        func unit(_ field: NSTextField, _ upper: Double = 1) -> Double {
            min(max(field.doubleValue / 100, 0), upper)
        }
        d.size.minimum = unit(sizeMinimum)
        d.size.jitter = unit(sizeJitter)
        d.opacity.minimum = unit(opacityMinimum)
        d.opacity.jitter = unit(opacityJitter)
        d.scatter = unit(scatter, BrushDynamics.maximumScatter)
        d.bothAxes = bothAxes.state == .on
        d.count = min(max(count.integerValue, 1), BrushDynamics.maximumCount)
        d.countJitter = unit(countJitter)
        if shapes {
            d.shape = BrushDynamics.TipShape(
                angleControl: Self.angleControls[max(angleControl.indexOfSelectedItem, 0)].0,
                angleJitter: unit(angleJitter),
                roundnessControl: Self.controls[max(roundnessControl.indexOfSelectedItem, 0)].0,
                roundnessMinimum: unit(roundnessMinimum), roundnessJitter: unit(roundnessJitter))
            let row = texture.indexOfSelectedItem
            let pattern = texturePatterns.indices.contains(row) ? texturePatterns[row] : nil
            d.texture = pattern.map {
                BrushDynamics.Texture(
                    pattern: $0,
                    scale: min(max(textureScale.doubleValue / 100, BrushDynamics.Texture.scaleRange.lowerBound),
                               BrushDynamics.Texture.scaleRange.upperBound),
                    depth: unit(textureDepth))
            }
            func clamped(_ field: NSTextField, _ range: ClosedRange<Double>) -> Double {
                min(max(field.doubleValue, range.lowerBound), range.upperBound)
            }
            d.dual = dualEnabled.state != .on ? nil : BrushDynamics.Dual(
                size: clamped(dualSize, BrushDynamics.Dual.sizeRange),
                hardness: unit(dualHardness),
                roundness: min(max(dualRoundness.doubleValue / 100, 0.01), 1),
                angleDegrees: clamped(dualAngle, -180...180),
                spacing: clamped(dualSpacing, BrushDynamics.Dual.spacingRange),
                scatter: unit(dualScatter, BrushDynamics.maximumScatter),
                bothAxes: dualBothAxes.state == .on,
                count: min(max(dualCount.integerValue, 1), BrushDynamics.maximumCount),
                countJitter: unit(dualCountJitter))
        }
        if colour {
            let tint = BrushDynamics.ColorDynamics(mix: unit(colourMix), hue: unit(colourHue),
                saturation: unit(colourSaturation), brightness: unit(colourBrightness))
            d.color = tint.isZero ? nil : tint
        }
        options.dynamics = d
        options.sizeControl = Self.controls[max(sizeControl.indexOfSelectedItem, 0)].0
        options.opacityControl = Self.controls[max(opacityControl.indexOfSelectedItem, 0)].0
        options.dynamics = options.dynamics?.normalized
    }

    @objc private func editSizeCurve(_ sender: NSButton) {
        showCurve(\BrushDynamics.size, from: sender)
    }

    @objc private func editOpacityCurve(_ sender: NSButton) {
        showCurve(\BrushDynamics.opacity, from: sender)
    }

    /// The response's curve in the shared editor, in its 0…255 space; each edit is
    /// stored as 0…1 points with the ends pinned at x = 0 and x = 1.
    private func showCurve(_ response: WritableKeyPath<BrushDynamics, BrushDynamics.Response>,
                           from anchor: NSButton) {
        read()
        let editor = CurveEditorView()
        let seed = dynamics[keyPath: response].curve ?? [[0, 0], [1, 1]]
        editor.points = seed.map { CurvePoint(x: $0[0] * 255, y: $0[1] * 255) }
        editor.onPointsChanged = { [weak self] points in self?.curveEdited(response, points) }
        let side = editor.intrinsicContentSize
        editor.frame = NSRect(origin: .zero, size: side)
        let content = NSViewController()
        content.view = editor
        let popover = NSPopover()
        popover.contentViewController = content
        popover.contentSize = side
        popover.behavior = .transient
        popover.delegate = self
        self.popover = popover
        popover.show(relativeTo: anchor.bounds, of: anchor, preferredEdge: .maxX)
    }

    private func curveEdited(_ response: WritableKeyPath<BrushDynamics, BrushDynamics.Response>,
                             _ points: [CurvePoint]) {
        var d = dynamics
        d[keyPath: response].curve = Self.pinned(points)
        options.dynamics = d.normalized
    }

    /// Editor points (0…255) as a dynamics curve: scaled to 0…1, the first x pinned to 0
    /// and the last to 1, an inner point the pinning would collide with dropped.
    static func pinned(_ points: [CurvePoint]) -> [[Double]] {
        let scaled = points.map { [min(max($0.x / 255, 0), 1), min(max($0.y / 255, 0), 1)] }
        guard scaled.count >= 2, let first = scaled.first, let last = scaled.last else {
            return [[0, 0], [1, 1]]
        }
        var kept = [[0, first[1]]]
        for point in scaled.dropFirst().dropLast()
        where point[0] > kept[kept.count - 1][0] && point[0] < 1 {
            kept.append(point)
        }
        kept.append([1, last[1]])
        return kept // the editor holds at most `CurveEditorView.maxPoints`, 16
    }

    func popoverDidClose(_ notification: Notification) {
        popover = nil
    }

    @objc private func resetClicked(_ sender: Any?) {
        options.dynamics = nil
        reload()
    }

    @objc private func applyClicked(_ sender: Any?) {
        read()
        let chosen = options
        dismiss(self)
        onApply(chosen)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
