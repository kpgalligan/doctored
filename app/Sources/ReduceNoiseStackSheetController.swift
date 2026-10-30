import AppKit

/// Layer ▸ Reduce Noise from Stack… (StackMode.swift): which statistic
/// to take across the aligned layers — Median or Mean. Apply hands it to
/// `onApply` (`EditorViewController.commitReduceNoise`).
final class ReduceNoiseStackSheetController: NSViewController {
    private let onApply: (StackMode) -> Void
    /// Median first: it is the preselected choice.
    private let modes: [StackMode] = [.median, .mean]
    private lazy var modeButtons: [NSButton] = modes.enumerated().map { position, mode in
        let button = NSButton(radioButtonWithTitle: mode.title, target: nil, action: nil)
        button.tag = position
        return button
    }

    init(onApply: @escaping (StackMode) -> Void) {
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ReduceNoiseStackSheetController does not support NSCoder")
    }

    override func loadView() {
        for button in modeButtons {
            // One shared action is what makes sibling radio buttons exclusive.
            button.target = self
            button.action = #selector(modeChanged(_:))
            button.font = DS.sans(13)
            let name = modes.indices.contains(button.tag) ? modes[button.tag].rawValue : "mode"
            button.identifier = NSUserInterfaceItemIdentifier("reduce-noise-\(name)")
        }
        modeButtons.first?.state = .on
        let modeStack = NSStackView(views: modeButtons)
        modeStack.orientation = .vertical
        modeStack.alignment = .leading
        modeStack.spacing = 6

        let grid = NSGridView(views: [[fieldLabel("Combine with:"), modeStack]])
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        grid.cell(for: modeStack)?.yPlacement = .top
        grid.cell(atColumnIndex: 0, rowIndex: 0).yPlacement = .top

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("reduce-noise-apply")
        view = makeSheetView(
            title: StackModes.noiseTitle,
            hint: "Median rejects what moves between frames; Mean averages noise best.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    @objc private func modeChanged(_ sender: Any?) {}

    @objc private func applyClicked(_ sender: Any?) {
        let chosen = modeButtons.first { $0.state == .on }?.tag ?? 0
        let mode = modes.indices.contains(chosen) ? modes[chosen] : .median
        dismiss(self)
        onApply(mode)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
