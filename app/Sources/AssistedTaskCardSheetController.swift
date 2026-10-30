import AppKit

/// The gate in front of a human assisted annotation session: the task card as
/// the workspace hashed it, the annotator's own id, the prior-exposure
/// declaration and the source-only attestation.
///
/// Begin is what starts the clock, and nothing behind this sheet is visible
/// until it is pressed — the source is handed to the canvas only after
/// `AssistedAnnotationSession.begin()` returns.
final class AssistedTaskCardSheetController: NSViewController {
    private let taskCard: String
    private let caseID: String
    private let annotatorField: NSTextField
    private let exposure = NSButton(checkboxWithTitle: "", target: nil, action: nil)
    private let attested = NSButton(checkboxWithTitle: "", target: nil, action: nil)
    private let status = NSTextField(wrappingLabelWithString: "")
    private let onBegin: (String, Bool, AssistedAttestation) -> Void
    private let onCancel: () -> Void
    private var beginButton: NSButton?

    static let attestationText =
        "I annotated this case from the displayed photograph and starting mask alone. "
        + "I consulted no reference mask, candidate result, comparison panel or score."

    init(taskCard: String, annotatorID: String, caseID: String,
         onBegin: @escaping (String, Bool, AssistedAttestation) -> Void,
         onCancel: @escaping () -> Void) {
        self.taskCard = taskCard
        self.caseID = caseID
        self.onBegin = onBegin
        self.onCancel = onCancel
        annotatorField = NSTextField(string: annotatorID)
        annotatorField.identifier = NSUserInterfaceItemIdentifier("assisted.annotator")
        annotatorField.widthAnchor.constraint(equalToConstant: 260).isActive = true
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AssistedTaskCardSheetController does not support NSCoder")
    }

    override func loadView() {
        let card = NSTextView()
        card.string = taskCard
        card.isEditable = false
        card.font = DS.sans(12)
        card.textColor = DS.textStrong
        card.drawsBackground = false
        let scroll = NSScrollView()
        scroll.documentView = card
        scroll.hasVerticalScroller = true
        scroll.borderType = .lineBorder
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.widthAnchor.constraint(equalToConstant: 592).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 260).isActive = true

        exposure.title = "I have already seen a reference mask, result or score for some case"
        exposure.font = DS.sans(12)
        attested.title = Self.attestationText
        attested.font = DS.sans(12)
        attested.target = self
        attested.action = #selector(attestationChanged(_:))
        if let cell = attested.cell as? NSButtonCell { cell.lineBreakMode = .byWordWrapping }
        attested.identifier = NSUserInterfaceItemIdentifier("assisted.attestation")
        status.textColor = .systemRed
        status.preferredMaxLayoutWidth = 592
        let grid = NSGridView(views: [[fieldLabel("Annotator id:"), annotatorField]])
        grid.columnSpacing = 12
        let content = NSStackView(views: [scroll, grid, exposure, attested, status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let begin = StickerButton(
            title: "Begin", style: .primary, target: self, action: #selector(beginClicked(_:)))
        begin.identifier = NSUserInterfaceItemIdentifier("assisted.begin")
        begin.isEnabled = false
        beginButton = begin
        view = makeSheetView(
            title: "Assisted edge annotation — case \(caseID)",
            hint: "5 minutes and 20 modifying gestures, one attempt. The timer starts when you "
                + "press Begin, and the photograph is not shown until then.",
            content: content,
            buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
                apply: begin),
            width: 640)
    }

    @objc func attestationChanged(_ sender: Any?) {
        beginButton?.isEnabled = attested.state == .on
    }

    @objc func beginClicked(_ sender: Any?) {
        let identifier = annotatorField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !identifier.isEmpty else {
            status.stringValue = "Record the annotator id before beginning."
            return
        }
        guard attested.state == .on else {
            status.stringValue = "The source-only attestation is required."
            return
        }
        let attestation = AssistedAttestation(
            sourceOnly: true, text: Self.attestationText,
            attestedAtUTC: AssistedAnnotation.timestamp())
        dismiss(self)
        onBegin(identifier, exposure.state == .on, attestation)
    }

    @objc func cancelClicked(_ sender: Any?) {
        dismiss(self)
        onCancel()
    }
}
