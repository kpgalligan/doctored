import AppKit

// The save panel's accessory shows the complete report for the chosen format and mode and disables
// a mode that format cannot write; after the write, `presentExportNotice` states once what did not
// travel with the file.

/// Accessory view for the Photoshop export save panel: the format popup (PSD,
/// Large Document PSB, TIFF with layers), the mode popup and the complete
/// preflight for whichever pair is chosen, scrollable.
///
/// It decides layout and nothing about what a file can hold — every sentence
/// shown here comes from the core's report.
final class PSDExportAccessoryController: NSViewController {
    private let formatPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let modePopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 520, height: 220))
    private let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 520, height: 220))
    private let reports: [LayeredContainer: [PSDExportMode: PSDExportReport]]
    private var container: LayeredContainer
    private var mode: PSDExportMode

    /// Called with the chosen format whenever the Format popup changes, so
    /// the panel can keep its file type and extension in step.
    var onContainerChange: ((LayeredContainer) -> Void)?

    init(reports: [LayeredContainer: [PSDExportMode: PSDExportReport]]) {
        self.reports = reports
        // The first format that can write this document at all (PSD unless
        // the canvas or the file is too large for it), then Layers unless it
        // is unavailable. A mode that cannot be written is never preselected
        // and never selectable.
        let container = LayeredContainer.allCases.first { candidate in
            reports[candidate]?.values.contains { $0.available } == true
        } ?? .psd
        self.container = container
        self.mode = Self.preferredMode(in: reports[container])
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("PSDExportAccessoryController does not support NSCoder")
    }

    var selectedContainer: LayeredContainer { container }
    var selectedMode: PSDExportMode { mode }

    private static func preferredMode(in reports: [PSDExportMode: PSDExportReport]?) -> PSDExportMode {
        reports?[.layers]?.available == true ? .layers : .flattened
    }

    override func loadView() {
        formatPopup.addItems(withTitles: LayeredContainer.allCases.map { $0.title })
        formatPopup.font = DS.sans(13)
        formatPopup.target = self
        formatPopup.action = #selector(formatChanged(_:))
        modePopup.addItems(withTitles: PSDExportMode.allCases.map { $0.title })
        modePopup.font = DS.sans(13)
        modePopup.autoenablesItems = false
        modePopup.target = self
        modePopup.action = #selector(modeChanged(_:))

        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        scroll.translatesAutoresizingMaskIntoConstraints = false
        text.isEditable = false
        text.isSelectable = true
        text.font = DS.sans(12)
        text.textContainerInset = NSSize(width: 8, height: 8)
        text.isHorizontallyResizable = false
        text.isVerticallyResizable = true
        text.autoresizingMask = [.width]
        text.textContainer?.widthTracksTextView = true
        scroll.documentView = text

        let formatLabel = NSTextField(labelWithString: "Format:")
        formatLabel.font = DS.sans(13)
        let label = NSTextField(labelWithString: "Contents:")
        label.font = DS.sans(13)
        let row = NSStackView(views: [formatLabel, formatPopup, label, modePopup])
        row.orientation = .horizontal
        row.spacing = 8
        row.setCustomSpacing(18, after: formatPopup)
        row.translatesAutoresizingMaskIntoConstraints = false

        let container = NSView(frame: NSRect(x: 0, y: 0, width: 560, height: 300))
        container.addSubview(row)
        container.addSubview(scroll)
        NSLayoutConstraint.activate([
            row.topAnchor.constraint(equalTo: container.topAnchor, constant: 12),
            row.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 20),
            scroll.topAnchor.constraint(equalTo: row.bottomAnchor, constant: 10),
            scroll.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 20),
            scroll.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -20),
            scroll.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -12),
            scroll.heightAnchor.constraint(equalToConstant: 220),
        ])
        view = container
        syncControls()
    }

    private func syncControls() {
        if let index = LayeredContainer.allCases.firstIndex(of: container) {
            formatPopup.selectItem(at: index)
        }
        for (index, candidate) in PSDExportMode.allCases.enumerated() {
            modePopup.item(at: index)?.isEnabled = reports[container]?[candidate]?.available ?? false
        }
        if let index = PSDExportMode.allCases.firstIndex(of: mode) {
            modePopup.selectItem(at: index)
        }
        text.string = summary(for: reports[container]?[mode])
    }

    private func summary(for report: PSDExportReport?) -> String {
        guard let report else {
            return "This mode could not be inspected."
        }
        var lines: [String] = []
        if let refusal = report.refusal {
            lines += [refusal, ""]
        }
        for (title, items) in [
            ("Lost in this document", report.losses),
            ("Noted for this document", report.observations),
            ("General export limitations", report.generalLimitations),
        ] {
            lines += ["", title]
            lines += items.isEmpty ? ["Nothing found for this document."] : items.map { "• " + $0 }
        }
        return lines.joined(separator: "\n")
    }

    /// A format stays selectable even when it cannot write this document, so
    /// its refusal — which names the format that can — is readable here.
    @objc private func formatChanged(_ sender: Any?) {
        let all = LayeredContainer.allCases
        let index = formatPopup.indexOfSelectedItem
        guard index >= 0, index < all.count else {
            syncControls()
            return
        }
        container = all[index]
        if reports[container]?[mode]?.available != true {
            mode = Self.preferredMode(in: reports[container])
        }
        syncControls()
        onContainerChange?(container)
    }

    @objc private func modeChanged(_ sender: Any?) {
        let all = PSDExportMode.allCases
        let index = modePopup.indexOfSelectedItem
        guard index >= 0, index < all.count, reports[container]?[all[index]]?.available == true else {
            syncControls()
            return
        }
        mode = all[index]
        syncControls()
    }
}
