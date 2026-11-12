import Anthropic
import Foundation

/// How a response's stop reason continues the loop (the SDK tool runner's buckets).
enum NextStep {
    /// Run its client tool calls, then send the next round.
    case runTools
    /// Send it back unchanged so the server continues it (`pause_turn`, `compaction`).
    case resume
    /// The turn's final reply: end_turn, stop_sequence, max_tokens, refusal,
    /// model_context_window_exceeded, no stop reason, or one this helper does not know.
    case stop

    init(stopReason: String?) {
        switch stopReason {
        case "tool_use": self = .runTools
        case "pause_turn", "compaction": self = .resume
        default: self = .stop
        }
    }
}

/// One complete response: the Message, and the turn it becomes in `messages`.
struct Reply {
    let message: RawJSON
    /// `{"role": "assistant", "content": [...]}`, every block as the API sent it.
    let param: RawJSON

    var stopReason: String? { message["stop_reason"]?.stringValue }

    var toolUses: [RawJSON] {
        (message["content"]?.arrayValue ?? []).filter { $0["type"]?.stringValue == "tool_use" }
    }
}

extension AgentSession {
    /// The agent loop of one `send`. Each round streams one response; a tool_use reply
    /// asks the app for every tool call at once and waits for every answer; the loop
    /// ends at the final reply (then compacts if due), at a cancel, at an error, or
    /// silently after `Request.maxIterations` rounds.
    ///
    /// A cancelled or failed response is never added to the history, and a cancel
    /// reports nothing. What did land stays: the user's text, complete replies, and
    /// tool results, which the next `send` may merge its text into (after a cancel, an
    /// error, the round cap, or a final reply whose tool calls were answered unrun).
    func runTurn(_ id: Int) async {
        defer { finishTurn(id) }
        guard let turn, turn.id == id else { return }
        var last: Reply?
        var compactionSkipped = false
        do {
            var rounds = 0
            loop: while rounds < Request.maxIterations {
                if isCancelled { return }
                rounds += 1
                let reply = try await stream(turn.setup.request.round(messages: history.messages, settings: turn.settings), turn)
                last = reply
                contextUsed = ContextUsage.used(by: reply.message)
                emit(.contextUsage(used: contextUsed, limit: contextLimit))
                for event in ContextUsage.clearedToolResults(in: reply.message) { emit(event) }
                history.append(reply.param)
                switch NextStep(stopReason: reply.stopReason) {
                case .runTools:
                    let uses = reply.toolUses
                    if uses.isEmpty { break loop }
                    let results = await runTools(uses, turn)
                    history.append(["role": "user", "content": .array(results)])
                case .resume:
                    continue
                case .stop:
                    // A final reply can still end in tool calls (max_tokens cut it off after
                    // a tool_use). They never run, but each gets an error result now: the API
                    // rejects any request whose tool_use has no tool_result straight after it,
                    // so without one every later send would fail.
                    let unanswered = reply.toolUses
                    if !unanswered.isEmpty {
                        let stop = reply.stopReason ?? "null"
                        let results = unanswered.map {
                            ToolResults.notRun(toolUseID: $0["id"] ?? .null, stopReason: stop)
                        }
                        history.append(["role": "user", "content": .array(results)])
                    }
                    compactionSkipped = try await compactIfDue(turn, endsInToolCall: !unanswered.isEmpty)
                    break loop
                }
            }
            if compactionSkipped { emit(.compactionFailed(reason: "no summary came back")) }
            switch last?.stopReason {
            case "max_tokens": emit(.error("The reply was cut off at the output limit."))
            case "refusal": emit(.error("The model declined this request."))
            case "model_context_window_exceeded":
                emit(.error("The reply was cut off because the conversation filled the model's context window."))
            default: break
            }
        } catch {
            if !isCancelled { emit(.error(Self.describe(error))) }
        }
    }

    /// Sends one request and streams its response: text deltas become text_delta, and
    /// thinking deltas (display "updates") progress_delta. Throws CancellationError on
    /// a cancel and the API's error for a failed request or an `error` event; returns
    /// only a response that reached `message_stop`.
    func stream(_ body: RawJSON, _ turn: TurnState) async throws -> Reply {
        let stream = try await turn.setup.api.stream(body: body, betas: Request.betas(turn.settings))
        self.turn?.stream = stream
        defer { self.turn?.stream = nil }
        // A cancel that came while the request waited for its headers.
        if isCancelled { stream.close() }
        var blockTypes: [Int: String] = [:]
        for try await event in stream {
            let json = event.json
            switch json["type"]?.stringValue {
            case "content_block_start":
                if let index = json["index"]?.intValue {
                    blockTypes[index] = json["content_block"]?["type"]?.stringValue
                }
            case "content_block_delta":
                guard let index = json["index"]?.intValue, let delta = json["delta"] else { break }
                switch (delta["type"]?.stringValue, blockTypes[index]) {
                case ("text_delta", "text"):
                    if let text = delta["text"]?.stringValue { emit(.textDelta(text)) }
                case ("thinking_delta", "thinking"):
                    if let text = delta["thinking"]?.stringValue { emit(.progressDelta(text)) }
                default:
                    break
                }
            default:
                break
            }
        }
        let message = try await stream.getFinalMessage()
        guard let param = stream.messageParam else {
            throw AnthropicError("The stream ended before a `message_start` event was received")
        }
        return Reply(message: message, param: param)
    }

    /// Asks the app for every tool call of one response at once, then waits for every
    /// answer, in the response's order. A tool the helper was not configured with is
    /// answered here, as the SDK runner answered it.
    func runTools(_ uses: [RawJSON], _ turn: TurnState) async -> [RawJSON] {
        var answers: [RawJSON?] = Array(repeating: nil, count: uses.count)
        var waits: [(index: Int, callID: String)] = []
        for (index, use) in uses.enumerated() {
            let name = use["name"]?.stringValue ?? ""
            let toolUseID = use["id"] ?? .null
            guard turn.setup.toolNames.contains(name) else {
                answers[index] = ToolResults.notFound(toolUseID: toolUseID, name: name)
                continue
            }
            let input = use["input"] ?? [:]
            emit(.toolCall(name: name, arguments: input))
            nextCallID += 1
            let callID = String(nextCallID)
            expectAnswer(callID, name: name, toolUseID: toolUseID)
            emit(.toolRequest(callID: callID, name: name, argumentsJSON: input.serializedString))
            waits.append((index, callID))
        }
        for wait in waits {
            answers[wait.index] = await waitForAnswer(wait.callID)
        }
        return answers.compactMap { $0 }
    }
}
