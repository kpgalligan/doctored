import AppKit

/// Select ▸ Person Parts…: the parts to select (Hair, Facial Skin, Body Skin,
/// Clothes), on All People or one person Vision separated, with Mode and Feather. The people
/// analysis runs once when the sheet opens and is handed to `applyPersonParts`; a crowd Vision
/// cannot separate offers All People only. No preview: the model runs once, on Apply.
final class PersonPartsSheetController: NSViewController {
    private weak var editor: EditorViewController?
    let capture: SelectionCapture
    /// Nil when Vision could not separate the people (the crowd fallback reads them at once).
    let analysis: SubjectAnalysis?
    private var partBoxes: [PersonPart: NSButton] = [:]
    private let peoplePopup = NSPopUpButton()
    private let modePopup = NSPopUpButton()
    private let featherField = NSTextField(string: "0")
    private var applyButton: NSButton!

    init(editor: EditorViewController, document: ImageDocument) throws {
        self.editor = editor
        capture = try SelectionCapture(document: document)
        do {
            analysis = try SubjectTargeting.analyze(capture, model: .people, in: document)
        } catch let error as DoctoredCoreError where error.message == PersonInstances.crowdMessage {
            analysis = nil
        }
        super.init(nibName: nil, bundle: nil)
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("PersonPartsSheetController does not support NSCoder") }

    override func loadView() {
        for (index, part) in PersonPart.allCases.enumerated() {
            let box = NSButton(checkboxWithTitle: part.title, target: self, action: #selector(partClicked(_:)))
            box.tag = index
            box.font = DS.sans(13)
            box.state = part == .hair ? .on : .off
            box.identifier = NSUserInterfaceItemIdentifier("person-parts-\(part.rawValue)")
            partBoxes[part] = box
        }
        let parts = NSStackView(views: PersonPart.allCases.compactMap { partBoxes[$0] })
        parts.orientation = .horizontal
        parts.spacing = 10
        let count = analysis?.instanceCount ?? 0
        peoplePopup.addItems(withTitles: ["All People"] + (0..<count).map { "Person \($0 + 1)" })
        peoplePopup.identifier = NSUserInterfaceItemIdentifier("person-parts-people")
        modePopup.addItems(withTitles: ["Replace", "Add", "Subtract", "Intersect"])
        modePopup.identifier = NSUserInterfaceItemIdentifier("person-parts-mode")
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.minimumFractionDigits = 0
        formatter.maximumFractionDigits = 1
        featherField.formatter = formatter
        featherField.doubleValue = 0
        featherField.identifier = NSUserInterfaceItemIdentifier("person-parts-feather")
        featherField.widthAnchor.constraint(equalToConstant: 80).isActive = true
        DSField.style(featherField)
        let unit = NSTextField(labelWithString: "px")
        unit.font = DS.mono(13)
        unit.textColor = DS.textMuted
        let people = NSStackView(views: [fieldLabel("People:"), peoplePopup])
        people.orientation = .horizontal
        people.spacing = 8
        let options = NSStackView(views: [fieldLabel("Feather:"), featherField, unit, modePopup])
        options.orientation = .horizontal
        options.spacing = 8
        let content = NSStackView(views: [parts, people, options])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        let hint = count == 0
            ? "Vision could not tell these people apart, so the parts are read on everyone at once."
            : "People are numbered left to right, as Select ▸ Select People… numbers them."
        view = makeSheetView(title: "Person Parts", hint: hint, content: content, buttonRow: makeButtonRow(
            cancel: sheetCancelButton(target: self, action: #selector(cancelClicked(_:))),
            apply: applyButton, leading: [sheetFootnote("one undo step")]), width: 470)
    }

    var selectedParts: [PersonPart] {
        PersonPart.allCases.filter { partBoxes[$0]?.state == .on }
    }

    /// The chosen person (1-based), nil for All People.
    var selectedPerson: Int? {
        peoplePopup.indexOfSelectedItem > 0 ? peoplePopup.indexOfSelectedItem : nil
    }

    @objc private func partClicked(_ sender: NSButton) {
        applyButton.isEnabled = !selectedParts.isEmpty
    }

    @objc func applyClicked(_ sender: Any?) {
        let parts = selectedParts
        // An empty field reads as 0, which is the field's own default.
        let feather = featherField.doubleValue
        guard !parts.isEmpty, feather.isFinite, (0...250).contains(feather) else {
            NSSound.beep()
            return
        }
        let mode: SelectionCombineMode = [.replace, .add, .subtract, .intersect][max(0, modePopup.indexOfSelectedItem)]
        let person = selectedPerson
        dismiss(self)
        editor?.applyPersonParts(capture, analysis: analysis, parts: parts, person: person, mode: mode,
                                 feather: feather)
    }
    @objc func cancelClicked(_ sender: Any?) { dismiss(self) }
}
