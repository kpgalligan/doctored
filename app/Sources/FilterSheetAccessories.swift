import AppKit

// The `SpatialKernel` route is `dr_image_filter_op` at 8 bits and the selected filter at 16.

/// The non-slider controls a `SliderSheetController` can carry under its grid:
/// a choice popup, an integer field and Radial Blur's centre
/// picker. Each row lines up with the slider grid's 106 pt label column, and
/// each reports its values as Doubles appended after the sliders' mapped
/// values, so a factory's `params` closure reads one array.

/// One labelled row in the slider grid's geometry.
private func accessoryRow(_ label: String, _ control: NSView) -> NSView {
    let grid = NSGridView(views: [[fieldLabel(label), control]])
    grid.rowSpacing = 10
    grid.columnSpacing = 12
    grid.column(at: 0).xPlacement = .trailing
    grid.column(at: 0).width = 106
    return grid
}

/// A popup of `titles`; its value is the selected index.
final class FilterChoiceAccessory: NSObject, FilterSheetAccessory {
    let view: NSView
    var onChange: (() -> Void)?
    private let popup = NSPopUpButton(frame: .zero, pullsDown: false)

    init(label: String, titles: [String], initial: Int) {
        popup.addItems(withTitles: titles)
        popup.selectItem(at: max(0, min(titles.count - 1, initial)))
        view = accessoryRow(label, popup)
        super.init()
        popup.target = self
        popup.action = #selector(changed(_:))
    }

    var values: [Double] { [Double(max(0, popup.indexOfSelectedItem))] }

    @objc private func changed(_ sender: Any?) { onChange?() }
}

/// An integer text field clamped to `range`, with a unit after it; its value
/// is the integer.
final class FilterIntegerFieldAccessory: NSObject, FilterSheetAccessory, NSTextFieldDelegate {
    let view: NSView
    var onChange: (() -> Void)?
    private let field = NSTextField(string: "")
    private let range: ClosedRange<Int>
    private var value: Int

    init(label: String, range: ClosedRange<Int>, initial: Int, unit: String) {
        self.range = range
        value = min(range.upperBound, max(range.lowerBound, initial))
        let formatter = NumberFormatter()
        formatter.numberStyle = .none
        formatter.allowsFloats = false
        formatter.minimum = NSNumber(value: range.lowerBound)
        formatter.maximum = NSNumber(value: range.upperBound)
        field.formatter = formatter
        field.integerValue = value
        field.widthAnchor.constraint(equalToConstant: 80).isActive = true
        DSField.style(field)
        let unitLabel = NSTextField(labelWithString: unit)
        unitLabel.font = DS.mono(12)
        unitLabel.textColor = DS.textMuted
        let row = NSStackView(views: [field, unitLabel])
        row.orientation = .horizontal
        row.spacing = 6
        view = accessoryRow(label, row)
        super.init()
        field.delegate = self
        field.target = self
        field.action = #selector(committed(_:))
    }

    var values: [Double] { [Double(value)] }

    /// Live: every edit that parses as an in-range integer re-previews.
    func controlTextDidChange(_ obj: Notification) { take(field.stringValue) }

    @objc private func committed(_ sender: Any?) {
        take(field.stringValue)
        field.integerValue = value
    }

    private func take(_ text: String) {
        guard let parsed = Int(text.trimmingCharacters(in: .whitespaces)) else { return }
        let clamped = min(range.upperBound, max(range.lowerBound, parsed))
        guard clamped != value else { return }
        value = clamped
        onChange?()
    }
}

/// Radial Blur's Blur Center: the canvas composite mapped so the FRAME (the
/// selection's bounds on the target, or the target's extent) fills the well,
/// and a click or drag places the centre. Values are `[x, y]` fractions of
/// the frame with a top-left origin, which is what the kernel reads. It shows
/// the image rather than a wireframe (a recorded decision).
final class RadialBlurCenterAccessory: FilterSheetAccessory {
    let view: NSView
    var onChange: (() -> Void)?
    private let well: CenterWell

