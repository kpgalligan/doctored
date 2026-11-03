import Anthropic
import DoctoredAgentKit
import XCTest

/// The built `doctored-agent` executable over its real stdio, as the app runs it: the
/// scrubbed environment, a loopback API reached through `configure.api_base` over the
/// real URLSession transport, and a dummy key.
final class ProcessTests: XCTestCase {
    /// `swift build --build-tests` (which `swift test` runs) leaves the executable beside the test bundle.
    private var helper: URL {
        get throws {
            let products = Bundle(for: Self.self).bundleURL.deletingLastPathComponent()
            let url = products.appendingPathComponent("doctored-agent")
            guard FileManager.default.isExecutableFile(atPath: url.path) else {
                throw ProtocolError("no doctored-agent at \(url.path); build the package first")
            }
            return url
        }
    }

    /// Bytes a pipe's handler appends to from its own queue.
    private final class Buffer: @unchecked Sendable {
        private let lock = NSLock()
        private var bytes = Data()
        func append(_ data: Data) { lock.withLock { bytes.append(data) } }
        var data: Data { lock.withLock { bytes } }
    }

    private struct Run {
        let events: [HelperEvent]
        let stderr: String
        let status: Int32
    }

    /// Writes `lines`, then plays the app: each tool_request is answered with `answer`'s
    /// result JSON. After `turn_finished` it closes stdin and waits for the exit.
    private func run(_ lines: [String], answer: (String) -> String = { _ in "null" }) throws -> Run {
        let process = Process()
        process.executableURL = try helper
        process.environment = ["HOME": NSHomeDirectory(), "TMPDIR": NSTemporaryDirectory(), "PATH": "/usr/bin:/bin"]
        let input = Pipe(), output = Pipe(), errors = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = errors
        let stdout = Buffer(), stderr = Buffer()
        output.fileHandleForReading.readabilityHandler = { stdout.append($0.availableData) }
        errors.fileHandleForReading.readabilityHandler = { stderr.append($0.availableData) }
        try process.run()
        defer { if process.isRunning { process.terminate() } }
        func write(_ line: String) { input.fileHandleForWriting.write(Data((line + "\n").utf8)) }
        lines.forEach(write)
        var events: [HelperEvent] = []
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline, !events.contains(.turnFinished) {
            let complete = stdout.data.split(separator: 0x0A, omittingEmptySubsequences: false).dropLast()
            for line in complete.dropFirst(events.count) {
                // stdout is the protocol only: every line one event.
                let event = try XCTUnwrap(HelperEvent(json: try RawJSON.parse(Data(line))), String(decoding: line, as: UTF8.self))
                events.append(event)
                if case .toolRequest(let callID, let name, _) = event {
                    write(#"{"type":"tool_result","call_id":"\#(callID)","result":\#(answer(name))}"#)
                }
            }
            usleep(2_000)
        }
        try input.fileHandleForWriting.close()
        while Date() < deadline, process.isRunning { usleep(5_000) }
        XCTAssertFalse(process.isRunning, "the helper exits when stdin closes")
        process.waitUntilExit()
        output.fileHandleForReading.readabilityHandler = nil
        errors.fileHandleForReading.readabilityHandler = nil
        stderr.append(errors.fileHandleForReading.readDataToEndOfFile())
        return Run(events: events, stderr: String(decoding: stderr.data, as: UTF8.self), status: process.terminationStatus)
    }

    private func configureLine(_ server: LoopbackServer) -> String {
        var configure = Fixtures.configure()
        configure.apiBase = server.baseURL
        return HostMessage.configure(configure).json.serializedString
    }

    func testTheHelperSpeaksTheProtocolAndBuildsTheSameBytesInEveryProcess() throws {
        let sse = SSE.reply(SSE.thinking(0, ["Looking"], signature: "c2ln"), SSE.text(1, "Hello")).joined()
        var bodies: [Data] = []
        for _ in 0..<2 {
            let server = try LoopbackServer([sse])
            defer { server.stop() }
            let run = try run(["{not json", configureLine(server), HostMessage.send(text: "Hi").json.serializedString])
            XCTAssertEqual(run.status, 0)
            guard case .error(let bad) = run.events.first else { return XCTFail("\(run.events)") }
            XCTAssertTrue(bad.hasPrefix("Bad message from the app: "), bad)
            XCTAssertEqual(run.events.dropFirst().first, .ready)
            // The 404 from the Models API leaves the limit unknown.
            XCTAssertTrue(run.events.contains(.contextUsage(used: 0, limit: nil)))
            XCTAssertEqual(run.events.filter { if case .contextUsage = $0 { false } else { true } }.suffix(4),
                           [.turnStarted, .progressDelta("Looking"), .textDelta("Hello"), .turnFinished])
            for line in run.stderr.split(separator: "\n") { XCTAssertTrue(line.hasPrefix("doctored-agent: "), String(line)) }
            let posts = server.requests.filter { $0.line.hasPrefix("POST /v1/messages") }
            XCTAssertEqual(posts.count, 1)
            XCTAssertTrue(server.requests.contains { $0.line.hasPrefix("GET /v1/models/claude-opus-5-5") })
            bodies.append(try XCTUnwrap(posts.first?.body))
        }
        XCTAssertEqual(bodies[0], bodies[1], "two helper processes send the same bytes")
        XCTAssertFalse(bodies[0].isEmpty)
    }

    func testAMultiMegabyteRenderGoesBackOverTheRealTransport() throws {
        let server = try LoopbackServer([
            SSE.reply(SSE.toolUse(0, id: "toolu_r", name: "render", input: "{}"), stop: "tool_use").joined(),
            SSE.reply(SSE.text(0, "Seen.")).joined(),
        ])
        defer { server.stop() }
        // About 6 MB of base64, as a large canvas render is.
        let image = String(repeating: "iVBORw0KGgoAAAANSUhEUgAA", count: 250_000)
        let started = Date()
        let run = try run([configureLine(server), HostMessage.send(text: "Look").json.serializedString]) { _ in
            #"{"content":[{"type":"image","data":"\#(image)","mimeType":"image/png"}]}"#
        }
        let elapsed = Date().timeIntervalSince(started)
        XCTAssertEqual(run.status, 0)
        XCTAssertTrue(run.events.contains(.toolResult(name: "render", isError: false, summary: "(image)")))
        XCTAssertEqual(run.events.last, .turnFinished)
        XCTAssertFalse(run.events.contains { if case .error = $0 { true } else { false } }, "\(run.events)")
        let posts = server.requests.filter { $0.line.hasPrefix("POST /v1/messages") }
        XCTAssertEqual(posts.count, 2)
        let body = try RawJSON.parse(try XCTUnwrap(posts.last?.body))
        let result = body["messages"]?[2]?["content"]?[0]
        XCTAssertEqual(result?["content"]?[0]?["source"]?["data"]?.stringValue?.count, image.count)
        print("multi-megabyte round trip: \(posts.last?.body.count ?? 0) byte body, \(String(format: "%.2f", elapsed)) s (debug)")
    }
}
