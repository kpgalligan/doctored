import AppKit

// The byte opacity drag is unchanged: it holds `liveEditBase`, so `refuseLiveGestureMutation`
// refuses a foreign edit BY NAME and the revision cannot move under it.

/// The controller half of the typed opacity drag (Phase 12E-2, P7-27): it
/// owns the session, so the whole gesture shares ONE capture, and it is the
/// only place the session is created or released.
extension EditorViewController {
    /// One tick of the Layers panel's opacity slider on a TYPED document.
    /// The first tick builds the session (and its one capture); every tick
    /// previews; `ended` commits exactly once and releases the session on
    /// EVERY exit, the throw included.
    func precisionOpacityScrub(_ value: Double, ended: Bool) throws {
        guard let document = document else { return }
        let session: PrecisionOpacityDrag
        if let existing = precisionOpacityDrag {
            session = existing
        } else {
            // A refused capture (a live gesture, a closed owner) throws here
            // and nothing is stored, so the next tick tries again cleanly.
            session = try PrecisionOpacityDrag(document, canvas: canvas)
            precisionOpacityDrag = session
        }
        guard ended else {
            session.preview(opacity: value)
            return
        }
        defer { precisionOpacityDrag = nil }
        _ = try session.finish(opacity: value)
    }

    /// The panel's closure target: the gesture is a UI action, so a refusal
    /// is shown in the document's own alert rather than swallowed.
    func precisionOpacityScrubbed(_ value: Double, ended: Bool) {
        guard let document = document else { return }
        do {
            try precisionOpacityScrub(value, ended: ended)
        } catch {
            document.presentError(error)
        }
    }
}
