import AppKit

// `HistoryUndoManager` defers `withUndoGroupDiscardingEmpty`'s outer group until the body's first
// history registration, so a body that records nothing leaves no Undo step.
//
// Undo callbacks retain transition IDs; the history owns state handles, and Undo and Redo find
// their transition at or next to the cursor (`DocumentHistory.entryIndex(id:)`).
// `HistoryUndoManager` observes outer group completion and explicit resets; retention evicts old
// transitions by removing only their own registrations while preserving the nearest Undo
// even if it alone exceeds budget; that removal (`dropHistoryUndo`,
// `removeAllActions(withTarget:)`) walks the manager's whole stack per eviction, the O(n) residual
// AppKit leaves with `levelsOfUndo` at 0; the history's own eviction is O(1) — `DocumentHistory`
// keeps `slots[head...]` behind the read-only `entries` view (`RetainedEntries`), advances `head`
// on a front eviction and compacts once the dead prefix reaches the live count
// (medians of three runs, 50 → 2000 entries, the capped edit grew 0.156 ms, 0.112 of it the walk).

extension Notification.Name {
    static let imageDocumentHistoryDidChange = Notification.Name("ImageDocumentHistoryDidChange")
}

/// The NSUndoManager target of one transition's registrations, so an
/// evicted transition's registrations are removed by target. It holds only
/// the id: evicted states are released.
final class HistoryUndoTarget {
    let id: String
    init(id: String) { self.id = id }
}

/// An enumerable sequence of full states, independent of NSUndoManager's opaque
/// stack. A real undo group maps to one transition. Undo callbacks keep IDs only;
/// trimming this sequence releases the history's actual pixel/selection owners.
///
/// `retainedBytes` is a running upper bound between full measures: each
/// closed transition grows it by the saturating difference its new state adds
/// to the one before it, and a write the bound cannot follow sets
/// `needsFullMeasure`. Invariant: the bound is never below the true retained
/// bytes when a budget decision is made; `enforceBudget` walks the whole
/// history only when the bound may exceed the budget or a full measure is due.
final class DocumentHistory {
    struct Entry {
        let id: String
        var name: String
        var state: EditingState
        let date: Date
        /// The transition INTO this entry (index − 1 ↔ index) changed no
        /// rendered pixel — a rename, label, lock, link, channel
        /// name/overlay, or a guide/ruler-origin edit — so undo and
        /// redo may carry the current display projection across it instead
        /// of preparing a new one.
        var renderNeutral: Bool = false
        /// The transition INTO this entry changed canvas geometry (or is a
        /// snapshot restore), so undo and redo across it restore the
        /// destination's own colour samplers, which the core moved
        /// with the canvas. Every other transition carries the
        /// current list onto its destination (`replaceDocument(at:with:)`).
        var changesGeometry: Bool = false
        let undoTarget: HistoryUndoTarget
    }

    /// The retained entries, oldest first: a read-only view of `slots[head...]`
    /// whose index 0 is the oldest retained entry, so `cursor` and every
    /// consumer's index stay relative to it.
    struct RetainedEntries: RandomAccessCollection {
        typealias Indices = Range<Int>
        fileprivate let slots: [Entry?]
        fileprivate let head: Int
        var startIndex: Int { 0 }
        var endIndex: Int { slots.count - head }
        subscript(position: Int) -> Entry {
            guard let entry = slots[head + position] else {
                preconditionFailure("history slot \(head + position) is evicted")
            }
            return entry
        }
    }

