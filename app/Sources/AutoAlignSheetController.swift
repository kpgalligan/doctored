import AppKit

/// Edit ▸ Auto-Align Layers… (AutoAlign.swift): which projection to
/// register the selected layers with — Auto, Perspective, Collage,
/// Cylindrical, Spherical or Reposition (AutoAlignProjection.swift) — and Lens Correction's
/// Vignette Removal (AutoAlignVignette.swift) and Geometric Distortion (AutoAlignDistortion.swift),
/// both always enabled: a projection that ignores a lens drops it (`Registration.lens`). Apply
/// hands all three to `onApply` (`EditorViewController.commitAutoAlign`).
final class AutoAlignSheetController: NSViewController {
    private let onApply: (AutoAlign.Registration, Bool, Bool) -> Void
    /// Auto first: it is the preselected choice.
    private let projections = AutoAlign.Registration.projections
    private lazy var projectionButtons: [NSButton] = projections.enumerated().map {
        position, projection in
        let button = NSButton(radioButtonWithTitle: projection.title, target: nil, action: nil)
        button.tag = position
        return button
    }

    /// Lens Correction ▸ Vignette Removal, off.
    private let vignetteBox = NSButton(checkboxWithTitle: "Vignette Removal", target: nil,
                                       action: nil)
    /// Lens Correction ▸ Geometric Distortion, off.
    private let distortionBox = NSButton(checkboxWithTitle: "Geometric Distortion", target: nil,
                                         action: nil)

    init(onApply: @escaping (AutoAlign.Registration, Bool, Bool) -> Void) {
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AutoAlignSheetController does not support NSCoder")
    }

    override func loadView() {
        for button in projectionButtons {
            // One shared action is what makes sibling radio buttons exclusive.
            button.target = self
            button.action = #selector(projectionChanged(_:))
            button.font = DS.sans(13)
            let name = projections.indices.contains(button.tag)
                ? projections[button.tag].projectionName : "projection"
            button.identifier = NSUserInterfaceItemIdentifier("auto-align-\(name)")
        }
        projectionButtons.first?.state = .on
        let stack = NSStackView(views: projectionButtons)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6

        vignetteBox.font = DS.sans(13)
        vignetteBox.state = .off
        vignetteBox.identifier = NSUserInterfaceItemIdentifier("auto-align-vignette")
        distortionBox.font = DS.sans(13)
        distortionBox.state = .off
        distortionBox.identifier = NSUserInterfaceItemIdentifier("auto-align-distortion")

        let lenses = NSStackView(views: [vignetteBox, distortionBox])
        lenses.orientation = .vertical
        lenses.alignment = .leading
        lenses.spacing = 6
        let lens = fieldLabel("Lens Correction:")
        let grid = NSGridView(views: [[fieldLabel("Projection:"), stack], [lens, lenses]])
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        grid.cell(for: stack)?.yPlacement = .top
        grid.cell(atColumnIndex: 0, rowIndex: 0).yPlacement = .top
        grid.cell(atColumnIndex: 0, rowIndex: 1).yPlacement = .top

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("auto-align-apply")
        view = makeSheetView(
            title: AutoAlign.title,
            hint: "The bottom selected layer (or the one position-locked layer) stays put, and "
                + "the canvas grows to hold the rest. Cylindrical and Spherical reproject every "
                + "layer, the reference too, and fit the canvas to them. Vignette Removal and "
                + "Geometric Distortion reproject Auto and Perspective onto a sphere; Collage and "
                + "Reposition ignore Vignette Removal and correct Geometric Distortion within "
                + "each layer's own frame.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    /// The radios' shared action: sharing it is what makes them exclusive.
    @objc private func projectionChanged(_ sender: Any?) {}

    private var chosenProjection: AutoAlign.Registration {
        let chosen = projectionButtons.first { $0.state == .on }?.tag ?? 0
        return projections.indices.contains(chosen) ? projections[chosen] : .auto
    }

    @objc private func applyClicked(_ sender: Any?) {
        let projection = chosenProjection
        let vignette = vignetteBox.isEnabled && vignetteBox.state == .on
        let distortion = distortionBox.isEnabled && distortionBox.state == .on
        dismiss(self)
        onApply(projection, vignette, distortion)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
