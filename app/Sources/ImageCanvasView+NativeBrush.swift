import AppKit

extension ImageCanvasView {
    func stampNativePixelAirbrush(in context: CGContext, at cursor: CGPoint, pressure: CGFloat) {
        let point = CGPoint(x: floor(cursor.x) + 0.5, y: floor(cursor.y) + 0.5)
        nativeStrokeSession?.stamp(at: point, diameter: 1, pressure: pressure)
        guard let session = nativeStrokeSession, session.acceptsInk else { return }
        context.saveGState()
        context.setAlpha(session.tip.flow * (session.pressureOpacity ? pressure : 1))
        context.fill(CGRect(x: point.x - 0.5, y: point.y - 0.5, width: 1, height: 1))
        context.restoreGState()
    }

    /// One beep when a canvas stroke reaches a stroke cap and stops laying ink;
    /// the dabs already laid commit at mouse-up.
    func beepIfStrokeInkStopped() {
        if nativeStrokeSession?.takeFullNotice() == true { NSSound.beep() }
    }

    /// Native Quick Mask owns u16 selection history. Its byte preview is not
    /// the committed coverage source.
    func nativeQuickMaskStroke() -> QuickMaskStroke? {
        guard nativeBrushEnabled, let session = nativeStrokeSession, let image else { return nil }
        do {
            let stroke = try session.resolved()
            return try QuickMaskStroke(nativeBrush: stroke, width: image.width,
                height: image.height, opacity: session.strokeOpacity, erase: session.erasing)
        } catch { NSApp.presentError(error); return nil }
    }
}
