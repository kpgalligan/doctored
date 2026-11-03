import AppKit

/// The Assistant tab of the right panel: a chat with the built-in agent
/// that edits the window's document through the shared tool dispatcher.
/// One session per panel (= per document window); Clear starts fresh.
final class AssistantPanelViewController: NSViewController {
    weak var document: ImageDocument?

    /// Called when the user clicks the Layers tab.
    var onShowLayers: (() -> Void)?

    /// Called when the user clicks the Channels tab.
    var onShowChannels: (() -> Void)?

    /// Called when the user clicks the Info tab.
    var onShowInfo: (() -> Void)?

    /// Called when the user clicks the Navigator tab.
    var onShowNavigator: (() -> Void)?

    /// Called when the user clicks the Paths tab.
    var onShowPaths: (() -> Void)?

    private var session: AssistantHelperSession?
    private var busy = false
    /// The delta event type ("text_delta" / "progress_delta") currently
    /// extending the newest paragraph; any other event ends it.
    private var streamingType: String?

    private let transcript = NSTextView()
    private let transcriptScroll = NSScrollView()
    private let inputField = NSTextField()
    private var sendButton: StickerButton!
    private let spinner = NSProgressIndicator()
    private var inputRow: NSStackView!
    /// Shown instead of the input row while no API key is available.
    private var noKeyBox: NSStackView!

    private var overflowButton: GhostButton!
    private var contextButton: GhostButton!
    private var contextUsage = AssistantContextUsage()
    /// The context panel and its meter, while open.
    private var contextPanel: OverflowPanel?
    private var contextMeter: AssistantContextMeterView?

    init() {
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AssistantPanelViewController does not support NSCoder")
    }

    deinit {
        session?.close()
    }

    // MARK: - View construction

