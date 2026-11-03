import Anthropic
import Foundation

/// The Messages request bodies, built from one `configure`.
///
/// The tool list and the system prompt are built once per configure and reused as
/// the same `RawJSON` on every round, so the cached prefix (tools, system, history)
/// is the same bytes on every request of a conversation and in every process. The
/// body's keys are in the contract's order (designs/assistant-helper.md, "History,
/// requests and context"), which the request-body tests pin.
public struct Request: Sendable {
    /// Thinking counts toward this as well as the reply.
    public static let maxTokens = 32_000
    /// Hard ceiling on API rounds within one turn; reaching it ends the turn silently.
    public static let maxIterations = 30

    /// Betas every request carries: progress notes, refusal fallback, and compaction blocks.
    public static let baseBetas = [
        "thinking-display-updates-2026-08-18",
        "server-side-fallback-2026-07-01",
        "compact-2026-09-04",
    ]
    /// Added while the user has tool-result clearing on.
    public static let contextManagementBeta = "context-management-2025-06-27"

    /// Old tool results are cleared, when the user turns it on, only past this many
    /// tokens in context and in chunks of at least `clearAtLeastTokens`, so the prompt
    /// cache breaks rarely. The API applies it per request, so it can fire mid-turn; it
    /// never touches tool search results (server-side). The app's Chat Settings sheet
    /// repeats the trigger in its wording.
    public static let clearTriggerTokens = 150_000
    public static let clearAtLeastTokens = 50_000
    public static let clearKeepToolUses = 5
    static let clearEditType = "clear_tool_uses_20250919"

    /// The BM25 tool search server tool, first in `tools`.
    static let toolSearch: RawJSON = ["type": "tool_search_tool_bm25_20251119", "name": "tool_search_tool_bm25"]

    public let model: String
    /// The app's system prompt, a blank line, and the search guide.
    public let system: RawJSON
    /// Tool search, then the catalog in the app's order; every tool but the core ones is deferred.
    public let tools: RawJSON

    public init(_ configure: Configure) {
        model = configure.model
        system = .string("\(configure.system)\n\n\(configure.searchGuide)")
        let core = Set(configure.coreTools)
        tools = .array([Self.toolSearch] + configure.tools.map { tool in
            var o: RawJSONObject = ["name": .string(tool.name)]
            if let d = tool.description { o["description"] = d }
            if let s = tool.inputSchema { o["input_schema"] = s }
            if !core.contains(tool.name) { o["defer_loading"] = true }
            return .object(o)
        })
    }

    /// One round of the turn loop.
    public func round(messages: [RawJSON], settings: ChatSettings) -> RawJSON {
        var o = head(messages)
        if settings.clearToolResults {
            o["context_management"] = [
                "edits": [[
                    "type": .string(Self.clearEditType),
                    "trigger": ["type": "input_tokens", "value": .number(.init(Self.clearTriggerTokens))],
                    "keep": ["type": "tool_uses", "value": .number(.init(Self.clearKeepToolUses))],
                    "clear_at_least": ["type": "input_tokens", "value": .number(.init(Self.clearAtLeastTokens))],
                ]],
            ]
        }
        o["stream"] = true
        return .object(o)
    }

    /// The end-of-turn summary request: the round's body without `context_management`
    /// (the API refuses a compaction beside a compaction edit, and a summary only shapes
    /// no reply), plus `compaction`. The helper sends no `stop_sequences` or output format.
    public func compaction(messages: [RawJSON], instructions: String) -> RawJSON {
        var o = head(messages)
        o["stream"] = true
        o["compaction"] = ["type": "summarize", "instructions": .string(instructions)]
        return .object(o)
    }

    /// The `anthropic-beta` names for a turn's requests, the summary request included.
    public static func betas(_ settings: ChatSettings) -> [String] {
        settings.clearToolResults ? baseBetas + [contextManagementBeta] : baseBetas
    }

    private func head(_ messages: [RawJSON]) -> RawJSONObject {
        [
            "model": .string(model),
            "max_tokens": .number(.init(Self.maxTokens)),
            "system": system,
            "tools": tools,
            "messages": .array(messages),
            // Automatic caching: the breakpoint moves to the newest block each round,
            // so the tool catalog, system prompt and history are reused.
            "cache_control": ["type": "ephemeral"],
            // Thinking is always on for Opus 5.5; effort is the dial (its API default
            // is also medium, set here so it doesn't drift).
            "output_config": ["effort": "medium"],
            // The model's notes between tool calls arrive as thinking blocks; "updates"
            // returns them as short readable summaries.
            "thinking": ["type": "adaptive", "display": "updates"],
            // A classifier refusal is retried on the model Anthropic picks for it.
            "fallbacks": "default",
        ]
    }
}
