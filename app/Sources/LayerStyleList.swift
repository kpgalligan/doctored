import AppKit

// The Layer Style sheet's checklist pieces (stacked effect instances): one `StyleListRow` per
// row — moved here from `LayerStyleSheetController.swift` — whose repeating-kind rows carry
// a "+", and the `LayerStyleInstanceBar` (↑, ↓, trash) shown over the pane of a selected
// instance. Wiring only; the instance rules live on `LayerStyle` (`duplicateInstance`,
// `moveInstance`, `removeInstance`).

/// One checklist row: a checkbox (none for Blending Options), the title, and
/// for a repeating kind a "+" button, drawn by the row itself so a click
/// anywhere on the title selects it. Selected rows get the layers panel's
/// highlight (DS.selectionFill plus a 1 px DS.accent ring).
final class StyleListRow: NSView {
    private let checkbox: NSButton?
    private let addButton: NSButton?
    private let title: String

    var isSelected = false {
        didSet { needsDisplay = true }
    }

    var isChecked: Bool {
        get { checkbox?.state == .on }
        set { checkbox?.state = newValue ? .on : .off }
    }

    /// Whether "+" may add an instance (false at `LayerStyle.maxInstances`).
    var canAdd = true {
        didSet { addButton?.isEnabled = canAdd && isAvailable }
    }

    var onSelect: (() -> Void)?
    var onToggle: ((Bool) -> Void)?
    var onAdd: (() -> Void)?

    /// False for an effect the document cannot hold (a 32-bit float
    /// document's refused effect): the checkbox and "+" are disabled, the row
    /// dimmed, and a click beeps.
    var isAvailable = true {
        didSet {
            checkbox?.isEnabled = isAvailable
            addButton?.isEnabled = canAdd && isAvailable
            alphaValue = isAvailable ? 1 : 0.45
        }
    }

    init(title: String, hasCheckbox: Bool, hasAdd: Bool = false) {
        self.title = title
        self.checkbox = hasCheckbox ? NSButton(checkboxWithTitle: "", target: nil, action: nil) : nil
        self.addButton = hasAdd ? NSButton(title: "+", target: nil, action: nil) : nil
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        heightAnchor.constraint(equalToConstant: 24).isActive = true
        if let checkbox = checkbox {
            checkbox.translatesAutoresizingMaskIntoConstraints = false
            checkbox.target = self
            checkbox.action = #selector(toggled(_:))
            addSubview(checkbox)
            NSLayoutConstraint.activate([
                checkbox.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
                checkbox.centerYAnchor.constraint(equalTo: centerYAnchor),
            ])
        }
        if let addButton = addButton {
            addButton.translatesAutoresizingMaskIntoConstraints = false
            addButton.bezelStyle = .inline
            addButton.font = DS.sans(12, weight: .semibold)
            addButton.target = self
            addButton.action = #selector(added(_:))
            addButton.toolTip = "Add another instance of this effect"
            addButton.setAccessibilityLabel("Add \(title)")
            addSubview(addButton)
            NSLayoutConstraint.activate([
                addButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
                addButton.centerYAnchor.constraint(equalTo: centerYAnchor),
            ])
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("StyleListRow does not support NSCoder")
    }

    override func draw(_ dirtyRect: NSRect) {
        if isSelected {
            let path = NSBezierPath(
                roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5),
                xRadius: DS.listRowRadius, yRadius: DS.listRowRadius)
            DS.selectionFill.setFill()
            path.fill()
            path.lineWidth = 1
            DS.accent.setStroke()
            path.stroke()
        }
        let attributes: [NSAttributedString.Key: Any] = [
            .font: DS.sans(13, weight: isSelected ? .semibold : .regular),
            .foregroundColor: isSelected ? DS.accent : DS.textStrong,
        ]
        let size = title.size(withAttributes: attributes)
        let x: CGFloat = checkbox == nil ? 12 : 30
        title.draw(
            at: NSPoint(x: x, y: bounds.midY - size.height / 2), withAttributes: attributes)
    }

    override func mouseDown(with event: NSEvent) {
        guard isAvailable else {
            NSSound.beep()
            return
        }
        onSelect?()
    }

    @objc private func toggled(_ sender: Any?) {
        onToggle?(checkbox?.state == .on)
    }

    @objc private func added(_ sender: Any?) {
        guard canAdd, isAvailable else {
            NSSound.beep()
            return
        }
        onAdd?()
    }
}

/// The selected instance's reorder and delete controls: ↑ draws it one
/// place higher (later in render order), ↓ one lower, the trash removes it.
/// Hidden for a single kind and for a repeating kind with no instance.
final class LayerStyleInstanceBar: NSView {
    private let up = NSButton(title: "↑", target: nil, action: nil)
    private let down = NSButton(title: "↓", target: nil, action: nil)
    private let trash: NSButton

    var onUp: (() -> Void)?
    var onDown: (() -> Void)?
    var onRemove: (() -> Void)?

    init() {
        let image = NSImage(systemSymbolName: "trash", accessibilityDescription: "Delete Effect")
        trash = image.map { NSButton(image: $0, target: nil, action: nil) }
            ?? NSButton(title: "Delete", target: nil, action: nil)
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        let label = fieldLabel("Instance")
        up.toolTip = "Move this instance up (it draws above the one over it)"
        down.toolTip = "Move this instance down"
        trash.toolTip = "Delete this instance"
        for (button, action) in [
            (up, #selector(upClicked(_:))), (down, #selector(downClicked(_:))),
            (trash, #selector(trashClicked(_:))),
        ] {
            button.bezelStyle = .inline
            button.target = self
            button.action = action
        }
        let row = NSStackView(views: [label, up, down, trash])
        row.orientation = .horizontal
        row.spacing = 8
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.topAnchor.constraint(equalTo: topAnchor),
            row.bottomAnchor.constraint(equalTo: bottomAnchor),
            row.leadingAnchor.constraint(equalTo: leadingAnchor),
            row.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("LayerStyleInstanceBar does not support NSCoder")
    }

    /// Shows the bar for an instance `instance` of `count`, ↑ and ↓ enabled
    /// where there is room in that direction.
    func update(instance: Int, count: Int) {
        up.isEnabled = instance + 1 < count
        down.isEnabled = instance > 0
        trash.isEnabled = count > 0
    }

    @objc private func upClicked(_ sender: Any?) { onUp?() }
    @objc private func downClicked(_ sender: Any?) { onDown?() }
    @objc private func trashClicked(_ sender: Any?) { onRemove?() }
}
