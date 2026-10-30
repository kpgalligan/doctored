import AppKit

/// The fill-layer dialog: Layer ▸ New Fill Layer ▸ Solid Color…,
/// Gradient… and Pattern…, and the same dialog reopened on an existing
/// fill layer by a double-click in the Layers panel or Layer ▸ Layer Content
/// Options….
///
/// A Solid Color fill shows a colour well (alpha included). A Gradient fill
/// shows the shared stop editor (`GradientStopsPane`) over Style, Angle,
/// Scale, Smoothness, Reverse and Align with Layer — the
/// layer-style gradient's controls, because it is the same `GradientFill`
/// rendered over the whole canvas, or with Align with Layer over the layer's
/// mask box (FillLayer.swift's ALIGN). A Pattern fill shows
/// the Pattern Overlay's rows (`PatternFillRows`: Pattern, Angle, Scale, Link
/// with Layer, Snap to Origin) over the layer's `reference` point, so Snap to
/// Origin puts the tile at the canvas origin and never sets an anchor.
///
/// The dialog edits a `FillLayerPayload` and hands it to `onApply`; the
/// commit, its undo step and its recorded step are the caller's
/// (`EditorViewController+FillLayers`). Colours are authored sRGB. There is no
/// live canvas preview: the gradient's ramp draws in the editor.
final class FillLayerSheetController: NSViewController, NSTextFieldDelegate {
    private let titleText: String
    private var payload: FillLayerPayload
    private let onApply: (FillLayerPayload) -> Void
    /// The layer's reference point (`RasterDocument.layerReference`): a
    /// linked pattern's origin is phase + this.
    private let reference: (x: Int, y: Int)
    private var patternRows: [PaneRow] = []
    private let colorWell = NSColorWell()
    private var pane: GradientStopsPane?
    private let stylePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let angleField = NSTextField(string: "")
    private let scaleField = NSTextField(string: "")
    private let smoothnessField = NSTextField(string: "")
    private let reverseBox = NSButton(checkboxWithTitle: "Reverse", target: nil, action: nil)
    private let alignBox = NSButton(checkboxWithTitle: "Align with Layer", target: nil, action: nil)

    private static let styleTitles = ["Linear", "Radial", "Angle", "Reflected", "Diamond"]

    init(title: String, payload: FillLayerPayload, reference: (x: Int, y: Int),
         onApply: @escaping (FillLayerPayload) -> Void) {
        titleText = title
        self.payload = payload
        self.reference = reference
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FillLayerSheetController does not support NSCoder")
    }

    override func loadView() {
        let content: NSView
        let hint: String
        if payload.patternFill != nil {
            content = patternContent()
            hint = "The pattern covers the whole canvas and follows it when the canvas changes "
                + "size. Paint on the layer's mask to choose where it shows."
        } else if let gradient = payload.gradient {
            content = gradientContent(gradient)
            hint = "The gradient spans the whole canvas and follows it when the canvas changes "
                + "size, or with Align with Layer the bounds of the layer's mask. Paint on the "
                + "layer's mask to choose where it shows; an aligned gradient takes a new mask's "
                + "bounds the next time it is re-rendered (Apply after changing a setting here, "
                + "or a canvas change)."
        } else {
            content = solidContent(payload.color ?? [0, 0, 0, 1])
            hint = "The color covers the whole canvas and follows it when the canvas changes "
                + "size. Paint on the layer's mask to choose where it shows."
        }
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("fill-layer-apply")
        view = makeSheetView(
            title: titleText, hint: hint, content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    // MARK: - Content

    private func solidContent(_ rgba: [Double]) -> NSView {
        colorWell.color = ArtworkColor.color(rgba) ?? .black
        colorWell.identifier = NSUserInterfaceItemIdentifier("fill-layer-color")
        colorWell.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            colorWell.widthAnchor.constraint(equalToConstant: 64),
            colorWell.heightAnchor.constraint(equalToConstant: 28),
        ])
        // A fill colour may be partly transparent.
        NSColorPanel.shared.showsAlpha = true
        return Self.grid([[fieldLabel("Color:"), colorWell]])
    }

