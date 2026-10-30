import AppKit

// The MCP worker wait is `BackgroundWork.waitForAnonymousOccupant` (this file), which the slider
// filter sheets' Apply (both depths, before dismissing: a slot still held keeps the sheet open with
// `BackgroundWork.busyMessage`; an 8-bit exact bypass, plan 0, skips it), `ByteFilterAdmission.run`
// (the sheets' Apply; `apply_filter` passes `waitForSlot: false`, since `execute` already waited)
// and the three 16-bit commits `PrecisionAdjustmentSession.commit(filter:)`, `commit(op:)` and
// `PrecisionScalarAdjustmentSession.commit` also use; a cancellable occupant (a `DocumentJobs`
// job, a fill review) or a replay/⌃F/batch context gets no wait, so those callers refuse at once.

/// The bounded wait for an anonymous worker-slot occupant, shared by
/// MCP calls (`AgentServer+WorkerWait.swift`), the slider filter sheets' Apply
/// and `ByteFilterAdmission.run`. A nonmodal owner (recovery autosave,
/// RAW local develop) can hold the one app-wide worker
/// slot for a moment; a caller waits for it to leave, bounded, and if the slot
/// is still held at the limit the caller refuses exactly as before.
///
/// The wait never covers an owner a user or agent can cancel or poll — a
/// `DocumentJobs` job or a Content-Aware Fill review ticket — so those still
/// refuse at once.
extension BackgroundWork {
    static let anonymousWaitLimit: TimeInterval = 5
    static let anonymousWaitSlice: TimeInterval = 0.01
    static let busyMessage = "Another background operation is running. Wait or cancel it first."
    /// The typed edit constructors' refusal while the slot is held
    /// (`PrecisionCapturedEdit`, `PrecisionPixelEdit` and the other 16-bit
    /// preparations): the one copy of that sentence.
    static let preparingBusyMessage =
        "Wait for the current background operation before preparing a high-depth edit."

    /// True when `error` is the worker slot's own refusal: `busyMessage` from
    /// `WorkerTask` (8 bits) or `preparingBusyMessage` from a typed edit's
    /// constructor (16 bits, after the bounded wait). A sheet whose Apply got
    /// one after it dismissed comes back instead of alerting.
    static func isBusyRefusal(_ error: Error) -> Bool {
        let text = error.localizedDescription
        return text == busyMessage || text == preparingBusyMessage
    }

    /// How long one waiter's request keeps recovery autosave from
    /// beginning its next write. Longer than recovery's 0.5 s tick plus one
    /// serve-thread hop's main latency, so a waiter that notes its request
    /// every slice is never overtaken by a write begun in the same turn that
    /// freed the slot.
    static let slotRequestWindow: TimeInterval = 1
    /// `systemUptime` of the latest `noteSlotRequest()`. Main thread only.
    private(set) static var slotRequestedAt: TimeInterval = -.infinity

    /// Main thread: a caller waiting for the worker slot says so, so
    /// `RecoveryCoordinator.tick` lets the slot stay free for it.
    static func noteSlotRequest() {
        assert(Thread.isMainThread)
        slotRequestedAt = ProcessInfo.processInfo.systemUptime
    }

    /// Main thread: a waiter asked for the slot within `slotRequestWindow`.
    static var slotRequested: Bool {
        ProcessInfo.processInfo.systemUptime - slotRequestedAt < slotRequestWindow
    }

    /// Owners a user or agent can cancel or poll: never waited on.
    static var occupantIsCancellable: Bool {
        DocumentJobs.active != nil || ContentAwareFillReview.isRunning
    }

    /// Main thread: false in a nested or modal context, a replay, a batch, a
    /// document job, a running fill review, or with a free slot.
    static var anonymousWaitApplies: Bool {
        assert(Thread.isMainThread)
        return !isWaiting
            && NSApp.modalWindow == nil
            && !ActionPlayer.isPlaying
            && BatchRun.active == nil
            && !occupantIsCancellable
            && occupied
    }

    /// Main thread. When `anonymousWaitApplies`, pumps the main run loop until
    /// the occupant leaves, a cancellable owner takes over, or the limit
    /// passes. Returns true when the slot is free.
    @discardableResult
    static func waitForAnonymousOccupant() -> Bool {
        assert(Thread.isMainThread)
        guard anonymousWaitApplies else { return !occupied }
        // Nonmodal owners drain their mailboxes from `.common`-mode timers,
        // which this pump fires even inside the caller's own main-thread work.
        let deadline = Date().addingTimeInterval(anonymousWaitLimit)
        while occupied, !occupantIsCancellable, Date() < deadline {
            noteSlotRequest()
            RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(anonymousWaitSlice))
        }
        return !occupied
    }

    /// Main thread. A sheet's Apply, before it dismisses: waits as above, and
    /// if the slot is still held shows `busyMessage` in the sheet's status
    /// line and beeps, so the sheet stays open with its values.
    static func awaitSlotForSheetApply(status: NSTextField) -> Bool {
        guard !waitForAnonymousOccupant() else { return true }
        status.stringValue = busyMessage
        status.isHidden = false
        NSSound.beep()
        return false
    }
}
