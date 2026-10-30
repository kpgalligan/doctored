import AppKit

/// One Assistant panel conversation, run by the agent helper in
/// Contents/Helpers — the Anthropic SDK's Tool Runner in a Bun-compiled
/// executable (`agent/`). JSON lines over the helper's stdin/stdout
/// (`agent/src/protocol.ts`): the helper streams text and asks for tool
/// calls, and each call runs here, on main, through the same dispatcher
/// the MCP server uses. Events reach `onEvent` on the main queue as parsed
/// objects ({"type": ...}).
final class AssistantHelperSession {
    var onEvent: (([String: Any]) -> Void)?

    private let process = Process()
    private let toHelper = Pipe()
    private let fromHelper = Pipe()
    /// Writes to the helper, off main: a tool result carrying a render can
    /// be megabytes, more than a pipe buffer holds.
    private let writeQueue = DispatchQueue(label: "doctored.assistant-helper.write")
    /// Touched only on the helper's readability handler thread.
    private var lineBuffer = Data()
    private var busy = false
    private var cancelRequested = false
    private var closed = false

    private init() {}

    /// The bundled helper, or `DR_ASSISTANT_HELPER` for a development build.
    static var executableURL: URL? {
        if let path = ProcessInfo.processInfo.environment["DR_ASSISTANT_HELPER"], !path.isEmpty {
            return URL(fileURLWithPath: path)
        }
        let bundled = Bundle.main.bundleURL
            .appendingPathComponent("Contents/Helpers/doctored-agent")
        return FileManager.default.isExecutableFile(atPath: bundled.path) ? bundled : nil
    }

    /// MCP tools the Assistant panel is never offered. `close_document` with
    /// `discard_changes` would let the model throw away unsaved work (and its
    /// recovery copy) without the user ever seeing the save prompt, so in the
    /// app the user closes a document; the helper only asks for tools in the
    /// list it is configured with, so dropping an entry here blocks the call.
    /// `catalogJSON()` itself stays whole: the MCP server and the
    /// catalog/handler parity check need every entry.
    static let panelExcludedTools: Set<String> = ["close_document"]

    static func open(
        apiKey: String, model: String, system: String, settings: AssistantChatSettings
    ) -> Result<AssistantHelperSession, OpenError>
    {
        guard let executable = executableURL else {
            return .failure(.init(message: "The assistant helper is not in this build."))
        }
        let tools: Any
        do {
            let catalog = try AgentServer.shared.catalogJSON()
            let parsed = try JSONSerialization.jsonObject(with: Data(catalog.utf8))
            if let entries = parsed as? [[String: Any]] {
                tools = entries.filter { entry in
                    !panelExcludedTools.contains(entry["name"] as? String ?? "")
                }
            } else {
                tools = parsed
            }
        } catch {
            return .failure(
                .init(message: "Could not build the tool catalog: \(error.localizedDescription)"))
        }
        var configure: [String: Any] = [
            "type": "configure", "api_key": apiKey, "model": model, "system": system,
            "tools": tools, "settings": settings.helperJSON,
            "core_tools": AssistantHelperPrompt.coreTools,
            "search_guide": AssistantHelperPrompt.searchGuide,
            "compaction_instructions": AssistantHelperPrompt.compactionInstructions,
        ]
        if let base = ProcessInfo.processInfo.environment["DR_ASSISTANT_BASE_URL"], !base.isEmpty {
            configure["api_base"] = base
        }

        let session = AssistantHelperSession()
        session.process.executableURL = executable
        session.process.standardInput = session.toHelper
        session.process.standardOutput = session.fromHelper
        // Nothing of the user's shell environment reaches the helper: the key
        // arrives over stdin, and no ANTHROPIC_* variable can override it.
        session.process.environment = [
            "HOME": NSHomeDirectory(), "TMPDIR": NSTemporaryDirectory(), "PATH": "/usr/bin:/bin",
        ]
        // A write after the helper has exited must fail, not raise SIGPIPE
        // and take the app down with it.
        _ = fcntl(session.toHelper.fileHandleForWriting.fileDescriptor, F_SETNOSIGPIPE, 1)
        session.fromHelper.fileHandleForReading.readabilityHandler = { [weak session] handle in
            session?.consume(handle.availableData)
        }
        session.process.terminationHandler = { [weak session] _ in
            DispatchQueue.main.async { session?.helperExited() }
        }
        do {
            try session.process.run()
        } catch {
            return .failure(
                .init(
                    message:
                        "Could not start the assistant helper: \(error.localizedDescription)"))
        }
        session.write(configure)
        return .success(session)
    }

