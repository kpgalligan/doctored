import Anthropic
import DoctoredAgentKit
import XCTest

/// Whole turns over scripted SSE: deltas, tools, stop reasons, retries and errors.
final class TurnTests: XCTestCase {
    func testTextAndProgressDeltas() async throws {
        let h = Harness(ScriptedAPI([.sse(SSE.reply(
            SSE.thinking(0, ["Checking", " the layers"], signature: "c2ln"), SSE.text(1, "Done", "."),
            startUsage: #"{"input_tokens":100,"cache_read_input_tokens":20,"cache_creation_input_tokens":5,"output_tokens":1}"#,
            usage: #"{"output_tokens":50}"#))]))
        await h.configure()
        await h.turn("Hi")
        XCTAssertEqual(h.sink.lastTurn, [
            .turnStarted, .progressDelta("Checking"), .progressDelta(" the layers"), .textDelta("Done"), .textDelta("."),
            .contextUsage(used: 175, limit: 1_000_000), .turnFinished,
        ])
        let history = await h.history
        XCTAssertEqual(history.count, 2)
        XCTAssertEqual(history[1].serializedString,
                       #"{"role":"assistant","content":[{"type":"thinking","thinking":"Checking the layers","signature":"c2ln"},{"type":"text","text":"Done."}]}"#)
    }

    func testAMultiToolRoundTripWithAnImage() async throws {
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.text(0, "Let me look."), SSE.toolUse(1, id: "toolu_a", name: "render", input: #"{"zoom""#, #":2}"#),
                           SSE.toolUse(2, id: "toolu_b", name: "get_document", input: #"{"document_id":"d1"}"#), stop: "tool_use")),
            .sse(SSE.reply(SSE.text(0, "A cat."))),
        ]))
        await h.configure()
        await h.line(HostMessage.send(text: "What is it?").json)
        await h.sink.waitUntil { _ in h.sink.toolRequests.count == 2 }
        // Every tool call of the response is asked for at once, before any answer.
        XCTAssertEqual(Array(h.sink.lastTurn.dropFirst(3)), [
            .toolCall(name: "render", arguments: try RawJSON.parse(#"{"zoom":2}"#)),
            .toolRequest(callID: "1", name: "render", argumentsJSON: #"{"zoom":2}"#),
            .toolCall(name: "get_document", arguments: try RawJSON.parse(#"{"document_id":"d1"}"#)),
            .toolRequest(callID: "2", name: "get_document", argumentsJSON: #"{"document_id":"d1"}"#),
        ])
        await h.answer("1", #"{"content":[{"type":"image","data":"iVBORw0KGgo="}]}"#)
        await h.answer("2", #"{"content":[{"type":"text","text":"{\"name\":\"cat.png\"}"}]}"#)
        await h.sink.waitUntil { $0.contains(.turnFinished) }
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(5)), [
            .toolResult(name: "render", isError: false, summary: "(image)"),
            .toolResult(name: "get_document", isError: false, summary: #"{"name":"cat.png"}"#),
            .textDelta("A cat."), .contextUsage(used: 15, limit: 1_000_000), .turnFinished,
        ])
        let messages = try XCTUnwrap(try h.api.body(1)["messages"]?.arrayValue)
        XCTAssertEqual(messages.count, 3)
        XCTAssertEqual(messages[2].serializedString,
                       #"{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_a","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}]},{"type":"tool_result","tool_use_id":"toolu_b","content":[{"type":"text","text":"{\"name\":\"cat.png\"}"}]}]}"#)
        XCTAssertEqual(h.sink.errors, [])
    }

    func testStopReasonsReportAfterTheReply() async {
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.text(0, "I can't"), stop: "refusal")),
            .sse(SSE.reply(SSE.text(0, "Long"), stop: "max_tokens")),
            .sse(SSE.reply(SSE.text(0, "Full"), stop: "model_context_window_exceeded")),
        ]))
        await h.configure()
        await h.turn("a")
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(2)), [.error("The model declined this request."), .turnFinished])
        await h.turn("b")
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(2)), [.error("The reply was cut off at the output limit."), .turnFinished])
        await h.turn("c")
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(2)), [
            .error("The reply was cut off because the conversation filled the model's context window."), .turnFinished,
        ])
    }

    func testA429IsRetriedAfterItsRetryAfter() async throws {
        let h = Harness(ScriptedAPI([
            .status(429, headers: ["retry-after": "2"], body: #"{"type":"error","error":{"type":"rate_limit_error","message":"Slow down"}}"#),
            .sse(SSE.reply(SSE.text(0, "ok"))),
        ]))
        await h.configure()
        await h.turn("Hi")
        XCTAssertEqual(h.sink.errors, [])
        XCTAssertEqual(h.api.requests.count, 2)
        XCTAssertEqual(h.api.sleeps, [2])
        XCTAssertEqual(h.api.requests[0].body, h.api.requests[1].body)
        XCTAssertEqual(h.api.requests[1].headers["x-stainless-retry-count"], "1")
    }

    func testRetriesStopAfterThree() async {
        let overloaded = ScriptedAPI.Reply.status(529, body: #"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#)
        let h = Harness(ScriptedAPI([overloaded, overloaded, overloaded, overloaded, .sse(SSE.reply(SSE.text(0, "late")))]))
        await h.configure()
        await h.turn("Hi")
        XCTAssertEqual(h.api.requests.count, 4)
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(2)), [.error("API error 529: Overloaded"), .turnFinished])
    }

    func testAMidStreamErrorEventIsReportedNotRetried() async {
        let h = Harness(ScriptedAPI([
            .sse([SSE.messageStart(), SSE.start(0, #"{"type":"text","text":""}"#), SSE.delta(0, #"{"type":"text_delta","text":"Hel"}"#),
                  SSE.errorEvent("overloaded_error", "Overloaded")]),
            .sse([SSE.messageStart(), SSE.errorEvent("mystery_error", "Something odd")]),
        ]))
        await h.configure()
        await h.turn("Hi")
        XCTAssertEqual(h.sink.lastTurn, [.turnStarted, .textDelta("Hel"), .error("API error 529: Overloaded"), .turnFinished])
        XCTAssertEqual(h.api.requests.count, 1)
        // An error type with no status of its own never reads "API error 200".
        await h.turn("Again")
        XCTAssertEqual(h.sink.errors.last, "API error: Something odd")
        // Neither partial reply reached the history; the texts merged.
        let history = await h.history
        XCTAssertEqual(RawJSON.array(history).serializedString,
                       #"[{"role":"user","content":[{"type":"text","text":"Hi"},{"type":"text","text":"Again"}]}]"#)
    }

    func testAStreamCutShortIsAnErrorAndAddsNothing() async {
        let h = Harness(ScriptedAPI([
            .sse([SSE.messageStart(), SSE.start(0, #"{"type":"text","text":""}"#), SSE.delta(0, #"{"type":"text_delta","text":"Hel"}"#)]),
        ]))
        await h.configure()
        await h.turn("Hi")
        XCTAssertEqual(h.sink.lastTurn, [
            .turnStarted, .textDelta("Hel"), .error("API error: The stream ended before its `message_stop` event."), .turnFinished,
        ])
        let history = await h.history
        XCTAssertEqual(history.count, 1)
    }

    func testPauseTurnIsSentBackToResume() async throws {
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.whole(0, #"{"type":"server_tool_use","id":"srvtoolu_1","name":"tool_search_tool_bm25","input":{"query":"blur"}}"#),
                           stop: "pause_turn")),
            .sse(SSE.reply(SSE.text(0, "Resumed"))),
        ]))
        await h.configure()
        await h.turn("Blur it")
        let messages = try XCTUnwrap(try h.api.body(1)["messages"]?.arrayValue)
        XCTAssertEqual(messages.map { $0["role"]?.stringValue }, ["user", "assistant"])
        XCTAssertEqual(h.sink.errors, [])
    }

    func testAnUnknownToolIsAnsweredHereAndThirtyRoundsEndSilently() async throws {
        let call = ScriptedAPI.Reply.sse(SSE.reply(SSE.toolUse(0, id: "toolu_x", name: "close_document", input: "{}"), stop: "tool_use"))
        let h = Harness(ScriptedAPI(Array(repeating: call, count: Request.maxIterations + 1)))
        await h.configure()
        await h.turn("Loop")
        XCTAssertEqual(h.api.requests.count, Request.maxIterations)
        XCTAssertEqual(h.sink.errors, [])
        XCTAssertEqual(h.sink.toolRequests.count, 0)
        let last = try XCTUnwrap(try h.api.body(1)["messages"]?.arrayValue?.last)
        XCTAssertEqual(last.serializedString,
                       #"{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_x","content":"Error: Tool 'close_document' not found","is_error":true}]}"#)
    }

    func testEachAppliedClearingIsReported() async {
        let edits = #","context_management":{"applied_edits":[{"type":"clear_tool_uses_20250919","cleared_tool_uses":3,"cleared_input_tokens":52000}]}"#
        let h = Harness(ScriptedAPI([.sse(SSE.reply(SSE.text(0, "ok"), extra: edits))]))
        await h.configure(Fixtures.configure(ChatSettings(autoCompactTokens: nil, clearToolResults: true)))
        await h.turn("Hi")
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(3)), [
            .contextUsage(used: 15, limit: 1_000_000), .toolResultsCleared(toolUses: 3, tokens: 52000), .turnFinished,
        ])
    }
}
