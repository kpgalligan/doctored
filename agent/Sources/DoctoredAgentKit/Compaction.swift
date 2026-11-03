import Anthropic
import Foundation

extension AgentSession {
    /// Compaction only ever runs when a turn ends: after its final reply, never between
    /// tool calls, and not after a cancel. Once the conversation has reached the user's
    /// auto-compact limit, one summary request is sent. A response that stops with
    /// `compaction` and holds a summary replaces the history with itself, as it came;
    /// anything else leaves the history unchanged, so the next turn end tries again
    /// (`reportCompaction`).
    ///
    /// `endsInToolCall` is a final reply that ends in tool calls that never ran (the loop
    /// has answered them with error results). The summary is then not asked for, as the
    /// SDK runner skipped it, and this returns true; the loop reports
    /// `compaction_failed` after it ends.
    func compactIfDue(_ turn: TurnState, endsInToolCall: Bool) async throws -> Bool {
        guard let limit = turn.settings.autoCompactTokens, contextUsed >= limit, !isCancelled else { return false }
        emit(.compactionStarted(used: contextUsed))
        if endsInToolCall {
            Log.warn("The pending compaction was skipped because the last turn ended with tool calls that were not run.")
            return true
        }
        let body = turn.setup.request.compaction(
            messages: history.messages, instructions: turn.setup.compactionInstructions)
        let reply = try await stream(body, turn)
        reportCompaction(reply)
        return false
    }

    /// A summary response: the summary block replaced the history, or nothing did.
    ///
    /// Success needs both the `compaction` stop reason and a `compaction` block whose
    /// `content` is a non-empty string. The API reports a summary it failed to write as
    /// a block with `content: null`, which it treats as a no-op when sent back, so
    /// swapping the history for one would drop the whole conversation for nothing.
    func reportCompaction(_ reply: Reply) {
        guard reply.stopReason == "compaction" else {
            // A response with no stop reason reads as JSON's `null`.
            let stop = reply.message["stop_reason"]?.stringValue ?? "null"
            emit(.compactionFailed(reason: "the summary stopped with \(stop)"))
            return
        }
        guard Self.holdsSummary(reply.message) else {
            Log.warn("Compaction produced no summary; keeping the conversation as it is.")
            emit(.compactionFailed(reason: "the summary came back empty"))
            return
        }
        let before = contextUsed
        contextUsed = ContextUsage.summarySize(of: reply.message)
        history.replaceWithSummary(reply.param)
        emit(.compacted(before: before, after: contextUsed))
        emit(.contextUsage(used: contextUsed, limit: contextLimit))
    }

    /// Whether `message` holds a `compaction` block with a non-empty `content` string.
    static func holdsSummary(_ message: RawJSON) -> Bool {
        (message["content"]?.arrayValue ?? []).contains { block in
            guard block["type"]?.stringValue == "compaction", let summary = block["content"]?.stringValue else {
                return false
            }
            return !summary.isEmpty
        }
    }
}
