import AppKit

/// Window ▸ Match Zoom, Match Location and Match Zoom and Location (`ViewMatch`): this view's zoom
/// and/or location pushed at every other open view, of this document and every other. Their MCP
/// twin is `match_views`. Like the other view items they write no document.
extension EditorViewController {
    /// Window ▸ Match Zoom.
    @objc func matchZoom(_ sender: Any?) {
        runViewMatch(zoom: true, location: false)
    }

    /// Window ▸ Match Location.
    @objc func matchLocation(_ sender: Any?) {
        runViewMatch(zoom: false, location: true)
    }

    /// Window ▸ Match Zoom and Location.
    @objc func matchZoomAndLocation(_ sender: Any?) {
        runViewMatch(zoom: true, location: true)
    }

    private func runViewMatch(zoom: Bool, location: Bool) {
        guard document?.doc != nil, !ViewMatch.targets(excluding: self).isEmpty else {
            NSSound.beep()
            return
        }
        do {
            try ViewMatch.match(from: self, zoom: zoom, location: location)
        } catch {
            NSSound.beep()
        }
    }

    /// The three items; nil means "not one of mine". Enabled when this view shows an image and
    /// another view is open anywhere in the app.
    func validateMatchViewsItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        switch item.action {
        case #selector(matchZoom(_:)), #selector(matchLocation(_:)),
             #selector(matchZoomAndLocation(_:)):
            return document?.doc != nil && !ViewMatch.targets(excluding: self).isEmpty
        default:
            return nil
        }
    }
}
