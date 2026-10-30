import AppKit

/// Settings ▸ Cloud: per cloud provider its API key (a secure field with Save Key and
/// Remove Key, and whether one is stored; the key is never shown back), its consent (Accept
/// Terms… shows the consent card, Revoke withdraws it), and the one agent switch, "Allow MCP
/// agents to use cloud tools", off by default. Everything is `CloudStore`'s, applied at once.
/// There is no MCP twin: the key, the consent and the switch are the user's own decisions about
/// sending pictures to a third party on their bill, so an agent cannot make them.
final class CloudSettingsPane: NSObject {
    let view: NSStackView
    /// The pane grew or shrank (a message line appeared): the host refits.
    var onResize: (() -> Void)?
    private var rows: [CloudProviderID: Row] = [:]
    private let agentSwitch: NSButton
    private let message = NSTextField(wrappingLabelWithString: "")
    private var observer: NSObjectProtocol?

    private struct Row {
        let keyStatus: NSTextField
        let keyField: NSSecureTextField
        let removeKey: NSButton
        let consentStatus: NSTextField
        let accept: NSButton
        let revoke: NSButton
    }

    override init() {
        view = NSStackView()
        agentSwitch = NSButton(checkboxWithTitle: "Allow MCP agents to use cloud tools", target: nil,
                               action: nil)
        super.init()
        view.orientation = .vertical
        view.alignment = .leading
        view.spacing = 14
        view.setAccessibilityIdentifier("settings.cloud")
        view.addArrangedSubview(label(
            "Cloud tools send part of your picture to a provider with your own API key, billed to "
                + "you. Keys are stored only on this Mac, in " + CloudStore.directory.path + "."))
        for provider in CloudProviders.all { view.addArrangedSubview(makeRow(provider)) }
        agentSwitch.target = self
        agentSwitch.action = #selector(agentSwitchChanged(_:))
        agentSwitch.setAccessibilityIdentifier("settings.cloud.agent-switch")
        view.addArrangedSubview(agentSwitch)
        view.addArrangedSubview(label(
            "Off, an MCP agent's generative_fill is refused before anything is sent."))
        message.font = DS.sans(11)
        message.textColor = DS.warningText
        message.preferredMaxLayoutWidth = 420
        message.isHidden = true
        message.setAccessibilityIdentifier("settings.cloud.message")
        view.addArrangedSubview(message)
        observer = NotificationCenter.default.addObserver(
            forName: CloudStore.didChange, object: nil, queue: .main) { [weak self] _ in self?.refresh() }
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

    private func button(_ title: String, _ id: CloudProviderID, _ action: Selector, _ name: String) -> NSButton {
        let button = StickerButton(title: title, style: .secondary, target: self, action: action)
        button.identifier = NSUserInterfaceItemIdentifier(id.rawValue)
        button.setAccessibilityIdentifier("settings.cloud.\(id.rawValue).\(name)")
        return button
    }

    private func makeRow(_ provider: GenerativeFillProvider) -> NSView {
        let id = provider.id
        let title = label("\(provider.displayName) (\(provider.model))", size: 13, color: DS.textStrong)
        title.font = DS.sans(13, weight: .semibold)
        let keyStatus = label("")
        keyStatus.setAccessibilityIdentifier("settings.cloud.\(id.rawValue).key-status")
        let keyField = NSSecureTextField(string: "")
        keyField.placeholderString = "\(provider.displayName) API key"
        keyField.widthAnchor.constraint(equalToConstant: 220).isActive = true
        DSField.style(keyField)
        keyField.setAccessibilityIdentifier("settings.cloud.\(id.rawValue).key-field")
        let save = button("Save Key", id, #selector(saveKeyClicked(_:)), "save-key")
        let remove = button("Remove Key", id, #selector(removeKeyClicked(_:)), "remove-key")
        let keyRow = NSStackView(views: [keyField, save, remove])
        keyRow.orientation = .horizontal
        keyRow.spacing = 8
        let consentStatus = label("")
        consentStatus.setAccessibilityIdentifier("settings.cloud.\(id.rawValue).consent-status")
        let accept = button("Accept Terms…", id, #selector(acceptClicked(_:)), "accept-terms")
        let revoke = button("Revoke", id, #selector(revokeClicked(_:)), "revoke")
        let consentRow = NSStackView(views: [consentStatus, accept, revoke])
        consentRow.orientation = .horizontal
        consentRow.spacing = 8
        rows[id] = Row(keyStatus: keyStatus, keyField: keyField, removeKey: remove,
                       consentStatus: consentStatus, accept: accept, revoke: revoke)
        let stack = NSStackView(views: [title, keyStatus, keyRow, consentRow])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        return stack
    }

    /// Every row and the switch from the store.
    func refresh() {
        for provider in CloudProviders.all {
            guard let row = rows[provider.id] else { continue }
            let stored = CloudStore.key(provider.id) != nil
            row.keyStatus.stringValue = stored ? "A key is saved." : "No key saved."
            row.removeKey.isEnabled = stored
            let consented = CloudStore.hasConsent(provider.id, termsVersion: provider.terms.version)
            row.consentStatus.stringValue = consented ? "Terms accepted." : "Terms not accepted."
            row.accept.isEnabled = !consented
            row.revoke.isEnabled = consented
        }
        agentSwitch.state = CloudStore.agentMayUseCloudTools ? .on : .off
    }

    private func provider(_ sender: Any?) -> GenerativeFillProvider? {
        guard let raw = (sender as? NSButton)?.identifier?.rawValue,
              let id = CloudProviderID(rawValue: raw) else { return nil }
        return CloudProviders.provider(id)
    }

    private func report(_ text: String?) {
        let wasHidden = message.isHidden
        message.stringValue = text ?? ""
        message.isHidden = text == nil
        if wasHidden != message.isHidden { onResize?() }
    }

    @objc private func saveKeyClicked(_ sender: Any?) {
        guard let provider = provider(sender), let row = rows[provider.id] else { NSSound.beep(); return }
        let typed = row.keyField.stringValue
        row.keyField.stringValue = ""
        guard CloudStore.saveKey(typed, for: provider.id) else {
            report(typed.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                ? "Type a key first." : "The key could not be saved in \(CloudStore.directory.path).")
            NSSound.beep()
            return
        }
        report(nil)
    }

    @objc private func removeKeyClicked(_ sender: Any?) {
        guard let provider = provider(sender) else { NSSound.beep(); return }
        if CloudStore.removeKey(provider.id) { report(nil) } else {
            report("The key could not be removed.")
            NSSound.beep()
        }
    }

    @objc private func acceptClicked(_ sender: Any?) {
        guard let provider = provider(sender) else { NSSound.beep(); return }
        guard CloudConsentCard.present(provider) else { return }
        do {
            try CloudStore.recordConsent(provider.id, termsVersion: provider.terms.version)
            report(nil)
        } catch {
            report(error.localizedDescription)
            NSSound.beep()
        }
    }

    @objc private func revokeClicked(_ sender: Any?) {
        guard let provider = provider(sender) else { NSSound.beep(); return }
        do {
            try CloudStore.revokeConsent(provider.id)
            report(nil)
        } catch {
            report(error.localizedDescription)
            NSSound.beep()
        }
    }

    @objc private func agentSwitchChanged(_ sender: Any?) {
        do {
            try CloudStore.setAgentMayUseCloudTools(agentSwitch.state == .on)
            report(nil)
        } catch {
            report(error.localizedDescription)
            NSSound.beep()
            refresh()
        }
    }
}
