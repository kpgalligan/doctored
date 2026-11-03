import Anthropic
import DoctoredAgentKit
import XCTest

final class CancelTests: XCTestCase {
    func testACancelMidStreamDropsTheReplyAndReportsNothing() async throws {
        let h = Harness(ScriptedAPI([
            .sse([SSE.messageStart(), SSE.start(0, #"{"type":"text","text":""}"#), SSE.delta(0, #"{"type":"text_delta","text":"Hel"}"#)],
                 hang: true),
            .sse(SSE.reply(SSE.text(0, "Fine"))),
        ]))
        await h.configure()
        await h.line(HostMessage.send(text: "First").json)
        await h.sink.waitUntil { $0.contains(.textDelta("Hel")) }
        await h.line(HostMessage.cancel.json)
        await h.sink.waitUntil { $0.contains(.turnFinished) }
        XCTAssertEqual(h.sink.lastTurn, [.turnStarted, .textDelta("Hel"), .turnFinished])
        XCTAssertEqual(h.api.requests.count, 1)
        // The history is as it was before the reply; the next text merges into the unanswered message.
        var history = await h.history
        XCTAssertEqual(RawJSON.array(history).serializedString, #"[{"role":"user","content":[{"type":"text","text":"First"}]}]"#)
        await h.turn("Second")
        history = await h.history
        XCTAssertEqual(try h.api.body(1)["messages"]?.serializedString,
                       #"[{"role":"user","content":[{"type":"text","text":"First"},{"type":"text","text":"Second"}]}]"#)
        XCTAssertEqual(history.count, 2)
        XCTAssertEqual(h.sink.errors, [])
    }

    func testACancelWithToolsPendingWaitsForEveryAnswer() async throws {
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.toolUse(0, id: "toolu_a", name: "render", input: "{}"),
                           SSE.toolUse(1, id: "toolu_b", name: "gaussian_blur", input: #"{"radius":4}"#), stop: "tool_use")),
            .sse(SSE.reply(SSE.text(0, "Carrying on"))),
        ]))
        await h.configure()
        await h.line(HostMessage.send(text: "Blur").json)
        await h.sink.waitUntil { _ in h.sink.toolRequests.count == 2 }
        await h.line(HostMessage.cancel.json)
        // The turn waits on: the app answers each call it was asked for, as AssistantHelperSession does after a cancel.
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertFalse(h.sink.events.contains(.turnFinished))
        let cancelled = #"{"content":[{"type":"text","text":"Cancelled by the user."}],"isError":true}"#
        for request in h.sink.toolRequests { await h.answer(request.callID, cancelled) }
        await h.sink.waitUntil { $0.contains(.turnFinished) }
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(3)), [
            .toolResult(name: "render", isError: true, summary: "Cancelled by the user."),
            .toolResult(name: "gaussian_blur", isError: true, summary: "Cancelled by the user."), .turnFinished,
        ])
        XCTAssertEqual(h.api.requests.count, 1)  // nothing is sent after the cancel
        XCTAssertEqual(h.sink.errors, [])
        // Every tool_use kept its tool_result, so the next turn is a valid request.
        await h.turn("Go on")
        let messages = try XCTUnwrap(try h.api.body(1)["messages"]?.arrayValue)
        XCTAssertEqual(messages.count, 3)
        let results = try XCTUnwrap(messages[2]["content"]?.arrayValue)
        XCTAssertEqual(results.map { $0["tool_use_id"]?.stringValue }, ["toolu_a", "toolu_b", nil])
        XCTAssertEqual(results.last?.serializedString, #"{"type":"text","text":"Go on"}"#)
        XCTAssertEqual(results.first?["is_error"], true)
    }

    func testACancelBeforeTheHeadersSendsNothingMore() async {
        // The first request waits on a hanging retry sleep; a cancel ends it without a retry.
        let gate = ScriptedAPI([.status(529, body: #"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#),
                                .sse(SSE.reply(SSE.text(0, "late")))])
        let factory = ClientFactory(transport: gate.factory.transport, sleeper: { _ in try await Task.sleep(nanoseconds: 60_000_000_000) })
        let sink = RecordingSink()
        let session = AgentSession(sink: sink, factory: factory)
        await session.receive(HostMessage.configure(Fixtures.configure()).json.serialized())
        await session.receive(HostMessage.send(text: "Hi").json.serialized())
        await sink.waitUntil { _ in gate.requests.count == 1 }
        await session.receive(HostMessage.cancel.json.serialized())
        await sink.waitUntil { $0.contains(.turnFinished) }
        XCTAssertEqual(gate.requests.count, 1)
        XCTAssertEqual(sink.errors, [])
    }
}

final class CompactionTests: XCTestCase {
    private let summary = SSE.reply(
        [SSE.start(0, #"{"type":"compaction","content":null}"#),
         SSE.delta(0, #"{"type":"compaction_delta","content":"The user blurred the sky."}"#), SSE.stop(0)],
        stop: "compaction", usage: #"{"output_tokens":321,"iterations":[{"type":"compaction","input_tokens":110,"output_tokens":321}]}"#)

    private func harness(_ replies: [ScriptedAPI.Reply]) async -> Harness {
        let h = Harness(ScriptedAPI(replies))
        await h.configure(Fixtures.configure(ChatSettings(autoCompactTokens: 100, clearToolResults: true)))
        return h
    }

    /// A final reply that brings the context to 110 tokens, past the limit of 100.
    private let finalReply = ScriptedAPI.Reply.sse(SSE.reply(SSE.text(0, "Done"), startUsage: #"{"input_tokens":90,"output_tokens":1}"#,
                                                             usage: #"{"output_tokens":20}"#))

    func testASummaryReplacesTheHistory() async throws {
        let h = await harness([finalReply, .sse(summary), .sse(SSE.reply(SSE.text(0, "Hello again")))])
        await h.turn("Blur the sky")
        XCTAssertEqual(h.sink.lastTurn, [
            .turnStarted, .textDelta("Done"), .contextUsage(used: 110, limit: 1_000_000), .compactionStarted(used: 110),
            .compacted(before: 110, after: 321), .contextUsage(used: 321, limit: 1_000_000), .turnFinished,
        ])
        let request = try h.api.body(1)
        XCTAssertEqual(request.objectValue?.keys,
                       ["model", "max_tokens", "system", "tools", "messages", "cache_control", "output_config", "thinking",
                        "fallbacks", "stream", "compaction"])
        XCTAssertEqual(request["compaction"]?.serializedString, #"{"type":"summarize","instructions":"Summarize the conversation."}"#)
        XCTAssertEqual(request["messages"]?.arrayValue?.count, 2)
        let history = await h.history
        XCTAssertEqual(RawJSON.array(history).serializedString,
                       #"[{"role":"assistant","content":[{"type":"compaction","content":"The user blurred the sky."}]}]"#)
        await h.turn("Next")
        XCTAssertEqual(try h.api.body(2)["messages"]?.serializedString,
                       #"[{"role":"assistant","content":[{"type":"compaction","content":"The user blurred the sky."}]},{"role":"user","content":[{"type":"text","text":"Next"}]}]"#)
    }

    func testASummaryThatDoesNotStopWithCompactionChangesNothing() async {
        let h = await harness([finalReply, .sse(SSE.reply(SSE.text(0, "Here is a summary")))])
        await h.turn("Blur the sky")
        // A summary request's stream is forwarded like any other.
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(4)), [
            .compactionStarted(used: 110), .textDelta("Here is a summary"),
            .compactionFailed(reason: "the summary stopped with end_turn"), .turnFinished,
        ])
        let history = await h.history
        XCTAssertEqual(history.count, 2)
        XCTAssertEqual(history[1]["content"]?[0]?["text"], "Done")
    }

    func testASummaryWithNoContentChangesNothing() async throws {
        // The API reports a summary it failed to write as a compaction block whose content is null
        // (no compaction_delta fills it in) or empty; swapping the history for it would lose everything.
        let empties = [
            [SSE.start(0, #"{"type":"compaction","content":null}"#), SSE.stop(0)],
            [SSE.start(0, #"{"type":"compaction","content":null}"#),
             SSE.delta(0, #"{"type":"compaction_delta","content":null}"#), SSE.stop(0)],
            [SSE.start(0, #"{"type":"compaction","content":""}"#), SSE.stop(0)],
        ]
        for blocks in empties {
            let empty = SSE.reply(blocks, stop: "compaction",
                                  usage: #"{"output_tokens":3,"iterations":[{"type":"compaction","input_tokens":110,"output_tokens":3}]}"#)
            let h = await harness([finalReply, .sse(empty), .sse(SSE.reply(SSE.text(0, "Hello again")))])
            await h.turn("Blur the sky")
            XCTAssertEqual(Array(h.sink.lastTurn.suffix(3)), [
                .compactionStarted(used: 110), .compactionFailed(reason: "the summary came back empty"), .turnFinished,
            ])
            XCTAssertFalse(h.sink.events.contains { if case .compacted = $0 { true } else { false } })
            let history = await h.history
            XCTAssertEqual(RawJSON.array(history).serializedString,
                           #"[{"role":"user","content":[{"type":"text","text":"Blur the sky"}]},{"role":"assistant","content":[{"type":"text","text":"Done"}]}]"#)
            // The next turn sends the whole conversation, not the empty block.
            await h.turn("Next")
            XCTAssertEqual(try h.api.body(2)["messages"]?.arrayValue?.count, 3)
        }
    }

    func testAFinalReplyEndingInAToolCallIsNotCompacted() async throws {
        let cut = ScriptedAPI.Reply.sse(SSE.reply(SSE.toolUse(0, id: "toolu_c", name: "render", input: "{}"), stop: "max_tokens",
                                                  startUsage: #"{"input_tokens":200,"output_tokens":1}"#))
        let h = await harness([cut, .sse(SSE.reply(SSE.text(0, "Carrying on")))])
        await h.turn("Render")
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(4)), [
            .compactionStarted(used: 205), .compactionFailed(reason: "no summary came back"),
            .error("The reply was cut off at the output limit."), .turnFinished,
        ])
        XCTAssertEqual(h.api.requests.count, 1)
        // The call never ran and no tool_request went out, but it was answered, so the next request is valid:
        // the tool_use is followed straight away by its tool_result, then the new text.
        XCTAssertTrue(h.sink.toolRequests.isEmpty)
        await h.turn("Go on")
        let messages = try XCTUnwrap(try h.api.body(1)["messages"]?.arrayValue)
        XCTAssertEqual(messages.count, 3)
        XCTAssertEqual(messages[1]["content"]?[0]?["type"], "tool_use")
        XCTAssertEqual(messages[2].serializedString,
                       #"{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_c","content":"Not run: the reply stopped with max_tokens before this call could run.","is_error":true},{"type":"text","text":"Go on"}]}"#)
    }

    func testAFinalReplysUnrunToolCallsAreAnsweredWithoutCompaction() async throws {
        // Compaction off: the same answer, and no compaction events.
        let h = Harness(ScriptedAPI([
            .sse(SSE.reply(SSE.text(0, "Let me"), SSE.toolUse(1, id: "toolu_e", name: "render", input: "{}"),
                           SSE.toolUse(2, id: "toolu_f", name: "nope", input: "{}"), stop: "max_tokens")),
            .sse(SSE.reply(SSE.text(0, "ok"))),
        ]))
        await h.configure()
        await h.turn("Render")
        XCTAssertEqual(Array(h.sink.lastTurn.suffix(2)), [.error("The reply was cut off at the output limit."), .turnFinished])
        await h.turn("Again")
        let messages = try XCTUnwrap(try h.api.body(1)["messages"]?.arrayValue)
        let results = try XCTUnwrap(messages[2]["content"]?.arrayValue)
        XCTAssertEqual(results.map { $0["tool_use_id"]?.stringValue }, ["toolu_e", "toolu_f", nil])
        XCTAssertEqual(results.map { $0["is_error"] }, [true, true, nil])
    }

    func testNoCompactionBelowTheLimitOrMidTurn() async {
        let h = await harness([
            .sse(SSE.reply(SSE.toolUse(0, id: "toolu_d", name: "nope", input: "{}"), stop: "tool_use",
                           startUsage: #"{"input_tokens":500,"output_tokens":1}"#)),
            .sse(SSE.reply(SSE.text(0, "Small"))),
        ])
        await h.turn("Go")
        XCTAssertEqual(h.api.requests.count, 2)
        XCTAssertFalse(h.sink.events.contains { if case .compactionStarted = $0 { true } else { false } })
    }
}
