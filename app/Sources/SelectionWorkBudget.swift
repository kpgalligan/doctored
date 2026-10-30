import Foundation

// `Reservation.runAsOwnLease` is `run` with that reservation left out of `scratchBytes` as read on
// the running thread only (a thread-dictionary id set, nest-safe), for a worker whose own core plan
// already charges the lease; `run` is unchanged, and the owners that subtract their lease
// explicitly (`ScopedAnalysis`, `NativeScalarStatistics`) keep doing so. The other preview owners
// whose worker re-reads the registry use it too: the 8-bit `TransformMaskPreview` geometry stage,
// `MaskSheetPreview`, `PrecisionProfileEditing`'s preview, the typed Evaluation sheet preview,
// `PrecisionOpacityDrag`, the Convert to Profile sheet's retained-state preview and
// `PrecisionAdjustmentSession`'s preview. Use it only when every byte of the reservation is charged
// by the worker's own stages: bytes held in Swift across stages go in a `PrecisionEditPayload`
// lease instead (the adjustment preview's coverage — only a `.create` insertion's
// u16 selection mask, n×2, because a destructive edit's coverage is its own `PrecisionScalarBuffer`
// payload and its core plan charges canvas×2; a described-artwork stage's scratch in
// `TransformMaskPreview` and `PrecisionGeometryGesture`, which keeps earlier parts' coverage live
// while later parts admit), and the reservation is the rest. The adjustment preview
// (`PrecisionAdjustmentSession` and `PrecisionProxySource`) reserves
// `PrecisionCapturedEdit.previewReservationBytes(display:)`: for a selected edit, whose compute and
// display preparation run one after the other, max(core scratch, the plan's `output_bytes` +
// `projectionScratchEstimate()`), else their sum.
//
// A commit releases its own cancelled preview's `SelectionWorkBudget` reservation before admission:
// byte Free Transform, ⌘T and Move Selected via
// `TransformMaskPreview.cancelReleasingReservations`, and the 16-bit gestures in
// `PrecisionGeometryGesture.finish` — `PreviewRenderer.cancel()` drops a not-yet-run debounced
// request's closure at once.

/// Preview closures can outlive their sheet and cooperative cancellation. A
/// reservation follows the closure's actual lifetime, including a debounced or
/// queued request that is replaced before it runs. Finals use BackgroundWork's
/// existing reservation and do not enter this registry a second time.
enum SelectionWorkBudget {
    private static let lock = NSLock()
    private static var reservations: [UUID: UInt64] = [:]

    /// Every registered preview's bytes, except — read on a thread inside
    /// `Reservation.runAsOwnLease` — that thread's own leases.
    static var scratchBytes: UInt64 {
        let own = ownLeases
        lock.lock()
        defer { lock.unlock() }
        return reservations.reduce(0) { own.contains($1.key) ? $0 : $0 + $1.value }
    }

    private static let ownLeaseKey = "SelectionWorkBudget.ownLeases"
    /// The reservations the CURRENT thread is running as its own lease.
    fileprivate static var ownLeases: Set<UUID> {
        get { Thread.current.threadDictionary[ownLeaseKey] as? Set<UUID> ?? [] }
        set { Thread.current.threadDictionary[ownLeaseKey] = newValue }
    }

    /// A preview owner can verify its own reservation has left the registry
    /// without drawing a conclusion from unrelated previews' aggregate bytes.
    static func contains(_ id: UUID) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return reservations[id] != nil
    }

    static func reservePreview(bytes: UInt64, outputBytes: UInt64) throws -> Reservation {
        assert(Thread.isMainThread)
        guard !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Wait for the current background operation before requesting a selection preview.")
        }
        guard bytes > outputBytes else {
            throw DoctoredCoreError(message: "The selection preview estimate must include its output coverage.")
        }
        _ = try RasterResources.shared.admit(scratch: bytes)
        let token = try RasterJobControl()
        guard token.setScratchLimit(bytes - outputBytes) else {
            throw DoctoredCoreError(message: "Could not bound selection preview memory.")
        }
        let id = UUID()
        lock.lock()
        reservations[id] = bytes
        lock.unlock()
        return Reservation(id: id, control: token)
    }

    final class Reservation {
        let id: UUID
        let control: RasterJobControl
        fileprivate init(id: UUID, control: RasterJobControl) {
            self.id = id
            self.control = control
        }
        deinit {
            SelectionWorkBudget.lock.lock()
            SelectionWorkBudget.reservations.removeValue(forKey: id)
            SelectionWorkBudget.lock.unlock()
        }
        func run<T>(_ compute: (RasterJobControl) -> T) -> T {
            withExtendedLifetime(self) { compute(control) }
        }
        /// `run`, with this reservation left out of `scratchBytes` as read on
        /// THIS thread while `compute` runs: the work's own core plan
        /// already charges these bytes, so counting the lease in its
        /// `otherRetained` too charged them twice. Every other thread still
        /// counts it. Nest-safe: the previous set is restored on exit.
        func runAsOwnLease<T>(_ compute: (RasterJobControl) -> T) -> T {
            let previous = SelectionWorkBudget.ownLeases
            SelectionWorkBudget.ownLeases = previous.union([id])
            defer { SelectionWorkBudget.ownLeases = previous }
            return run(compute)
        }
    }
}
