import AppKit

/// The per-provider consent card: what the provider does with the crop it is sent
/// (training, retention, billing, quoted from the ADR's cloud data terms, with the provider's own
/// page), Accept or Cancel. A nested app-modal `ModalCardWindowController`, never an `NSAlert`,
/// as the model licence card (`ModelsSettingsPane.presentLicenseCard`). Nothing is sent before the
/// user accepts; Settings ▸ Cloud shows the consent and revokes it.
enum CloudConsentCard {
    /// Shows the card and answers whether the user accepted. Probes replace it.
    static var present: (GenerativeFillProvider) -> Bool = { provider in
        ConsentCard(provider).run()
    }
}

/// The card's controller: its buttons need a target that lives through the modal session.
private final class ConsentCard: NSObject {
    private let provider: GenerativeFillProvider
    private var modal: ModalCardWindowController?

    init(_ provider: GenerativeFillProvider) {
        self.provider = provider
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
        let terms = provider.terms
        let link = NSButton(title: terms.url.absoluteString, target: self, action: #selector(openTerms(_:)))
        link.bezelStyle = .inline
        link.setAccessibilityIdentifier("cloud-consent-link")
        let content = NSStackView(views: [
            text("Generative Fill sends the area around your selection, the selection itself and your "
                + "prompt to \(provider.displayName) (\(provider.model)) with your own API key, and puts "
                + "the picture it answers on a new layer."),
            text("Training: \(terms.training)"),
            text("Retention: \(terms.retention)"),
            text("Billing: \(terms.billing)"),
            link,
            text("Accepting records your consent for these terms. Settings ▸ Cloud revokes it."),
        ])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 8
        let accept = StickerButton(title: "Accept and Send", style: .primary, target: self,
                                   action: #selector(acceptClicked(_:)))
        accept.setAccessibilityIdentifier("cloud-consent-accept")
        let cancel = StickerButton(title: "Cancel", style: .secondary, target: self,
                                   action: #selector(cancelClicked(_:)))
        cancel.setAccessibilityIdentifier("cloud-consent-cancel")
        let title = "Send to \(provider.displayName)?"
        let window = ModalCardWindowController(title: title, card: makeSheetView(
            title: title, content: content, buttonRow: makeButtonRow(cancel: cancel, apply: accept)))
        modal = window
        return window.runModal() == .OK
    }

    @objc private func openTerms(_ sender: Any?) { NSWorkspace.shared.open(provider.terms.url) }
    @objc private func acceptClicked(_ sender: Any?) { modal?.end(.OK) }
    @objc private func cancelClicked(_ sender: Any?) { modal?.end(.cancel) }
}