    var entries: RetainedEntries { RetainedEntries(slots: slots, head: head) }
    /// Storage: the retained entries are `slots[head...]` and are never
    /// nil; a front eviction clears its slot and advances `head` instead of
    /// shifting the array, and `enforceBudget` compacts once the dead prefix
    /// is at least as long as the live part (amortized O(1) per eviction).
    /// Internal code reads and writes through `self[retained:]`, never through
    /// an `entries` view, which would share the buffer and make the next slot
    /// write copy it.
    private var slots: [Entry?] = []
    private var head = 0
    private(set) var cursor = 0
    private(set) var pendingID: String?
    private(set) var retainedBytes: UInt64 = 0
    private(set) var baselineBytes: UInt64 = 0
    private(set) var accountingError: String?
    private(set) var evictedCount = 0
    /// Set when `retainedBytes` may no longer be an upper bound; cleared by a
    /// successful `measure()`.
    private var needsFullMeasure = true
    /// The History panel's budget presets as divisors of the working limit
    /// (64 MiB … 1 GiB at the 2 GiB pin, 256 MiB … 4 GiB at 8 GiB); the
    /// default budget is one quarter of it.
    static let budgetDivisors: [UInt64] = [32, 16, 8, 4, 2]
    static let defaultBudgetDivisor: UInt64 = 4
    static var budgetPresets: [UInt64] {
        budgetDivisors.map { RasterResources.shared.workingLimit / $0 }
    }
    var budgetBytes: UInt64 =
        RasterResources.shared.workingLimit / DocumentHistory.defaultBudgetDivisor
    /// Bounds the enumerable state graph even when every edit shares pixels.
    /// Qualification can lower the cap; production never exceeds 4096 steps.
    let maximumTransitions: Int
    var rebuilding = false
    var navigating = false
    var suspended = 0
    weak var document: ImageDocument?

    init(document: ImageDocument, maximumTransitions: Int = 4096) {
        self.document = document
        self.maximumTransitions = min(max(1, maximumTransitions), 4096)
    }

    /// The History Brush's source: the id of the state it paints from,
    /// set from the History panel's row menu or named by `history_brush_stroke`.
    /// nil, or an id the history no longer retains, means the oldest retained
    /// state (`brushSource()`). The setter posts nothing: its caller notifies.
    var brushSourceID: String?

    /// The state the History Brush paints from: `brushSourceID`'s while it is
    /// retained, else the oldest retained entry; nil for an empty history.
    func brushSource() -> Entry? {
        if let id = brushSourceID, let index = entryIndex(id: id) { return self[retained: index] }
        return count > 0 ? self[retained: 0] : nil
    }

    var incrementalBytes: UInt64 { retainedBytes - min(retainedBytes, baselineBytes) }
    var isOverBudget: Bool { incrementalBytes > budgetBytes }
    var currentID: String? { holds(cursor) ? self[retained: cursor].id : nil }

    /// The number of retained entries.
    private var count: Int { slots.count - head }

    private func holds(_ index: Int) -> Bool { index >= 0 && index < count }

    /// The retained entry at `index` (relative to `head`); retained slots are
    /// never nil.
    private subscript(retained index: Int) -> Entry {
        get {
            guard let entry = slots[head + index] else {
                preconditionFailure("history slot \(head + index) is evicted")
            }
            return entry
        }
        set { slots[head + index] = newValue }
    }

    /// Tries `cursor` (Undo's entry), then `cursor + 1` (Redo's), then
    /// the last matching entry (a rebuild's synthetic Undo walks back from
    /// the last entry). Ids are unique UUIDs, so the answer equals
    /// `entries.firstIndex(where: { $0.id == id })`.
    func entryIndex(id: String) -> Int? {
        if holds(cursor), self[retained: cursor].id == id { return cursor }
        if holds(cursor + 1), self[retained: cursor + 1].id == id { return cursor + 1 }
        return (0..<count).last(where: { self[retained: $0].id == id })
    }

    func reset(_ state: EditingState?) {
        slots = state.map {
            let id = UUID().uuidString
            return [Entry(id: id, name: "Initial State", state: $0, date: Date(),
                          undoTarget: HistoryUndoTarget(id: id))]
        } ?? []
        head = 0
        cursor = 0
        pendingID = nil
        evictedCount = 0
        brushSourceID = nil
        measure()
        notify()
    }

