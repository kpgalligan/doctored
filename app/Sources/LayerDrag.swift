import AppKit

// Dragging layers between documents: drag rows out of one window's
// Layers panel, or drag a layer with the Move tool until the pointer leaves
// its window, and drop on another document's canvas or Layers panel. The
// dragged layers (subtrees included) are COPIED into the destination above
// its active layer, as ONE "Paste Layers" undo step there; the source
// document is not changed. ⇧ at the drop keeps their canvas position (Paste
// in Place); otherwise they land by Edit ▸ Paste's rule.
//
// The drop is the layer clipboard's paste (`pasteLayers`,
// `LayerClipboard.swift`) fed an entry made for the drag, so depth and
// profile conversion, the Paste Profile Mismatch alert and every refusal are
// that paste's. The clipboard itself is neither read nor written. MCP
// `copy_layers_to_document` is the twin.
//
// One thing differs from Copy and Paste of layers: a dragged layer carries NO
// image-source state. A camera RAW layer lands as the pixels it shows,
// rasterized on the drag's own copy
// (`LayerDrag.asPixels`); the source document's layer is untouched.
//
// The drag is in-process only: the pasteboard carries a marker type, and the
// layers themselves stay in `LayerDrag.current` for the session's lifetime.

enum LayerDrag {
    /// The marker a canvas-started layer drag puts on the drag pasteboard.
    /// (A panel-started drag carries the panel's own row type.)
    static let pasteboardType = NSPasteboard.PasteboardType("com.kgalligan.doctored.layer-drag")

    /// One drag in flight: where it came from and which entries it carries.
    struct Session {
        weak var source: ImageDocument?
        let indices: [Int]
        /// The view the drag started in (a panel's table or a canvas).
        weak var sourceView: AnyObject?
    }

    /// Set when a drag begins and cleared when it ends, by the drag's source.
    static var current: Session?

    /// The session behind a drag over some window, or nil when the drag is
    /// not a layer drag (or its source document has gone).
    static func session(for info: NSDraggingInfo) -> Session? {
        guard let session = current, session.source?.doc != nil, !session.indices.isEmpty,
              info.draggingPasteboard.availableType(
                  from: [pasteboardType, LayersPanelViewController.layerRowType]) != nil
        else { return nil }
        return session
    }

    /// The layers a drop pastes: `indices` of `document`, subtrees included,
    /// as their own clip document — what Edit ▸ Copy of layers holds, without
    /// touching the clipboard, and with every camera RAW layer's retained
    /// source rasterized (`asPixels`).
    static func entry(from document: ImageDocument, indices: [Int]) throws -> LayerClipboard.Entry {
        guard let doc = document.doc else {
            throw DoctoredCoreError(message: "The source document has no image.")
        }
        guard let clip = try doc.duplicatingToNewDocument(indices) else {
            throw DoctoredCoreError(message: "The dragged layers could not be copied.")
        }
        return LayerClipboard.Entry(
            token: UUID().uuidString, layers: try asPixels(clip),
            sourceRoots: doc.layerTree.independentRoots(indices))
    }

    /// `clip` with every layer that holds a retained image source (a camera
    /// RAW layer, the only kind that keeps one) rasterized: a
    /// dragged layer lands in the other document as its current pixels and
    /// carries no source state there. Done on the CLIP, so the source
    /// document keeps its layers as they are. Each is the core's own
    /// `rasterize` source edit — the one Layer ▸ Rasterize RAW Layer
    /// runs — which keeps the rendered pixels exactly and the layer's index.
    /// A layer whose pixels are locked cannot be rasterized and refuses the
    /// drag by name.
    private static func asPixels(_ clip: RasterDocument) throws -> RasterDocument {
        var result = clip
        for index in 0..<clip.layerCount where clip.hasRetainedSource(index) {
            let request = try PrecisionPixelCommand(["op": "rasterize", "layer": index]).request
            do {
                if let next = try result.editingRetainedSource(request: request) { result = next }
            } catch {
                let name = clip.layerInfo(index)?.name ?? "Layer \(index)"
                throw DoctoredCoreError(
                    message: "“\(name)” keeps an image source and could not be copied as "
                        + "pixels (\(error.localizedDescription)). Unlock its pixels or "
                        + "rasterize it first.")
            }
        }
        return result
    }

    /// The image a canvas-started drag carries under the pointer.
    static func dragImage() -> NSImage {
        let size = NSSize(width: 44, height: 44)
        return NSImage(size: size, flipped: false) { rect in
            let plate = NSBezierPath(roundedRect: rect.insetBy(dx: 2, dy: 2), xRadius: 8, yRadius: 8)
            NSColor.black.withAlphaComponent(0.55).setFill()
            plate.fill()
            NSColor.white.withAlphaComponent(0.9).setStroke()
            plate.lineWidth = 1
            plate.stroke()
            if let symbol = NSImage(systemSymbolName: "square.3.layers.3d",
                                    accessibilityDescription: "Layers") {
                symbol.draw(in: rect.insetBy(dx: 10, dy: 10), from: .zero, operation: .sourceOver,
                            fraction: 1)
            }
            return true
        }
    }
}
