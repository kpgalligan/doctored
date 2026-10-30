import AppKit

/// Accessory view for File ▸ Export as PDF…'s save panel: the
/// page image's depth (typed documents only), its compression and a
/// JPEG-only quality slider. It decides layout and the two couplings JPEG
/// forces — 16 bits means ZIP, JPEG means 8 bits — and nothing else; the
/// options are validated by `PDFExportOptions.validate()` and nothing is
/// remembered between exports.
final class PDFExportAccessoryController: NSViewController {
    private let depthPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let compressionPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let qualitySlider = NSSlider(
        value: 90, minValue: 10, maxValue: 100, target: nil, action: nil)
    private let qualityLabel = NSTextField(labelWithString: "Quality:")
    private let qualityValueLabel = NSTextField(labelWithString: "90")
    private let offersDepth: Bool
    private var current: PDFExportOptions

    /// What the panel's controls say now.
    var options: PDFExportOptions { current }

    init(defaults: PDFExportOptions, offersDepth: Bool) {
        current = defaults
        current.jpegQuality = min(max(defaults.jpegQuality, 10), 100)
        self.offersDepth = offersDepth
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PDFExportAccessoryController does not support NSCoder")
    }

    override func loadView() {
        depthPopup.addItems(withTitles: ["8-bit", "16-bit"])
        compressionPopup.addItems(withTitles: ["ZIP", "JPEG"])
        for popup in [depthPopup, compressionPopup] {
            popup.font = DS.sans(13)
            popup.target = self
        }
        depthPopup.action = #selector(depthChanged(_:))
        compressionPopup.action = #selector(compressionChanged(_:))

        qualitySlider.isContinuous = true
        qualitySlider.target = self
        qualitySlider.action = #selector(qualityChanged(_:))
        qualitySlider.widthAnchor.constraint(equalToConstant: 150).isActive = true

        qualityLabel.font = DS.sans(13)
        qualityValueLabel.font = DS.mono(12)
        qualityValueLabel.alignment = .right
        qualityValueLabel.widthAnchor.constraint(equalToConstant: 32).isActive = true

        let depthLabel = NSTextField(labelWithString: "Bits/Channel:")
        let compressionLabel = NSTextField(labelWithString: "Compression:")
        for label in [depthLabel, compressionLabel] {
            label.font = DS.sans(13)
        }
        var rows: [[NSView]] = []
        if offersDepth {
            rows.append([depthLabel, depthPopup, NSGridCell.emptyContentView])
        }
        rows.append([compressionLabel, compressionPopup, NSGridCell.emptyContentView])
        rows.append([qualityLabel, qualitySlider, qualityValueLabel])
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.translatesAutoresizingMaskIntoConstraints = false

        let container = NSView(
            frame: NSRect(x: 0, y: 0, width: 420, height: offersDepth ? 116 : 84))
        container.addSubview(grid)
        NSLayoutConstraint.activate([
            grid.topAnchor.constraint(equalTo: container.topAnchor, constant: 12),
            grid.centerXAnchor.constraint(equalTo: container.centerXAnchor),
            grid.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -12),
        ])
        view = container
        syncControls()
    }

    private func syncControls() {
        depthPopup.selectItem(at: current.bitDepth == 16 ? 1 : 0)
        compressionPopup.selectItem(at: current.compression == .jpeg ? 1 : 0)
        qualitySlider.integerValue = current.jpegQuality
        qualityValueLabel.stringValue = "\(current.jpegQuality)"
        let jpeg = current.compression == .jpeg
        qualitySlider.isEnabled = jpeg
        qualityLabel.textColor = jpeg ? .labelColor : .disabledControlTextColor
        qualityValueLabel.textColor = jpeg ? .labelColor : .disabledControlTextColor
    }

    /// Picking 16 bits picks ZIP: JPEG is 8-bit only.
    @objc private func depthChanged(_ sender: Any?) {
        current.bitDepth = depthPopup.indexOfSelectedItem == 1 ? 16 : 8
        if current.bitDepth == 16 { current.compression = .zip }
        syncControls()
    }

    /// Picking JPEG picks 8 bits.
    @objc private func compressionChanged(_ sender: Any?) {
        current.compression = compressionPopup.indexOfSelectedItem == 1 ? .jpeg : .zip
        if current.compression == .jpeg { current.bitDepth = 8 }
        syncControls()
    }

    @objc private func qualityChanged(_ sender: Any?) {
        current.jpegQuality = qualitySlider.integerValue
        qualityValueLabel.stringValue = "\(current.jpegQuality)"
    }
}