    /// Returns a target only when a new manager registration is needed. Subsequent
    /// mutations in the same outer group update that transition's final state.
    func record(before: EditingState, after: EditingState, name: String,
                renderNeutral: Bool = false, changesGeometry: Bool = false) -> HistoryUndoTarget? {
        guard !rebuilding, !navigating, suspended == 0, !before.equivalent(to: after) else { return nil }
        if count == 0 { reset(before) }
        if let pendingID, self[retained: count - 1].id == pendingID {
            self[retained: count - 1].state = after
            self[retained: count - 1].name = name
            // One transition stays neutral only while every edit merged
            // into it is.
            self[retained: count - 1].renderNeutral =
                self[retained: count - 1].renderNeutral && renderNeutral
            // ...and changes geometry once any edit merged into it does.
            self[retained: count - 1].changesGeometry =
                self[retained: count - 1].changesGeometry || changesGeometry
            notify()
            return nil
        }
        if cursor < count - 1 {
            slots.removeSubrange((head + cursor + 1)...)
            needsFullMeasure = true
        }
        // Nonundoable view-target/preferences changes belong to the state the
        // next edit started from, without adding a separate history step.
        if self[retained: cursor].state.document !== before.document
            || self[retained: cursor].state.selection != before.selection {
            needsFullMeasure = true
        }
        replaceState(at: cursor, with: before)
        let id = UUID().uuidString
        let target = HistoryUndoTarget(id: id)
        slots.append(Entry(id: id, name: name, state: after, date: Date(),
                           renderNeutral: renderNeutral, changesGeometry: changesGeometry,
                           undoTarget: target))
        cursor = count - 1
        pendingID = id
        notify()
        return target
    }

    func closeGroup() {
        guard !rebuilding, !navigating, suspended == 0, pendingID != nil else { return }
        if pendingID != nil, let state = document?.captureEditingState(), holds(cursor) {
            replaceState(at: cursor, with: state)
        }
        addClosedTransitionBound()
        pendingID = nil
        enforceBudget()
        notify()
    }

    func refreshCurrentViewState() {
        guard !rebuilding, !navigating, suspended == 0,
              let state = document?.captureEditingState(), holds(cursor),
              self[retained: cursor].state.document === state.document else { return }
        if self[retained: cursor].state.selection != state.selection { needsFullMeasure = true }
        self[retained: cursor].state = state
    }

    func replaceUntrackedCurrentState() {
        guard !rebuilding, !navigating, suspended == 0,
              let state = document?.captureEditingState(), holds(cursor) else { return }
        replaceState(at: cursor, with: state)
        measure()
        notify()
    }

    /// Swaps ONLY entry `index`'s document — the colour-sampler carry of a
    /// non-geometry undo/redo, whose document differs from the
    /// entry's in nothing but its sampler list. That difference is
    /// render-neutral, as a guide edit is, so both neighbours keep
    /// their flags; the retained bound may no longer hold, so a full measure
    /// is due.
    func replaceDocument(at index: Int, with document: RasterDocument) {
        guard holds(index) else { return }
        self[retained: index].state = self[retained: index].state.replacingDocument(document)
        needsFullMeasure = true
    }

    /// A different document at an endpoint voids the render-neutral claim of
    /// both transitions that touch it.
    private func replaceState(at index: Int, with state: EditingState) {
        if self[retained: index].state.document !== state.document {
            self[retained: index].renderNeutral = false
            if holds(index + 1) { self[retained: index + 1].renderNeutral = false }
        }
        self[retained: index].state = state
    }

    func move(to index: Int) {
        cursor = index
        if pendingID != nil { needsFullMeasure = true }
        pendingID = nil
        measureBaseline()
        notify()
    }