    init(image: CGImage?, canvas: CGSize, frame: CGRect, initial: CGPoint = CGPoint(x: 0.5, y: 0.5)) {
        // The well keeps the frame's aspect inside 180 pt.
        let side: CGFloat = 180
        let aspect = frame.height > 0 ? frame.width / frame.height : 1
        let size = aspect >= 1 ? CGSize(width: side, height: max(24, side / aspect))
            : CGSize(width: max(24, side * aspect), height: side)
        // The part of the frame on the canvas, cropped from the composite and
        // drawn where it sits in the frame (a layer may overhang the canvas).
        var crop: NSImage?
        var placement = CGRect(x: 0, y: 0, width: 1, height: 1)
        let visible = frame.intersection(CGRect(origin: .zero, size: canvas))
        if let image, canvas.width > 0, canvas.height > 0, frame.width > 0, frame.height > 0,
           !visible.isNull, !visible.isEmpty {
            let sx = CGFloat(image.width) / canvas.width, sy = CGFloat(image.height) / canvas.height
            let rect = CGRect(x: visible.minX * sx, y: visible.minY * sy, width: visible.width * sx,
                              height: visible.height * sy).integral
            crop = image.cropping(to: rect).map { NSImage(cgImage: $0, size: .zero) }
            placement = CGRect(x: (visible.minX - frame.minX) / frame.width,
                               y: (visible.minY - frame.minY) / frame.height,
                               width: visible.width / frame.width, height: visible.height / frame.height)
        }
        well = CenterWell(image: crop, placement: placement,
                          center: CGPoint(x: min(1, max(0, initial.x)), y: min(1, max(0, initial.y))))
        well.translatesAutoresizingMaskIntoConstraints = false
        well.widthAnchor.constraint(equalToConstant: size.width).isActive = true
        well.heightAnchor.constraint(equalToConstant: size.height).isActive = true
        view = accessoryRow("Blur Center:", well)
        well.onMove = { [weak self] in self?.onChange?() }
    }

    var values: [Double] { [Double(well.center.x), Double(well.center.y)] }
}

/// The picker's drawing and mouse handling; flipped, so y grows downward as
/// the fractions do.
private final class CenterWell: NSView {
    private let image: NSImage?
    /// Where `image` sits in the well, in unit fractions of it.
    private let placement: CGRect
    var center: CGPoint
    var onMove: (() -> Void)?

    init(image: NSImage?, placement: CGRect, center: CGPoint) {
        self.image = image
        self.placement = placement
        self.center = center
        super.init(frame: .zero)
        toolTip = "Click or drag to place the blur centre"
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("CenterWell does not support NSCoder") }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        DS.canvasVoid.setFill()
        bounds.fill()
        let target = CGRect(x: bounds.minX + placement.minX * bounds.width,
                            y: bounds.minY + placement.minY * bounds.height,
                            width: placement.width * bounds.width, height: placement.height * bounds.height)
        image?.draw(in: target, from: .zero, operation: .sourceOver, fraction: 1,
                    respectFlipped: true, hints: nil)
        let point = CGPoint(x: bounds.minX + center.x * bounds.width, y: bounds.minY + center.y * bounds.height)
        let cross = NSBezierPath()
        cross.move(to: CGPoint(x: point.x - 8, y: point.y)); cross.line(to: CGPoint(x: point.x + 8, y: point.y))
        cross.move(to: CGPoint(x: point.x, y: point.y - 8)); cross.line(to: CGPoint(x: point.x, y: point.y + 8))
        cross.lineWidth = 3
        NSColor.black.withAlphaComponent(0.6).setStroke()
        cross.stroke()
        cross.lineWidth = 1
        NSColor.white.setStroke()
        cross.stroke()
        DS.border.setStroke()
        NSBezierPath(rect: bounds.insetBy(dx: 0.5, dy: 0.5)).stroke()
    }

    override func mouseDown(with event: NSEvent) { place(event) }
    override func mouseDragged(with event: NSEvent) { place(event) }

    private func place(_ event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        guard bounds.width > 0, bounds.height > 0 else { return }
        center = CGPoint(x: min(1, max(0, (p.x - bounds.minX) / bounds.width)),
                         y: min(1, max(0, (p.y - bounds.minY) / bounds.height)))
        needsDisplay = true
        onMove?()
    }
}