    private func gradientContent(_ gradient: GradientFill) -> NSView {
        let pane = GradientStopsPane(fill: gradient)
        self.pane = pane
        stylePopup.addItems(withTitles: Self.styleTitles)
        stylePopup.selectItem(at: GradientFill.styles.firstIndex(of: gradient.style) ?? 0)
        stylePopup.identifier = NSUserInterfaceItemIdentifier("fill-layer-style")
        for (field, value, id) in [
            (angleField, gradient.angle, "fill-layer-angle"),
            (scaleField, gradient.scale * 100, "fill-layer-scale"),
            (smoothnessField, (gradient.smoothness ?? 1) * 100, "fill-layer-smoothness"),
        ] {
            let formatter = NumberFormatter()
            formatter.numberStyle = .decimal
            formatter.maximumFractionDigits = 1
            field.formatter = formatter
            field.alignment = .right
            field.doubleValue = value
            field.identifier = NSUserInterfaceItemIdentifier(id)
            DSField.style(field)
            field.widthAnchor.constraint(equalToConstant: 62).isActive = true
        }
        reverseBox.state = gradient.reverse ? .on : .off
        reverseBox.font = DS.sans(13)
        reverseBox.identifier = NSUserInterfaceItemIdentifier("fill-layer-reverse")
        alignBox.state = gradient.alignWithLayer ? .on : .off
        alignBox.font = DS.sans(13)
        alignBox.identifier = NSUserInterfaceItemIdentifier("fill-layer-align")
        let grid = Self.grid([
            [fieldLabel("Style:"), stylePopup],
            [fieldLabel("Angle:"), Self.suffixed(angleField, "°")],
            [fieldLabel("Scale:"), Self.suffixed(scaleField, "%")],
            [fieldLabel("Smoothness:"), Self.suffixed(smoothnessField, "%")],
            [NSGridCell.emptyContentView, reverseBox],
            [NSGridCell.emptyContentView, alignBox],
        ])
        let stack = NSStackView(views: [pane, grid])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        return stack
    }

    /// The Pattern Overlay's rows over `payload.patternFill`. The layer's
    /// "origin" is −reference and its offset unknown, so Snap to Origin sets
    /// the phase that puts a linked tile at the canvas origin, anchor nil.
    private func patternContent() -> NSView {
        let binding = EffectBinding<PatternFill>(
            get: { [weak self] in self?.payload.patternFill ?? PatternFill() },
            set: { [weak self] value in self?.payload.patternFill = value })
        let reference = self.reference
        patternRows = PatternFillRows.make(
            binding: binding,
            layerOrigin: { CGPoint(x: -reference.x, y: -reference.y) },
            layerOffset: { nil })
        patternRows.forEach { $0.reload() }
        return PaneRows.grid(patternRows)
    }

    private static func grid(_ rows: [[NSView]]) -> NSGridView {
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        return grid
    }

    private static func suffixed(_ field: NSTextField, _ suffix: String) -> NSView {
        let label = NSTextField(labelWithString: suffix)
        label.font = DS.sans(13)
        label.textColor = DS.textMuted
        let row = NSStackView(views: [field, label])
        row.orientation = .horizontal
        row.alignment = .centerY
        row.spacing = 6
        return row
    }

    // MARK: - Actions

    /// The payload the controls spell right now; nil when the solid colour
    /// cannot be read as sRGB.
    private func edited() -> FillLayerPayload? {
        var out = payload
        if out.patternFill != nil {
            // Placement follows the layer's reference at render: no anchor.
            out.patternFill?.anchor = nil
        } else if var gradient = payload.gradient {
            if let stops = pane?.fill.stops { gradient.stops = stops }
            let index = min(max(stylePopup.indexOfSelectedItem, 0), GradientFill.styles.count - 1)
            gradient.style = GradientFill.styles[index]
            let angle = angleField.doubleValue
            gradient.angle = angle.isFinite ? angle : gradient.angle
            // The core's range for a gradient's scale: 10 to 150 %.
            let scale = scaleField.doubleValue / 100
            gradient.scale = scale.isFinite ? min(max(scale, 0.1), 1.5) : gradient.scale
            // Smoothness 0 to 100 %, stored as a fraction and nil at 100 %.
            let smoothness = smoothnessField.doubleValue / 100
            if smoothness.isFinite {
                let clamped = min(max(smoothness, 0), 1)
                gradient.smoothness = clamped >= 1 ? nil : clamped
            }
            gradient.reverse = reverseBox.state == .on
            gradient.alignWithLayer = alignBox.state == .on
            out.gradient = gradient
        } else {
            guard let rgba = ArtworkColor.rgba(colorWell.color) else { return nil }
            out.color = rgba
        }
        return out
    }

    @objc private func applyClicked(_ sender: Any?) {
        view.window?.makeFirstResponder(nil)
        guard let result = edited() else {
            NSSound.beep()
            return
        }
        colorWell.deactivate()
        dismiss(self)
        onApply(result)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        colorWell.deactivate()
        dismiss(self)
    }
}
