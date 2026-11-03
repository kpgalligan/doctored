import Anthropic
import DoctoredAgentKit
import Foundation
import XCTest

/// Records every event in order; tests wait on what they expect.
final class RecordingSink: EventSink, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [HelperEvent] = []

    func emit(_ event: HelperEvent) {
        // Each event must survive the wire: one line, read back as itself.
        let line = event.line
        precondition(line.last == 0x0A && !line.dropLast().contains(0x0A))
        lock.withLock { recorded.append(event) }
    }

    var events: [HelperEvent] { lock.withLock { recorded } }

    /// The events since the last `turn_started` up to and including `turn_finished`.
    var lastTurn: [HelperEvent] {
        let all = events
        guard let start = all.lastIndex(of: .turnStarted) else { return [] }
        return Array(all[start...])
    }

    var errors: [String] {
        events.compactMap { if case .error(let m) = $0 { return m } else { return nil } }
    }

    var toolRequests: [(callID: String, name: String, arguments: String)] {
        events.compactMap {
            if case .toolRequest(let id, let name, let args) = $0 { return (id, name, args) } else { return nil }
        }
    }

    /// Waits until `condition` holds over the events, failing the test after `timeout` seconds.
    func waitUntil(_ timeout: TimeInterval = 10, file: StaticString = #filePath, line: UInt = #line,
                   _ condition: ([HelperEvent]) -> Bool) async
    {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition(events) { return }
            try? await Task.sleep(nanoseconds: 2_000_000)
        }
        XCTFail("timed out waiting; events: \(events)", file: file, line: line)
    }

    func count(of event: HelperEvent) -> Int { events.filter { $0 == event }.count }
}

/// The API, scripted: each `POST /v1/messages` takes the next reply; `GET /v1/models/…`
/// answers `models`. Every request is recorded, and retry sleeps are recorded instead
/// of slept.
final class ScriptedAPI: @unchecked Sendable {
    enum Reply: Sendable {
        /// A `200` event stream: each event a chunk; with `hang`, the body never ends
        /// until the client closes it.
        case sse([String], hang: Bool = false)
        /// A JSON response with this status.
        case status(Int, headers: [String: String] = [:], body: String)
    }

    private let lock = NSLock()
    private var replies: [Reply]
    private var recorded: [HTTPRequest] = []
    private var slept: [TimeInterval] = []
    let models: Reply

    init(_ replies: [Reply] = [], models: Reply = .status(200, body: #"{"id":"claude-opus-5-5","max_input_tokens":1000000}"#)) {
        self.replies = replies
        self.models = models
    }

    func enqueue(_ reply: Reply) { lock.withLock { replies.append(reply) } }

    /// The Messages requests, in order.
    var requests: [HTTPRequest] { lock.withLock { recorded } }
    var sleeps: [TimeInterval] { lock.withLock { slept } }

    /// The body of Messages request `index`, parsed in its own key order.
    func body(_ index: Int) throws -> RawJSON {
        let all = requests
        guard all.indices.contains(index), let data = all[index].body else {
            throw ProtocolError("no request \(index) (\(all.count) sent)")
        }
        return try RawJSON.parse(data)
    }

    var factory: ClientFactory {
        ClientFactory(
            transport: { MockTransport { [self] request in try await self.handle(request) } },
            sleeper: { [self] seconds in lock.withLock { slept.append(seconds) } })
    }

    private func handle(_ request: HTTPRequest) async throws -> HTTPResponse {
        let reply: Reply
        if request.method.uppercased() == "GET" {
            reply = models
        } else {
            reply = try lock.withLock {
                recorded.append(request)
                guard !replies.isEmpty else { throw ProtocolError("no reply scripted for request \(recorded.count)") }
                return replies.removeFirst()
            }
        }
        switch reply {
        case .status(let code, let headers, let body):
            var h = HTTPHeaders(headers)
            h["content-type"] = "application/json"
            return HTTPResponse(statusCode: code, headers: h, body: Data(body.utf8), request: request)
        case .sse(let events, let hang):
            let (stream, continuation) = ByteStream.makeStream()
            for event in events { continuation.yield(Data(event.utf8)) }
            if !hang { continuation.finish() }
            return HTTPResponse(
                statusCode: 200, headers: ["content-type": "text/event-stream", "request-id": "req_test"],
                stream: stream, request: request, onClose: { continuation.finish() })
        }
    }
}

/// A session over a scripted API, driven with the app's lines.
struct Harness {
    let sink = RecordingSink()
    let api: ScriptedAPI
    let session: AgentSession

    init(_ api: ScriptedAPI = ScriptedAPI()) {
        self.api = api
        session = AgentSession(sink: sink, factory: api.factory)
    }

    func line(_ json: RawJSON) async { await session.receive(json.serialized()) }
    func line(_ text: String) async { await session.receive(Data(text.utf8)) }

    /// Configures the session and waits for `ready` and the context limit's report.
    func configure(_ configure: Configure = Fixtures.configure()) async {
        await line(HostMessage.configure(configure).json)
        await sink.waitUntil { $0.contains(.ready) && $0.contains { if case .contextUsage = $0 { true } else { false } } }
    }

    /// Sends user text and waits for the turn to finish.
    func turn(_ text: String) async {
        let finished = sink.count(of: .turnFinished)
        await line(HostMessage.send(text: text).json)
        await sink.waitUntil { events in events.filter { $0 == .turnFinished }.count > finished }
    }

    /// Answers one tool_request as the app does.
    func answer(_ callID: String, _ result: String) async {
        await line(#"{"type":"tool_result","call_id":"\#(callID)","result":\#(result)}"#)
    }

    var history: [RawJSON] { get async { await session.history.messages } }
}

enum Fixtures {
    static let catalog: [CatalogTool] = [
        // The schema's keys are deliberately not alphabetical: they must reach the request as sent.
        tool("render", #"{"type":"object","properties":{"zoom":{"type":"number"},"document_id":{"type":"string"}}}"#),
        tool("get_document", #"{"type":"object","properties":{}}"#),
        tool("gaussian_blur", #"{"type":"object","required":["radius"],"properties":{"radius":{"type":"number"}}}"#),
    ]

    static func tool(_ name: String, _ schema: String) -> CatalogTool {
        CatalogTool(name: name, description: .string("The \(name) tool."), inputSchema: try? RawJSON.parse(schema))
    }

    static func configure(_ settings: ChatSettings = ChatSettings(autoCompactTokens: nil, clearToolResults: false))
        -> Configure
    {
        Configure(
            apiKey: "sk-ant-dummy-test-key", model: "claude-opus-5-5", system: "You edit images.", tools: catalog,
            coreTools: ["render", "get_document", "list_documents", "open_document"], searchGuide: "Search for tools.",
            compactionInstructions: "Summarize the conversation.", settings: settings, apiBase: "http://127.0.0.1:9")
    }
}