    @discardableResult
    func send(_ text: String) -> Bool {
        guard !closed, process.isRunning, !busy else { return false }
        busy = true
        write(["type": "send", "text": text])
        return true
    }

    /// Chat Settings changed; applies from the next turn.
    func updateSettings(_ settings: AssistantChatSettings) {
        guard !closed else { return }
        write(["type": "settings", "settings": settings.helperJSON])
    }

    /// Stops the turn: the helper aborts its request, and tool calls it
    /// already asked for answer "Cancelled" instead of running.
    func cancel() {
        guard busy else { return }
        cancelRequested = true
        write(["type": "cancel"])
    }

    /// Idempotent. Closing stdin ends the helper; a helper that does not
    /// exit promptly is terminated.
    func close() {
        guard !closed else { return }
        closed = true
        onEvent = nil
        fromHelper.fileHandleForReading.readabilityHandler = nil
        let process = self.process
        let input = toHelper.fileHandleForWriting
        writeQueue.async {
            try? input.close()
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                if process.isRunning { process.terminate() }
            }
        }
    }

    // MARK: - Helper output

    /// Splits the stream into lines on the handler thread; each line is
    /// handled on main.
    private func consume(_ data: Data) {
        if data.isEmpty { return }  // EOF; termination is reported separately
        lineBuffer.append(data)
        while let newline = lineBuffer.firstIndex(of: UInt8(ascii: "\n")) {
            let line = lineBuffer[lineBuffer.startIndex..<newline]
            lineBuffer.removeSubrange(lineBuffer.startIndex...newline)
            guard
                let object = (try? JSONSerialization.jsonObject(with: Data(line)))
                    as? [String: Any]
            else { continue }
            DispatchQueue.main.async { [weak self] in self?.handle(object) }
        }
    }

    private func handle(_ event: [String: Any]) {
        guard !closed else { return }
        switch event["type"] as? String {
        case "tool_request":
            runTool(event)
            return
        case "turn_started":
            busy = true
            cancelRequested = false
        case "turn_finished":
            busy = false
        default:
            break
        }
        onEvent?(event)
    }

    /// Runs one tool the helper asked for and answers it. The helper may
    /// ask for several at once; each hop to main runs them one at a time.
    private func runTool(_ request: [String: Any]) {
        guard let callID = request["call_id"] as? String,
            let name = request["name"] as? String
        else { return }
        let arguments = request["arguments_json"] as? String ?? "{}"
        let result: String
        if cancelRequested {
            result =
                #"{"content":[{"type":"text","text":"Cancelled by the user."}],"isError":true}"#
        } else {
            // The App Nap assertion both agent trampolines hold (AppActivity).
            result = AppActivity.userInitiated("running the \(name) tool") {
                AgentServer.shared.execute(tool: name, argumentsJSON: arguments)
            }
        }
        // The result is already JSON; splice it in rather than re-encoding a render.
        guard let idJSON = try? JSONSerialization.data(withJSONObject: [callID]),
            let idArray = String(data: idJSON, encoding: .utf8)
        else { return }
        let quotedID = idArray.dropFirst().dropLast()
        writeLine(#"{"type":"tool_result","call_id":\#(quotedID),"result":\#(result)}"#)
    }

    private func helperExited() {
        guard !closed else { return }
        closed = true
        fromHelper.fileHandleForReading.readabilityHandler = nil
        onEvent?([
            "type": "error",
            "message": "The assistant helper stopped. Clear the conversation to start a new one.",
        ])
        if busy {
            busy = false
            onEvent?(["type": "turn_finished"])
        }
    }

    // MARK: - Writing

    private func write(_ message: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: message),
            let line = String(data: data, encoding: .utf8)
        else { return }
        writeLine(line)
    }

    private func writeLine(_ line: String) {
        let handle = toHelper.fileHandleForWriting
        let data = Data((line + "\n").utf8)
        writeQueue.async {
            try? handle.write(contentsOf: data)
        }
    }
}
