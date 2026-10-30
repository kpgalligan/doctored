import AppKit

// The Working Memory dialog is a `ModalCardWindowController` card; MCP reads the policy read-only
// through `resource_usage`'s `working_limit` (the app-wide preference exemption).

/// Doctored ▸ Working Memory… — the working-limit preference
/// (`WorkingLimit`). A `ModalCardWindowController` for the Shortcuts
/// sheet's reason: it has to work at the Welcome window, before any
/// document exists. OK stores the choice (Automatic removes the key);
/// Cancel changes nothing. The stored value takes effect at the next launch.
final class WorkingLimitSheetController: NSViewController {
    private let limit = WorkingLimit.current
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var modal: ModalCardWindowController?

    init() {
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("WorkingLimitSheetController does not support NSCoder")
    }

    static func run() {
        WorkingLimitSheetController().present()
    }

    private static func text(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .memory)
    }

    override func loadView() {
        let ram = limit.physicalMemoryBytes
        let summary = NSTextField(wrappingLabelWithString:
            "This Mac has \(Self.text(ram)) of memory. "
            + "Working limit now: \(Self.text(limit.effectiveBytes)) (\(limit.source.label)).")
        summary.font = DS.sans(13)
        summary.textColor = DS.textStrong
        summary.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2

        popup.addItem(withTitle: "Automatic (\(Self.text(limit.automaticBytes)))")
        var choices = WorkingLimit.preferenceChoices(physicalMemory: ram)
        let stored = WorkingLimit.storedPreference(.standard).map {
            WorkingLimit.clamped($0, physicalMemory: ram)
        }
        if let stored, !choices.contains(stored) {
            choices.append(stored)
            choices.sort()
        }
        for bytes in choices {
            popup.addItem(withTitle: Self.text(bytes))
            popup.lastItem?.representedObject = NSNumber(value: bytes)
        }
        if let stored, let index = choices.firstIndex(of: stored) {
            popup.selectItem(at: index + 1)
        } else {
            popup.selectItem(at: 0)
        }
        popup.setAccessibilityIdentifier("working-limit-popup")

        let row = NSStackView(views: [fieldLabel("Working limit"), popup])
        row.orientation = .horizontal
        row.alignment = .centerY
        row.spacing = 8

        var notes = "Takes effect the next time Doctored starts."
        if limit.source == .environment {
            notes += " \(WorkingLimit.environmentKey) overrides this setting for this run."
        }
        let footnote = NSTextField(wrappingLabelWithString: notes)
        footnote.font = DS.sans(11)
        footnote.textColor = DS.textMuted
        footnote.preferredMaxLayoutWidth = sheetWidth - sheetInset * 2

        let content = NSStackView(views: [summary, row, footnote])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let ok = StickerButton(title: "OK", style: .primary, target: self,
                               action: #selector(okClicked(_:)))
        let cancel = StickerButton(title: "Cancel", style: .secondary, target: self,
                                   action: #selector(cancelClicked(_:)))
        view = makeSheetView(
            title: "Working Memory",
            hint: "The most memory Doctored plans to use for images, undo and edits.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: ok))
    }

    private func present() {
        modal = ModalCardWindowController(title: "Working Memory", card: view)
        _ = modal?.runModal()
        modal = nil
    }

    @objc private func okClicked(_ sender: Any?) {
        let bytes = (popup.selectedItem?.representedObject as? NSNumber)?.uint64Value
        WorkingLimit.storePreference(bytes, .standard)
        modal?.end(.OK)
    }

    @objc private func cancelClicked(_ sender: Any?) {
        modal?.end(.cancel)
    }
}
