import AppKit

/// The Info panel's Color Samplers section: a header with the readout
/// mode popup, then one row per sampler — "#1 3×3  120, 64, 12", followed by
/// "→ 135, 70, 15" while a tone sheet previews. Hidden while the list is
/// empty. The popup persists `ViewToolOptions.colorSamplerReadoutIndex`; the
/// panel supplies the readings (`InfoPanelViewController.refreshColorSamplers`).
final class ColorSamplerReadoutView: NSView {
    private let modePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let rows = NSStackView()
    private var samplers = ColorSamplerSet()
    private var readings: [ColorSamplerReading] = []
    private var after: [ColorSamplerReading]?

    private var mode: ColorSamplerReadoutMode {
        ColorSamplerReadoutMode(rawValue: ToolOptionsStore.shared.view.colorSamplerReadoutIndex)
            ?? .rgb
    }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        translatesAutoresizingMaskIntoConstraints = false

        let caption = NSTextField(labelWithString: "")
        caption.attributedStringValue = DS.microLabel("Color Samplers")

        let menu = NSMenu()
        for item in ColorSamplerReadoutMode.allCases {
            let menuItem = NSMenuItem(title: item.title, action: nil, keyEquivalent: "")
            menuItem.tag = item.rawValue
            menu.addItem(menuItem)
        }
        modePopup.menu = menu
        modePopup.selectItem(withTag: mode.rawValue)
        modePopup.controlSize = .small
        modePopup.font = DS.sans(11)
        modePopup.target = self
        modePopup.action = #selector(modeChanged(_:))
        modePopup.toolTip = "How the sampler rows print their values"
        modePopup.setAccessibilityIdentifier("info.colorSamplers.mode")

        let header = NSStackView(views: [caption, modePopup])
        header.orientation = .horizontal
        header.spacing = 8

        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 4
        rows.setAccessibilityIdentifier("info.colorSamplers.rows")

        let column = NSStackView(views: [header, rows])
        column.translatesAutoresizingMaskIntoConstraints = false
        column.orientation = .vertical
        column.alignment = .leading
        column.spacing = 6
        addSubview(column)
        NSLayoutConstraint.activate([
            column.topAnchor.constraint(equalTo: topAnchor),
            column.bottomAnchor.constraint(equalTo: bottomAnchor),
            column.leadingAnchor.constraint(equalTo: leadingAnchor),
            column.trailingAnchor.constraint(equalTo: trailingAnchor),
            rows.widthAnchor.constraint(equalTo: column.widthAnchor),
        ])
        isHidden = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("ColorSamplerReadoutView does not support NSCoder")
    }

    /// The committed readings and, while a sheet previews, the after ones;
    /// an `after` whose count does not match the list is dropped.
    func update(samplers: ColorSamplerSet, readings: [ColorSamplerReading],
                after: [ColorSamplerReading]?) {
        self.samplers = samplers
        self.readings = readings
        self.after = after?.count == samplers.items.count ? after : nil
        render()
    }

    @objc private func modeChanged(_ sender: NSPopUpButton) {
        guard let tag = sender.selectedItem?.tag,
              ColorSamplerReadoutMode(rawValue: tag) != nil else { return }
        ToolOptionsStore.shared.view.colorSamplerReadoutIndex = tag
        render()
        // App-wide, so every other window's Info section re-renders too.
        NotificationCenter.default.post(name: .editorViewChromeDidChange, object: nil)
    }

    private func render() {
        isHidden = samplers.items.isEmpty
        modePopup.selectItem(withTag: mode.rawValue)
        let mode = self.mode
        let lines = samplers.items.enumerated().map { index, sampler -> String in
            let value = readings.indices.contains(index) ? readings[index].text(mode) : "—"
            var line = "#\(index + 1) \(sampler.sampleSize)×\(sampler.sampleSize)  \(value)"
            if let after { line += " → \(after[index].text(mode))" }
            return line
        }
        var labels = rows.arrangedSubviews.compactMap { $0 as? NSTextField }
        while labels.count < lines.count {
            let label = NSTextField(labelWithString: "")
            label.font = DS.mono(11)
            label.textColor = DS.textStrong
            label.lineBreakMode = .byTruncatingTail
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            label.setAccessibilityIdentifier("info.colorSamplers.row\(labels.count + 1)")
            rows.addArrangedSubview(label)
            label.widthAnchor.constraint(lessThanOrEqualTo: rows.widthAnchor).isActive = true
            labels.append(label)
        }
        while labels.count > lines.count, let last = labels.popLast() {
            rows.removeArrangedSubview(last)
            last.removeFromSuperview()
        }
        for (label, line) in zip(labels, lines) where label.stringValue != line {
            label.stringValue = line
        }
    }
}
