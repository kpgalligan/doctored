import AppKit

// `commitPendingCanvasSessions(document)` is a no-op with no open text, shape or Free Transform
// session (`hasPendingCanvasSession`), else `ImageDocument.commitPendingCanvasSessions()` inside
// `withUndoGroupDiscardingEmpty` (one Undo step, and none for a refused or no-op commit)
// under `SessionCommitRefusal.capturing`, throwing the refusal's own sentence when the commit left
// a session open.
//
// `contact_sheet_from_sources` never commits its read-only sources: it refuses by name.

extension AgentServer {
    /// Commits an open canvas session — text entry, a reopened shape, a Free
    /// Transform — before an MCP writer reads the composite: the agent twin of
    /// the guard File ▸ Save / Save As / Export run first
    /// (`ImageDocument.save`/`saveAs`, `EditorViewController+Export`), so
    /// what the agent writes is what the canvas shows. The commit runs inside
    /// its own undo group (off the event path, `performGroupedEdit`'s rule), so
    /// it is one Undo step — and a commit that recorded nothing (refused, or
    /// an untouched session) leaves none: `withUndoGroupDiscardingEmpty`
    /// drops the empty group. A refused commit is still presented in
    /// the window — the session is the user's — and the reply carries that
    /// refusal's own sentence.
    func commitPendingCanvasSessions(_ document: ImageDocument) throws {
        guard document.hasPendingCanvasSession else { return }
        let result = SessionCommitRefusal.capturing {
            document.withUndoGroupDiscardingEmpty { document.commitPendingCanvasSessions() }
        }
        guard result.committed else {
            throw ToolError(message: SessionCommitRefusal.agentMessage(captured: result.sentence))
        }
    }
}
