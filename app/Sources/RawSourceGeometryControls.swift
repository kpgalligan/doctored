import AppKit

/// Explicit binary64 profile values. Text entry is authoritative; displayed
/// decimals are never parsed from a rounded slider label.
final class RawSourceGeometryControls: NSObject, NSTextFieldDelegate {
    let view = NSStackView()
    var onChange: (() -> Void)?
    private let original: RawLensGeometry?
    private var current: RawLensGeometry
    private var authored = false
    private var fields: [NSTextField] = []
    private var filterPopup: NSPopUpButton?

    init(geometry: RawLensGeometry?) {
        original = geometry
        current = geometry ?? RawLensGeometry(profileID: "manual", center: [0.5, 0.5],
            radial: [0, 0, 0], redScale: 1, blueScale: 1, filter: .bilinear,
            referenceSize: nil, referenceFromSource: nil)
        super.init()
        view.orientation = .vertical
        view.alignment = .leading
        view.spacing = 6
        let title = NSTextField(labelWithString: "Explicit lens geometry (native 16-bit)")
        title.font = DS.sans(12, weight: .semibold)
        view.addArrangedSubview(title)
        let basis = geometry?.referenceSize.map { "\($0[0])×\($0[1]) saved reference" }
            ?? "Current canonical source"
        let note = NSTextField(labelWithString: "Optical center basis: \(basis). An automatic Lensfun profile is Enable Profile Corrections.")
        note.font = DS.sans(11)
        note.textColor = DS.textMuted
        view.addArrangedSubview(note)
        for (index, pair) in zip(["Profile ID", "Center X", "Center Y", "k1", "k2", "k3",
                                   "Red scale", "Blue scale"],
                                  [current.profileID, String(current.center[0]), String(current.center[1]),
                                   String(current.radial[0]), String(current.radial[1]),
                                   String(current.radial[2]), String(current.redScale),
                                   String(current.blueScale)]).enumerated() {
            let row = NSStackView()
            row.orientation = .horizontal
            row.spacing = 8
            let caption = NSTextField(labelWithString: pair.0)
            caption.font = DS.sans(11)
            caption.widthAnchor.constraint(equalToConstant: 98).isActive = true
            let field = NSTextField(string: pair.1)
            field.font = DS.sans(11)
            field.setAccessibilityIdentifier("raw-lens-\(index)")
            field.widthAnchor.constraint(equalToConstant: 145).isActive = true
            field.tag = index
            field.target = self
            field.action = #selector(valueChanged(_:))
            field.delegate = self
            fields.append(field)
            row.addArrangedSubview(caption)
            row.addArrangedSubview(field)
            view.addArrangedSubview(row)
        }
        let filter = NSPopUpButton()
        filter.addItems(withTitles: ["Bilinear", "Bicubic"])
        filter.selectItem(at: current.filter == .bilinear ? 0 : 1)
        filter.setAccessibilityIdentifier("raw-lens-filter")
        filter.target = self
        filter.action = #selector(filterChanged(_:))
        filterPopup = filter
        view.addArrangedSubview(filter)
    }
    @available(*, unavailable) required init?(coder: NSCoder) {
        fatalError("RawSourceGeometryControls does not support NSCoder")
    }
    var settings: RawLensGeometry? { authored || current != original ? current : nil }
    func apply(_ geometry: RawLensGeometry?) {
        current = geometry ?? RawLensGeometry(profileID: "manual", center: [0.5, 0.5],
            radial: [0, 0, 0], redScale: 1, blueScale: 1, filter: .bilinear,
            referenceSize: nil, referenceFromSource: nil)
        authored = false
        updateFields()
    }
    func reset() { apply(original) }
    private func updateFields() {
        let values = [current.profileID, String(current.center[0]), String(current.center[1]),
            String(current.radial[0]), String(current.radial[1]), String(current.radial[2]),
            String(current.redScale), String(current.blueScale)]
        for (field, value) in zip(fields, values) { field.stringValue = value }
        filterPopup?.selectItem(at: current.filter == .bilinear ? 0 : 1)
    }
    func controlTextDidEndEditing(_ notification: Notification) {
        if let field = notification.object as? NSTextField { valueChanged(field) }
    }
    @objc private func valueChanged(_ sender: NSTextField) {
        var candidate = current
        if sender.tag == 0 { candidate.profileID = sender.stringValue }
        else {
            guard let value = Double(sender.stringValue), value.isFinite else {
                NSSound.beep(); updateFields(); return
            }
            switch sender.tag {
            case 1: candidate.center[0] = value
            case 2: candidate.center[1] = value
            case 3: candidate.radial[0] = value
            case 4: candidate.radial[1] = value
            case 5: candidate.radial[2] = value
            case 6: candidate.redScale = value
            case 7: candidate.blueScale = value
            default: return
            }
        }
        guard (try? candidate.validate()) != nil else { NSSound.beep(); updateFields(); return }
        guard candidate != current else { return }
        current = candidate
        authored = true
        onChange?()
    }
    @objc private func filterChanged(_ sender: NSPopUpButton) {
        current.filter = sender.indexOfSelectedItem == 1 ? .bicubic : .bilinear
        authored = true
        onChange?()
    }
}
