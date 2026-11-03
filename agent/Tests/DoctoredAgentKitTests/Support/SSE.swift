import Anthropic
import Foundation

/// Messages stream events as the API writes them, for scripted replies.
enum SSE {
    static func event(_ name: String, _ data: String) -> String { "event: \(name)\ndata: \(data)\n\n" }

    /// A JSON string literal for `text`.
    static func quoted(_ text: String) -> String { RawJSON.string(text).serializedString }

    static func messageStart(id: String = "msg_1", usage: String = #"{"input_tokens":10,"output_tokens":1}"#) -> String {
        event("message_start", #"{"type":"message_start","message":{"id":"\#(id)","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null,"stop_sequence":null,"usage":\#(usage)}}"#)
    }

    static func start(_ index: Int, _ block: String) -> String {
        event("content_block_start", #"{"type":"content_block_start","index":\#(index),"content_block":\#(block)}"#)
    }

    static func delta(_ index: Int, _ delta: String) -> String {
        event("content_block_delta", #"{"type":"content_block_delta","index":\#(index),"delta":\#(delta)}"#)
    }

    static func stop(_ index: Int) -> String {
        event("content_block_stop", #"{"type":"content_block_stop","index":\#(index)}"#)
    }

    static func text(_ index: Int, _ parts: String...) -> [String] {
        [start(index, #"{"type":"text","text":""}"#)]
            + parts.map { delta(index, #"{"type":"text_delta","text":\#(quoted($0))}"#) } + [stop(index)]
    }

    static func thinking(_ index: Int, _ parts: [String], signature: String) -> [String] {
        [start(index, #"{"type":"thinking","thinking":"","signature":""}"#)]
            + parts.map { delta(index, #"{"type":"thinking_delta","thinking":\#(quoted($0))}"#) }
            + [delta(index, #"{"type":"signature_delta","signature":\#(quoted(signature))}"#), stop(index)]
    }

    /// A tool_use whose input arrives in `parts` of JSON text.
    static func toolUse(_ index: Int, id: String, name: String, input parts: String...) -> [String] {
        [start(index, #"{"type":"tool_use","id":"\#(id)","name":"\#(name)","input":{}}"#)]
            + parts.map { delta(index, #"{"type":"input_json_delta","partial_json":\#(quoted($0))}"#) } + [stop(index)]
    }

    /// A block that arrives whole in its start event (server tool use and results).
    static func whole(_ index: Int, _ block: String) -> [String] { [start(index, block), stop(index)] }

    static func messageEnd(_ stop: String?, usage: String = #"{"output_tokens":5}"#, extra: String = "") -> [String] {
        let reason = stop.map(quoted) ?? "null"
        return [
            event("message_delta", #"{"type":"message_delta","delta":{"stop_reason":\#(reason),"stop_sequence":null},"usage":\#(usage)\#(extra)}"#),
            event("message_stop", #"{"type":"message_stop"}"#),
        ]
    }

    /// A whole reply.
    static func reply(_ blocks: [String]..., stop: String? = "end_turn", startUsage: String = #"{"input_tokens":10,"output_tokens":1}"#,
                      usage: String = #"{"output_tokens":5}"#, extra: String = "", id: String = "msg_1") -> [String]
    {
        [messageStart(id: id, usage: startUsage)] + blocks.flatMap { $0 } + messageEnd(stop, usage: usage, extra: extra)
    }

    static func errorEvent(_ type: String, _ message: String) -> String {
        event("error", #"{"type":"error","error":{"type":"\#(type)","message":\#(quoted(message))}}"#)
    }
}
