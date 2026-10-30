import AppKit

// A refusal `SessionCommitRefusal.capturing` captures is still presented in the window; capture
// only listens — the NSAlert refusal sites
// (`refuseLockedEdit`/`refuseLockedRerender`/`refuseFrozenAlpha`/`refuseTransform`) `note` their
// `messageText`, and `ImageDocument.willPresentError` notes every error a document presents (so a
// new refusal presented through `document.presentError` is captured with no edit;
// `NSApp.presentError` bypasses it). A beep-only refusal goes through
// `SessionCommitRefusal.beep(_:)` (note, then beep; outside a capturing scope it only beeps), so it
// names its reason too: `applyEdit`'s entry guard ("Finish the current gesture or edit
// first. Nothing changed.") and its post-transform beep ("<action> was refused and nothing
// changed." for a nil result, "The document changed while <action> was running. Nothing changed."
// otherwise), the `SilentEditRefusal` branches of `ImageDocument.presentError` and
// `admitsPublication` (the refusal's message), `commitRasterTransformJob`'s nil result and
// `commitDescribedTransform`'s refused placement (`refuseUntransformableSet`'s sentences). A
// declined `confirmRasterize` prompt notes "The prompt to rasterize “<name>” was cancelled in the
// document window."; a cancelled transform job (`commitRasterTransformJob`,
// `commitPrecisionTransformSession`, `commitFloatingTransform`) notes `JobCancelled.message`; and
// `commitDescribedTransform` answers `.refused`, keeping the session, when `applyEdit` did not
// change the document.
//
// The last note wins, so `refuseLockedRerender` after `applyEdit`'s nil beep stays the agent's
// sentence.

/// The sentence a refused canvas-session commit presented, captured so an MCP
/// writer that commits the session first (the agent twin of File ▸ Save /
/// Export's guard) can answer with the SAME words the window shows.
/// The refusal is still presented in the window — the session is the
/// user's — so this only listens: the NSAlert refusals `note` their
/// `messageText`, `ImageDocument.willPresentError` notes every error the
/// document presents, and a beep-only refusal goes through `beep(_:)` so it
/// names its reason too. Main thread only.
enum SessionCommitRefusal {
    private static var active = false
    private static var captured: String?

    /// Runs `body` with capture on and answers its result with the LAST noted
    /// sentence (nil if none). Nests: an inner scope saves and restores the
    /// outer one, so the outer scope sees none of the inner scope's notes.
    static func capturing(_ body: () -> Bool) -> (committed: Bool, sentence: String?) {
        let outerActive = active
        let outerCaptured = captured
        active = true
        captured = nil
        defer {
            active = outerActive
            captured = outerCaptured
        }
        let committed = body()
        return (committed, captured)
    }

    /// Records `sentence` as the refusal of the current scope; a no-op
    /// outside one.
    static func note(_ sentence: String) {
        guard active else { return }
        captured = sentence
    }

    /// A beep-only refusal: notes `sentence` for the current scope (a no-op
    /// outside one), then beeps.
    static func beep(_ sentence: String) {
        note(sentence)
        NSSound.beep()
    }

    /// The agent's reply when the commit left a session open: the refusal's
    /// own sentence when one was presented, a sentence naming the open
    /// session otherwise (a beep-only refusal).
    static func agentMessage(captured: String?) -> String {
        if let sentence = captured {
            return "The open canvas session could not be committed, so nothing was written: "
                + sentence
        }
        return "An open text, shape, Free Transform, Puppet Warp or Perspective Warp session on the canvas "
            + "could not be committed, so nothing was written. Commit or cancel it in the "
            + "document window, then try again."
    }
}

extension EditorViewController {
    /// True while a canvas session `commitPendingSessions()` would commit is
    /// open: a Free Transform, a reopened shape, text entry, Puppet Warp or
    /// Perspective Warp.
    var hasPendingCanvasSession: Bool {
        transformSession != nil || shapeEditSession != nil || canvas.hasActiveTextSession
            || canvas.puppetWarpSession != nil || canvas.perspectiveWarpSession != nil
    }
}

extension ImageDocument {
    /// True when any of this document's editors has an open canvas session.
    var hasPendingCanvasSession: Bool {
        windowControllers.contains {
            ($0.contentViewController as? EditorViewController)?.hasPendingCanvasSession == true
        }
    }

    /// Every error this document presents passes here first, so a refusal a
    /// session commit presents through `presentError` is captured for the
    /// agent (`SessionCommitRefusal`) with no edit at the presenting site. A
    /// user cancel is not a refusal sentence.
    override func willPresentError(_ error: Error) -> Error {
        let nsError = error as NSError
        if !(nsError.domain == NSCocoaErrorDomain && nsError.code == NSUserCancelledError) {
            SessionCommitRefusal.note(error.localizedDescription)
        }
        return super.willPresentError(error)
    }
}
