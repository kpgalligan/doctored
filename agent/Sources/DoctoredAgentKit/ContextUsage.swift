import Anthropic
import Foundation

/// Context-meter arithmetic and the usage fields read from a response.
public enum ContextUsage {
    /// Tokens in the conversation after a response: everything the request sent, cached
    /// or not, plus the reply, which becomes part of the next request.
    public static func used(by message: RawJSON) -> Int {
        let usage = message["usage"]
        func count(_ key: String) -> Int { usage?[key]?.intValue ?? 0 }
        return count("input_tokens") + count("cache_read_input_tokens") + count("cache_creation_input_tokens")
            + count("output_tokens")
    }

    /// What remains of the conversation after a compaction: the summary's output tokens
    /// (its `compaction` iteration), until the next response's usage replaces the estimate.
    public static func summarySize(of message: RawJSON) -> Int {
        let iterations = message["usage"]?["iterations"]?.arrayValue ?? []
        let summary = iterations.first { $0["type"]?.stringValue == "compaction" }
        return summary?["output_tokens"]?.intValue ?? 0
    }

    /// One event per tool-result clearing the API applied to the request.
    public static func clearedToolResults(in message: RawJSON) -> [HelperEvent] {
        let edits = message["context_management"]?["applied_edits"]?.arrayValue ?? []
        return edits.compactMap { edit in
            guard edit["type"]?.stringValue == Request.clearEditType else { return nil }
            return .toolResultsCleared(
                toolUses: edit["cleared_tool_uses"]?.intValue ?? 0, tokens: edit["cleared_input_tokens"]?.intValue ?? 0)
        }
    }
}
