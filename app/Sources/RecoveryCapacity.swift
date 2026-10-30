import Foundation

// `RecoveryCapacity` charges the reopen's sequential decode and projection as max(), not a sum, and
// a refused checkpoint shows "Recovery off" in the status bar
// (`EditorViewController+Recovery.swift`).

/// Recovery writes ordinary native images independently. Serialized snapshots
/// do not preserve their in-memory Arc sharing, so admitting only encoder peak
/// scratch could publish a set that exceeds the working budget when reopened.
/// Reopening decodes every state, then projects the current one; each stage
/// admits itself, so the two sequential stages are charged max(), not a sum.
/// A refusal is a `Refusal`: the capture's own admission, or this reopen check, with the
/// figures `get_recovery_status` reports as `memory`.
enum RecoveryCapacity {
    static let offPrefix = "Crash recovery is off for this document:"

    struct Refusal: LocalizedError {
        enum Stage: String { case capture, reopen }
        let stage: Stage
        let neededBytes: UInt64   // capture: scratch (+ own when on its own); reopen: the peak
        let inUseBytes: UInt64    // capture: retained + preview scratch after eviction; reopen: 0
        let limitBytes: UInt64
        let message: String
        var errorDescription: String? { message }
        var report: [String: Any] {
            ["stage": stage.rawValue, "needed_bytes": neededBytes,
             "in_use_bytes": inUseBytes, "limit_bytes": limitBytes]
        }
    }

    private static func memory(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(min(bytes, UInt64(Int64.max))), countStyle: .memory)
    }

    @discardableResult
    static func validate(_ captured: RecoveryStore.Captured,
                         limit: UInt64 = RasterResources.shared.workingLimit) throws -> UInt64 {
        let states = [captured.state] + captured.snapshots.map(\.state)
        var retained: UInt64 = 0
        var temporary: UInt64 = 0
        for state in states {
            retained += state.reopenedRetainedEstimate
            let decode = state.manifest.decodeEstimateBytes
            temporary = max(temporary, decode - min(decode, state.reopenedRetainedEstimate))
        }
        // Recovery prepares the current composite after loading every snapshot.
        // Include the declared core projection fallback and final output; host
        // object/allocator overhead remains an estimate, as in Phase 4 admission.
        let peak = retained + max(temporary, captured.state.projectionScratchEstimate)
        guard peak < limit else {
            let remedy = captured.snapshots.isEmpty ? captured.state.refusalRemedy
                : "Save session snapshots as .drsnapshot files and remove them from the session, or reduce the document."
            throw Refusal(stage: .reopen, neededBytes: peak, inUseBytes: 0, limitBytes: limit, message:
                "\(offPrefix) this recovery set would need \(memory(peak)) when reopened, "
                + "at least the \(memory(limit)) working limit. " + remedy)
        }
        return peak
    }

    /// The capture's own admission refused. `inUse` is the retained union plus
    /// preview scratch, measured after eviction; "close other documents"
    /// appears only when another document is open.
    static func captureRefusal(scratch: UInt64, own: UInt64, inUse: UInt64, limit: UInt64,
                               snapshots: Bool, otherDocuments: Bool, remedy: String) -> Refusal {
        let (sum, overflow) = scratch.addingReportingOverflow(own)
        let alone = overflow ? UInt64.max : sum
        if alone >= limit {
            return Refusal(stage: .capture, neededBytes: alone, inUseBytes: inUse, limitBytes: limit, message:
                "\(offPrefix) writing its recovery copy needs \(memory(alone)) on its own, "
                + "at least the \(memory(limit)) working limit. \(remedy)")
        }
        let advice: String
        switch (snapshots, otherDocuments) {
        case (true, true): advice = "Remove session snapshots or close other documents to turn it back on."
        case (true, false): advice = "Remove session snapshots to turn it back on."
        case (false, true): advice = "Close other documents to turn it back on."
        case (false, false): advice = remedy
        }
        return Refusal(stage: .capture, neededBytes: scratch, inUseBytes: inUse, limitBytes: limit, message:
            "\(offPrefix) writing its recovery copy needs \(memory(scratch)) of working memory "
            + "and \(memory(inUse)) is already in use; the limit is \(memory(limit)). " + advice)
    }
}
