import AppKit

// The RAW, filter, export and metadata managers and the New Document card read `listing()` and show
// each bad file as "file — reason" with Delete File (Repair on RAW only), hidden when there is none
// (`update` answers whether that flipped, so the owner calls `ModalCardWindowController.refit()`
// only then).

/// The "files that are not presets" rows a preference-library manager shows
/// under its list: one popup naming each broken file and
/// why, a Delete File button and — for the RAW library only, the one with a
/// field-by-field recovery — a Repair button.
///
/// Lifted out of `RawPresetManagerSheet` so the four manager sheets and the
/// New Document card list a bad file the same way instead of hiding it behind
/// a refusal of the whole library. The section owns no library: the owner
/// hands in the delete (and repair) call, the `changed` callback that reloads
/// its own listing, and the `report` callback that shows a refusal in its own
/// message line. Library writes are preferences, so neither button makes an
/// Undo or Action step.
final class BrokenLibraryFilesSection: NSObject {
    let view: NSStackView
    private let popup = NSPopUpButton()
    private let deleteButton: NSButton
    private let repairButton: NSButton?
    private let delete: (String) throws -> Void
    private let repair: ((String) throws -> Void)?
    private let changed: () -> Void
    private let report: (String) -> Void
    private var broken: [PreferenceLibraryBroken] = []

    /// `repair` nil means the library has no repair, and no Repair button is
    /// built at all.
    init(title: String, identifierPrefix: String, delete: @escaping (String) throws -> Void,
         repair: ((String) throws -> Void)?,
         changed: @escaping () -> Void, report: @escaping (String) -> Void) {
        self.delete = delete
        self.repair = repair
        self.changed = changed
        self.report = report
        popup.widthAnchor.constraint(equalToConstant: 330).isActive = true
        popup.setAccessibilityIdentifier(identifierPrefix + "-broken-list")
        deleteButton = StickerButton(title: "Delete File", style: .secondary, target: nil, action: nil)
        deleteButton.setAccessibilityIdentifier(identifierPrefix + "-delete-file")
        if repair != nil {
            let button = StickerButton(title: "Repair", style: .secondary, target: nil, action: nil)
            button.setAccessibilityIdentifier(identifierPrefix + "-repair")
            repairButton = button
        } else {
            repairButton = nil
        }
        let buttons = NSStackView(views: [repairButton, deleteButton].compactMap { $0 })
        buttons.spacing = 8
        view = NSStackView(views: [fieldLabel(title), popup, buttons])
        view.orientation = .vertical
        view.alignment = .leading
        view.spacing = 8
        view.isHidden = true
        super.init()
        popup.target = self
        popup.action = #selector(selectionChanged(_:))
        deleteButton.target = self
        deleteButton.action = #selector(deleteClicked(_:))
        repairButton?.target = self
        repairButton?.action = #selector(repairClicked(_:))
        update([])
    }

    /// Each row reads "file — reason". Delete File is enabled when there are
    /// rows, Repair when the selected row is repairable. The section hides
    /// when the library has no broken file; the answer is true when that
    /// visibility FLIPPED, so the owner resizes its card only then.
    @discardableResult
    func update(_ broken: [PreferenceLibraryBroken]) -> Bool {
        self.broken = broken
        popup.removeAllItems()
        popup.addItems(withTitles: broken.map { "\($0.file) — \($0.reason)" })
        popup.isEnabled = !broken.isEmpty
        deleteButton.isEnabled = !broken.isEmpty
        repairButton?.isEnabled = selected?.repairable ?? false
        let wasHidden = view.isHidden
        view.isHidden = broken.isEmpty
        return wasHidden != view.isHidden
    }

    private var selected: PreferenceLibraryBroken? {
        broken.indices.contains(popup.indexOfSelectedItem) ? broken[popup.indexOfSelectedItem] : nil
    }

    @objc private func selectionChanged(_ sender: Any?) {
        repairButton?.isEnabled = selected?.repairable ?? false
    }

    @objc private func deleteClicked(_ sender: Any?) {
        perform(delete)
    }

    @objc private func repairClicked(_ sender: Any?) {
        guard let repair else { return }
        perform(repair)
    }

    private func perform(_ body: (String) throws -> Void) {
        guard let entry = selected else { return }
        do {
            try body(entry.file)
            changed()
        } catch {
            report(error.localizedDescription)
            NSSound.beep()
        }
    }
}
