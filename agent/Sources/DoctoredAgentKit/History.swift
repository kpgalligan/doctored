import Anthropic
import Foundation

/// The conversation, kept as the JSON that was sent and received and replayed verbatim.
///
/// It is append-only: nothing already sent is ever rewritten. Opus 5.5 replays its
/// earlier thinking blocks only while everything before them is byte-identical (and
/// rejects the request outright on newer accounts), and an unchanged prefix is also
/// what keeps the prompt cache warm, so old canvas renders stay in it. Each assistant
/// message is the stream's `messageParam`: every block as the API sent it, thinking
/// signatures, tool search results, compaction and fallback blocks and fields this
/// SDK does not model included, in their key order (`RawJSON`). The one sanctioned
/// rewrite is the API's own compaction at the end of a turn (`replaceWithSummary`).
/// The one merge is `pushUserText`'s, into a user message the API never answered.
public struct History: Sendable, Equatable {
    public private(set) var messages: [RawJSON]

    public init(_ messages: [RawJSON] = []) {
        self.messages = messages
    }

    /// Appends user text, merging it into a trailing user message: tool results that
    /// a cancel or an error left unanswered, or a message whose request failed.
    public mutating func pushUserText(_ text: String) {
        let block: RawJSON = ["type": "text", "text": .string(text)]
        guard let last = messages.last, last["role"]?.stringValue == "user", case .object(var message) = last else {
            messages.append(["role": "user", "content": [block]])
            return
        }
        message.modifyValue(forKey: "content") { content in
            switch content {
            case .string(let s): content = [["type": "text", "text": .string(s)], block]
            case .array(var blocks):
                content = .null
                blocks.append(block)
                content = .array(blocks)
            default: content = [block]
            }
        }
        messages[messages.count - 1] = .object(message)
    }

    /// Appends one message: an assistant reply as received, or the user's tool results.
    public mutating func append(_ message: RawJSON) {
        messages.append(message)
    }

    /// The compaction swap: the summary response, sent back as it came, replaces the
    /// messages it summarizes.
    public mutating func replaceWithSummary(_ message: RawJSON) {
        messages = [message]
    }
}
