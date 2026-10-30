import AppKit
import Darwin

/// One MCP reply over the agent socket, written by curl's termination handler and read on main.
final class AgentHTTPReply: @unchecked Sendable {
    private let lock = NSLock()
    private var body: Data?
    private var failure: String?
    private var finishedAt: Date?
    let started = Date()

    var isDone: Bool {
        lock.lock()
        defer { lock.unlock() }
        return finishedAt != nil
    }

    func finish(data: Data?, error: Error?) {
        lock.lock()
        body = data
        failure = error.map { "\($0)" }
        finishedAt = Date()
        lock.unlock()
    }

    /// Seconds from the request to its reply.
    var elapsed: TimeInterval {
        lock.lock()
        defer { lock.unlock() }
        return (finishedAt ?? Date()).timeIntervalSince(started)
    }

    /// The CallToolResult: its isError flag, first text and the raw body.
    func result() throws -> (isError: Bool, text: String, raw: String) {
        lock.lock()
        let data = body
        let error = failure
        lock.unlock()
        if let error { throw ProbeFailure(description: "HTTP MCP call failed: \(error)") }
        guard let data,
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let result = root["result"] as? [String: Any] else {
            throw ProbeFailure(description: "Malformed HTTP MCP reply: "
                               + String(decoding: data ?? Data(), as: UTF8.self))
        }
        let content = result["content"] as? [[String: Any]] ?? []
        let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String ?? ""
        return (result["isError"] as? Bool == true, text, String(decoding: data, as: UTF8.self))
    }
}

/// Set once `AgentServer.unbindSocket()` returns; read by the watchdog thread.
private final class ReleaseReturned: @unchecked Sendable {
    private let lock = NSLock()
    private var returned = false
    var isSet: Bool {
        lock.lock()
        defer { lock.unlock() }
        return returned
    }
    func set() {
        lock.lock()
        returned = true
        lock.unlock()
    }
}

/// curl's stdout, gathered off main as it arrives (so a reply larger than the
/// pipe's buffer never stalls curl), and curl's exit: the reply is finished
/// once both the end of stdout and the exit have been seen, whichever is last.
private final class CurlCall: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()
    private var sawEnd = false
    private var exit: (status: Int32, stderr: String)?
    private var finished = false
    private let reply: AgentHTTPReply

    init(reply: AgentHTTPReply) { self.reply = reply }

    func append(_ chunk: Data) {
        lock.lock()
        if chunk.isEmpty { sawEnd = true } else { data.append(chunk) }
        lock.unlock()
        finishIfDone()
    }

    func exited(_ status: Int32, stderr: String) {
        lock.lock()
        exit = (status, stderr)
        lock.unlock()
        finishIfDone()
    }

    private func finishIfDone() {
        lock.lock()
        guard sawEnd, !finished, let exit else {
            lock.unlock()
            return
        }
        finished = true
        let body = data
        lock.unlock()
        if exit.status == 0 {
            reply.finish(data: body, error: nil)
        } else {
            reply.finish(data: nil, error: ProbeFailure(
                description: "curl exited \(exit.status): \(exit.stderr)"))
        }
    }
}

/// POSTs one tools/call to the live server over its Unix socket and returns at
/// once: `/usr/bin/curl --unix-socket` in a `Process`, the reply finished in its
/// termination handler, so main never blocks on the call.
func postTool(_ tool: String, arguments: [String: Any] = [:],
              document: ImageDocument? = nil) throws -> AgentHTTPReply {
    var arguments = arguments
    if let document { arguments["document_id"] = AgentServer.shared.documentID(for: document) }
    let object: [String: Any] = [
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": ["name": tool, "arguments": arguments],
    ]
    guard let socket = AgentServer.shared.boundSocketPath else {
        throw ProbeFailure(description: "The agent socket is not bound")
    }
    let body = try JSONSerialization.data(withJSONObject: object)
    let curl = Process()
    curl.executableURL = URL(fileURLWithPath: "/usr/bin/curl")
    curl.arguments = ["--unix-socket", socket, "-sS", "-H", "Content-Type: application/json",
                      "--data-binary", "@-", "http://localhost/mcp"]
    let input = Pipe(), output = Pipe(), errors = Pipe()
    curl.standardInput = input
    curl.standardOutput = output
    curl.standardError = errors
    let reply = AgentHTTPReply()
    let call = CurlCall(reply: reply)
    output.fileHandleForReading.readabilityHandler = { handle in
        let chunk = handle.availableData
        if chunk.isEmpty { handle.readabilityHandler = nil }
        call.append(chunk)
    }
    curl.terminationHandler = { process in
        let stderr = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        call.exited(process.terminationStatus, stderr: stderr)
    }
    try curl.run()
    input.fileHandleForWriting.write(body)
    try input.fileHandleForWriting.close()
    return reply
}

