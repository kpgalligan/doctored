import Foundation

// `AgentServeGate.hop` queues each piece of the call with `DispatchQueue.main.async` and waits for
// it on a condition; the worker wait sleeps on the serve thread between hops. Nothing stops the
// serve thread: the quit releases the socket without a join (`AgentServer+Socket.swift`), so a call
// still in flight simply ends with the process.

/// How a socket MCP call reaches main. The core's serve
/// thread used to `DispatchQueue.main.sync` the whole call, so the worker
/// wait pumped main in place, and a stop on main joined a serve thread
/// that was itself blocked on main. Now each piece of the call is a
/// `DispatchQueue.main.async` hop the serve thread waits for, the worker
/// wait happens on the serve thread between hops, and the quit's
/// `dr_agent_socket_release` never joins the serve thread, so main
/// never waits on it. The context is the `AgentServer.shared` singleton, which
/// lives for the process.

/// What one main hop produced.
enum AgentHopResult {
    case finished(String)
    case waitForWorker
}

/// The one place the serve thread waits on main. Nothing on the serve thread
/// may block on main except through `hop`.
final class AgentServeGate {
    static let shared = AgentServeGate()

    /// One hop's result, guarded by the gate's condition.
    private final class Slot {
        var result: AgentHopResult?
    }

    private let condition = NSCondition()

    /// Serve thread only: runs `body` on main and waits for it.
    func hop(_ body: @escaping () -> AgentHopResult) -> AgentHopResult {
        assert(!Thread.isMainThread)
        let slot = Slot()
        let condition = condition
        DispatchQueue.main.async {
            let result = body()
            condition.lock()
            slot.result = result
            condition.broadcast()
            condition.unlock()
        }
        condition.lock()
        defer { condition.unlock() }
        while true {
            if let result = slot.result { return result }
            condition.wait()
        }
    }
}

extension AgentServer {
    /// Serve thread only: one socket MCP call as a series of main hops. While
    /// the worker wait applies the serve thread sleeps between hops, so
    /// main keeps dispatching events; the hop after the limit runs the call,
    /// which refuses busy exactly as before.
    func executeFromServeThread(tool: String, argumentsJSON: String) -> String {
        assert(!Thread.isMainThread)
        // Set on main by the first hop, so a call queued behind a long main
        // block still gets its full wait. Only hop bodies touch it, and they
        // run one at a time on main.
        var deadline: Date?
        let arguments =
            (try? JSONSerialization.jsonObject(with: Data(argumentsJSON.utf8)))
            as? [String: Any] ?? [:]
        while true {
            let outcome = AgentServeGate.shared.hop {
                let limit = deadline ?? Date().addingTimeInterval(Self.workerWaitLimit)
                deadline = limit
                if Date() < limit, self.workerWaitApplies(tool: tool, arguments: arguments) {
                    BackgroundWork.noteSlotRequest()
                    return .waitForWorker
                }
                return .finished(
                    self.execute(tool: tool, argumentsJSON: argumentsJSON, waitsForWorker: false))
            }
            switch outcome {
            case .finished(let result):
                return result
            case .waitForWorker:
                Thread.sleep(forTimeInterval: Self.workerWaitSlice)
            }
        }
    }
}
