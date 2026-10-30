import AppKit

/// Settings ▸ Models: each catalogue model with its state (not downloaded, downloading
/// with progress, ready, failed), size on disk, licence (a link), tier, training-data note, and
/// Download, Cancel and Delete. Download first shows the licence card for a version the user has
/// not accepted (`confirmLicense`, a nested `ModalCardWindowController`, never an `NSAlert`) and
/// records the acceptance in the store. It reads `ModelStore.shared` and follows its
/// `didChange`. There is no MCP twin for a download: it fetches tens of MB from a third-party
/// host under a licence the user must accept (`list_models` reports the state).
final class ModelsSettingsPane: NSObject {
    let view: NSStackView
    /// The pane grew or shrank (a failure line appeared): the host refits.
    var onResize: (() -> Void)?
    /// Shows the licence card and answers whether the user accepted; probes replace it.
    var confirmLicense: (ModelCatalogEntry) -> Bool = ModelsSettingsPane.presentLicenseCard
    private let store: ModelStore
    private var rows: [String: Row] = [:]
    private let message = NSTextField(wrappingLabelWithString: "")
    private var observer: NSObjectProtocol?

    /// One model's controls.
    private struct Row {
        let state: NSTextField
        let progress: NSProgressIndicator
        let size: NSTextField
        let download: NSButton
        let cancel: NSButton
        let delete: NSButton
    }