/// Pumps main in 10 ms slices until `done`, returning the longest gap between
/// successive `RunLoop.main.run(until:)` returns: the proxy for main getting
/// back to its event loop. `label` names the step in the timeout, so a
/// timed-out run says which wait never ended.
private func pumpMeasuringGap(_ label: String, until done: () -> Bool,
                              seconds: TimeInterval = 30) throws -> TimeInterval {
    let deadline = Date().addingTimeInterval(seconds)
    var last = Date()
    var gap: TimeInterval = 0
    while !done() {
        if Date() > deadline {
            throw ProbeFailure(description: "Timed out pumping main (\(label))")
        }
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        let now = Date()
        gap = max(gap, now.timeIntervalSince(last))
        last = now
    }
    return gap
}

/// `AgentServer.unbindSocket()` on main, under a 10 s watchdog that ends the
/// process if the release ever waits on the serve thread. Returns the
/// seconds the release took.
private func releaseUnderWatchdog() -> TimeInterval {
    let returned = ReleaseReturned()
    DispatchQueue.global().asyncAfter(deadline: .now() + 10) {
        guard !returned.isSet else { return }
        fputs("FAIL [agent release mid-call]: AgentServer.unbindSocket() did not return within "
              + "10 s (deadlock)\n", stderr)
        fflush(stdout)
        _exit(1)
    }
    let started = Date()
    AgentServer.shared.unbindSocket()
    returned.set()
    return Date().timeIntervalSince(started)
}

/// `<a fresh 0700 folder under NSTemporaryDirectory()>/agent.sock`: a scratch
/// socket path, so the probe never binds the person's own socket.
func scratchAgentSocket() throws -> String {
    var template = Array((NSTemporaryDirectory() as NSString)
        .appendingPathComponent("rz-probe-XXXXXX").utf8CString)
    guard let folder = template.withUnsafeMutableBufferPointer({ buffer -> String? in
        guard let base = buffer.baseAddress, mkdtemp(base) != nil else { return nil }
        return String(cString: base)
    }) else {
        throw ProbeFailure(description: "Could not make a scratch socket folder")
    }
    return (folder as NSString).appendingPathComponent("agent.sock")
}

/// Binds the live server at `path` through `DR_AGENT_SOCKET`, restoring the
/// variable afterwards, and requires that it bound exactly there.
private func bindAgentSocket(at path: String) throws {
    let old = ProcessInfo.processInfo.environment[AgentServer.socketEnvironmentKey]
    try require(setenv(AgentServer.socketEnvironmentKey, path, 1) == 0,
                "Could not point DR_AGENT_SOCKET at a scratch socket")
    defer {
        if let old {
            setenv(AgentServer.socketEnvironmentKey, old, 1)
        } else {
            unsetenv(AgentServer.socketEnvironmentKey)
        }
    }
    AgentServer.shared.bindSocket()
    try require(AgentServer.shared.boundSocketPath == path,
                "The live agent transport did not bind the scratch socket \(path)")
}

/// Runs `body` with the live server bound on a scratch socket of its own.
func withLiveAgentServer(_ body: () throws -> Void) throws {
    let server = AgentServer.shared
    try require(!server.isRunning, "The probe did not own the live agent transport")
    try bindAgentSocket(at: try scratchAgentSocket())
    defer { server.unbindSocket() }
    try body()
}

/// An HTTP MCP call waiting for a transient worker waits on the
/// serve thread, so main keeps returning to its event loop.
func verifyAgentServeThreadWait() throws {
    let document = try fixture()
    defer { document.close() }
    _ = try editor(document)
    _ = try select(document, side: 200)
    let busy = "Another background operation is running"
    let limit = AgentServer.workerWaitLimit
    var gapA: TimeInterval = 0
    var elapsedA: TimeInterval = 0
    var gapB: TimeInterval = 0
    var elapsedB: TimeInterval = 0
    // The new window's first layout and draw is main work of its own: let it
    // land before any gap is measured.
    let settle = Date().addingTimeInterval(0.5)
    while Date() < settle { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    try withLiveAgentServer {
        // (a) The worker is released after 1 s; the move waits, then succeeds.
        // Only this probe's own pump releases the holder: a main thread parked
        // in the call's hop never gets back here, so the call refuses busy at
        // the limit instead of succeeding.
        let a = try WorkerHolder()
        let moved = try postTool("move_selected_pixels", arguments: ["layer": 0, "dx": 5, "dy": 3],
                                 document: document)
        var released = false
        var pendingAtRelease = false
        gapA = try pumpMeasuringGap("(a) holder release", until: {
            if !released && moved.elapsed >= 1.0 {
                released = true
                pendingAtRelease = !moved.isDone
                a.release(after: 0)
            }
            return a.taken
        })
        _ = try pumpMeasuringGap("(a) move reply", until: { moved.isDone })
        elapsedA = moved.elapsed
        let resultA = try moved.result()
        try require(pendingAtRelease, "(a) main returned to its event loop while the call waited")
        try require(!resultA.isError, "(a) the move after a transient worker succeeds: \(resultA.raw)")
        try require(elapsedA >= 0.9, String(format: "(a) the call waited for the holder (%.3f s)",
                                            elapsedA))
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 5)

        // (b) The worker is still held at the limit: busy, and main stays free.
        let held = try WorkerHolder()
        do {
            let refused = try postTool("move_selected_pixels",
                                       arguments: ["layer": 0, "dx": 2, "dy": 2], document: document)
            // Seen only if main is back in this pump while the call still waits.
            var sawPendingMidWait = false
            gapB = try pumpMeasuringGap("(b) wait to the limit", until: {
                let elapsed = refused.elapsed
                if !refused.isDone && elapsed >= 1.0 && elapsed < limit - 0.5 {
                    sawPendingMidWait = true
                }
                return elapsed >= limit - 0.5
            })
            _ = try pumpMeasuringGap("(b) busy reply", until: { refused.isDone })
            elapsedB = refused.elapsed
            let resultB = try refused.result()
            try require(resultB.isError && resultB.text.contains(busy),
                        "(b) a slot still held at the limit refuses busy: \(resultB.raw)")
            try require(elapsedB >= limit - 0.05 && elapsedB <= limit + 3,
                        String(format: "(b) the refusal came at the %.0f s limit (%.3f s)",
                               limit, elapsedB))
            try require(sawPendingMidWait, "(b) main returned to its event loop during the wait")
        } catch {
            try? held.releaseNow()
            throw error
        }
        try held.releaseNow()
    }
    print(String(format: "PASS: agent serve-thread wait (a: moved after %.2f s, main gap %.3f s; "
                 + "b: busy at %.2f s, main gap %.3f s)", elapsedA, gapA, elapsedB, gapB))
}

