import AppKit

/// Window ▸ Match Zoom, Match Location and Match Zoom and Location, and MCP `match_views`.
///
/// **The rule.** Zoom: every other view takes the source view's magnification. Location: every
/// other view centres on the same FRACTION of its own canvas as the source's centre is of the
/// source's canvas — the same pixel when the two canvases are the same size, and the matching
/// place on a resized copy. Rotation is never matched: it is the one thing each window keeps, as
/// under Sync Pan & Zoom. The targets are every view of every open document with an image but the
/// source, the source document's own other views included. It is a one-shot command: nothing stays
/// linked afterwards.
///
/// Only the view seams (`viewState`, `applyViewState`) are used, so the menu and the agent share
/// this one path, and neither writes a document: no undo step, no dirty flag, no history entry.
enum ViewMatch {
    /// One target after the match: its editor and its READ-BACK state (the clip view constrains
    /// its own origin, so the request is not the answer).
    struct Matched {
        let editor: EditorViewController
        let state: EditorViewState
    }

    /// The centre a view of a `canvas`-sized document takes to match `source`: the same fraction
    /// of the canvas on each axis, clamped into it.
    static func center(for source: EditorViewState, in canvas: CGSize) -> CGPoint {
        func fraction(_ value: CGFloat, of length: CGFloat) -> CGFloat {
            guard length > 0 else { return 0.5 }
            return min(max(value / length, 0), 1)
        }
        return CGPoint(x: fraction(source.center.x, of: source.canvas.width) * canvas.width,
                       y: fraction(source.center.y, of: source.canvas.height) * canvas.height)
    }

    /// Every open view but `source`, in document order and then window order.
    static func targets(excluding source: EditorViewController) -> [EditorViewController] {
        NSDocumentController.shared.documents.compactMap { $0 as? ImageDocument }
            .filter { $0.doc != nil }
            .flatMap(\.editors)
            .filter { $0 !== source }
    }

    /// Pushes `source`'s zoom and/or location at every other view.
    ///
    /// A target document with Sync Pan & Zoom on moves its own sibling views through its
    /// `propagateViewSync`, as any `applyViewState` does. The SOURCE document's propagation is
    /// held for the duration: its views are all targets already, and a target pushing its state
    /// back would move the source mid-match.
    @discardableResult
    static func match(from source: EditorViewController, zoom: Bool, location: Bool) throws
        -> [Matched]
    {
        let state = source.viewState
        let sourceDocument = source.document
        let wasPropagating = sourceDocument?.isPropagatingViewSync ?? false
        sourceDocument?.isPropagatingViewSync = true
        defer { sourceDocument?.isPropagatingViewSync = wasPropagating }
        var matched: [Matched] = []
        for target in targets(excluding: source) {
            let before = target.viewState
            // Zoom alone keeps the target's own centre, passed explicitly so a magnification
            // change cannot drift it.
            let center = location ? Self.center(for: state, in: before.canvas) : before.center
            let after = try target.applyViewState(zoom: zoom ? state.zoom : nil, center: center)
            matched.append(Matched(editor: target, state: after))
        }
        return matched
    }
}
