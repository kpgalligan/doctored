import AppKit

/// The assistant's Chat Settings, app-wide in user defaults. The helper
/// receives them in `configure` and again whenever they are saved; a
/// change applies from the next turn.
struct AssistantChatSettings: Equatable {
    static let defaultCompactTokens = 300_000
    static let compactTokensRange = 20_000...900_000
    /// The helper's fixed clearing threshold (CLEAR_TRIGGER_TOKENS), for the sheet's wording.
    static let clearTriggerTokens = 150_000

    /// Compact at the end of a turn once context reaches `autoCompactTokens`.
    var autoCompact = true
    var autoCompactTokens = defaultCompactTokens
    /// Let the API clear old tool results past `clearTriggerTokens`.
    var clearToolResults = false

    private static let autoCompactKey = "AssistantAutoCompact"
    private static let autoCompactTokensKey = "AssistantAutoCompactTokens"
    private static let clearToolResultsKey = "AssistantClearToolResults"

    static func load(from defaults: UserDefaults = .standard) -> AssistantChatSettings {
        var settings = AssistantChatSettings()
        if defaults.object(forKey: autoCompactKey) != nil {
            settings.autoCompact = defaults.bool(forKey: autoCompactKey)
        }
        let tokens = defaults.integer(forKey: autoCompactTokensKey)
        if compactTokensRange.contains(tokens) {
            settings.autoCompactTokens = tokens
        }
        settings.clearToolResults = defaults.bool(forKey: clearToolResultsKey)
        return settings
    }

    func save(to defaults: UserDefaults = .standard) {
        defaults.set(autoCompact, forKey: Self.autoCompactKey)
        defaults.set(autoCompactTokens, forKey: Self.autoCompactTokensKey)
        defaults.set(clearToolResults, forKey: Self.clearToolResultsKey)
    }

    /// The helper protocol's `ChatSettings` (agent/src/protocol.ts).
    var helperJSON: [String: Any] {
        [
            "auto_compact_tokens": autoCompact ? autoCompactTokens as Any : NSNull(),
            "clear_tool_results": clearToolResults,
        ]
    }
}

/// What a Chat Settings save changed, for the panel to act on.
struct AssistantChatSettingsChange {
    var settings: AssistantChatSettings
    /// The stored API key was replaced or removed; a session keeps the key
    /// it opened with, so the conversation has to start over.
    var keyChanged: Bool
}

/// The Assistant panel's Chat Settings… sheet: the Anthropic API key and the
/// conversation's context management. The home for future assistant
/// settings (model choice, other providers' keys).
final class ChatSettingsSheetController: NSViewController {
    private let onSave: (AssistantChatSettingsChange) -> Void
    private let initial = AssistantChatSettings.load()

    private let keyField = NSSecureTextField()
    private let removeKeyBox = NSButton(
        checkboxWithTitle: "Remove the stored key", target: nil, action: nil)
    private let autoCompactBox = NSButton(
        checkboxWithTitle: "Compact when context reaches", target: nil, action: nil)
    private let compactTokensField = NSTextField()
    private let clearToolResultsBox = NSButton(
        checkboxWithTitle:
            "Clear old tool results past "
            + "\(AssistantChatSettings.clearTriggerTokens.formatted()) tokens",
        target: nil, action: nil)

    init(onSave: @escaping (AssistantChatSettingsChange) -> Void) {
        self.onSave = onSave
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ChatSettingsSheetController does not support NSCoder")
    }