/// Releasing the agent socket while a call is in flight returns at once
/// instead of joining a serve thread blocked on main. The
/// release unlinks the socket file and abandons the listener without a join,
/// so the call still in flight finishes and answers over its open connection.
///
/// (c)'s only release trigger is a 1 ms timer that sees the save's modal worker
/// wait (`BackgroundWork.isWaiting`): the case waits for the worker to
/// settle first, saves to a per-call destination, and names the reply when the
/// release never ran. The 30 s pump bound stays: the release must take under
/// 3 s, so a longer bound would only delay the same failure.
func verifyAgentReleaseMidCall() throws {
    var releaseC: TimeInterval = 0
    try withLiveAgentServer {
        guard let socket = AgentServer.shared.boundSocketPath else {
            throw ProbeFailure(description: "(c) the agent socket is not bound")
        }
        // (c) Release while save_copy runs on main (inside its modal worker wait).
        let large = try fixture(width: 2400, height: 1600)
        defer { large.close() }
        _ = try editor(large)
        let destination = directory.appendingPathComponent(
            "release-mid-call-\(UUID().uuidString).rz")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
        var releasedC: TimeInterval?
        let timer = Timer(timeInterval: 0.001, repeats: true) { tick in
            guard BackgroundWork.isWaiting else { return }
            tick.invalidate()
            releasedC = releaseUnderWatchdog()
        }
        RunLoop.main.add(timer, forMode: .common)
        let saved: AgentHTTPReply
        do {
            saved = try postTool("save_copy", arguments: ["path": destination.path], document: large)
        } catch {
            timer.invalidate()
            throw error
        }
        do {
            _ = try pumpMeasuringGap("(c) save_copy reply", until: { saved.isDone })
        } catch {
            timer.invalidate()
            throw error
        }
        guard let released = releasedC else {
            timer.invalidate()
            throw ProbeFailure(description: "(c) save_copy answered without being seen in its "
                               + "worker wait, so the release never ran: "
                               + ((try? saved.result().raw) ?? "no reply body"))
        }
        releaseC = released
        try require(releaseC < 3, String(format: "(c) the release returned promptly (%.3f s)", releaseC))
        try require(!FileManager.default.fileExists(atPath: socket),
                    "(c) the release removed the socket file \(socket)")
        try require(AgentServer.shared.boundSocketPath == nil, "(c) the server reports no socket")
        let resultC = try saved.result()
        try require(!resultC.isError, "(c) the call in flight still answers: \(resultC.raw)")
        try pumpUntil({ !BackgroundWork.occupied }, seconds: 20)
        try require(FileManager.default.fileExists(atPath: destination.path),
                    "(c) the running save still finished")

        // (d) A rebind at a fresh path answers.
        try bindAgentSocket(at: try scratchAgentSocket())
        let listed = try postTool("list_documents")
        _ = try pumpMeasuringGap("(d) list_documents reply", until: { listed.isDone })
        let resultD = try listed.result()
        try require(!resultD.isError, "(d) list_documents after a rebind: \(resultD.raw)")
    }
    print(String(format: "PASS: agent release mid-call (c: release %.3f s while running, socket "
                 + "file gone, the save finished; d: a rebind at a fresh path answers)", releaseC))
}
