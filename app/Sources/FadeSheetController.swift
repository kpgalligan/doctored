import AppKit

/// Edit ▸ Fade… (v1): the last step's result blended back with the state
/// before it, at an Opacity (0–100, default 100) and in a Mode (the app's
/// blend modes; a 32-bit float document lists only the modes it admits),
/// previewed live on the canvas.
///
/// The controls are Apply Image's (`ChannelMathControls`). The candidate and
/// its selection's coverage are captured when the sheet opens
/// (`FadeInputs`), so the preview runs on `PreviewRenderer`'s queue without
/// reaching into the document; a typed document's preview runs under its own
/// scratch reservation, as Apply Image's typed preview does. Apply computes
/// the result on the document again and commits it as one step named
/// "Fade <step>" in place of the step it fades (`ImageDocument.commitFade`);
/// an Apply that changes nothing (Normal at 100 %) closes with no step.
final class FadeSheetController: NSViewController {
    private weak var editor: EditorViewController?
    private weak var canvas: ImageCanvasView?
    private let candidate: FadeCandidate
    private let inputs: FadeInputs

    private let blendPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let opacitySlider = NSSlider(
        value: 100, minValue: 0, maxValue: 100, target: nil, action: nil)
    private let opacityField = NSTextField(string: "100")
    private let status = NSTextField(wrappingLabelWithString: "")
    private let renderer = PreviewRenderer()
    private var previewControl: RasterJobControl?

    init(editor: EditorViewController, candidate: FadeCandidate) {
        self.editor = editor
        self.canvas = editor.canvas
        self.candidate = candidate
        self.inputs = FadeInputs(candidate, selection: candidate.selection.selection)
        super.init(nibName: nil, bundle: nil)
        renderer.onRender = { [weak self] image in self?.canvas?.previewImage = image }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FadeSheetController does not support NSCoder")
    }

    override func loadView() {
        fillModes()
        blendPopup.target = self
        blendPopup.action = #selector(controlChanged(_:))
        blendPopup.identifier = NSUserInterfaceItemIdentifier("fade.mode")
        opacitySlider.target = self
        opacitySlider.action = #selector(sliderChanged(_:))
        opacitySlider.identifier = NSUserInterfaceItemIdentifier("fade.opacity")
        opacityField.target = self
        opacityField.action = #selector(fieldChanged(_:))
        opacityField.identifier = NSUserInterfaceItemIdentifier("fade.opacity.field")
        let opacityRow = ChannelMathControls.opacityRow(slider: opacitySlider, field: opacityField)

        let grid = NSGridView(views: [
            [fieldLabel("Opacity:"), opacityRow],
            [fieldLabel("Mode:"), blendPopup],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        status.textColor = .systemRed
        status.font = DS.sans(11)
        status.preferredMaxLayoutWidth = 350
        status.maximumNumberOfLines = 3
        let content = NSStackView(views: [grid, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 6

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        let hint = "Blends the result of \(candidate.name) with the image before it. "
            + (candidate.kind == .region
                ? "The mode applies where the step ran."
                : "The mode applies where the stroke changed the layer.")
        view = makeSheetView(
            title: "Fade \(candidate.name)", hint: hint, content: content,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    /// Apply Image's blend popup; a float document keeps only the modes it
    /// admits (`NativeLayerBlending.floatModes`), a Grayscale one only its
    /// own (`grayModes(for:)`, #1 phase 2c), a Lab Color one only
    /// `labModes` (#1 Lab phase 2c), and drops emptied groups.
    private func fillModes() {
        ChannelMathControls.fillBlendModes(blendPopup, singlePlane: false)
        let after = candidate.after
        let kept: [DrBlendMode]? = after.isLab ? NativeLayerBlending.labModes
            : after.isGray ? NativeLayerBlending.grayModes(for: after)
            : after.isFloat ? NativeLayerBlending.floatModes : nil
        guard let kept, let menu = blendPopup.menu else { return }
        for item in menu.items.reversed() where !item.isSeparatorItem {
            let admitted = kept.contains { Int($0.rawValue) == item.tag }
            if !admitted { menu.removeItem(item) }
        }
        var previousWasSeparator = true
        for item in menu.items {
            if item.isSeparatorItem, previousWasSeparator { menu.removeItem(item); continue }
            previousWasSeparator = item.isSeparatorItem
        }
        if let last = menu.items.last, last.isSeparatorItem { menu.removeItem(last) }
        blendPopup.selectItem(withTag: Int(DR_BLEND_NORMAL.rawValue))
    }

    private var mode: DrBlendMode { ChannelMathControls.blendMode(blendPopup) }
    private var opacity: Double { ChannelMathControls.opacity(opacitySlider) }

    // MARK: - Preview

    /// The canvas shows the faded result; nil (the document as it is) when
    /// the fade changes nothing.
    private func requestPreview() {
        status.stringValue = ""
        let inputs = self.inputs, mode = self.mode, opacity = self.opacity
        let after = candidate.after
        previewControl?.cancel()
        previewControl = nil
        guard after.isHighDepth else {
            renderer.request {
                guard let result = try? inputs.result(mode: mode, opacity: opacity, control: nil)
                else { return nil }
                return result.flattened()?.makeCGImage(in: result.colorSpace)
            }
            return
        }
        do {
            let scratch = try inputs.scratch()
            let reservation = try SelectionWorkBudget.reservePreview(bytes: scratch, outputBytes: 0)
            previewControl = reservation.control
            renderer.cancelCompute = { reservation.control.cancel() }
            let layer = candidate.layer
            renderer.request {
                reservation.run { token in
                    guard let result = try? inputs.result(
                        mode: mode, opacity: opacity, control: token, ownReservation: scratch),
                        !token.isCancelled else { return nil }
                    return try? PrecisionScalarOperations.preview(
                        result, base: .composite, layer: layer, ownReservation: scratch)
                }
            }
        } catch {
            canvas?.previewImage = nil
            status.stringValue = error.localizedDescription
        }
    }

    // MARK: - Actions

    @objc private func controlChanged(_ sender: Any?) {
        requestPreview()
    }

    @objc private func sliderChanged(_ sender: Any?) {
        ChannelMathControls.syncField(opacityField, to: opacitySlider)
        requestPreview()
    }

    @objc private func fieldChanged(_ sender: Any?) {
        ChannelMathControls.syncSlider(opacitySlider, to: opacityField)
        requestPreview()
    }

    private func closePreview() {
        previewControl?.cancel()
        previewControl = nil
        renderer.cancel()
        canvas?.previewImage = nil
    }

    @objc private func applyClicked(_ sender: Any?) {
        let mode = self.mode, opacity = self.opacity
        closePreview()
        dismiss(self)
        guard let document = editor?.document,
              document.currentFadeCandidate?.after === candidate.after else {
            NSSound.beep()
            return
        }
        let record: [ActionStep] = .step(
            "fade",
            ["opacity": ActionArgs.number(opacity), "mode": DrBlendMode.displayName(for: mode)],
            note: "Edit ▸ Fade…")
        do {
            // nil: the fade changes nothing, so there is no step to make.
            guard let result = try document.fadedDocument(
                candidate, mode: mode, opacity: opacity, control: nil) else { return }
            document.commitFade(candidate, result: result, record: record)
        } catch is JobCancelled {
            return
        } catch {
            document.presentError(error)
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        closePreview()
        dismiss(self)
    }
}
