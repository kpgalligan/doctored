import Anthropic
import DoctoredAgentKit
import XCTest

final class ProtocolTests: XCTestCase {
    func testConfigureDecodesTheAppsLine() throws {
        // The shape AssistantHelperSession.open writes (JSONSerialization's key order is arbitrary).
        let line = #"""
            {"settings":{"auto_compact_tokens":null,"clear_tool_results":true},"type":"configure","model":"claude-opus-5-5",
             "api_key":"sk-dummy","system":"S","search_guide":"G","compaction_instructions":"C","api_base":"http://127.0.0.1:1",
             "core_tools":["render"],"tools":[{"inputSchema":{"type":"object","b":1,"a":2},"name":"render","description":"R"}]}
            """#
        guard case .configure(let c) = try HostMessage.decode(Data(line.utf8)) else { return XCTFail("not configure") }
        XCTAssertEqual(c.apiKey, "sk-dummy")
        XCTAssertEqual(c.model, "claude-opus-5-5")
        XCTAssertEqual(c.settings, ChatSettings(autoCompactTokens: nil, clearToolResults: true))
        XCTAssertEqual(c.apiBase, "http://127.0.0.1:1")
        XCTAssertEqual(c.coreTools, ["render"])
        XCTAssertEqual(c.tools.map(\.name), ["render"])
        // The schema keeps the app's key order.
        XCTAssertEqual(c.tools[0].inputSchema?.serializedString, #"{"type":"object","b":1,"a":2}"#)
    }

    func testHostMessagesRoundTrip() throws {
        let messages: [HostMessage] = [
            .configure(Fixtures.configure(ChatSettings(autoCompactTokens: 20_000, clearToolResults: true))),
            .send(text: "Blur the sky\u{2028}please"),
            .cancel,
            .settings(ChatSettings(autoCompactTokens: nil, clearToolResults: false)),
            .toolResult(callID: "7", result: .null),
            .toolResult(callID: "8", result: try RawJSON.parse(#"{"content":[{"type":"text","text":"ok"}],"isError":false}"#)),
            .other(type: "reset"),
        ]
        for message in messages {
            XCTAssertEqual(try HostMessage.decode(message.json.serialized()), message)
        }
    }

    func testToolResultKeepsAbsentApartFromNull() throws {
        XCTAssertEqual(try HostMessage.decode(Data(#"{"type":"tool_result","call_id":"1"}"#.utf8)),
                       .toolResult(callID: "1", result: nil))
        XCTAssertEqual(try HostMessage.decode(Data(#"{"type":"tool_result","call_id":"1","result":null}"#.utf8)),
                       .toolResult(callID: "1", result: .null))
    }

    func testBadLinesThrow() {
        for line in ["not json", "[1]", "null", #"{"type":"send"}"#, #"{"type":"tool_result","call_id":3}"#,
                     #"{"type":"configure","api_key":"k"}"#]
        {
            XCTAssertThrowsError(try HostMessage.decode(Data(line.utf8)), line)
        }
    }

    func testHelperEventsRoundTripOnOneLine() throws {
        let events: [HelperEvent] = [
            .ready, .turnStarted, .textDelta("Hi\n\"there\""), .progressDelta("Looking"),
            .contextUsage(used: 12, limit: nil), .contextUsage(used: 12, limit: 1_000_000),
            .toolCall(name: "render", arguments: try RawJSON.parse(#"{"zoom":2,"document_id":"d"}"#)),
            .toolRequest(callID: "1", name: "render", argumentsJSON: #"{"zoom":2}"#),
            .toolResult(name: "render", isError: true, summary: "bad"), .error("API error 529: Overloaded"),
            .compactionStarted(used: 300_000), .compacted(before: 300_000, after: 2_000),
            .compactionFailed(reason: "the summary stopped with end_turn"), .toolResultsCleared(toolUses: 3, tokens: 60_000),
            .turnFinished,
        ]
        for event in events {
            let line = event.line
            XCTAssertEqual(line.last, 0x0A)
            XCTAssertFalse(line.dropLast().contains(0x0A))
            XCTAssertEqual(HelperEvent(json: try RawJSON.parse(line.dropLast())), event)
        }
    }

    func testEventBytesAreTheContractsBytes() {
        // Compact JSON, `type` first, then the fields in the contract's order.
        XCTAssertEqual(String(decoding: HelperEvent.contextUsage(used: 5, limit: nil).line, as: UTF8.self),
                       "{\"type\":\"context_usage\",\"used\":5,\"limit\":null}\n")
        XCTAssertEqual(HelperEvent.toolRequest(callID: "3", name: "render", argumentsJSON: "{}").json.serializedString,
                       #"{"type":"tool_request","call_id":"3","name":"render","arguments_json":"{}"}"#)
        XCTAssertEqual(HelperEvent.toolResult(name: "x", isError: false, summary: "(image)").json.serializedString,
                       #"{"type":"tool_result","name":"x","is_error":false,"summary":"(image)"}"#)
        XCTAssertEqual(HelperEvent.toolResultsCleared(toolUses: 2, tokens: 9).json.serializedString,
                       #"{"type":"tool_results_cleared","tool_uses":2,"tokens":9}"#)
    }

    func testLineSplitterFramesOnLineFeedOnly() {
        var splitter = LineSplitter()
        var lines = splitter.append(Data("{\"a\":1}\r\n{\"b\":".utf8))
        lines += splitter.append(Data("\"x\u{2028}y\"}\n\n".utf8))
        lines += splitter.append(Data("{\"c\":3}".utf8))
        XCTAssertEqual(lines.map { String(decoding: $0, as: UTF8.self) }, ["{\"a\":1}", "{\"b\":\"x\u{2028}y\"}", ""])
        XCTAssertEqual(splitter.finish().map { String(decoding: $0, as: UTF8.self) }, "{\"c\":3}")
        XCTAssertNil(splitter.finish())
    }

    func testLineSplitterTakesAMegabyteLineInChunks() {
        var splitter = LineSplitter()
        let payload = String(repeating: "A", count: 3_000_000)
        let whole = Data("{\"data\":\"\(payload)\"}\n".utf8)
        var lines: [Data] = []
        var offset = 0
        while offset < whole.count {
            let end = min(offset + 65_536, whole.count)
            lines += splitter.append(whole.subdata(in: offset..<end))
            offset = end
        }
        XCTAssertEqual(lines.count, 1)
        XCTAssertEqual(lines.first?.count, whole.count - 1)
    }
}

final class ToolResultsTests: XCTestCase {
    private func convert(_ json: String?) throws -> ConvertedToolResult {
        try ToolResults.convert(json.map { try RawJSON.parse($0) })
    }

    func testTextAndImageBecomeBlocks() throws {
        let result = try convert(#"{"content":[{"type":"text","text":"Rendered."},{"type":"image","data":"QUJD"},{"type":"image","data":"REVG","mimeType":"image/jpeg"}]}"#)
        XCTAssertFalse(result.isError)
        XCTAssertEqual(result.summary, "Rendered.")
        XCTAssertEqual(RawJSON.array(result.blocks).serializedString,
                       #"[{"type":"text","text":"Rendered."},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}},{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"REVG"}}]"#)
    }

    func testImageFirstSummarizesAsImage() throws {
        XCTAssertEqual(try convert(#"{"content":[{"type":"image","data":"QQ=="},{"type":"text","text":"t"}]}"#).summary, "(image)")
    }

    func testFailuresBecomeErrorText() throws {
        for (json, message) in [(nil, "The tool failed to execute."), ("null", "The tool failed to execute."),
                                (#"{"content":[]}"#, "The tool returned no content."),
                                (#"{"content":[{"type":"resource","uri":"x"}]}"#, "The tool returned no content."),
                                (#"{"isError":true}"#, "The tool returned no content.")]
        {
            let result = try convert(json)
            XCTAssertTrue(result.isError)
            XCTAssertEqual(result.summary, message)
            XCTAssertEqual(result.blocks, [["type": "text", "text": .string(message)]])
        }
    }

    func testAnErrorKeepsItsBlocksAndImages() throws {
        let result = try convert(#"{"content":[{"type":"text","text":"No such layer."},{"type":"image","data":"QQ=="}],"isError":true}"#)
        XCTAssertTrue(result.isError)
        XCTAssertEqual(result.blocks.count, 2)
        XCTAssertEqual(ToolResults.block(toolUseID: "toolu_1", result).serializedString,
                       #"{"type":"tool_result","tool_use_id":"toolu_1","content":[{"type":"text","text":"No such layer."},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"QQ=="}}],"is_error":true}"#)
    }

    func testSuccessBlockHasNoErrorFlag() throws {
        let block = ToolResults.block(toolUseID: "toolu_2", try convert(#"{"content":[{"type":"text","text":"ok"}]}"#))
        XCTAssertEqual(block.serializedString, #"{"type":"tool_result","tool_use_id":"toolu_2","content":[{"type":"text","text":"ok"}]}"#)
        XCTAssertEqual(ToolResults.notFound(toolUseID: "toolu_3", name: "nope").serializedString,
                       #"{"type":"tool_result","tool_use_id":"toolu_3","content":"Error: Tool 'nope' not found","is_error":true}"#)
    }

    func testSummaryKeepsTwoHundredUTF16Units() throws {
        let long = String(repeating: "x", count: 250)
        XCTAssertEqual(try convert(#"{"content":[{"type":"text","text":"\#(long)"}]}"#).summary.count, 200)
        // A surrogate pair at the boundary is not split.
        let emoji = String(repeating: "x", count: 199) + "😀" + "tail"
        let summary = try convert(#"{"content":[{"type":"text","text":"\#(emoji)"}]}"#).summary
        XCTAssertEqual(summary, String(repeating: "x", count: 199))
        // The block keeps the whole text.
        XCTAssertEqual(try convert(#"{"content":[{"type":"text","text":"\#(long)"}]}"#).blocks[0]["text"]?.stringValue, long)
    }
}

final class HistoryTests: XCTestCase {
    func testUserTextStartsOrMergesIntoTheTrailingUserMessage() throws {
        var history = History()
        history.pushUserText("one")
        history.pushUserText("two")
        XCTAssertEqual(RawJSON.array(history.messages).serializedString,
                       #"[{"role":"user","content":[{"type":"text","text":"one"},{"type":"text","text":"two"}]}]"#)
        history.append(try RawJSON.parse(#"{"role":"assistant","content":[{"type":"text","text":"hi"}]}"#))
        history.pushUserText("three")
        XCTAssertEqual(history.messages.count, 3)
        XCTAssertEqual(history.messages[2].serializedString, #"{"role":"user","content":[{"type":"text","text":"three"}]}"#)
    }

    func testMergeKeepsTheMessagesKeysAndTurnsStringContentIntoBlocks() throws {
        var history = History([try RawJSON.parse(#"{"content":"cancelled","role":"user","extra":1}"#)])
        history.pushUserText("again")
        XCTAssertEqual(history.messages[0].serializedString,
                       #"{"content":[{"type":"text","text":"cancelled"},{"type":"text","text":"again"}],"role":"user","extra":1}"#)
    }

    func testContextArithmetic() throws {
        let message = try RawJSON.parse(#"{"usage":{"input_tokens":100,"cache_read_input_tokens":2000,"cache_creation_input_tokens":null,"output_tokens":40,"iterations":[{"type":"message","output_tokens":9},{"type":"compaction","output_tokens":321}]},"context_management":{"applied_edits":[{"type":"clear_tool_uses_20250919","cleared_tool_uses":4,"cleared_input_tokens":61000},{"type":"clear_thinking_20251015","cleared_thinking_turns":1}]}}"#)
        XCTAssertEqual(ContextUsage.used(by: message), 2140)
        XCTAssertEqual(ContextUsage.summarySize(of: message), 321)
        XCTAssertEqual(ContextUsage.clearedToolResults(in: message), [.toolResultsCleared(toolUses: 4, tokens: 61000)])
        XCTAssertEqual(ContextUsage.used(by: try RawJSON.parse("{}")), 0)
        XCTAssertEqual(ContextUsage.summarySize(of: try RawJSON.parse(#"{"usage":{}}"#)), 0)
    }
}
