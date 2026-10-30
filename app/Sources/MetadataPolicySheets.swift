import AppKit

/// Save a metadata policy under a name, and manage the library — the
/// `ExportPresetSaveSheet` / `ExportPresetManagerSheet` pair exactly. A policy
/// is a PREFERENCE: saving one changes no picture and registers no undo step.
final class MetadataPolicySaveSheet: NSObject {
    private let policy: MetadataPolicy
    private let name = NSTextField()
    private let replace = NSButton(
        checkboxWithTitle: "Replace a policy with this name", target: nil, action: nil)
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?

    private init(policy: MetadataPolicy) {
        self.policy = policy
        super.init()
    }

    @discardableResult
    static func run(policy: MetadataPolicy) -> Bool {
        MetadataPolicySaveSheet(policy: policy).run()
    }

    private func run() -> Bool {
        DSField.style(name)
        name.widthAnchor.constraint(equalToConstant: 330).isActive = true
        name.placeholderString = "Policy name"
        name.setAccessibilityIdentifier("metadata-policy-name")
        replace.font = DS.sans(12)
        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340

        let summary = NSTextField(wrappingLabelWithString: policy.summary)
        summary.font = DS.sans(11)
        summary.preferredMaxLayoutWidth = 340

        let content = NSStackView(views: [name, summary, replace, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10

        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let save = StickerButton(title: "Save Policy", style: .primary,
                                 target: self, action: #selector(save(_:)))
        save.setAccessibilityIdentifier("metadata-policy-save")
        modal = ModalCardWindowController(title: "Save Metadata Policy", card: makeSheetView(
            title: "Save Metadata Policy",
            hint: "A policy says what each export keeps: EXIF, XMP, IPTC, the print resolution, "
                + "and an optional author/copyright stamp written into the XMP packet. "
                + "Orientation is never a choice — the rotation is already in the pixels.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: save)))
        return modal?.runModal() == .OK
    }

    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }

    @objc private func save(_ sender: Any?) {
        do {
            try MetadataPolicyLibrary.save(
                NamedMetadataPolicy(name: name.stringValue, policy: policy),
                replace: replace.state == .on)
            modal?.end(.OK)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }
}

/// List, edit, save and delete named metadata policies. The sheet only edits
/// and PICKS; the export workspace applies whatever it returns, so one path
/// serves the sheet, the Batch window and MCP alike.
final class MetadataPolicyManageSheet: NSObject {
    private var entries: [NamedMetadataPolicy] = []
    private let current: MetadataPolicy
    private let popup = NSPopUpButton()
    private let exif = NSPopUpButton()
    private let xmp = NSPopUpButton()
    private let iptc = NSPopUpButton()
    private let density = NSPopUpButton()
    private let author = NSTextField()
    private let copyright = NSTextField()
    private let message = NSTextField(wrappingLabelWithString: "")
    private var modal: ModalCardWindowController?
    private var selected: MetadataPolicy?
    private var deleteButton: NSButton?
    private var brokenSection: BrokenLibraryFilesSection?

    private init(current: MetadataPolicy) {
        self.current = current
        super.init()
    }

    /// The policy the caller should adopt, or nil when the sheet was
    /// cancelled.
    static func run(current: MetadataPolicy) -> MetadataPolicy? {
        MetadataPolicyManageSheet(current: current).run()
    }

    private func run() -> MetadataPolicy? {
        popup.widthAnchor.constraint(equalToConstant: 330).isActive = true
        popup.setAccessibilityIdentifier("metadata-policy-list")
        popup.target = self
        popup.action = #selector(selectionChanged(_:))

        for (rules, identifier) in [(exif, "exif"), (xmp, "xmp"), (iptc, "iptc"),
                                    (density, "density")] {
            rules.addItems(withTitles: PacketRule.allCases.map(\.displayName))
            rules.setAccessibilityIdentifier("metadata-policy-\(identifier)")
            rules.widthAnchor.constraint(equalToConstant: 120).isActive = true
        }
        for (field, placeholder, identifier) in [(author, "Author", "author"),
                                                 (copyright, "Copyright", "copyright")] {
            DSField.style(field)
            field.placeholderString = placeholder
            field.setAccessibilityIdentifier("metadata-policy-\(identifier)")
            field.widthAnchor.constraint(equalToConstant: 330).isActive = true
        }
        apply(current)

        let rulesRow = NSStackView(views: [labelled("EXIF", exif), labelled("XMP", xmp),
                                           labelled("IPTC", iptc), labelled("Resolution", density)])
        rulesRow.spacing = 12
        rulesRow.alignment = .top

        let save = StickerButton(title: "Save As…", style: .secondary,
                                 target: self, action: #selector(saveCurrent(_:)))
        let delete = StickerButton(title: "Delete", style: .secondary,
                                   target: self, action: #selector(deleteCurrent(_:)))
        delete.setAccessibilityIdentifier("metadata-policy-delete")
        deleteButton = delete
        let actions = NSStackView(views: [save, delete])
        actions.spacing = 8

        message.font = DS.sans(11)
        message.textColor = DS.textMuted
        message.preferredMaxLayoutWidth = 340

        // The bad files, listed with their reasons. Delete File is the UI twin
        // of delete_metadata_policy {file}; there is no repair outside RAW.
        let broken = BrokenLibraryFilesSection(
            title: "Files that are not policies:", identifierPrefix: "metadata-policy",
            delete: { try MetadataPolicyLibrary.delete(file: $0) }, repair: nil,
            changed: { [weak self] in self?.reload() },
            report: { [weak self] in self?.message.stringValue = $0 })
        brokenSection = broken
        let content = NSStackView(views: [popup, rulesRow, author, copyright, actions,
                                          broken.view, message])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 12

        let cancel = sheetCancelButton(target: self, action: #selector(cancel(_:)))
        let use = StickerButton(title: "Use", style: .primary,
                                target: self, action: #selector(use(_:)))
        reload()
        modal = ModalCardWindowController(title: "Metadata Policies", card: makeSheetView(
            title: "Metadata Policies",
            hint: "Default defers to “Strip metadata”. Dropping the resolution needs EXIF and XMP "
                + "dropped too — each carries its own copy of the ppi — and a stamp needs the XMP "
                + "packet written.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: use)))
        _ = modal?.runModal()
        return selected
    }

    private func labelled(_ title: String, _ control: NSPopUpButton) -> NSStackView {
        let label = NSTextField(labelWithString: title)
        label.font = DS.sans(11)
        label.textColor = DS.textMuted
        let stack = NSStackView(views: [label, control])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 4
        return stack
    }

    private func apply(_ policy: MetadataPolicy) {
        for (control, rule) in [(exif, policy.exif), (xmp, policy.xmp), (iptc, policy.iptc),
                                (density, policy.density)] {
            control.selectItem(at: PacketRule.allCases.firstIndex(of: rule) ?? 0)
        }
        author.stringValue = policy.stamp?.author ?? ""
        copyright.stringValue = policy.stamp?.copyright ?? ""
    }

    private var edited: MetadataPolicy {
        func rule(_ control: NSPopUpButton) -> PacketRule {
            let index = control.indexOfSelectedItem
            return PacketRule.allCases.indices.contains(index)
                ? PacketRule.allCases[index] : .default
        }
        let stamp = MetadataStamp(
            author: author.stringValue.isEmpty ? nil : author.stringValue,
            copyright: copyright.stringValue.isEmpty ? nil : copyright.stringValue)
        return MetadataPolicy(exif: rule(exif), xmp: rule(xmp), iptc: rule(iptc),
                              density: rule(density), stamp: stamp.isEmpty ? nil : stamp)
    }

    private func reload() {
        do {
            let listing = try MetadataPolicyLibrary.listing()
            entries = listing.entries.map(\.item)
            popup.removeAllItems()
            popup.addItem(withTitle: "Custom")
            popup.addItems(withTitles: entries.map(\.name))
            deleteButton?.isEnabled = !entries.isEmpty
            message.stringValue = entries.isEmpty && listing.broken.isEmpty
                ? "No metadata policies saved yet." : ""
            if brokenSection?.update(listing.broken) == true { modal?.refit() }
        } catch {
            entries = []
            popup.removeAllItems()
            popup.addItem(withTitle: "Custom")
            deleteButton?.isEnabled = false
            if brokenSection?.update([]) == true { modal?.refit() }
            message.stringValue = error.localizedDescription
        }
    }

    @objc private func selectionChanged(_ sender: Any?) {
        let index = popup.indexOfSelectedItem - 1
        guard entries.indices.contains(index) else { return }
        apply(entries[index].policy)
    }

    @objc private func saveCurrent(_ sender: Any?) {
        MetadataPolicySaveSheet.run(policy: edited)
        reload()
    }

    @objc private func deleteCurrent(_ sender: Any?) {
        let index = popup.indexOfSelectedItem - 1
        guard entries.indices.contains(index) else { return }
        do {
            try MetadataPolicyLibrary.delete(name: entries[index].name)
            reload()
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
        }
    }

    @objc private func use(_ sender: Any?) {
        let policy = edited
        do {
            // The CORE validates; a policy the export would refuse is caught
            // here rather than at delivery.
            _ = try RasterExport.validateMetadataPolicy(policy.json)
        } catch {
            message.stringValue = error.localizedDescription
            NSSound.beep()
            return
        }
        selected = policy
        modal?.end(.OK)
    }

    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
}
