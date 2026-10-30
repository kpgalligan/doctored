import AppKit

/// File ▸ File Info… — four fields: Author, Copyright Notice, Description
/// and Keywords (`FileInfo.swift`).
///
/// The sheet only edits text. It reads the document's file info when it
/// opens and hands the edited copy to `EditorViewController.commitFileInfo`,
/// which runs the core's setter on the document's current handle; the limits
/// and their sentences are the core's and are presented from there.
final class FileInfoSheetController: NSViewController {
    private weak var editor: EditorViewController?
    /// The fields as the sheet opened: what "changed" is measured against.
    private let shown: FileInfo

    private let authorField = NSTextField()
    private let copyrightField = NSTextField()
    private let keywordsField = NSTextField()
    private let descriptionView = NSTextView(frame: NSRect(x: 0, y: 0, width: 258, height: 96))
    private let descriptionScroll = NSScrollView()

    private static let fieldWidth: CGFloat = 258

    /// nil when there is no document to describe. The caller beeps.
    init?(editor: EditorViewController) {
        guard let doc = editor.document?.doc else { return nil }
        self.editor = editor
        self.shown = doc.fileInfo
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("FileInfoSheetController does not support NSCoder")
    }

    override func loadView() {
        for (field, value, placeholder) in [
            (authorField, shown.creatorText, "Separate names with ;"),
            (copyrightField, shown.copyright, ""),
            (keywordsField, shown.keywordsText, "Separate keywords with , or ;"),
        ] {
            DSField.style(field)
            field.font = DS.sans(13)
            field.stringValue = value
            field.placeholderString = placeholder
            field.lineBreakMode = .byTruncatingTail
            field.cell?.isScrollable = true
            field.widthAnchor.constraint(equalToConstant: Self.fieldWidth).isActive = true
        }

        // Plain text, and no automatic substitution: what is typed is what
        // is stored.
        descriptionView.isEditable = true
        descriptionView.isRichText = false
        descriptionView.isAutomaticQuoteSubstitutionEnabled = false
        descriptionView.isAutomaticDashSubstitutionEnabled = false
        descriptionView.isAutomaticTextReplacementEnabled = false
        descriptionView.allowsUndo = true
        descriptionView.font = DS.sans(13)
        descriptionView.textColor = DS.textStrong
        descriptionView.drawsBackground = true
        descriptionView.backgroundColor = .textBackgroundColor
        descriptionView.textContainerInset = NSSize(width: 2, height: 4)
        descriptionView.isVerticallyResizable = true
        descriptionView.isHorizontallyResizable = false
        descriptionView.autoresizingMask = [.width]
        descriptionView.minSize = NSSize(width: 0, height: 0)
        descriptionView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        descriptionView.textContainer?.widthTracksTextView = true
        descriptionView.string = shown.description

        descriptionScroll.translatesAutoresizingMaskIntoConstraints = false
        descriptionScroll.documentView = descriptionView
        descriptionScroll.hasVerticalScroller = true
        descriptionScroll.autohidesScrollers = true
        descriptionScroll.borderType = .lineBorder
        descriptionScroll.drawsBackground = true
        descriptionScroll.backgroundColor = .textBackgroundColor

        let grid = NSGridView(views: [
            [fieldLabel("Author:"), authorField],
            [fieldLabel("Copyright Notice:"), copyrightField],
            [fieldLabel("Description:"), descriptionScroll],
            [fieldLabel("Keywords:"), keywordsField],
        ])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        grid.row(at: 2).yPlacement = .top

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.title = "OK"
        let row = makeButtonRow(
            cancel: cancelButton, apply: applyButton,
            leading: [sheetFootnote("one undo step")])
        // `makeButtonRow` makes the button the default one, and a default
        // button takes Return ahead of the first responder, so Return could
        // never start a new line in Description
        // (`ActionJSONSheetController`). It answers ⌘Return instead.
        applyButton.keyEquivalentModifierMask = [.command]
        view = makeSheetView(
            title: "File Info",
            hint: "Saved with the document and written into JPEG, PNG and TIFF exports.",
            content: grid, buttonRow: row)
        NSLayoutConstraint.activate([
            descriptionScroll.widthAnchor.constraint(equalToConstant: Self.fieldWidth),
            descriptionScroll.heightAnchor.constraint(equalToConstant: 96),
        ])
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        view.window?.makeFirstResponder(authorField)
    }

    /// The form as a value. A field left exactly as it opened keeps the
    /// value it opened with, so a list the one-line field cannot spell (an
    /// author whose name holds a semicolon) is not rewritten by an edit to
    /// another field.
    ///
    /// Every field goes through `FileInfo.typed` first: a Tab pressed in
    /// Description is a character there, and the core would refuse it after
    /// the sheet had already closed.
    private var edited: FileInfo {
        var info = shown
        if authorField.stringValue != shown.creatorText {
            info.creator = FileInfo.entries(
                FileInfo.typed(authorField.stringValue, lineFeeds: false), separators: ";")
        }
        if copyrightField.stringValue != shown.copyright {
            info.copyright = FileInfo.typed(copyrightField.stringValue, lineFeeds: false)
        }
        if descriptionView.string != shown.description {
            info.description = FileInfo.typed(descriptionView.string, lineFeeds: true)
        }
        if keywordsField.stringValue != shown.keywordsText {
            info.keywords = FileInfo.entries(
                FileInfo.typed(keywordsField.stringValue, lineFeeds: false), separators: ",;")
        }
        return info
    }

    @objc private func applyClicked(_ sender: Any?) {
        let info = edited
        dismiss(self)
        editor?.commitFileInfo(info, shown: shown)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
