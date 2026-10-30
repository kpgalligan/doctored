import AppKit

/// A fit-to-view preview with the same top-left pixel coordinates as the canvas.
/// Color Range samples the immutable source, never the displayed selection mask.
final class SelectionPreviewView: NSView {
    var image: CGImage? { didSet { needsDisplay = true } }
    var onPoint: ((CGPoint) -> Void)?
    override var isFlipped: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        widthAnchor.constraint(equalToConstant: 250).isActive = true
        heightAnchor.constraint(equalToConstant: 188).isActive = true
    }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("SelectionPreviewView does not support NSCoder") }

    private var imageRect: CGRect {
        guard let image else { return .zero }
        let scale = min(bounds.width / CGFloat(image.width), bounds.height / CGFloat(image.height))
        let size = CGSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
        return CGRect(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2,
                      width: size.width, height: size.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        DS.chromeBackground.setFill()
        bounds.fill()
        guard let image else { return }
        NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height)).draw(
            in: imageRect, from: .zero, operation: .sourceOver, fraction: 1,
            respectFlipped: true, hints: [.interpolation: NSImageInterpolation.high])
    }

    override func mouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let rect = imageRect
        guard let image, rect.contains(point), rect.width > 0, rect.height > 0 else { return }
        onPoint?(CGPoint(x: (point.x - rect.minX) / rect.width * CGFloat(image.width),
                         y: (point.y - rect.minY) / rect.height * CGFloat(image.height)))
    }
}

func selectionSheetField(_ value: Double, identifier: String, target: AnyObject,
                         action: Selector) -> NSTextField {
    let field = NSTextField(string: String(value))
    field.identifier = NSUserInterfaceItemIdentifier(identifier)
    field.target = target
    field.action = action
    field.widthAnchor.constraint(equalToConstant: 82).isActive = true
    DSField.style(field)
    return field
}

func selectionPreviewRow(_ source: SelectionPreviewView, _ mask: SelectionPreviewView) -> NSStackView {
    let previews = [source, mask].enumerated().map { index, preview -> NSStackView in
        let title = fieldLabel(index == 0 ? "Captured image" : "Selection coverage (white = selected)")
        let column = NSStackView(views: [title, preview])
        column.orientation = .vertical
        column.alignment = .leading
        column.spacing = 5
        return column
    }
    let row = NSStackView(views: previews)
    row.orientation = .horizontal
    row.spacing = 12
    return row
}
