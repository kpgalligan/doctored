import AppKit

// The helpers of the filter workspaces' UI actions (their `uiActions` entries are in
// actions.swift). Each calls the code a drag, a button or a scroll in the workspace calls; none
// synthesizes an event. The workspace is a sheet, cancelled at teardown (scenes.swift), never
// applied.

/// The open Liquify workspace.
private func liquifySheet(_ context: Context) throws -> LiquifySheetController {
    guard let sheet = context.window?.attachedSheet?.contentViewController as? LiquifySheetController else {
        throw ProbeFailure(description: "no Liquify workspace is open (open_sheet liquify first)")
    }
    return sheet
}

/// A canvas point in the Liquify preview's own proxy pixels (the inverse of the session's
/// `canvasPoint(proxy:)`), refused outside the layer.
private func liquifyProxyPoint(_ point: CGPoint, _ session: LiquifySession) throws -> CGPoint {
    let layer = CGRect(x: session.origin.x, y: session.origin.y, width: session.width, height: session.height)
    try require(layer.contains(point), "\(point) is outside the layer \(layer)")
    return CGPoint(x: (point.x - layer.minX) * CGFloat(session.proxy.width) / layer.width,
                   y: (point.y - layer.minY) * CGFloat(session.proxy.height) / layer.height)
}

/// `liquify_stroke`: a drag in the Liquify preview from `from` to `to` (canvas pixels) with the
/// current tool, as the preview forwards one: a press, ten moves along the line and a release.
func liquifyStroke(from: CGPoint, to: CGPoint, _ context: Context) throws {
    let sheet = try liquifySheet(context)
    let start = try liquifyProxyPoint(from, sheet.session), end = try liquifyProxyPoint(to, sheet.session)
    try require(sheet.strokeBegan(at: start, modifiers: []), "the stroke was refused: \(sheet.status.stringValue)")
    for step in 1...10 {
        let t = CGFloat(step) / 10
        sheet.strokeMoved(to: CGPoint(x: start.x + (end.x - start.x) * t, y: start.y + (end.y - start.y) * t))
    }
    sheet.strokeEnded()
    try require(sheet.session.strokes.count == 1, "the workspace holds \(sheet.session.strokes.count) strokes")
}

/// `liquify_actual_size`: the preview's 100% button, then a scroll of the preview that centres
/// `center` (canvas pixels), as a person scrolls to the part they are working on. At Fit the
/// preview shows the mesh scaled down, where its one-pixel lines drop out.
func liquifyActualSize(center: CGPoint, _ context: Context) throws {
    let sheet = try liquifySheet(context)
    let point = try liquifyProxyPoint(center, sheet.session)
    sheet.actualClicked(nil)
    guard let scroll = sheet.canvasView.enclosingScrollView else {
        throw ProbeFailure(description: "the Liquify preview is not in a scroll view")
    }
    let clip = scroll.contentView
    let visible = clip.bounds.size, document = sheet.canvasView.bounds.size
    // The preview is flipped, so its origin is the top-left corner, as the point's is.
    let origin = CGPoint(x: min(max(0, point.x - visible.width / 2), max(0, document.width - visible.width)),
                         y: min(max(0, point.y - visible.height / 2), max(0, document.height - visible.height)))
    clip.scroll(to: origin)
    scroll.reflectScrolledClipView(clip)
    try require(scroll.magnification == 1, "the preview is at \(scroll.magnification)×, not 100%")
}
