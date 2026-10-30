import AppKit

/// Image ▸ Trim… (CanvasTrim.swift): what to trim away — transparent
/// pixels, or the colour of the top-left or the bottom-right pixel — and
/// which of the four edges may move. Apply hands both to `onApply`
/// (`EditorViewController.commitTrim`), which crops the canvas or beeps when
/// there is nothing to trim.
final class TrimSheetController: NSViewController {
    private let onApply: (TrimBasis, TrimEdges) -> Void
    private let basisButtons: [NSButton] = TrimBasis.allCases.map {
        let button = NSButton(radioButtonWithTitle: $0.title, target: nil, action: nil)
        button.tag = $0.rawValue
        return button
    }
    private let edgeButtons: [(TrimEdges, NSButton)] = [
        (TrimEdges.top, "Top"), (TrimEdges.bottom, "Bottom"),
        (TrimEdges.left, "Left"), (TrimEdges.right, "Right"),
    ].map { ($0.0, NSButton(checkboxWithTitle: $0.1, target: nil, action: nil)) }

    init(onApply: @escaping (TrimBasis, TrimEdges) -> Void) {
        self.onApply = onApply
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("TrimSheetController does not support NSCoder")
    }

    override func loadView() {
        for button in basisButtons {
            // One shared action is what makes sibling radio buttons exclusive.
            button.target = self
            button.action = #selector(basisChanged(_:))
            button.font = DS.sans(13)
            button.identifier = NSUserInterfaceItemIdentifier("trim-basis-\(button.tag)")
        }
        basisButtons.first?.state = .on
        for (edge, button) in edgeButtons {
            button.state = .on
            button.font = DS.sans(13)
            button.identifier = NSUserInterfaceItemIdentifier("trim-edge-\(edge.rawValue)")
        }
        let basisStack = NSStackView(views: basisButtons)
        basisStack.orientation = .vertical
        basisStack.alignment = .leading
        basisStack.spacing = 6
        let edgeStack = NSStackView(views: edgeButtons.map { $0.1 })
        edgeStack.orientation = .horizontal
        edgeStack.spacing = 14

        let grid = NSGridView(views: [
            [fieldLabel("Based on:"), basisStack],
            [fieldLabel("Trim away:"), edgeStack],
        ])
        grid.rowSpacing = 14
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        grid.cell(for: basisStack)?.yPlacement = .top
        grid.cell(atColumnIndex: 0, rowIndex: 0).yPlacement = .top

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("trim-apply")
        view = makeSheetView(
            title: "Trim",
            hint: "Crops the canvas inward past the rows and columns that are transparent, "
                + "or that match a corner pixel exactly. Layers keep their pixels.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: applyButton))
    }

    @objc private func basisChanged(_ sender: Any?) {}

    @objc private func applyClicked(_ sender: Any?) {
        let basis = basisButtons.first { $0.state == .on }
            .flatMap { TrimBasis(rawValue: $0.tag) } ?? .transparent
        var edges: TrimEdges = []
        for (edge, button) in edgeButtons where button.state == .on { edges.insert(edge) }
        dismiss(self)
        onApply(basis, edges)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