    func measure() {
        guard let document, let current = document.captureEditingState() else { return }
        var baseline = [current]
        if let saved = document.historySavedState { baseline.append(saved) }
        do {
            baselineBytes = try EditingState.retainedBytes(in: baseline)
            retainedBytes = try EditingState.retainedBytes(in: entries.map(\.state) + baseline)
            accountingError = nil
            needsFullMeasure = false
        } catch {
            accountingError = error.localizedDescription
            needsFullMeasure = true
        }
    }

    /// The current and saved states only; `retainedBytes` keeps its bound.
    private func measureBaseline() {
        guard let document, let current = document.captureEditingState() else { return }
        var baseline = [current]
        if let saved = document.historySavedState { baseline.append(saved) }
        do {
            baselineBytes = try EditingState.retainedBytes(in: baseline)
        } catch {
            accountingError = error.localizedDescription
            needsFullMeasure = true
        }
    }

    /// Grows the bound by what the closed transition's state adds to the one
    /// before it, `U([prev, new]) − U([prev])`, saturating: a union that
    /// measures below its part cannot shrink the bound, and sets
    /// `needsFullMeasure` so the next budget check walks the history.
    private func addClosedTransitionBound() {
        guard cursor > 0, holds(cursor) else {
            needsFullMeasure = true
            measureBaseline()
            return
        }
        do {
            let previous = try EditingState.retainedBytes(in: [self[retained: cursor - 1].state])
            let both = try EditingState.retainedBytes(in: [self[retained: cursor - 1].state,
                                                           self[retained: cursor].state])
            if both < previous { needsFullMeasure = true }
            let (grown, overflow) = retainedBytes.addingReportingOverflow(
                both - min(both, previous))
            retainedBytes = overflow ? UInt64.max : grown
        } catch { needsFullMeasure = true }
        measureBaseline()
    }

    /// Keep the nearest undo transition even when it alone exceeds the budget.
    /// This explicit exception prevents an expensive completed edit from silently
    /// losing Undo. Other old states/redo tails are discarded and reported.
    ///
    /// Storage: a front eviction clears the oldest slot and advances
    /// `head` — O(1), and the evicted state is released at once — instead of
    /// `removeFirst()` shifting every retained entry; a tail eviction removes
    /// the last slot. After the loop the dead prefix is dropped once it is at
    /// least as long as the live part, so each eviction costs amortized O(1).
    /// What stays O(n) is `dropHistoryUndo`'s NSUndoManager walk
    /// (`ImageDocument+History.swift`).
    func enforceBudget() {
        guard !rebuilding, !navigating, suspended == 0 else { return }
        if needsFullMeasure || (accountingError == nil && isOverBudget) { measure() }
        var evicted: [HistoryUndoTarget] = []
        while count > 2,
              count - 1 > maximumTransitions || (accountingError == nil && isOverBudget) {
            if cursor > 1 {
                slots[head] = nil
                head += 1
                self[retained: 0].name = "Retained Boundary"
                cursor -= 1
                evicted.append(self[retained: 0].undoTarget)
            } else if count - 1 > cursor {
                if let removed = slots.removeLast() { evicted.append(removed.undoTarget) }
            } else { break }
            evictedCount += 1
            if accountingError == nil && isOverBudget { measure() }
        }
        if head > 0 && head >= slots.count - head {
            slots.removeFirst(head)
            head = 0
        }
        if !evicted.isEmpty { document?.dropHistoryUndo(for: evicted) }
    }

