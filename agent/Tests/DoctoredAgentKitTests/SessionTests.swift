import Anthropic
import DoctoredAgentKit
import XCTest

/// configure, the refusals, settings, and the context limit.
final class SessionTests: XCTestCase {
    func testSendBeforeConfigureIsRefusedWithoutTurnEvents() async {
        let h = Harness()
        await h.line(HostMessage.send(text: "Hi").json)
        XCTAssertEqual(h.sink.events, [.error("The assistant helper was not configured.")])
        XCTAssertEqual(h.api.requests.count, 0)
    }

    func testASendDuringATurnIsRefused() async {
        let h = Harness(ScriptedAPI([.sse([SSE.messageStart()], hang: true)]))
        await h.configure()
        await h.line(HostMessage.send(text: "First").json)
        await h.sink.waitUntil { _ in h.api.requests.count == 1 }
        await h.line(HostMessage.send(text: "Second").json)
        XCTAssertEqual(h.sink.errors, ["The assistant is busy."])
        XCTAssertEqual(h.sink.count(of: .turnStarted), 1)
        await h.line(HostMessage.cancel.json)
        await h.sink.waitUntil { $0.contains(.turnFinished) }
        // The refused text never reached the history.
        let history = await h.history
        XCTAssertEqual(RawJSON.array(history).serializedString, #"[{"role":"user","content":[{"type":"text","text":"First"}]}]"#)
    }

    func testBadAndBlankLines() async {
        let h = Harness()
        await h.line("   ")
        await h.line("")
        await h.line("{nope")
        await h.line(#"{"type":"send"}"#)
        XCTAssertEqual(h.sink.events.count, 2)
        for message in h.sink.errors { XCTAssertTrue(message.hasPrefix("Bad message from the app: "), message) }
        XCTAssertEqual(h.sink.errors.last, "Bad message from the app: send needs a string text")
        // An unknown type, and the reset the app never sends, are ignored.
        await h.line(#"{"type":"reset"}"#)
        await h.line(#"{"type":"tool_result","call_id":"99","result":null}"#)
        XCTAssertEqual(h.sink.events.count, 2)
    }

    func testConfigureAnswersReadyThenTheContextLimit() async {
        let h = Harness()
        await h.configure()
        XCTAssertEqual(h.sink.events, [.ready, .contextUsage(used: 0, limit: 1_000_000)])
    }

    func testAModelsFailureLeavesTheLimitUnknown() async {
        let api = ScriptedAPI(models: .status(404, body: #"{"type":"error","error":{"type":"not_found_error","message":"no"}}"#))
        let h = Harness(api)
        await h.configure()
        XCTAssertEqual(h.sink.events, [.ready, .contextUsage(used: 0, limit: nil)])
    }

    func testSettingsApplyFromTheNextTurn() async throws {
        // Settings that change while a tool call waits must not reach the same turn's next round, nor its
        // end-of-turn compaction check: round 2's reply (200 tokens) is past the new limit of 100.
        let base = "thinking-display-updates-2026-08-18,server-side-fallback-2026-07-01,compact-2026-09-04"
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.toolUse(0, id: "toolu_s", name: "render", input: "{}"), stop: "tool_use")),
            .sse(SSE.reply(SSE.text(0, "rendered"), startUsage: #"{"input_tokens":190,"output_tokens":1}"#,
                           usage: #"{"output_tokens":10}"#)),
            .sse(SSE.reply(SSE.text(0, "ok"))),
        ]))
        await h.configure()
        await h.line(HostMessage.send(text: "one").json)
        await h.sink.waitUntil { _ in h.sink.toolRequests.count == 1 }
        await h.line(HostMessage.settings(ChatSettings(autoCompactTokens: 100, clearToolResults: true)).json)
        await h.answer(h.sink.toolRequests[0].callID, #"{"content":[{"type":"text","text":"ok"}]}"#)
        await h.sink.waitUntil { $0.contains(.turnFinished) }
        XCTAssertEqual(h.api.requests.count, 2)
        XCTAssertFalse(h.sink.events.contains { if case .compactionStarted = $0 { true } else { false } })
        for round in 0..<2 {
            XCTAssertNil(try h.api.body(round)["context_management"])
            XCTAssertEqual(h.api.requests[round].headers["anthropic-beta"], base)
        }
        await h.turn("two")
        XCTAssertNotNil(try h.api.body(2)["context_management"])
        XCTAssertEqual(h.api.requests[2].headers["anthropic-beta"], base + ",context-management-2025-06-27")
    }

    func testErrorWording() {
        let response = HTTPResponse(statusCode: 200, body: Data(), request: HTTPRequest(method: "POST", url: URL(fileURLWithPath: "/")))
        let mapped = APIStatusError("raw", response: response, body: ["error": ["type": "overloaded_error", "message": "Overloaded"]],
                                    statusCode: 529)
        XCTAssertEqual(AgentSession.describe(mapped), "API error 529: Overloaded")
        let unmapped = APIStatusError("{'error': ...}", response: response, body: ["error": ["type": "mystery", "message": "Odd"]])
        XCTAssertEqual(AgentSession.describe(unmapped), "API error: Odd")
        let bare = APIStatusError("Error code: 502", response: HTTPResponse(statusCode: 502, body: Data(), request: response.request), body: nil)
        XCTAssertEqual(AgentSession.describe(bare), "API error 502: Error code: 502")
        XCTAssertEqual(AgentSession.describe(APIConnectionError(request: nil)), "API error: Connection error.")
        XCTAssertEqual(AgentSession.describe(ProtocolError("send needs a string text")), "send needs a string text")
    }
}
