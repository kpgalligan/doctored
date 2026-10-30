import AppKit

/// Select ▸ Modify's titles for the morphology operations. The
/// operation strings (`grow`/`shrink`) stay what saved Actions persist; only
/// the titles a user reads change.
enum SelectionMorph {
    static func displayName(for operation: String) -> String {
        switch operation {
        case "grow": return "Expand"
        case "shrink": return "Contract"
        default: return operation.capitalized
        }
    }
}

/// The native feather/morphology sheet captures its selection and target before
/// showing controls; Apply cannot accidentally modify a newer selection.
final class NativeSelectionMorphSheetController: NSViewController {
    private weak var document: ImageDocument?
    private let capture: SelectionCapture
    private let operation: String
    private let amount: NSTextField
    private let status = NSTextField(wrappingLabelWithString: "")

    init(document: ImageDocument, operation: String) throws {
        self.document = document
        capture = try SelectionCapture(document: document, needsSource: false)
        guard capture.selection.selection != nil else { throw DoctoredCoreError(message: "Make a selection first.") }
        self.operation = operation
        amount = NSTextField(string: "4")
        amount.identifier = NSUserInterfaceItemIdentifier("nativeSelection.radius")
        amount.widthAnchor.constraint(equalToConstant: 100).isActive = true
        super.init(nibName: nil, bundle: nil)
    }
    @available(*, unavailable) required init?(coder: NSCoder) { fatalError() }

    override func loadView() {
        let grid = NSGridView(views: [[fieldLabel(operation == "border" ? "Width (px):" : "Radius (px):"), amount]])
        grid.columnSpacing = 12
        status.textColor = .systemRed
        status.preferredMaxLayoutWidth = 370
        let content = NSStackView(views: [grid, status]); content.orientation = .vertical; content.alignment = .leading
        view = makeSheetView(title: "\(SelectionMorph.displayName(for: operation)) Selection", hint: "Native coverage remains at 65,536 levels. Layer pixels stay unchanged.",
            content: content, buttonRow: makeButtonRow(cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: sheetApplyButton(target: self, action: #selector(applyClicked(_:)))))
    }
    @objc func applyClicked(_ sender: Any?) {
        guard let document else { return }
        do {
            guard let radius = Double(amount.stringValue), radius.isFinite, (0...250).contains(radius),
                  let selection = capture.selection.selection else { throw DoctoredCoreError(message: "Use a finite radius from 0 to 250 pixels.") }
            try capture.requireCurrent(in: document)
            let result = try selection.nativeModified(operation, radius: radius)
            try capture.requireCurrent(in: document)
            let record: [ActionStep] = operation == "border" ? .modifySelection(operation: operation, width: radius) : .modifySelection(operation: operation, radius: radius)
            document.setCanvasSelection(result, actionName: "\(SelectionMorph.displayName(for: operation)) Selection", record: record)
            dismiss(self)
        } catch { status.stringValue = error.localizedDescription }
    }
    @objc func cancelClicked(_ sender: Any?) { dismiss(self) }
}

extension EditorViewController {
    func presentNativeSelectionMorph(_ operation: String) -> Bool {
        guard let document, document.doc?.isHighDepth == true || document.canvasSelection?.hasNativeCoverage == true else { return false }
        do { presentAsSheet(try NativeSelectionMorphSheetController(document: document, operation: operation)) }
        catch { document.presentError(error) }
        return true
    }

    func nativeWandClicked(_ point: CGPoint, mode: SelectionCombineMode) -> Bool {
        guard let document, document.doc?.isHighDepth == true else { return false }
        let options = ToolOptionsStore.shared.select
        do {
            let capture = try SelectionCapture(document: document, needsSource: false)
            let tolerance = Int((max(0, min(255, options.tolerance)) * 257).rounded())
            // Sample — All layers reads the composite, Current layer the
            // active record alone.
            let scope: HistogramSource = options.wandSampleAllLayers
                ? .composite : .layer(document.activeLayerIndex)
            let samples = try capture.nativeSource(["kind": "magic_wand", "x": Int(point.x), "y": Int(point.y),
                "tolerance": tolerance, "contiguous": options.contiguous,
                "sample_size": options.wandSampleSize], in: document, title: "Magic Wand",
                scope: scope)
            var arguments: [String: Any] = ["x": Int(point.x), "y": Int(point.y), "tolerance": options.tolerance,
                "contiguous": options.contiguous, "mode": mode.agentName, "feather": options.feather]
            if !options.wandSampleAllLayers { arguments["sample_all_layers"] = false }
            // The Sample Size, recorded only off Point Sample.
            if options.wandSampleSize != 1 { arguments["sample_size"] = options.wandSampleSize }
            try capture.commit(.mask16(samples), in: document, name: "Magic Wand", mode: mode, feather: options.feather,
                record: .step("select_magic_wand", arguments, note: "Magic Wand tool"))
        } catch { document.presentError(error) }
        return true
    }
}
