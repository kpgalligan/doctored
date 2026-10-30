import AppKit

// MCP `execute` waits up to 5 s (this file) for an anonymous worker occupant to leave before
// a call that may need the slot, skipping reads, observers, background requests,
// nested/modal/replay/batch contexts, `DocumentJobs` jobs and running Content-Aware Fill reviews; a
// slot still busy at the limit refuses as before.

/// Back-to-back MCP calls. A worker-backed commit (`fill`, the
/// 8-bit `move_selected_pixels`, …) is consumed by its owner's run-loop timer,
/// and a nonmodal owner (recovery autosave, RAW local
/// develop) can hold the one app-wide worker slot for a moment. A call that
/// lands in that window used to answer "Another background operation is
/// running" and succeed on a retry. A call now waits, bounded, for such an
/// anonymous occupant to leave before the handler runs; if the slot is still
/// busy at the limit the handler refuses exactly as it did before. HTTP MCP
/// calls wait on the serve thread between main hops
/// (`AgentServer+ServeThread.swift`); the in-place pump in
/// `waitForIdleWorker` remains for direct main callers (Assistant panel,
/// Actions, probes).
///
/// The wait never covers an owner an agent can cancel or poll itself — a
/// `DocumentJobs` job or a Content-Aware Fill review ticket — so a stale-job
/// probe's mid-job call and a synchronous fill preview still refuse at once.
/// The wait itself is `BackgroundWork.waitForAnonymousOccupant`
/// (`WorkerSlotWait.swift`), shared with the slider filter sheets' Apply and
/// `ByteFilterAdmission.run`; this file keeps the tool checks.
extension AgentServer {
    static let workerWaitLimit: TimeInterval = BackgroundWork.anonymousWaitLimit
    static let workerWaitSlice: TimeInterval = BackgroundWork.anonymousWaitSlice

    /// Tools that observe or cancel the occupant, or are the CAF ticket
    /// surface whose own refusal contract is immediate.
    static let workerWaitExemptTools: Set<String> = [
        "job_status", "cancel_job", "resource_usage", "batch_status", "cancel_batch",
        "get_recovery_status", "content_aware_fill", "preview_content_aware_fill",
        "discard_content_aware_fill_preview",
    ]

    /// Main thread: false for an exempt tool, a pure read (`get_*` /
    /// `list_*` never claim the worker: a read that did would simply refuse
    /// as before), a background request, a nested or modal context, a
    /// replay, a batch, a document job, a running fill review, or a free slot.
    func workerWaitApplies(tool: String, arguments: [String: Any]) -> Bool {
        assert(Thread.isMainThread)
        return !Self.workerWaitExemptTools.contains(tool)
            && !tool.hasPrefix("get_") && !tool.hasPrefix("list_")
            && boolArg(arguments, "background") != true
            && BackgroundWork.anonymousWaitApplies
    }

    /// Before `target(arguments)` captures anything, for a direct main-thread
    /// caller. Returns at once unless `workerWaitApplies`.
    func waitForIdleWorker(tool: String, arguments: [String: Any]) {
        assert(Thread.isMainThread)
        guard workerWaitApplies(tool: tool, arguments: arguments) else { return }
        BackgroundWork.waitForAnonymousOccupant()
    }
}
