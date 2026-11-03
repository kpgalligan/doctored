import Anthropic
import Foundation

/// One helper's conversation: what `configure` set up, the history, the turn in
/// flight and the tool calls it waits on. The stdin loop hands it each line in order
/// (`receive`); a turn runs in its own task on this actor, so a `cancel` or a
/// `tool_result` is handled between the turn's suspension points.
public actor AgentSession {
    /// What one `configure` set up.
    struct Setup {
        let api: any MessagesAPI
        let request: Request
        let toolNames: Set<String>
        let compactionInstructions: String
        let model: String
        let generation: Int
    }

    /// The turn in flight. It keeps what it started with: a `settings` or `configure`
    /// message applies from the next turn.
    struct TurnState {
        let id: Int
        let setup: Setup
        let settings: ChatSettings
        var task: Task<Void, Never>?
        var cancelled = false
        /// The reply being streamed, closed by a cancel.
        var stream: ReplyStream?
    }

    /// A tool_request the app has not answered yet, or an answer no one waits for yet.
    struct PendingCall {
        let name: String
        let toolUseID: RawJSON
        var answer: RawJSON?
        var waiter: CheckedContinuation<RawJSON, Never>?
    }

    let sink: any EventSink
    let factory: ClientFactory
    var setup: Setup?
    /// Chat Settings; configure sets them and a settings message replaces them between turns.
    var settings = ChatSettings.initial
    public internal(set) var history = History()
    var turn: TurnState?
    var nextTurnID = 0
    var nextCallID = 0
    var generation = 0
    /// Tokens in the conversation as of the last response, and the model's window.
    var contextUsed = 0
    var contextLimit: Int?
    var calls: [String: PendingCall] = [:]

    public init(sink: any EventSink, factory: ClientFactory = .live) {
        self.sink = sink
        self.factory = factory
    }

    func emit(_ event: HelperEvent) {
        sink.emit(event)
    }

    /// Handles one line from the app. A blank line is skipped; one that does not parse
    /// is answered with an error event and changes nothing.
    public func receive(_ line: Data) {
        if line.allSatisfy({ $0 == 0x20 || ($0 >= 0x09 && $0 <= 0x0D) }) { return }
        let message: HostMessage
        do {
            message = try HostMessage.decode(line)
        } catch {
            emit(.error("Bad message from the app: \(Self.describe(error))"))
            return
        }
        switch message {
        case .configure(let configure): self.configure(configure)
        case .send(let text): startTurn(text)
        case .cancel: cancelTurn()
        case .toolResult(let callID, let result): answer(callID, result)
        case .settings(let settings): self.settings = settings
        case .other: break
        }
    }

    /// The stdin loop ended: the app is gone or closed the session.
    public func shutdown() async {
        cancelTurn()
        await setup?.api.close()
    }

    // MARK: - configure

    private func configure(_ message: Configure) {
        let api: any MessagesAPI
        do {
            api = try factory.make(apiKey: message.apiKey, baseURL: message.apiBase)
        } catch {
            emit(.error("Bad message from the app: \(Self.describe(error))"))
            return
        }
        if let old = setup?.api {
            // A running turn keeps the client it started with; it closes after that turn.
            let running = turn?.task
            Task {
                await running?.value
                await old.close()
            }
        }
        generation += 1
        setup = Setup(
            api: api, request: Request(message), toolNames: Set(message.tools.map(\.name)),
            compactionInstructions: message.compactionInstructions, model: message.model, generation: generation)
        settings = message.settings
        emit(.ready)
        lookUpContextLimit(api, model: message.model, generation: generation)
    }

    /// The model's context window, from the Models API, for the panel's meter.
    private func lookUpContextLimit(_ api: any MessagesAPI, model: String, generation: Int) {
        Task {
            let limit = await api.maxInputTokens(model: model)
            guard generation == self.generation else { return }
            contextLimit = limit
            emit(.contextUsage(used: contextUsed, limit: contextLimit))
        }
    }

    // MARK: - turns

    private func startTurn(_ text: String) {
        guard let setup else {
            emit(.error("The assistant helper was not configured."))
            return
        }
        guard turn == nil else {
            emit(.error("The assistant is busy."))
            return
        }
        nextTurnID += 1
        let id = nextTurnID
        turn = TurnState(id: id, setup: setup, settings: settings)
        emit(.turnStarted)
        history.pushUserText(text)
        // The task cannot start before this method returns, so `task` is set first.
        turn?.task = Task { await self.runTurn(id) }
    }

    /// Stops the turn: the reply being streamed is closed and nothing after it is sent.
    /// Tool calls already asked for are still waited on; the app answers them
    /// "Cancelled by the user.", so every tool_use in the history keeps a tool_result.
    private func cancelTurn() {
        guard turn != nil else { return }
        turn?.cancelled = true
        turn?.stream?.close()
        turn?.task?.cancel()
    }

    /// Always the last event of a turn.
    func finishTurn(_ id: Int) {
        guard turn?.id == id else { return }
        turn = nil
        emit(.turnFinished)
    }

    var isCancelled: Bool { turn?.cancelled ?? true }

    // MARK: - tool calls

    /// Registers a call before its tool_request is written, so the answer always finds it.
    func expectAnswer(_ callID: String, name: String, toolUseID: RawJSON) {
        calls[callID] = PendingCall(name: name, toolUseID: toolUseID)
    }

    /// The app's answer: converted, reported, and handed to the turn waiting on it.
    /// An answer to no pending call (an unknown id, or a second answer) is ignored.
    private func answer(_ callID: String, _ result: RawJSON?) {
        guard var call = calls[callID], call.answer == nil else { return }
        let converted = ToolResults.convert(result)
        emit(.toolResult(name: call.name, isError: converted.isError, summary: converted.summary))
        let block = ToolResults.block(toolUseID: call.toolUseID, converted)
        if let waiter = call.waiter {
            calls[callID] = nil
            waiter.resume(returning: block)
        } else {
            call.answer = block
            calls[callID] = call
        }
    }

    /// The tool_result block for `callID`, once the app answers. Never cancelled: a
    /// cancel leaves the wait to the app's "Cancelled by the user." answer.
    func waitForAnswer(_ callID: String) async -> RawJSON {
        if let block = calls[callID]?.answer {
            calls[callID] = nil
            return block
        }
        return await withCheckedContinuation { continuation in
            calls[callID]?.waiter = continuation
        }
    }

    // MARK: - errors

    /// An error as the panel shows it: `API error <status>: <error.message>` for an API
    /// error, `API error: <message>` for one with no status (a connection that failed or
    /// timed out, a stream that ended early, an `error` event of a type Slaude does not
    /// map), else the error's own message.
    public static func describe(_ error: any Error) -> String {
        if let error = error as? APIStatusError {
            let detail = error.errorMessage ?? error.message
            // An `error` event of a type Slaude does not map keeps the stream's 200,
            // which is not the error's status.
            let status = (200..<300).contains(error.statusCode) ? "" : " \(error.statusCode)"
            return "API error\(status): \(detail)".trimmingCharacters(in: .whitespacesAndNewlines)
        }
        if let error = error as? APIError {
            return "API error: \(error.message)".trimmingCharacters(in: .whitespacesAndNewlines)
        }
        if let error = error as? AnthropicError { return error.message }
        if let error = error as? ProtocolError { return error.message }
        if let error = error as? LocalizedError, let text = error.errorDescription { return text }
        return String(describing: error)
    }
}