    override func loadView() {
        let root = NSView(frame: NSRect(x: 0, y: 0, width: DS.panelWidth, height: 400))
        root.wantsLayer = true
        root.layer?.backgroundColor = DS.chromeBackground.cgColor

        let tabs = PanelTabsView(titles: EditorPanelTab.titles, activeIndex: 2) { [weak self] index in
            if index == 0 { self?.onShowLayers?() }
            if index == 1 { self?.onShowChannels?() }
            if index == 3 { self?.onShowInfo?() }
            if index == 4 { self?.onShowNavigator?() }
            if index == 5 { self?.onShowPaths?() }
        }
        tabs.translatesAutoresizingMaskIntoConstraints = false

        let clearButton = GhostButton(
            symbol: "arrow.counterclockwise", fallback: "↺", caption: nil,
            tooltip: "Clear the conversation", action: #selector(clearConversation(_:)))
        clearButton.target = self
        clearButton.translatesAutoresizingMaskIntoConstraints = false

        overflowButton = GhostButton(
            symbol: "ellipsis", fallback: "…", caption: nil,
            tooltip: "More", action: #selector(showOverflowMenu(_:)))
        overflowButton.target = self
        overflowButton.translatesAutoresizingMaskIntoConstraints = false

        contextButton = GhostButton(
            symbol: "gauge.with.dots.needle.33percent", fallback: "◔", caption: nil,
            tooltip: "Context usage", action: #selector(showContextUsage(_:)))
        contextButton.target = self
        contextButton.translatesAutoresizingMaskIntoConstraints = false

        transcript.isEditable = false
        transcript.isRichText = true
        transcript.drawsBackground = false
        transcript.textContainerInset = NSSize(width: 8, height: 10)
        transcriptScroll.translatesAutoresizingMaskIntoConstraints = false
        transcriptScroll.documentView = transcript
        transcriptScroll.hasVerticalScroller = true
        transcriptScroll.drawsBackground = false
        transcript.autoresizingMask = [.width]
        transcript.textContainer?.widthTracksTextView = true

        inputField.placeholderString = "Ask about or edit this image…"
        inputField.target = self
        inputField.action = #selector(sendClicked(_:))
        DSField.style(inputField)
        inputField.font = DS.sans(13)
        inputField.translatesAutoresizingMaskIntoConstraints = false

        sendButton = StickerButton(
            title: "Send", style: .primary, target: self, action: #selector(sendClicked(_:)))
        sendButton.translatesAutoresizingMaskIntoConstraints = false

        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        spinner.translatesAutoresizingMaskIntoConstraints = false

        inputRow = NSStackView(views: [inputField, sendButton])
        inputRow.translatesAutoresizingMaskIntoConstraints = false
        inputRow.orientation = .horizontal
        inputRow.spacing = 8
        inputRow.alignment = .centerY

        // Without a key, the input row gives way to a pointer at Chat
        // Settings, where the key is entered, changed and removed.
        let noKeyMessage = NSTextField(
            wrappingLabelWithString:
                "Add your Anthropic API key in Chat Settings to use the assistant.")
        noKeyMessage.font = DS.sans(12)
        noKeyMessage.textColor = DS.textMuted
        let settingsButton = StickerButton(
            title: "Chat Settings…", style: .primary, target: self,
            action: #selector(showChatSettings(_:)))
        noKeyBox = NSStackView(views: [noKeyMessage, settingsButton])
        noKeyBox.translatesAutoresizingMaskIntoConstraints = false
        noKeyBox.orientation = .vertical
        noKeyBox.alignment = .leading
        noKeyBox.spacing = 8

        root.addSubview(tabs)
        root.addSubview(clearButton)
        root.addSubview(overflowButton)
        root.addSubview(contextButton)
        root.addSubview(transcriptScroll)
        root.addSubview(spinner)
        root.addSubview(inputRow)
        root.addSubview(noKeyBox)

        NSLayoutConstraint.activate([
            // The tab row runs edge to edge; the clear button (and spinner)
            // sit in a slim strip below it, over the transcript's top edge.
            tabs.topAnchor.constraint(equalTo: root.topAnchor),
            tabs.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            tabs.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            tabs.heightAnchor.constraint(equalToConstant: DS.tabHeight),

            // Right to left: the overflow menu, Clear, the context meter.
            overflowButton.topAnchor.constraint(equalTo: tabs.bottomAnchor, constant: 4),
            overflowButton.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),

            clearButton.centerYAnchor.constraint(equalTo: overflowButton.centerYAnchor),
            clearButton.trailingAnchor.constraint(
                equalTo: overflowButton.leadingAnchor, constant: -4),

            transcriptScroll.topAnchor.constraint(
                equalTo: overflowButton.bottomAnchor, constant: 4),
            transcriptScroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            transcriptScroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            transcriptScroll.bottomAnchor.constraint(equalTo: inputRow.topAnchor, constant: -10),

            contextButton.centerYAnchor.constraint(equalTo: overflowButton.centerYAnchor),
            contextButton.trailingAnchor.constraint(
                equalTo: clearButton.leadingAnchor, constant: -4),

            spinner.centerYAnchor.constraint(equalTo: overflowButton.centerYAnchor),
            spinner.trailingAnchor.constraint(equalTo: contextButton.leadingAnchor, constant: -8),

            inputRow.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            inputRow.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            inputRow.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -12),

            noKeyBox.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            noKeyBox.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            noKeyBox.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -12),
        ])
        view = root
        refreshKeyState()
    }

    private func refreshKeyState() {
        let hasKey = APIKeyStore.resolve() != nil
        inputRow.isHidden = !hasKey
        noKeyBox.isHidden = hasKey
    }

    // MARK: - Actions

    /// The overflow button's menu: the panel's less frequent commands.
    @objc private func showOverflowMenu(_ sender: Any?) {
        let menu = NSMenu()
        let settings = NSMenuItem(
            title: "Chat Settings…", action: #selector(showChatSettings(_:)), keyEquivalent: "")
        settings.target = self
        menu.addItem(settings)
        menu.popUp(
            positioning: nil, at: NSPoint(x: 0, y: overflowButton.bounds.height + 2),
            in: overflowButton)
    }

    /// Opens the context panel under its button (reopening it if it is
    /// already open, like the options bar's More).
    @objc private func showContextUsage(_ sender: Any?) {
        contextPanel?.close()
        let meter = AssistantContextMeterView(usage: contextUsage)
        let panel = OverflowPanel(content: meter)
        panel.onClose = { [weak self] in
            self?.contextPanel = nil
            self?.contextMeter = nil
        }
        contextPanel = panel
        contextMeter = meter
        panel.show(below: contextButton)
    }

    private func setContextUsage(_ usage: AssistantContextUsage) {
        contextUsage = usage
        contextMeter?.update(usage)
    }

    /// Chat Settings…: the API key and context management. Refused while a
    /// turn runs — a key change would end the conversation under it.
    @objc private func showChatSettings(_ sender: Any?) {
        guard !busy else {
            NSSound.beep()
            return
        }
        presentAsSheet(
            ChatSettingsSheetController { [weak self] change in
                self?.chatSettingsSaved(change)
            })
    }

    private func chatSettingsSaved(_ change: AssistantChatSettingsChange) {
        refreshKeyState()
        if change.keyChanged {
            appendMeta(APIKeyStore.hasStoredKey ? "API key saved." : "API key removed.")
            endConversationForKeyChange()
        }
        session?.updateSettings(change.settings)
    }

    /// A session holds the key it was opened with, so a changed key only
    /// takes effect in a new conversation.
    private func endConversationForKeyChange() {
        guard let session = session else { return }
        session.close()
        self.session = nil
        setContextUsage(
            AssistantContextUsage(
                used: 0, limit: contextUsage.limit, reported: contextUsage.reported))
        appendMeta("The next message starts a new conversation.")
    }

    @objc private func clearConversation(_ sender: Any?) {
        session?.close()
        session = nil
        // The window is the model's, so it survives; the usage starts over.
        setContextUsage(
            AssistantContextUsage(
                used: 0, limit: contextUsage.limit, reported: contextUsage.reported))
        setBusy(false)
        transcript.textStorage?.setAttributedString(NSAttributedString())
    }

    @objc private func sendClicked(_ sender: Any?) {
        if busy {
            // The sticker button reads "Stop" in this state.
            session?.cancel()
            return
        }
        let text = inputField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        guard ensureSession() else { return }
        appendUser(text)
        inputField.stringValue = ""
        if session?.send(text) != true {
            appendError("The assistant is busy.")
        }
    }

    /// Creates the session on first use (needs the key and the document).
    private func ensureSession() -> Bool {
        if session != nil { return true }
        guard let apiKey = APIKeyStore.resolve() else {
            refreshKeyState()
            return false
        }
        let model =
            UserDefaults.standard.string(forKey: "AssistantModel") ?? "claude-opus-5-5"
        let opened = AssistantHelperSession.open(
            apiKey: apiKey, model: model, system: systemPrompt(),
            settings: AssistantChatSettings.load())
        switch opened {
        case .success(let session):
            session.onEvent = { [weak self] event in self?.handleEvent(event) }
            self.session = session
            return true
        case .failure(let error):
            appendError(error.message)
            return false
        }
    }

    private func systemPrompt() -> String {
        var context = "No document is open yet — use open_document or list_documents first."
        if let document = document, let doc = document.doc {
            let id = AgentServer.shared.documentID(for: document)
            context = """
                The user is looking at document id \(id): \"\(document.displayName ?? "Untitled")\" \
                (\(doc.width)×\(doc.height) px, \(doc.layerCount) layer(s)). Operate on this \
                document unless told otherwise — pass document_id \(id) on every tool call.
                """
        }
        return """
            You are the assistant built into Doctored, a macOS layered image editor. You edit \
            the user's open image by calling tools; the user sees every change live and each \
            edit is one undo step (reads, view state, preset writes and no-op calls add \
            none). \(context) Layer index 0 is the bottom layer; an \
            index may name a layer GROUP, whose children come BEFORE it in the list and \
            name it in parent — get_document reports each entry's kind, depth and \
            children. \
            Coordinates start at the canvas top-left corner with y increasing downward. Use the \
            render tool to look at the canvas before visual edits and again afterwards to \
            verify the result. Keep replies to a sentence or two; never repeat tool output \
            the user can already see. \
            \(AssistantActionAuthoring.systemPromptSection)
            """
    }

    // MARK: - Events

    private func handleEvent(_ event: [String: Any]) {
        let type = event["type"] as? String
        if type != streamingType { streamingType = nil }
        switch type {
        case "text_delta":
            appendDelta(
                event["text"] as? String ?? "", type: "text_delta",
                font: DS.sans(13), color: DS.textStrong, spacingAbove: 6)
        case "progress_delta":
            // The model's note between tool calls (thinking display "updates").
            appendDelta(
                event["text"] as? String ?? "", type: "progress_delta",
                font: DS.sans(12), color: DS.textFaint, spacingAbove: 4)
        case "compaction_started":
            let used = (event["used"] as? Int ?? 0).formatted()
            appendMeta("Compacting the conversation (\(used) tokens)…")
        case "compacted":
            let before = (event["before"] as? Int ?? 0).formatted()
            let after = (event["after"] as? Int ?? 0).formatted()
            appendMeta(
                "Compacted: \(before) → about \(after) tokens. Earlier renders are gone; "
                    + "the assistant re-renders when it needs to look.")
        case "compaction_failed":
            let reason = event["reason"] as? String ?? "no summary came back"
            appendMeta(
                "Compaction didn't finish (\(reason)); it will try again after the next turn.")
        case "tool_results_cleared":
            let uses = event["tool_uses"] as? Int ?? 0
            let tokens = (event["tokens"] as? Int ?? 0).formatted()
            appendMeta(
                "Cleared \(uses) old tool results (\(tokens) tokens) from the model's view.")
        case "context_usage":
            setContextUsage(AssistantContextUsage(
                used: event["used"] as? Int ?? 0, limit: event["limit"] as? Int, reported: true))
        case "turn_started":
            setBusy(true)
        case "tool_call":
            let name = event["name"] as? String ?? "?"
            appendMeta("⚙ \(name)\(argsSummary(event["arguments"]))")
        case "tool_result":
            if event["is_error"] as? Bool == true {
                appendMeta("   ✗ \(event["summary"] as? String ?? "failed")")
            }
        case "error":
            appendError(event["message"] as? String ?? "Unknown error.")
        case "turn_finished":
            setBusy(false)
        default:
            break
        }
    }

    private func argsSummary(_ arguments: Any?) -> String {
        guard let arguments = arguments as? [String: Any], !arguments.isEmpty else {
            return ""
        }
        let parts = arguments
            .filter { $0.key != "document_id" }
            .map { "\($0.key): \($0.value)" }
            .sorted()
            .joined(separator: ", ")
        guard !parts.isEmpty else { return "" }
        return " (\(String(parts.prefix(70))))"
    }

    private func setBusy(_ value: Bool) {
        busy = value
        sendButton.title = value ? "Stop" : "Send"
        sendButton.needsDisplay = true
        if value {
            spinner.startAnimation(nil)
        } else {
            spinner.stopAnimation(nil)
        }
    }

    // MARK: - Transcript

    private func append(_ text: String, font: NSFont, color: NSColor, spacingAbove: CGFloat) {
        guard let storage = transcript.textStorage else { return }
        let paragraph = NSMutableParagraphStyle()
        paragraph.paragraphSpacingBefore = spacingAbove
        paragraph.lineSpacing = 2
        let line = NSAttributedString(
            string: (storage.length > 0 ? "\n" : "") + text,
            attributes: [.font: font, .foregroundColor: color, .paragraphStyle: paragraph])
        storage.append(line)
        transcript.scrollToEndOfDocument(nil)
    }

    private func appendUser(_ text: String) {
        guard let storage = transcript.textStorage else { return }
        if storage.length > 0 {
            append("", font: DS.sans(4), color: .clear, spacingAbove: 0)
        }
        append(text, font: DS.sans(13, weight: .semibold), color: DS.textStrong, spacingAbove: 8)
    }

    /// Streams into the newest paragraph; the first delta of a run opens a
    /// new paragraph.
    private func appendDelta(
        _ text: String, type: String, font: NSFont, color: NSColor, spacingAbove: CGFloat
    ) {
        guard streamingType == type, let storage = transcript.textStorage, storage.length > 0
        else {
            append(text, font: font, color: color, spacingAbove: spacingAbove)
            streamingType = type
            return
        }
        let attributes = storage.attributes(at: storage.length - 1, effectiveRange: nil)
        storage.append(NSAttributedString(string: text, attributes: attributes))
        transcript.scrollToEndOfDocument(nil)
    }

    private func appendMeta(_ text: String) {
        append(text, font: DS.mono(11), color: DS.textFaint, spacingAbove: 4)
    }

    private func appendError(_ text: String) {
        append(text, font: DS.sans(12), color: .systemRed, spacingAbove: 6)
    }
}
