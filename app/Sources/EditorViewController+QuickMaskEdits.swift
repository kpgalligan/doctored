import AppKit

/// Filters ▸ Blur… and the Gradient tool in Quick Mask (QuickMaskEdits
/// .swift): both edit the Quick Mask coverage instead of refusing. Each is
/// one file-clean step, recorded as the tool call that mirrors it
/// (`apply_filter filter: "blur"` and `gradient`, each with `target:
/// "quick_mask"`, AgentServer+QuickMaskEdits.swift).
extension EditorViewController {
    /// Filters ▸ Blur… while Quick Mask is on: the sigma sheet over the
    /// Quick Mask coverage. False outside Quick Mask, so `showBlur` carries
    /// on to the layer's blur.
    func showQuickMaskBlur() -> Bool {
        guard let document = document, document.quickMaskActive else { return false }
        guard document.canvasSelection != nil, !canvas.isPaintingStroke else {
            NSSound.beep()
            return true
        }
        let expected = document.selectionHistory
        presentAsSheet(QuickMaskBlurSheetController { [weak self] sigma in
            self?.applyQuickMaskBlur(sigma: sigma, expected: expected)
        })
        return true
    }

    /// The sheet's Apply. `expected` is the Quick Mask state the sheet opened
    /// over; a different one refuses by name, as the Quick Mask fill does.
    private func applyQuickMaskBlur(sigma: Double, expected: SelectionHistoryState) {
        guard let document = document else { return }
        guard document.quickMaskActive, expected == document.selectionHistory else {
            document.presentError(DoctoredCoreError(message: FillSettings.quickMaskChanged))
            return
        }
        do {
            let changed = try document.applyQuickMaskBlur(
                sigma: sigma,
                record: .filter("blur", ["sigma": ActionArgs.number(sigma)], target: "quick_mask")
                    .map { step in
                        // A Quick Mask edit names no layer.
                        var step = step
                        step.arguments.removeValue(forKey: "layer")
                        return step
                    })
            if !changed { NSSound.beep() }
        } catch {
            document.presentError(error)
        }
    }

    /// The Gradient tool's drag while Quick Mask is on: the bar's ramp laid
    /// over the Quick Mask coverage. False outside Quick Mask, so
    /// `gradientCommitted` carries on to the document's gradient.
    func gradientQuickMask(from a: CGPoint, to b: CGPoint) -> Bool {
        guard let document = document, document.quickMaskActive else { return false }
        guard !canvas.isPaintingStroke else {
            NSSound.beep()
            return true
        }
        let ramp = gradientToolRamp
        var record: [ActionStep] = .gradientStops(from: a, to: b, ramp: ramp, target: "quick_mask")
        for index in record.indices { record[index].arguments.removeValue(forKey: "layer") }
        do {
            let changed = try document.applyQuickMaskGradient(ramp, from: a, to: b, record: record)
            if !changed { NSSound.beep() }
        } catch {
            document.presentError(error)
        }
        return true
    }

    /// Filters ▸ Blur… in Quick Mask: live when there is coverage to blur.
    /// nil outside Quick Mask and for every other item.
    func validateQuickMaskEditItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard item.action == #selector(showBlur(_:)), let document = document,
              document.quickMaskActive
        else { return nil }
        return document.canvasSelection != nil
    }
}

/// The Quick Mask blur's sheet: one sigma, in pixels.
final class QuickMaskBlurSheetController: NSViewController {
    private let onApply: (Double) -> Void
    private let sigmaField = NSTextField(string: "4")

    init(onApply: @escaping (Double) -> Void) {
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("QuickMaskBlurSheetController does not support NSCoder")
    }

    override func loadView() {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.maximumFractionDigits = 2
        formatter.minimum = NSNumber(value: QuickMaskBlur.sigmaRange.lowerBound)
        formatter.maximum = NSNumber(value: QuickMaskBlur.sigmaRange.upperBound)
        sigmaField.formatter = formatter
        sigmaField.doubleValue = 4
        sigmaField.alignment = .right
        sigmaField.identifier = NSUserInterfaceItemIdentifier("quick-mask-blur-sigma")
        DSField.style(sigmaField)
        sigmaField.widthAnchor.constraint(equalToConstant: 72).isActive = true
        let unit = NSTextField(labelWithString: "px")
        unit.font = DS.sans(13)
        unit.textColor = DS.textMuted
        let row = NSStackView(views: [sigmaField, unit])
        row.orientation = .horizontal
        row.alignment = .centerY
        row.spacing = 6
        let grid = NSGridView(views: [[fieldLabel("Sigma:"), row]])
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("quick-mask-blur-apply")
        view = makeSheetView(
            title: "Gaussian Blur",
            hint: "Softens the Quick Mask coverage, 0.65 to 75.5 pixels. The selection takes "
                + "the softened edge when you leave Quick Mask.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    @objc private func applyClicked(_ sender: Any?) {
        view.window?.makeFirstResponder(nil)
        let sigma = sigmaField.doubleValue
        guard QuickMaskBlur.sigmaRange.contains(sigma) else {
            NSSound.beep()
            return
        }
        dismiss(self)
        onApply(sigma)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
