import AppKit
import Darwin

/// A thread-safe one-way latch the holder's worker spins on.
final class ProbeGate: @unchecked Sendable {
    private let lock = NSLock()
    private var opened = false
    var isOpen: Bool {
        lock.lock()
        defer { lock.unlock() }
        return opened
    }
    func open() {
        lock.lock()
        opened = true
        lock.unlock()
    }
}

/// An anonymous worker occupant (a transient owner): a `WorkerTask`
/// that runs until its gate opens and is then `take()`n by a `.common` timer,
/// the way the nonmodal owners drain their mailboxes.
final class WorkerHolder {
    private let gate: ProbeGate
    private let task: WorkerTask<Void>
    private var timer: Timer?
    private(set) var taken = false

    init() throws {
        let gate = ProbeGate()
        self.gate = gate
        task = try WorkerTask { while !gate.isOpen { usleep(1000) } }
    }

    func release(after delay: TimeInterval) {
        guard timer == nil, !taken else { return }
        let opensAt = Date().addingTimeInterval(delay)
        let timer = Timer(timeInterval: 0.01, repeats: true) { [weak self] _ in self?.tick(opensAt) }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    func releaseNow() throws {
        release(after: 0)
        try pumpUntil({ self.taken && !BackgroundWork.occupied }, seconds: 5)
    }

    private func tick(_ opensAt: Date) {
        guard Date() >= opensAt else { return }
        gate.open()
        guard task.take() != nil else { return }
        taken = true
        timer?.invalidate()
        timer = nil
    }
}

private func timedCall(_ tool: String, document: ImageDocument,
                       arguments: [String: Any] = [:]) throws
    -> (response: [String: Any], raw: String, seconds: TimeInterval) {
    let started = Date()
    let (response, raw) = try transformResponse(tool, document: document, arguments: arguments)
    return (response, raw, Date().timeIntervalSince(started))
}

private func payload(_ response: [String: Any]) -> [String: Any]? {
    guard let content = response["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String
    else { return nil }
    return try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any]
}

/// An MCP call made while an anonymous worker holds the slot waits,
/// bounded, for it to leave; observers answer at once; a slot still held at
/// the limit refuses exactly as before; a DocumentJobs job or a running
/// Content-Aware Fill review is never waited on.
func verifyAgentWaitsForWorker() throws {
    let document = try fixture()
    defer { document.close() }
    _ = try editor(document)
    _ = try select(document, side: 200)
    let busy = "Another background operation is running"
    let limit = AgentServer.workerWaitLimit

    // (a) A holder taken ~0.3 s later: the move waits for it and succeeds.
    let a = try WorkerHolder()
    a.release(after: 0.3)
    let moved = try timedCall("move_selected_pixels", document: document,
                              arguments: ["layer": 0, "dx": 5, "dy": 3])
    try require(a.taken, "(a) the holder was taken during the wait")
    try require(moved.response["isError"] as? Bool != true,
                "(a) move_selected_pixels after a transient worker succeeds: \(moved.raw)")
    try require(moved.seconds >= 0.25,
                String(format: "(a) the call waited for the holder (%.3f s)", moved.seconds))
    try pumpUntil({ !BackgroundWork.occupied }, seconds: 5)

    // (b) and (c) A holder never released: observers answer at once, the
    // move refuses busy at the limit.
    let held = try WorkerHolder()
    do {
        let usage = try timedCall("resource_usage", document: document)
        try require(usage.response["isError"] as? Bool != true && usage.seconds < limit - 1,
                    String(format: "(b) resource_usage answers while held (%.3f s): %@",
                           usage.seconds, usage.raw))
        let refused = try timedCall("move_selected_pixels", document: document,
                                    arguments: ["layer": 0, "dx": 2, "dy": 2])
        try require(refused.response["isError"] as? Bool == true && refused.raw.contains(busy),
                    "(c) a slot still held at the limit refuses busy: \(refused.raw)")
        try require(refused.seconds >= limit - 0.05 && refused.seconds < limit + 3,
                    String(format: "(c) the refusal came at the %.0f s limit (%.3f s)",
                           limit, refused.seconds))
    } catch {
        try? held.releaseNow()
        throw error
    }
    try held.releaseNow()

    // (d) A running DocumentJobs job: reads answer and edits refuse at once.
    _ = try DocumentJobs.start(document: document, layer: 0, title: "Worker Wait Probe",
        scratch: 4096, record: .notACommand, showProgress: false) { _, control in
        while !control.isCancelled { usleep(1000) }
        return nil
    }
    guard let job = DocumentJobs.active else { throw ProbeFailure(description: "(d) no job") }
    do {
        let read = try timedCall("get_document", document: document)
        try require(read.response["isError"] as? Bool != true && read.seconds < limit - 1,
                    String(format: "(d) get_document during a job (%.3f s): %@",
                           read.seconds, read.raw))
        let edit = try timedCall("move_selected_pixels", document: document,
                                 arguments: ["layer": 0, "dx": 1, "dy": 1])
        try require(edit.response["isError"] as? Bool == true && edit.seconds < limit - 1,
                    String(format: "(d) move_selected_pixels during a job refuses at once "
                           + "(%.3f s): %@", edit.seconds, edit.raw))
    } catch {
        job.control.cancel()
        try? pumpUntil({ DocumentJobs.active == nil && !BackgroundWork.occupied })
        throw error
    }
    job.control.cancel()
    try pumpUntil({ DocumentJobs.active == nil && !BackgroundWork.occupied })

    // (d') A background Content-Aware Fill review: the same, never waited on.
    let large = try fixture()
    defer { large.close() }
    _ = try editor(large)
    _ = try select(large, side: 700)
    let started = try transformResponse("preview_content_aware_fill", document: large,
        arguments: ["layer": 0, "ring": 0, "seed": 12345, "sample": "current",
                    "background": true])
    guard let previewID = payload(started.response)?["preview_id"] as? String else {
        throw ProbeFailure(description: "(d') no fill preview_id: \(started.raw)")
    }
    let discard = {
        _ = try? transformResponse("discard_content_aware_fill_preview", document: large,
                                   arguments: ["preview_id": previewID])
        try? pumpUntil({ !BackgroundWork.occupied }, seconds: 10)
    }
    do {
        try pumpUntil({ BackgroundWork.occupied }, seconds: 5)
        try require(ContentAwareFillReview.isRunning, "(d') the fill review is running")
        let read = try timedCall("get_document", document: large)
        try require(read.response["isError"] as? Bool != true && read.seconds < limit - 1,
                    String(format: "(d') get_document during a fill preview (%.3f s): %@",
                           read.seconds, read.raw))
        // The handler's own preparation over a 700 px selection shares the
        // CPU with the fill at -Onone, so its time is not a fixed bound; a
        // waited call would take at least `limit`, so `limit - 1` (as in (b)
        // and (d)) still proves it was not waited on.
        let edit = try timedCall("move_selected_pixels", document: large,
                                 arguments: ["layer": 0, "dx": 1, "dy": 1])
        try require(edit.response["isError"] as? Bool == true && edit.seconds < limit - 1,
                    String(format: "(d') move_selected_pixels during a fill preview refuses "
                           + "without waiting (%.3f s): %@", edit.seconds, edit.raw))
    } catch {
        discard()
        throw error
    }
    discard()
    try require(!BackgroundWork.occupied, "(d') the discarded review freed the worker slot")
    print(String(format: "PASS: agent worker wait (a: waited %.2f s then moved; b: observer "
                 + "answered; c: busy at the %.0f s limit; d: job and fill review not waited on)",
                 moved.seconds, limit))
}
