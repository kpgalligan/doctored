import AppKit
import Darwin

// MCP and Assistant `undo`, `redo` and `restore_history` refuse by name
// while the document's undo group is open or deferred. No product path
// services a call inside one today, so the case forces the window from
// top-level main-thread code, as a UI event or a timer would.

private func recordingRefusal(_ verb: String) -> String {
    "Another edit is still being recorded on this document. Wait for it to finish, then \(verb)."
}

private func recordRename(_ document: ImageDocument, _ label: String) {
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename", record: .notACommand, renderNeutral: true) {
            try $0.withLayerName(0, label)
        }
    }
}

/// A modal worker wait that spins on a gate; a `.common` timer opens it once
/// `done` answers true, or after 10 s.
private func recordingWait(until done: @escaping () -> Bool) throws {
    let gate = ProbeGate()
    let deadline = Date().addingTimeInterval(10)
    let timer = Timer(timeInterval: 0.01, repeats: true) { _ in
        if done() || Date() > deadline { gate.open() }
    }
    RunLoop.main.add(timer, forMode: .common)
    defer { timer.invalidate() }
    try BackgroundWork.wait(try WorkerTask { while !gate.isOpen { usleep(1000) } },
                            title: "Recording probe")
}

/// One Assistant-shaped call's reply, stored by its main-queue block.
private final class AssistantReply {
    var reply: (isError: Bool, text: String)?

    /// Enqueues `tool` as the Assistant helper does: a `DispatchQueue.main.async`
    /// block that runs `execute` whenever main next turns its run loop.
    init(_ tool: String, _ document: ImageDocument, _ arguments: [String: Any] = [:]) {
        DispatchQueue.main.async { [self] in
            do {
                let call = try agentCall(tool, document, arguments)
                reply = (call.isError, call.text)
            } catch {
                reply = (false, "the call threw: \(error)")
            }
        }
    }
}

private func requireRefusal(_ reply: (isError: Bool, text: String)?, _ verb: String,
                            _ label: String) throws {
    guard let reply else { throw ProbeFailure(description: "\(label): no reply within 10 s") }
    try require(reply.isError && reply.text == recordingRefusal(verb),
                "\(label) refuses with the recording sentence: \(reply.text)")
}

private func requireRefusal(_ reply: AgentHTTPReply, _ verb: String, _ label: String) throws {
    guard reply.isDone else { throw ProbeFailure(description: "\(label): no reply within 10 s") }
    let result = try reply.result()
    try requireRefusal((result.isError, result.text), verb, label)
}

func verifyUndoRefusedWhileRecording() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    _ = try editor(document)
    guard let manager = document.undoManager else {
        throw ProbeFailure(description: "Recording fixture undo manager")
    }
    recordRename(document, "A")
    recordRename(document, "B")
    let history = document.history
    try require(history.cursor == 2 && history.entries.count == 3,
                "Setup: two renames put the cursor at 2 (\(history.cursor))")
    let alerts = AutomationAlerts.recorded.count
    try withLiveAgentServer {
        // (a) A plain explicit group, open across a modal worker wait.
        let firstID = history.entries[0].id
        let undo = AssistantReply("undo", document)
        let restore = AssistantReply("restore_history", document, ["state_id": firstID])
        let redo = try postTool("redo", document: document)
        let countA = history.entries.count
        var unchanged = false
        var waitA: Error?
        withUndoGroup(manager) {
            recordRename(document, "C")
            let ids = history.entries.map(\.id)
            let cursor = history.cursor
            do {
                try recordingWait(until: { undo.reply != nil && restore.reply != nil && redo.isDone })
            } catch { waitA = error }
            unchanged = history.entries.map(\.id) == ids && history.cursor == cursor
        }
        if let waitA { throw waitA }
        try requireRefusal(undo.reply, "undo", "(a) Assistant undo")
        try requireRefusal(restore.reply, "restore_history", "(a) Assistant restore_history")
        try requireRefusal(redo, "redo", "(a) HTTP redo")
        try require(unchanged, "(a) no reply changed the cursor or the entries")
        try require(history.cursor == 3 && history.entries.count == countA + 1
                    && document.doc?.layerInfo(0)?.name == "C",
                    "(a) the group recorded one new entry \"C\": cursor \(history.cursor), "
                    + "\(history.entries.count) entries")
        try require(manager.groupingLevel == 0, "(a) the group closed (\(manager.groupingLevel))")

        // (b) A deferred group (`withUndoGroupDiscardingEmpty`) with no
        // registration: level 0, but still recording.
        let undoB = try postTool("undo", document: document)
        let idsB = history.entries.map(\.id)
        let cursorB = history.cursor
        var unchangedB = false
        let waitB: Result<Void, Error> = document.withUndoGroupDiscardingEmpty {
            let waited = Result { try recordingWait(until: { undoB.isDone }) }
            unchangedB = history.entries.map(\.id) == idsB && history.cursor == cursorB
            return waited
        }
        try waitB.get()
        try requireRefusal(undoB, "undo", "(b) HTTP undo")
        try require(unchangedB && history.entries.map(\.id) == idsB && history.cursor == cursorB,
                    "(b) the refusal changed nothing and the body added no entry")
        try require(manager.groupingLevel == 0, "(b) no group is left open (\(manager.groupingLevel))")
        try require(AutomationAlerts.recorded.count == alerts,
                    "No alert was raised: \(AutomationAlerts.recorded.suffix(2))")

        // A history move after the recording ends is ok.
        let final = try postTool("undo", document: document)
        try pumpUntil({ final.isDone }, seconds: 10)
        let result = try final.result()
        try require(!result.isError, "The undo after the recording is ok: \(result.raw)")
        try require(history.cursor == 2 && document.doc?.layerInfo(0)?.name == "B",
                    "The undo stepped back to \"B\": cursor \(history.cursor)")
    }
    print("PASS: undo while recording — MCP and Assistant undo, redo and restore_history refuse "
        + "by name inside an open and a deferred undo group, change nothing, raise no alert, "
        + "and undo is ok once the recording ends")
}