    override init() {
        store = ModelStore.shared
        view = NSStackView()
        super.init()
        view.orientation = .vertical
        view.alignment = .leading
        view.spacing = 14
        view.setAccessibilityIdentifier("models-pane")
        let intro = NSTextField(wrappingLabelWithString:
            "Models download on demand from their official host into "
            + store.directory.path + ". Each is checked against its published SHA-256 before use.")
        intro.font = DS.sans(12)
        intro.textColor = DS.textMuted
        intro.preferredMaxLayoutWidth = 420
        view.addArrangedSubview(intro)
        for entry in store.catalog { view.addArrangedSubview(makeRow(entry)) }
        message.font = DS.sans(11)
        message.textColor = DS.warningText
        message.preferredMaxLayoutWidth = 420
        message.isHidden = true
        message.setAccessibilityIdentifier("models-message")
        view.addArrangedSubview(message)
        observer = NotificationCenter.default.addObserver(
            forName: ModelStore.didChange, object: nil, queue: .main) { [weak self] _ in self?.refresh() }
        refresh()
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    private func label(_ text: String, size: CGFloat = 11, color: NSColor = DS.textMuted) -> NSTextField {
        let field = NSTextField(wrappingLabelWithString: text)
        field.font = DS.sans(size)
        field.textColor = color
        field.preferredMaxLayoutWidth = 420
        return field
    }

    private func makeRow(_ entry: ModelCatalogEntry) -> NSView {
        let title = label(entry.name, size: 13, color: DS.textStrong)
        title.font = DS.sans(13, weight: .semibold)
        let state = label("")
        state.setAccessibilityIdentifier("model-state-\(entry.id)")
        let progress = NSProgressIndicator()
        progress.isIndeterminate = false
        progress.minValue = 0
        progress.maxValue = 1
        progress.style = .bar
        progress.widthAnchor.constraint(equalToConstant: 160).isActive = true
        progress.setAccessibilityIdentifier("model-progress-\(entry.id)")
        let size = label("")
        size.setAccessibilityIdentifier("model-size-\(entry.id)")
        let stateRow = NSStackView(views: [state, progress, size])
        stateRow.orientation = .horizontal
        stateRow.spacing = 10
        let link = NSButton(title: "Licence: \(entry.license.name)", target: self, action: #selector(openLicense(_:)))
        link.bezelStyle = .inline
        link.identifier = NSUserInterfaceItemIdentifier(entry.id)
        link.setAccessibilityIdentifier("model-license-\(entry.id)")
        let tier = label("Tier: \(entry.license.tier.title)")
        let training = label("Training data: \(entry.trainingData)")
        let download = button("Download", entry, #selector(downloadClicked(_:)), "model-download-\(entry.id)")
        let cancel = button("Cancel", entry, #selector(cancelClicked(_:)), "model-cancel-\(entry.id)")
        let delete = button("Delete", entry, #selector(deleteClicked(_:)), "model-delete-\(entry.id)")
        let buttons = NSStackView(views: [download, cancel, delete])
        buttons.orientation = .horizontal
        buttons.spacing = 8
        rows[entry.id] = Row(state: state, progress: progress, size: size, download: download,
                             cancel: cancel, delete: delete)
        let stack = NSStackView(views: [title, stateRow, link, tier, training, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        return stack
    }

    private func button(_ title: String, _ entry: ModelCatalogEntry, _ action: Selector, _ id: String) -> NSButton {
        let button = StickerButton(title: title, style: .secondary, target: self, action: action)
        button.identifier = NSUserInterfaceItemIdentifier(entry.id)
        button.setAccessibilityIdentifier(id)
        return button
    }

    /// Every row from the store.
    func refresh() {
        for entry in store.catalog {
            guard let row = rows[entry.id] else { continue }
            let state = store.state(entry.id)
            switch state {
            case .notDownloaded: row.state.stringValue = "Not downloaded"
            case .downloading(let fraction): row.state.stringValue = String(format: "Downloading %.0f%%", fraction * 100)
            case .ready: row.state.stringValue = "Ready"
            case .failed(let reason): row.state.stringValue = "Failed: \(reason)"
            }
            if case .downloading(let fraction) = state {
                row.progress.isHidden = false
                row.progress.doubleValue = fraction
            } else {
                row.progress.isHidden = true
            }
            let bytes = store.bytesOnDisk(entry.id)
            row.size.stringValue = ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
                + " of " + ByteCountFormatter.string(fromByteCount: entry.totalBytes, countStyle: .file)
            let downloading: Bool = { if case .downloading = state { return true } else { return false } }()
            row.download.isEnabled = !downloading && state != .ready
            row.cancel.isEnabled = downloading
            // Off while downloading: the job's last step compiles on the downloader's queue and
            // then writes `verified.json`, which a Delete during it could not stop (Cancel first).
            row.delete.isEnabled = !downloading && (bytes > 0 || state == .ready)
        }
    }

    private func entry(_ sender: Any?) -> ModelCatalogEntry? {
        guard let id = (sender as? NSButton)?.identifier?.rawValue else { return nil }
        return store.entry(id)
    }

    private func report(_ text: String?) {
        let wasHidden = message.isHidden
        message.stringValue = text ?? ""
        message.isHidden = text == nil
        if wasHidden != message.isHidden { onResize?() }
    }

    @objc private func downloadClicked(_ sender: Any?) {
        guard let entry = entry(sender) else { NSSound.beep(); return }
        report(nil)
        do {
            if !store.isAccepted(entry.id) {
                guard confirmLicense(entry) else { return }
                try store.accept(entry.id)
            }
            try store.download(entry.id)
        } catch {
            report(error.localizedDescription)
            NSSound.beep()
        }
    }

    @objc private func cancelClicked(_ sender: Any?) {
        guard let entry = entry(sender) else { NSSound.beep(); return }
        store.cancel(entry.id)
    }

    @objc private func deleteClicked(_ sender: Any?) {
        guard let entry = entry(sender) else { NSSound.beep(); return }
        report(nil)
        do { try store.delete(entry.id) } catch {
            report(error.localizedDescription)
            NSSound.beep()
        }
    }

    @objc private func openLicense(_ sender: Any?) {
        guard let entry = entry(sender) else { return }
        NSWorkspace.shared.open(entry.license.url)
    }

    // MARK: - The licence card

    /// The licence card: the model, its licence (name and link), tier and training-data note,
    /// Accept or Cancel. A nested app-modal card over Settings.
    static func presentLicenseCard(_ entry: ModelCatalogEntry) -> Bool {
        let card = LicenseCard(entry)
        return card.run()
    }
}

/// The licence card's controller: the card's buttons need a target that lives through the
/// modal session.
private final class LicenseCard: NSObject {
    private let entry: ModelCatalogEntry
    private var modal: ModalCardWindowController?

    init(_ entry: ModelCatalogEntry) {
        self.entry = entry
        super.init()
    }

    func run() -> Bool {
        let text = { (string: String) -> NSTextField in
            let field = NSTextField(wrappingLabelWithString: string)
            field.font = DS.sans(12)
            field.textColor = DS.textStrong
            field.preferredMaxLayoutWidth = 376
            return field
        }
        let link = NSButton(title: "\(entry.license.name): \(entry.license.url.absoluteString)", target: self,
                            action: #selector(openLicense(_:)))
        link.bezelStyle = .inline
        link.setAccessibilityIdentifier("model-license-card-link")
        let content = NSStackView(views: [
            text("\(entry.name), version \(String(entry.version.prefix(12)))"),
            link,
            text("Tier: \(entry.license.tier.title)"),
            text("Training data: \(entry.trainingData)"),
            text("Downloading fetches \(ByteCountFormatter.string(fromByteCount: entry.totalBytes, countStyle: .file)) "
                + "from the model's official host. Accepting records your acceptance for this version."),
        ])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 8
        let accept = StickerButton(title: "Accept and Download", style: .primary, target: self,
                                   action: #selector(acceptClicked(_:)))
        accept.setAccessibilityIdentifier("model-license-accept")
        let cancel = StickerButton(title: "Cancel", style: .secondary, target: self,
                                   action: #selector(cancelClicked(_:)))
        let window = ModalCardWindowController(title: "Model Licence", card: makeSheetView(
            title: "Model Licence", content: content, buttonRow: makeButtonRow(cancel: cancel, apply: accept)))
        modal = window
        return window.runModal() == .OK
    }

    @objc private func openLicense(_ sender: Any?) { NSWorkspace.shared.open(entry.license.url) }
    @objc private func acceptClicked(_ sender: Any?) { modal?.end(.OK) }
    @objc private func cancelClicked(_ sender: Any?) { modal?.end(.cancel) }
}
