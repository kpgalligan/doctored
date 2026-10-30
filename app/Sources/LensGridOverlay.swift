import AppKit

/// Filters ▸ Lens Correction's alignment grid: a Show Grid checkbox in the sheet and the
/// square grid it lays over the canvas while the sheet is up, so straightening a horizon or a
/// converging wall is judged against true lines, not the picture's own.
///
/// The accessory reports no values (the grid is a viewing aid; nothing it does reaches the
/// filter). Its row is a view that adds the overlay to the canvas when it enters a window (the
/// sheet appearing) and removes it when it leaves one (Apply or Cancel), so the grid lives exactly
/// as long as the sheet.
final class LensGridAccessory: NSObject, FilterSheetAccessory {
    let view: NSView
    var onChange: (() -> Void)?
    private let checkbox: NSButton
    private let overlay = LensGridOverlayView()
    private weak var canvas: ImageCanvasView?

    init(canvas: ImageCanvasView) {
        self.canvas = canvas
        let host = LensGridHostView()
        checkbox = NSButton(checkboxWithTitle: "Show Grid", target: nil, action: nil)
        checkbox.state = .on
        let grid = NSGridView(views: [[NSView(), checkbox]])
        grid.columnSpacing = 12
        grid.column(at: 0).width = 106
        grid.translatesAutoresizingMaskIntoConstraints = false
        host.addSubview(grid)
        NSLayoutConstraint.activate([
            grid.leadingAnchor.constraint(equalTo: host.leadingAnchor),
            grid.trailingAnchor.constraint(lessThanOrEqualTo: host.trailingAnchor),
            grid.topAnchor.constraint(equalTo: host.topAnchor),
            grid.bottomAnchor.constraint(equalTo: host.bottomAnchor),
        ])
        view = host
        super.init()
        checkbox.target = self
        checkbox.action = #selector(toggled(_:))
        host.onWindowChange = { [weak self] inWindow in self?.place(inWindow) }
    }

    var values: [Double] { [] }

    @objc private func toggled(_ sender: Any?) { place(view.window != nil) }

    /// Shows the overlay over the whole canvas while the sheet is in a window and the box is
    /// ticked; removes it otherwise.
    private func place(_ inWindow: Bool) {
        guard inWindow, checkbox.state == .on, let canvas else {
            overlay.removeFromSuperview()
            return
        }
        if overlay.superview !== canvas {
            overlay.frame = canvas.bounds
            overlay.autoresizingMask = [.width, .height]
            canvas.addSubview(overlay)
        }
        overlay.needsDisplay = true
    }
}

/// The accessory's row: reports when it enters or leaves a window.
private final class LensGridHostView: NSView {
    var onWindowChange: ((Bool) -> Void)?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        onWindowChange?(window != nil)
    }
}

/// The grid itself: a flipped canvas subview in canvas pixels (the canvas's own coordinates),
/// with lines every max(W, H)/16 pixels counted out from the canvas centre, so the centre lines
/// are the picture's axes and the cells are square. It never takes a click.
final class LensGridOverlayView: NSView {
    /// Sixteen cells across the longer side: fine enough to check a horizon against, coarse
    /// enough to leave the picture readable.
    private static let cellsAcrossLongerSide: CGFloat = 16

    override var isFlipped: Bool { true }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        let size = bounds.size
        let step = max(size.width, size.height) / Self.cellsAcrossLongerSide
        guard step > 0 else { return }
        let center = CGPoint(x: bounds.midX, y: bounds.midY)
        let path = NSBezierPath()
        var offset: CGFloat = 0
        while offset <= max(size.width, size.height) / 2 {
            for x in Set([center.x - offset, center.x + offset]) where x >= bounds.minX && x <= bounds.maxX {
                path.move(to: CGPoint(x: x, y: bounds.minY))
                path.line(to: CGPoint(x: x, y: bounds.maxY))
            }
            for y in Set([center.y - offset, center.y + offset]) where y >= bounds.minY && y <= bounds.maxY {
                path.move(to: CGPoint(x: bounds.minX, y: y))
                path.line(to: CGPoint(x: bounds.maxX, y: y))
            }
            offset += step
        }
        // One screen point wide at any zoom: the canvas draws in image pixels, scaled by the
        // scroll view's magnification.
        let magnification = max(enclosingScrollView?.magnification ?? 1, 0.001)
        // The document grid's ink, a little heavier than its major lines (0.30): this grid is
        // up only while it is being used to line something up.
        path.lineWidth = 1 / magnification
        DS.gridInk.withAlphaComponent(0.45).setStroke()
        path.stroke()
    }
}
