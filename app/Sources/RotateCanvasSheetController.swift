import AppKit

/// Image ▸ Rotate Arbitrary… — the sheet behind Phase 12F-1b3's destructive
/// whole-canvas rotation.
///
/// **No live preview, and that is a decision.** An arbitrary rotation EXPANDS
/// the canvas, and `ImageCanvasView.previewImage` is a canvas-sized display
/// slot with nowhere to put a bigger picture; showing one would mean a
/// transform plate this feature deliberately does not build. What the sheet
/// shows instead is the exact answer to the only question a preview would
/// have answered — the new canvas size — read from the CORE's own plan
/// (`RasterDocument.canvasRotationPlan`), refreshed whenever the angle field
/// or the direction control reports a change — the angle field is not
/// continuous, so it reports on Return or when editing ends, and the sampler
/// does not enter the plan at all.
///
/// The sampler popup opens at Bicubic and deliberately neither reads nor
/// writes `transformSampler`: that preference belongs to the Free Transform
/// and Crop tools, and a whole-canvas rotation is a different decision the
/// user makes once per use.
final class RotateCanvasSheetController: NSViewController {
    private let document: ImageDocument
    private let onApply: (Double, DrResizeFilter) -> Void
    private let originalWidth: Int
    private let originalHeight: Int

    private let angleField = NSTextField(string: "")
    private let directionControl = NSSegmentedControl(
        labels: ["Clockwise", "Counterclockwise"], trackingMode: .selectOne, target: nil,
        action: nil)
    private let samplerPopUp = NSPopUpButton(frame: .zero, pullsDown: false)
    private let sizeLabel = NSTextField(labelWithString: "")

    /// The popup's vocabulary, in the MCP tool's own spelling so the sheet
    /// and a recorded step cannot name the same kernel two ways.
    private static let samplers: [(title: String, name: String, filter: DrResizeFilter)] = [
        ("Nearest", "nearest", DR_FILTER_NEAREST),
        ("Bilinear", "bilinear", DR_FILTER_BILINEAR),
        ("Bicubic", "bicubic", DR_FILTER_CATMULL_ROM),
        ("Lanczos", "lanczos", DR_FILTER_LANCZOS3),
    ]

    init(document: ImageDocument, onApply: @escaping (Double, DrResizeFilter) -> Void) {
        self.document = document
        self.onApply = onApply
        self.originalWidth = document.doc?.width ?? 1
        self.originalHeight = document.doc?.height ?? 1
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("RotateCanvasSheetController does not support NSCoder")
    }

    override func loadView() {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.allowsFloats = true
        formatter.maximumFractionDigits = 4
        angleField.formatter = formatter
        angleField.doubleValue = 0
        angleField.widthAnchor.constraint(equalToConstant: 90).isActive = true
        DSField.style(angleField)
        angleField.identifier = NSUserInterfaceItemIdentifier("rotate-canvas-angle")
        angleField.target = self
        angleField.action = #selector(valueChanged(_:))

        directionControl.selectedSegment = 0
        directionControl.target = self
        directionControl.action = #selector(valueChanged(_:))
        directionControl.identifier = NSUserInterfaceItemIdentifier("rotate-canvas-direction")

        samplerPopUp.addItems(withTitles: Self.samplers.map(\.title))
        samplerPopUp.selectItem(withTitle: "Bicubic")
        samplerPopUp.identifier = NSUserInterfaceItemIdentifier("rotate-canvas-sampler")

        sizeLabel.font = DS.mono(13)
        sizeLabel.textColor = DS.textMuted
        sizeLabel.identifier = NSUserInterfaceItemIdentifier("rotate-canvas-size")

        let currentLabel = fieldLabel("\(originalWidth) × \(originalHeight) px")
        currentLabel.font = DS.mono(13)
        currentLabel.textColor = DS.textMuted

        let grid = NSGridView(views: [
            [fieldLabel("Current size:"), currentLabel],
            [fieldLabel("Angle (°):"), angleField],
            [fieldLabel("Direction:"), directionControl],
            [fieldLabel("Resample:"), samplerPopUp],
            [fieldLabel("New canvas:"), sizeLabel],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("rotate-canvas-apply")
        view = makeSheetView(
            title: "Rotate canvas",
            hint: "Turns the whole picture by any angle and grows the canvas to hold it. "
                + "Every layer, mask and saved channel is resampled; text and shape "
                + "layers are redrawn and stay editable. An exact quarter turn is lossless.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
        refreshSize()
    }

    /// The SIGNED angle the fields currently name. Positive is clockwise, so
    /// Counterclockwise negates the magnitude typed in the field.
    private var signedDegrees: Double {
        let value = angleField.doubleValue
        return directionControl.selectedSegment == 1 ? -value : value
    }

    private var chosenSampler: (name: String, filter: DrResizeFilter) {
        let title = samplerPopUp.titleOfSelectedItem ?? "Bicubic"
        let entry = Self.samplers.first { $0.title == title } ?? Self.samplers[2]
        return (entry.name, entry.filter)
    }

    /// The read-only "New canvas" row, fed by the core plan and by nothing
    /// else — the host computes no rectangle of its own.
    private func refreshSize() {
        guard let plan = RasterDocument.canvasRotationPlan(
            width: originalWidth, height: originalHeight, degrees: signedDegrees)
        else {
            sizeLabel.stringValue = "—"
            return
        }
        guard plan.width > 0, plan.height > 0 else {
            sizeLabel.stringValue = "too large"
            return
        }
        sizeLabel.stringValue = "\(plan.width) × \(plan.height) px"
    }

    @objc private func valueChanged(_ sender: Any?) {
        refreshSize()
    }

    @objc private func applyClicked(_ sender: Any?) {
        let degrees = signedDegrees
        guard degrees.isFinite else {
            NSSound.beep()
            return
        }
        let sampler = chosenSampler.filter
        // Canvas Size's order: dismiss first, then hand the values to the
        // controller, which re-plans against the document's CURRENT handle.
        dismiss(self)
        onApply(degrees, sampler)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
