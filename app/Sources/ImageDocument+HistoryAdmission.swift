import Foundation

// Undo, redo and navigation never refuse:
// `HistoryUndoManager.undo()`/`redo()` first ask `historyStepOverage` (the restored state's
// `projectionScratchEstimate()` through `RasterResources.overLimitBytes`) and record a
// `HistoryMemoryWarning` keyed to the history entry they land on (this file; `navigateHistory`
// collects its steps into one), shown as the status bar's "Over working limit"
// (`EditorViewController+HistoryAdmission.swift`) and returned as MCP `memory_warning`. The
// estimate's measurement record (batch 166) is the doc comment on this file's `ImageDocument`
// extension.

/// An undo, redo or History navigation that went over the working limit.
/// Undo never refuses (the maintainer's decision, 2026-10-04):
/// the step proceeds and leaves this warning, keyed to the history entry it
/// landed on, for the status bar and MCP's `memory_warning`.
struct HistoryMemoryWarning: Equatable {
    /// The history entry now shown; the warning is visible only while it is current.
    let entryID: String
    let overBytes: UInt64
    /// The full sentence.
    let detail: String
    /// The status segment.
    var text: String { "Over working limit" }
}

/// The estimate of a history step's first projection: the restored
/// state's display projection, charged before `HistoryUndoManager` steps.
///
/// Charge: `projectionScratchEstimate()` at both depths — at 16 bits
/// `PreparedDocumentEdit`'s own, at 8 bits the margined projection (the
/// fit's first candidate; `commitStageBytes`, its second, was not needed).
///
/// Record (batch 166): the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB,
/// `MallocLargeCache=0`, a fresh instance per run, E =
/// `ri_interval_max_phys_footprint` − pre around the last MCP call, 3 runs, a
/// baseline twin at E 0 ×3. P = the step's charge; navigation P = the largest
/// step charge + max(0, R_post − R_pre), R = `unique_retained − cache`.
/// E per run / P / P/E:
///   U1 photo45 (8256×5504) invert → undo   642,744,560 / 850,248,016 / 866,107,992   P 1,499,553,792  1.731
///   U3 png8-48 (8064×6048) invert → undo   970,752,648 / 972,243,592 / 618,004,720   P 1,609,445,376  1.655
///   U4 45 MP place → undo → redo           850,821,768 / 480,575,632 / 567,984,272   P 1,499,553,792  1.762
///   U5 the same + live Gaussian 4 → undo   856,834,696 / 850,018,952 / 856,621,704   P 1,499,553,792  1.750
///   U6 U5 → undo → redo                    905,085,600 / 904,839,840 / 905,085,600   P 1,499,553,792  1.657
///   N8 png8-48 invert ×2 → Initial State   1,147,732,760 / 1,153,041,176 / 1,147,421,176  P 1,609,445,376  1.396
///   U7 P16 (6000×4000, 16-bit) invert → undo  549,601,712 / 563,528,112 / 546,750,896  P 795,586,399  1.412
///   N16 P16 invert ×2 → Initial State      423,756,232 / 568,115,632 / 423,117,160   P 795,586,399  1.400
/// Each navigation step was estimated live (N8, N16: two steps, each at the
/// full-size charge). Rule: every row ≥ ⌈1.25 × max E⌉.
/// Since m8 rose to 7, the 8-bit charges rise by 7/16 − 6/16 of the structural
/// estimate (U1/U4–U6 P 1,567,715,328; U3/N8 1,682,601,984), so P/E only rises.
extension ImageDocument {
    /// The next Undo (redo false) or Redo's estimated overage: nil when it fits, is not applicable, or the estimate throws.
    func historyStepOverage(redo: Bool) -> (name: String, overBytes: UInt64)? {
        guard let manager = undoManager as? HistoryUndoManager, manager.groupingLevel == 0,
              !history.rebuilding, let current = doc else { return nil }
        let entries = history.entries
        let cursor = history.cursor
        let transition = redo ? cursor + 1 : cursor
        let destination = redo ? cursor + 1 : cursor - 1
        guard transition >= 1, transition < entries.count,
              destination >= 0, destination < entries.count else { return nil }
        let name = entries[transition].name
        guard (redo ? manager.redoActionName : manager.undoActionName) == name else { return nil }
        let restored = entries[destination].state.document
        guard restored !== current else { return nil }
        if entries[transition].renderNeutral,
           carriedProjection(from: current, to: restored) != nil { return nil }
        do {
            let charge = try restored.projectionScratchEstimate()
            let own = (try? restored.ownRetainedBytes()) ?? 0
            guard let over = try RasterResources.shared.overLimitBytes(scratch: charge, own: own)
            else { return nil }
            return (name, over)
        } catch {
            return nil
        }
    }

    /// Called by HistoryUndoManager before `super.undo()/redo()`. A no-op while `history.rebuilding`.
    func recordHistoryStepOverage(_ overage: (name: String, overBytes: UInt64)?, redo: Bool,
                                  destinationID: String?) {
        guard !history.rebuilding else { return }
        if let manager = undoManager as? HistoryUndoManager, let running = manager.navigationOverage {
            manager.navigationOverage = max(running, overage?.overBytes ?? 0)
            return
        }
        guard let overage, let destinationID else {
            historyMemoryWarningStorage = nil
            return
        }
        let detail = "\(redo ? "Redo" : "Undo") “\(overage.name)” went over the working limit by "
            + "\(Self.historyWarningMemory(overage.overBytes)). \(Self.historyWarningRemedy)"
        historyMemoryWarningStorage = HistoryMemoryWarning(
            entryID: destinationID, overBytes: overage.overBytes, detail: detail)
    }

    /// `historyMemoryWarningStorage` while its entryID is `history.entries[history.cursor].id`; else nil.
    var historyMemoryWarning: HistoryMemoryWarning? {
        guard let warning = historyMemoryWarningStorage,
              warning.entryID == history.currentID else { return nil }
        return warning
    }

    /// `navigateHistory`'s warning for the entry it landed on: the largest
    /// step overage of the walk, or none.
    func publishNavigationOverage(_ overBytes: UInt64, destinationName: String) {
        if overBytes > 0, let current = history.currentID {
            let detail = "Going to “\(destinationName)” in History went over the working limit by "
                + "\(Self.historyWarningMemory(overBytes)). \(Self.historyWarningRemedy)"
            historyMemoryWarningStorage = HistoryMemoryWarning(
                entryID: current, overBytes: overBytes, detail: detail)
        } else {
            historyMemoryWarningStorage = nil
        }
        history.notify()
    }

    static let historyWarningRemedy = "Free memory by closing other documents or lowering the "
        + "history budget in Window ▸ History, or raise the limit in Doctored ▸ Working Memory… "
        + "(it applies after a restart)."

    /// As `RasterResources.admit` formats bytes.
    static func historyWarningMemory(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(min(bytes, UInt64(Int64.max))),
                                  countStyle: .memory)
    }
}
