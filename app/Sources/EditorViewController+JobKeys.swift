import AppKit

// Esc on the canvas (below every gesture and session, above Deselect, swallowed while the job
// drains) — and, through `JobEscape.swift` (the rule is
// `DocumentJobs.escapeCancelsUserJob`), Esc that nothing else consumed, outside a text view, in the
// editor window (`EditorWindow`'s `keyDown` and `cancelOperation`; with the window itself first
// responder it first hands every key back to the canvas) and in the four floating windows, History,
// Snapshots and Layer Comps (`JobEscapePanel`, the panel's document) and Actions
// (`JobEscapeWindow`, Play's target) — and ⌘Z/⇧⌘Z (Undo/Redo read "Cancel <title>") cancel the
// document's running UI-started job (`showProgress` marks UI origin) through
// `DocumentJobs.cancelUserJob`, the panel's Cancel, so its completion still runs; an agent's job is
// not cancelled by the keys.

// ⌘Z/⇧⌘Z and Esc while a `DocumentJobs` job runs.
//
// - The keys cancel only a UI-started job (the panel's Cancel path, so the
//   job's completion — heal pin clearing, alerts — still runs as `cancelled`).
//   An agent's job is not the user's latest action: over one, ⌘Z keeps
//   popping a step and the agent's job ends stale in band.
// - Esc sits below every in-flight canvas gesture and session (transform,
//   guide, crop, shape edit, move, selection stroke, marquee, stroke, lasso)
//   and above Deselect. It swallows every press while the UI job is still
//   draining, so a second or auto-repeated Esc never deselects.
// - While such a job runs, both Undo and Redo read "Cancel <title>": no step
//   pops and nothing lands in Redo. Once it is cancelled, a second ⌘Z pops a
//   step normally.
// - MCP `undo`/`redo` refuse by name instead
//   (`DocumentJobs.historyStepRefusal`).

extension EditorViewController {
    /// ⌘Z/⇧⌘Z: true when it cancelled the document's running UI job instead
    /// of popping a step. A focused field keeps its typing undo.
    func undoKeyCancelsJob() -> Bool {
        if view.window?.firstResponder is NSText { return false }
        guard let document else { return false }
        return DocumentJobs.cancelUserJob(on: document)
    }

    /// Esc with no canvas gesture or session of its own: true while the
    /// document's UI job is still running (cancelling it on the first press),
    /// so Escape does not also deselect.
    func escapeCancelsJob() -> Bool {
        DocumentJobs.escapeCancelsUserJob(on: document)
    }

    /// Undo/Redo while a UI job runs: enabled, titled "Cancel <title>". nil
    /// otherwise, so the regular arms decide.
    func validateJobKeyItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        guard item.action == #selector(undo(_:)) || item.action == #selector(redo(_:)),
              !(view.window?.firstResponder is NSText), let document,
              let job = DocumentJobs.userCancellableJob(on: document) else { return nil }
        if let menuItem = item as? NSMenuItem { menuItem.title = "Cancel \(job.title)" }
        return true
    }
}