    override func loadView() {
        let hasStoredKey = APIKeyStore.hasStoredKey
        keyField.placeholderString =
            hasStoredKey ? "Stored — type a new key to replace it" : "sk-ant-…"
        DSField.style(keyField)
        keyField.widthAnchor.constraint(equalToConstant: 250).isActive = true
        removeKeyBox.font = DS.sans(13)
        removeKeyBox.isHidden = !hasStoredKey
        removeKeyBox.target = self
        removeKeyBox.action = #selector(controlsChanged(_:))

        autoCompactBox.font = DS.sans(13)
        autoCompactBox.state = initial.autoCompact ? .on : .off
        autoCompactBox.target = self
        autoCompactBox.action = #selector(controlsChanged(_:))
        compactTokensField.stringValue = initial.autoCompactTokens.formatted()
        DSField.style(compactTokensField)
        compactTokensField.alignment = .right
        compactTokensField.widthAnchor.constraint(equalToConstant: 84).isActive = true
        let tokensUnit = NSTextField(labelWithString: "tokens")
        tokensUnit.font = DS.sans(13)
        tokensUnit.textColor = DS.textMuted
        let compactRow = NSStackView(views: [autoCompactBox, compactTokensField, tokensUnit])
        compactRow.orientation = .horizontal
        compactRow.spacing = 6

        clearToolResultsBox.font = DS.sans(13)
        clearToolResultsBox.state = initial.clearToolResults ? .on : .off

        var rows: [[NSView]] = [
            [fieldLabel("API key:"), keyField],
            [NSGridCell.emptyContentView, removeKeyBox],
        ]
        if APIKeyStore.environmentOverrides {
            rows.append([
                NSGridCell.emptyContentView,
                note("ANTHROPIC_API_KEY is set for this launch and is used instead."),
            ])
        }
        rows += [
            [fieldLabel("Compaction:"), compactRow],
            [
                NSGridCell.emptyContentView,
                note(
                    "At the end of a turn, the conversation is replaced by a summary. "
                        + "Canvas renders don't survive it; the assistant re-renders."),
            ],
            [fieldLabel("Tool results:"), clearToolResultsBox],
            [
                NSGridCell.emptyContentView,
                note(
                    "The API may clear them in the middle of a turn. "
                        + "Tool search results are never cleared."),
            ],
        ]
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106

        let cancelButton = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let saveButton = sheetApplyButton(target: self, action: #selector(saveClicked(_:)))
        view = makeSheetView(
            title: "Chat Settings",
            hint: "For the assistant in every window. Changes apply from the next message.",
            content: grid,
            buttonRow: makeButtonRow(cancel: cancelButton, apply: saveButton))
        controlsChanged(nil)
    }

    /// An 11px faint note under a row, wrapped to the control column.
    private func note(_ text: String) -> NSTextField {
        let label = NSTextField(wrappingLabelWithString: text)
        label.font = DS.sans(11)
        label.textColor = DS.textFaint
        label.preferredMaxLayoutWidth = 250
        return label
    }

    @objc private func controlsChanged(_ sender: Any?) {
        compactTokensField.isEnabled = autoCompactBox.state == .on
        keyField.isEnabled = removeKeyBox.state == .off
    }

    /// The limit as typed, ignoring grouping separators; nil when it is not
    /// a whole number in `compactTokensRange`.
    private var typedCompactTokens: Int? {
        let digits = compactTokensField.stringValue.filter(\.isNumber)
        guard let tokens = Int(digits), AssistantChatSettings.compactTokensRange.contains(tokens)
        else { return nil }
        return tokens
    }

    @objc private func saveClicked(_ sender: Any?) {
        var settings = initial
        settings.autoCompact = autoCompactBox.state == .on
        settings.clearToolResults = clearToolResultsBox.state == .on
        if settings.autoCompact {
            guard let tokens = typedCompactTokens else {
                NSSound.beep()
                view.window?.makeFirstResponder(compactTokensField)
                return
            }
            settings.autoCompactTokens = tokens
        }

        var keyChanged = false
        let newKey = keyField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if removeKeyBox.state == .on {
            guard APIKeyStore.remove() else {
                NSSound.beep()
                return
            }
            keyChanged = true
        } else if !newKey.isEmpty {
            guard APIKeyStore.save(newKey) else {
                NSSound.beep()
                return
            }
            keyChanged = true
        }

        settings.save()
        dismiss(self)
        onSave(AssistantChatSettingsChange(settings: settings, keyChanged: keyChanged))
    }

    @objc private func cancelClicked(_ sender: Any?) {
        dismiss(self)
    }
}
