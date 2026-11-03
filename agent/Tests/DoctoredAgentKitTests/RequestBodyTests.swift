import Anthropic
import DoctoredAgentKit
import XCTest

/// What goes over the wire: the body's shape and headers, a prefix that stays the same
/// bytes, and assistant content replayed exactly as it was received.
final class RequestBodyTests: XCTestCase {
    func testARoundsBodyAndHeaders() async throws {
        let h = Harness(ScriptedAPI([.sse(SSE.reply(SSE.text(0, "ok")))]))
        await h.configure()
        await h.turn("Hi")
        let request = try XCTUnwrap(h.api.requests.first)
        XCTAssertEqual(request.method.uppercased(), "POST")
        XCTAssertEqual(request.url.path, "/v1/messages")
        // Slaude keeps `beta=true` as a client-side qualifier; the anthropic-beta header carries the betas.
        XCTAssertNil(request.query("beta"))
        XCTAssertEqual(request.headers["anthropic-version"], "2026-10-21")
        XCTAssertEqual(request.headers["anthropic-beta"],
                       "thinking-display-updates-2026-08-18,server-side-fallback-2026-07-01,compact-2026-09-04")
        XCTAssertEqual(request.headers["Authorization"], "Bearer sk-ant-dummy-test-key")
        let body = try h.api.body(0)
        XCTAssertEqual(body.objectValue?.keys, ["model", "max_tokens", "system", "tools", "messages", "cache_control",
                                                "output_config", "thinking", "fallbacks", "stream"])
        XCTAssertEqual(body["model"], "claude-opus-5-5")
        XCTAssertEqual(body["max_tokens"], 32000)
        XCTAssertEqual(body["system"], "You edit images.\n\nSearch for tools.")
        XCTAssertEqual(body["tools"]?.serializedString, """
            [{"type":"tool_search_tool_bm25_20251119","name":"tool_search_tool_bm25"},\
            {"name":"render","description":"The render tool.","input_schema":{"type":"object","properties":{"zoom":{"type":"number"},"document_id":{"type":"string"}}}},\
            {"name":"get_document","description":"The get_document tool.","input_schema":{"type":"object","properties":{}}},\
            {"name":"gaussian_blur","description":"The gaussian_blur tool.","input_schema":{"type":"object","required":["radius"],"properties":{"radius":{"type":"number"}}},"defer_loading":true}]
            """)
        XCTAssertEqual(body["cache_control"]?.serializedString, #"{"type":"ephemeral"}"#)
        XCTAssertEqual(body["output_config"]?.serializedString, #"{"effort":"medium"}"#)
        XCTAssertEqual(body["thinking"]?.serializedString, #"{"type":"adaptive","display":"updates"}"#)
        XCTAssertEqual(body["fallbacks"], "default")
        XCTAssertEqual(body["stream"], true)
        // 2026-10-21 rejects sampling parameters; none are sent.
        for key in ["temperature", "top_p", "top_k", "context_management"] { XCTAssertNil(body[key], key) }
    }

    func testClearingAddsItsEdit() async throws {
        let h = Harness(ScriptedAPI([.sse(SSE.reply(SSE.text(0, "ok")))]))
        await h.configure(Fixtures.configure(ChatSettings(autoCompactTokens: nil, clearToolResults: true)))
        await h.turn("Hi")
        XCTAssertEqual(try h.api.body(0)["context_management"]?.serializedString,
                       #"{"edits":[{"type":"clear_tool_uses_20250919","trigger":{"type":"input_tokens","value":150000},"keep":{"type":"tool_uses","value":5},"clear_at_least":{"type":"input_tokens","value":50000}}]}"#)
        XCTAssertEqual(try h.api.body(0).objectValue?.keys.suffix(2), ["context_management", "stream"])
    }

    func testThePrefixIsTheSameBytesOnEveryRound() async throws {
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.toolUse(0, id: "toolu_1", name: "render", input: "{}"), stop: "tool_use", id: "msg_1")),
            .sse(SSE.reply(SSE.thinking(0, ["More"], signature: "czI="),
                           SSE.toolUse(1, id: "toolu_2", name: "gaussian_blur", input: #"{"radius":3}"#), stop: "tool_use", id: "msg_2")),
            .sse(SSE.reply(SSE.text(0, "Blurred."), id: "msg_3")),
            .sse(SSE.reply(SSE.text(0, "Again."), id: "msg_4")),
        ]))
        await h.configure()
        await h.line(HostMessage.send(text: "Blur").json)
        for callID in ["1", "2"] {
            await h.sink.waitUntil { _ in h.sink.toolRequests.contains { $0.callID == callID } }
            await h.answer(callID, #"{"content":[{"type":"text","text":"done \#(callID)"}]}"#)
        }
        await h.sink.waitUntil { $0.contains(.turnFinished) }
        await h.turn("Once more")
        let bodies = try (0..<4).map { try h.api.body($0) }
        for (earlier, later) in zip(bodies, bodies.dropFirst()) {
            XCTAssertEqual(later["tools"]?.serialized(), earlier["tools"]?.serialized())
            XCTAssertEqual(later["system"]?.serialized(), earlier["system"]?.serialized())
            let before = try XCTUnwrap(earlier["messages"]?.arrayValue)
            let after = try XCTUnwrap(later["messages"]?.arrayValue)
            XCTAssertGreaterThan(after.count, before.count)
            // Each earlier message is sent again as the same bytes, except the trailing user
            // message, which only grows (the merge appends to it).
            for (index, message) in before.enumerated() where index < before.count - 1 || message["role"] != "user" {
                XCTAssertEqual(after[index].serialized(), message.serialized())
            }
        }
    }

    func testTwoHelpersBuildTheSameBytes() async throws {
        // Two sessions built from the same configure line, as two helper processes would be.
        let line = HostMessage.configure(Fixtures.configure()).json.serialized()
        var bodies: [Data] = []
        for _ in 0..<2 {
            let h = Harness(ScriptedAPI([.sse(SSE.reply(SSE.text(0, "ok")))]))
            await h.session.receive(line)
            await h.turn("Same text")
            bodies.append(try XCTUnwrap(h.api.requests.first?.body))
        }
        XCTAssertEqual(bodies[0], bodies[1])
    }

    func testRepliesAreReplayedByteForByte() async throws {
        let search = #"{"type":"server_tool_use","id":"srvtoolu_01","name":"tool_search_tool_bm25","input":{"query":"blur","limit":3}}"#
        // Keys out of alphabetical order, a field this SDK does not model, and a number's own text.
        let found = #"{"type":"tool_search_tool_result","tool_use_id":"srvtoolu_01","zeta_unmodelled":{"y":2,"x":[1,2.50,{"q":null}]},"content":{"type":"tool_search_tool_search_result","tool_references":[{"type":"tool_reference","tool_name":"gaussian_blur"}]}}"#
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.thinking(0, ["Plan", " it"], signature: "EqQBCkgIARAB/+sig=="), SSE.whole(1, search), SSE.whole(2, found),
                           SSE.text(3, "Found ", "it."))),
            .sse(SSE.reply(SSE.text(0, "ok"))),
        ]))
        await h.configure()
        await h.turn("Find blur")
        await h.turn("Thanks")
        let expected = #"{"role":"assistant","content":[{"type":"thinking","thinking":"Plan it","signature":"EqQBCkgIARAB/+sig=="},"#
            + search + "," + found + #",{"type":"text","text":"Found it."}]}"#
        XCTAssertEqual(try h.api.body(1)["messages"]?[1]?.serializedString, expected)
        let raw = String(decoding: try XCTUnwrap(h.api.requests[1].body), as: UTF8.self)
        XCTAssertTrue(raw.contains(expected), "the request body carries the reply's exact bytes")
    }
}
