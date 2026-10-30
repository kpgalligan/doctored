import AppKit

/// Runs `body` inside an explicit undo group and closes only groups above the
/// level already owned by its caller. Existing AppKit/event groups stay open.
///
/// At level zero, temporarily turn off automatic event grouping before opening
/// our group. Otherwise NSUndoManager creates both an implicit event group and
/// our explicit group. Draining both inside a modal timer can leave AppKit's
/// event bookkeeping expecting the implicit group; a later NSDocument Save then
/// attempts an unmatched endUndoGrouping. One explicitly owned group avoids
/// that stale event boundary while retaining one Undo entry per MCP command.
///
/// Event grouping is restored after the owned group closes, including on error.
/// Nested callers preserve the outer group's policy and lifetime. A one-entry
/// Action run's steps (`run_action`, Batch) therefore still collapse into the
/// player's single Undo transition.
///
/// Inside `withUndoGroupDiscardingEmpty`'s deferral with no group open, the
/// body runs ungrouped: its first history registration opens the deferred
/// outer group.
func withUndoGroup<T>(_ manager: UndoManager?, _ body: () throws -> T) rethrows -> T {
    guard let manager else { return try body() }
    if (manager as? HistoryUndoManager)?.defersOuterGroup == true, manager.groupingLevel == 0 {
        return try body()
    }
    let base = manager.groupingLevel
    let suppressAutomaticGroup = base == 0 && manager.groupsByEvent
    if suppressAutomaticGroup { manager.groupsByEvent = false }
    manager.beginUndoGrouping()
    defer {
        while manager.groupingLevel > base {
            manager.endUndoGrouping()
        }
        if suppressAutomaticGroup { manager.groupsByEvent = true }
    }
    return try body()
}
