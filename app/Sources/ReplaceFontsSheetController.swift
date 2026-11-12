import AppKit

/// Layer ▸ Replace Missing Fonts…: one row per font family this document's text layers name but
/// this Mac lacks, each with a popup of the installed families (the Type options bar's list)
/// after "Don't Replace". Apply replaces each chosen family on every layer that names it as ONE
/// undo step (`RasterDocument.replacingMissingFonts`, MissingFonts.swift); the MCP twin is
/// `replace_missing_fonts`. The sheet captures the document, revision and editing state at
/// creation, as Artwork Colors does, and refuses an Apply after any of them changed.
final class ReplaceFontsSheetController: NSViewController {
    private weak var document: ImageDocument?
    private let source: RasterDocument
    private let state: EditingState?
    private let revision: UInt64
    private let missing: [MissingFontLayer]
    /// The distinct missing families, sorted: one row each.
    private let families: [String]
    /// The popups' choices after "Don't Replace".
    private let installed: [String]
    private let preferred: String
    private var popups: [NSPopUpButton] = []
    private let notice = NSTextField(wrappingLabelWithString: "")

    /// `preferred` is preselected in every row: the Type tool's current family.
    init(document: ImageDocument, preferred: String) throws {
        guard let source = document.doc else {
            throw NativeArtworkEvaluation.failure("Open a document first.")
        }
        let missing = source.missingFontLayers()
        guard !missing.isEmpty else {
            throw NativeArtworkEvaluation.failure("Every text layer's font family is installed.")
        }
        self.document = document
        self.source = source
        self.state = document.captureEditingState()
        self.revision = document.revision
        self.missing = missing
        self.families = Set(missing.map(\.family)).sorted()
        self.installed = TextStyle.installedFamilies().sorted()
        self.preferred = preferred
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ReplaceFontsSheetController does not support NSCoder")
    }

    override func loadView() {
        var rows: [[NSView]] = []
        for (index, family) in families.enumerated() {
            let count = missing.filter { $0.family == family }.count
            let label = fieldLabel("\(family) — \(count) \(count == 1 ? "layer" : "layers")")
            label.lineBreakMode = .byTruncatingTail
            label.toolTip = family
            label.widthAnchor.constraint(lessThanOrEqualToConstant: 170).isActive = true
            let popup = NSPopUpButton(frame: .zero, pullsDown: false)
            popup.addItems(withTitles: ["Don't Replace"] + installed)
            popup.selectItem(at: installed.firstIndex(of: preferred).map { $0 + 1 } ?? 0)
            popup.widthAnchor.constraint(equalToConstant: 200).isActive = true
            popup.identifier = NSUserInterfaceItemIdentifier("replaceFonts.family\(index)")
            popups.append(popup)
            rows.append([label, popup])
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 8
        grid.columnSpacing = 10
        notice.font = DS.sans(11)
        notice.preferredMaxLayoutWidth = 376
        notice.widthAnchor.constraint(equalToConstant: 376).isActive = true
        let stack = NSStackView(views: [grid, notice])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        view = makeSheetView(
            title: "Replace Missing Fonts",
            hint: "These font families are not installed on this Mac, so their layers keep their "
                + "saved pixels and cannot re-render. A replacement re-renders every layer that "
                + "names the family, keeping its text, size, colours and transform.",
            content: stack,
            buttonRow: makeButtonRow(
                cancel: sheetCancelButton(target: self, action: #selector(cancel(_:))),
                apply: sheetApplyButton(target: self, action: #selector(apply(_:)))))
    }

    @objc private func apply(_ sender: Any?) {
        guard let document, document.doc === source, document.revision == revision,
              document.isCurrentEditingState(state) else {
            notice.stringValue =
                "The document changed while this sheet was open. Nothing was replaced."
            return
        }
        var replacements: [String: String] = [:]
        for (family, popup) in zip(families, popups) where popup.indexOfSelectedItem > 0 {
            replacements[family] = installed[popup.indexOfSelectedItem - 1]
        }
        guard !replacements.isEmpty else {
            notice.stringValue = "Choose a family for at least one missing font."
            return
        }
        // Re-rendering rewrites each layer's pixels, so it answers to their locks.
        for row in missing where replacements[row.family] != nil {
            if let refusal = source.lockRefusal(row.layer, kind: DR_EDIT_PIXELS) {
                notice.stringValue =
                    refusal.message + " Unlock it in Layer ▸ Lock to replace its font."
                return
            }
        }
        // Not recordable (designs/replace-missing-fonts.md MF-6): a visible placeholder.
        var failure: Error?
        document.applyEdit("Replace Missing Fonts",
                           record: .unrecorded("Replace Missing Fonts")) { input in
            do { return try input.replacingMissingFonts(replacements) }
            catch { failure = error; return nil }
        }
        if let failure {
            notice.stringValue = Self.sentence(failure, in: source)
            return
        }
        guard document.doc !== source else {
            notice.stringValue = "Nothing was replaced."
            return
        }
        dismiss(self)
    }

    /// `failure`'s sentence; a layer whose TRANSPARENCY lock refused the new layout is named as
    /// the lock (the raster's size and position change, which a frozen alpha cannot follow).
    private static func sentence(_ failure: Error, in doc: RasterDocument) -> String {
        guard let stopped = failure as? MissingFontFailure,
              doc.transparencyLockOwner(stopped.layer) != nil
        else { return failure.localizedDescription }
        let name = doc.layerInfo(stopped.layer)?.name ?? "This layer"
        return "“\(name)” has its transparency locked, and re-rendering lays the layer out again, "
            + "so its raster changes size and position. Unlock Transparency in Layer ▸ Lock to "
            + "replace its font."
    }

    @objc private func cancel(_ sender: Any?) { dismiss(self) }
}
