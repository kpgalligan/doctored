import AppKit
import QuartzCore

// A crop box past the image: the canvas is exactly image-sized and `draw(_:)` clips to it,
// so the part of an extended box beyond the image would vanish. `cropExtentLayer` is a
// `CAShapeLayer` sublayer of the canvas's own layer, which is not masked to its bounds
// (`EditorViewController.loadView`), in the canvas's flipped image coordinates: it strokes the
// extended box's outline and draws its eight handles, and is hidden for a box inside the image,
// which `drawCropOverlay` draws as before. View feedback only.

extension ImageCanvasView {
    /// Brings `cropExtentLayer` up to date with `cropOverlay` and answers whether the box
    /// reaches past the image (and so is drawn by the layer).
    @discardableResult
    func updateCropExtent() -> Bool {
        guard let overlay = cropOverlay, !isTransforming,
              !bounds.contains(overlay.rect), let host = layer else {
            if let shape = cropExtentLayer, !shape.isHidden {
                CATransaction.begin()
                CATransaction.setDisableActions(true)
                shape.isHidden = true
                CATransaction.commit()
            }
            return false
        }
        let shape = cropExtentLayer ?? makeCropExtentLayer(in: host)
        let scale = magnification
        let rect = overlay.rect
        let handles = CGMutablePath()
        let size = ImageCanvasView.transformHandleSize / scale
        for point in CropSession.handles(of: rect) {
            handles.addRect(CGRect(x: point.x - size / 2, y: point.y - size / 2,
                                   width: size, height: size))
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        shape.isHidden = false
        shape.frame = bounds
        shape.path = CGPath(rect: rect, transform: nil)
        shape.lineWidth = 1 / scale
        if let knobs = shape.sublayers?.first as? CAShapeLayer {
            knobs.frame = shape.bounds
            knobs.path = handles
            knobs.lineWidth = 1 / scale
        }
        CATransaction.commit()
        return true
    }

    private func makeCropExtentLayer(in host: CALayer) -> CAShapeLayer {
        let shape = CAShapeLayer()
        shape.fillColor = nil
        shape.strokeColor = NSColor.white.withAlphaComponent(0.95).cgColor
        shape.shadowColor = NSColor.black.cgColor
        shape.shadowOpacity = 0.45
        shape.shadowRadius = 1
        shape.shadowOffset = .zero
        let knobs = CAShapeLayer()
        knobs.fillColor = NSColor.white.cgColor
        knobs.strokeColor = NSColor.black.withAlphaComponent(0.65).cgColor
        shape.addSublayer(knobs)
        host.addSublayer(shape)
        cropExtentLayer = shape
        return shape
    }
}
