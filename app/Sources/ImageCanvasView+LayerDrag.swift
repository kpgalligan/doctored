import AppKit

/// The canvas as the SOURCE of a layer drag (LayerDrag.swift): a Move
/// drag with no pixel selection whose pointer leaves the window stops moving
/// the layer and becomes a drag of the layer itself, to drop on another
/// document. The controller cancels the move and names the layers
/// (`EditorViewController+LayerDrag.swift`); this file starts the dragging
/// session and answers as its source.
extension ImageCanvasView: NSDraggingSource {
    /// Whether this Move drag tick has left the window with layers in hand.
    func layerDragLeftWindow() -> Bool {
        guard tool == .move, selection == nil, !isTransforming, let window else { return false }
        return !window.frame.contains(NSEvent.mouseLocation)
    }

    /// Starts the dragging session from the drag event that left the window.
    func beginLayerDragSession(with event: NSEvent) {
        let marker = NSPasteboardItem()
        marker.setString("layers", forType: LayerDrag.pasteboardType)
        let item = NSDraggingItem(pasteboardWriter: marker)
        let image = LayerDrag.dragImage()
        // View coordinates are canvas pixels: keep the image its screen size.
        let side = image.size.width / magnification
        let point = convert(event.locationInWindow, from: nil)
        item.setDraggingFrame(
            CGRect(x: point.x - side / 2, y: point.y - side / 2, width: side, height: side),
            contents: image)
        beginDraggingSession(with: [item], event: event, source: self)
    }

    func draggingSession(
        _ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext
    ) -> NSDragOperation {
        context == .withinApplication ? .copy : []
    }

    func draggingSession(
        _ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation
    ) {
        LayerDrag.current = nil
    }
}