    func report() -> [String: Any] {
        refreshCurrentViewState()
        measure()
        var result: [String: Any] = [
            "states": entries.enumerated().map { index, entry in
                ["id": entry.id, "name": entry.name, "index": index,
                 "current": index == cursor, "saved": entry.state.document === document?.historySavedDocument,
                 "selection_only": index > 0 && entry.state.document === entries[index - 1].state.document] as [String: Any]
            },
            "cursor": cursor, "undo_count": cursor, "redo_count": max(0, count - cursor - 1),
            "budget_bytes": budgetBytes, "retained_bytes": retainedBytes,
            "baseline_bytes": baselineBytes, "incremental_history_bytes": incrementalBytes,
            "over_budget": isOverBudget, "evicted_states": evictedCount,
            "maximum_transitions": maximumTransitions,
            "single_edit_policy": "retain the nearest reversible transition even if it alone exceeds the history budget",
            "retention_policy": "Enforced after completed edits, saved-marker changes and explicit budget changes. Navigation keeps the same retained state union; moving to a smaller active state can increase incremental history bytes by reducing its baseline credit. The next enforcement trims older states while retaining one reversible transition.",
            "accounting": "unique core buffers and Swift selection payloads; CGImage/contour sizes estimated; active and saved-file baseline excluded from history budget",
        ]
        if let currentID { result["current_state_id"] = currentID }
        if let source = brushSource() { result["brush_source_id"] = source.id }
        if let accountingError { result["accounting_error"] = accountingError }
        return result
    }

    func notify() {
        guard let document else { return }
        NotificationCenter.default.post(name: .imageDocumentHistoryDidChange, object: document)
    }
}

/// Preserve NSUndoManager behavior while observing completed outer groups and
/// explicit history resets. All registrations/restores remain in ImageDocument.
final class HistoryUndoManager: UndoManager {
    weak var document: ImageDocument?
    /// True while `withUndoGroupDiscardingEmpty` defers its outer group to the
    /// body's first history registration.
    var defersOuterGroup = false
    /// A deferred group was opened during the current deferral.
    private(set) var deferredGroupOpened = false

    /// At a history registration inside a deferring body: opens the outer
    /// group whenever none is open. `groupingLevel == 0` alone decides. This is
    /// defensive: a serviced undo cannot close the group mid-body (`undo()`
    /// raises inside an open explicit group), but any other mid-body
    /// `endUndoGrouping` that leaves no group open is reopened at the next one.
    func openDeferredGroupIfNeeded() {
        guard defersOuterGroup, groupingLevel == 0 else { return }
        beginUndoGrouping()
        deferredGroupOpened = true
    }

    /// Ends the deferral and closes the group it opened; the final end
    /// reaches `closeGroup` as any outer group's does.
    func closeDeferredGroup() {
        defersOuterGroup = false
        guard deferredGroupOpened else { return }
        while groupingLevel > 0 { endUndoGrouping() }
        deferredGroupOpened = false
    }

    /// Non-nil while navigateHistory runs: the largest step overage so far (0 = none).
    var navigationOverage: UInt64?

    /// Each step estimates the restored state's first projection and
    /// records any overage as a warning, then proceeds; undo never refuses.
    override func undo() {
        guard document?.isPublishingEditingState != true else { return }
        let over = document?.historyStepOverage(redo: false)
        document?.recordHistoryStepOverage(over, redo: false, destinationID: destinationID(redo: false))
        super.undo()
    }

    override func redo() {
        guard document?.isPublishingEditingState != true else { return }
        let over = document?.historyStepOverage(redo: true)
        document?.recordHistoryStepOverage(over, redo: true, destinationID: destinationID(redo: true))
        super.redo()
    }

    /// The entry the next step lands on: undo `cursor − 1`, redo `cursor + 1`.
    private func destinationID(redo: Bool) -> String? {
        guard let history = document?.history else { return nil }
        let index = history.cursor + (redo ? 1 : -1)
        guard index >= 0, index < history.entries.count else { return nil }
        return history.entries[index].id
    }

    override func endUndoGrouping() {
        super.endUndoGrouping()
        if groupingLevel == 0, !isUndoing, !isRedoing { document?.history.closeGroup() }
    }

    override func removeAllActions() {
        super.removeAllActions()
        if document?.history.rebuilding != true { document?.history.reset(document?.captureEditingState()) }
    }
}
