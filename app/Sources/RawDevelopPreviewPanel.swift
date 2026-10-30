import AppKit

/// Shared native detail, comparison and full-image histogram panel for Develop
/// and explicit decoder migration. Pixel work stays in the native source APIs.
final class RawDevelopPreviewPanel: NSView {
    let canvas = RawDevelopCanvasView()
    private let histogram = HistogramView()
    private let zoom = NSSegmentedControl(labels: ["Fit", "100%"], trackingMode: .selectOne,
        target: nil, action: nil)
    private let comparison = NSSegmentedControl(labels: ["After", "Before", "Split"], trackingMode: .selectOne,
        target: nil, action: nil)
    private let clipping = NSButton(checkboxWithTitle: "Clipping", target: nil, action: nil)
    private let horizontal = NSSlider(value: 0.5, minValue: 0, maxValue: 1, target: nil, action: nil)
    private let vertical = NSSlider(value: 0.5, minValue: 0, maxValue: 1, target: nil, action: nil)
    private let location = NSTextField(labelWithString: "Fit — entire image")
    private let histogramTitle = NSTextField(labelWithString: "After · whole-image RGB histogram")
    private let endpoints = NSTextField(wrappingLabelWithString: "Exact endpoints appear after native development.")
    private let status = NSTextField(wrappingLabelWithString: "Preparing native preview…")
    private var previewFrame: RawDevelopPreviewFrame?
    var onChange: (() -> Void)?
    private(set) var viewport = RawDevelopViewport()

    init(width: CGFloat, height: CGFloat) {
        super.init(frame: .zero)
        widthAnchor.constraint(equalToConstant: width).isActive = true
        heightAnchor.constraint(equalToConstant: height).isActive = true
        zoom.selectedSegment = 0
        comparison.selectedSegment = 0
        for control in [zoom, comparison] {
            control.target = self
            control.action = #selector(controlChanged(_:))
        }
        zoom.setAccessibilityIdentifier("raw-develop-zoom")
        comparison.setAccessibilityIdentifier("raw-develop-comparison")
        clipping.setAccessibilityIdentifier("raw-develop-clipping")
        clipping.target = self
        clipping.action = #selector(controlChanged(_:))
        clipping.toolTip = "Blue: any RGB component is 0 (at or below 0.0 in a 32-bit float development). "
            + "Red: any RGB component is 65535 (at or above white, 1.0, in a 32-bit float development). "
            + "Magenta: both. Uses all native pixels."
        for (index, slider) in [horizontal, vertical].enumerated() {
            slider.isContinuous = true
            slider.target = self
            slider.action = #selector(locationChanged(_:))
            slider.isEnabled = false
            slider.setAccessibilityIdentifier(index == 0 ? "raw-develop-center-x" : "raw-develop-center-y")
            slider.setAccessibilityLabel(index == 0 ? "Detail horizontal position" : "Detail vertical position")
        }
        let toolbar = NSStackView(views: [zoom, comparison, clipping])
        toolbar.orientation = .horizontal
        toolbar.spacing = 8
        toolbar.alignment = .centerY
        let navigation = NSStackView(views: [fieldLabel("X"), horizontal, fieldLabel("Y"), vertical])
        navigation.orientation = .horizontal
        navigation.spacing = 6
        horizontal.widthAnchor.constraint(equalToConstant: max(80, (width - 52) / 2)).isActive = true
        vertical.widthAnchor.constraint(equalTo: horizontal.widthAnchor).isActive = true
        histogram.display = .rgb
        histogram.setAccessibilityIdentifier("raw-develop-histogram")
        histogram.widthAnchor.constraint(equalToConstant: width).isActive = true
        histogram.heightAnchor.constraint(equalToConstant: 66).isActive = true
        canvas.widthAnchor.constraint(equalToConstant: width).isActive = true
        canvas.heightAnchor.constraint(greaterThanOrEqualToConstant: 140).isActive = true
        for label in [location, histogramTitle, endpoints, status] {
            label.font = DS.mono(10)
            label.textColor = DS.textMuted
        }
        endpoints.preferredMaxLayoutWidth = width
        status.preferredMaxLayoutWidth = width
        status.setAccessibilityIdentifier("raw-develop-status")
        location.setAccessibilityIdentifier("raw-develop-location")
        endpoints.setAccessibilityIdentifier("raw-develop-endpoints")
        let stack = NSStackView(views: [toolbar, canvas, navigation, location, histogramTitle, histogram, endpoints, status])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 7
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([stack.topAnchor.constraint(equalTo: topAnchor),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor), stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor)])
        canvas.onPan = { [weak self] delta in
            guard let self else { return }
            self.viewport.center = CGPoint(x: min(1, max(0, self.viewport.center.x + delta.x)),
                y: min(1, max(0, self.viewport.center.y + delta.y)))
            self.horizontal.doubleValue = self.viewport.center.x
            self.vertical.doubleValue = self.viewport.center.y
            self.requestChange()
        }
        canvas.onViewportChange = { [weak self] in self?.requestChange() }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("RawDevelopPreviewPanel does not support NSCoder") }

    func currentViewport() -> RawDevelopViewport {
        var viewport = self.viewport
        viewport.backingSize = canvas.backingSize
        viewport.extendedRange = screenHasHeadroom
        return viewport
    }

    /// The window's screen can show more than SDR white
    /// (`DR_EDR_HEADROOM` overrides it in a test instance).
    private var screenHasHeadroom: Bool {
        EDRDisplay.potentialHeadroom(of: canvas.window?.screen ?? window?.screen) > 1
    }

    func setStatus(_ text: String, ready: Bool) {
        status.stringValue = text
        if !ready {
            // An obsolete result is never presented under new slider values.
            previewFrame = nil
            canvas.preview = nil
            histogram.bins = nil
            endpoints.stringValue = ""
        }
    }

    func present(_ frame: RawDevelopPreviewFrame) {
        self.previewFrame = frame
        canvas.preview = frame
        updateReadout()
    }

    func showAfter() {
        comparison.selectedSegment = RawDevelopCanvasView.Comparison.after.rawValue
        canvas.comparison = .after
        updateReadout()
    }

    func clear() {
        onChange = nil
        previewFrame = nil
        canvas.preview = nil
        canvas.applyExtendedRange(false)
        canvas.onPan = nil
        canvas.onViewportChange = nil
        histogram.bins = nil
        status.stringValue = ""
        endpoints.stringValue = ""
    }

    private func requestChange() {
        viewport.backingSize = canvas.backingSize
        viewport.extendedRange = screenHasHeadroom
        onChange?()
    }

    @objc private func controlChanged(_ sender: Any?) {
        canvas.comparison = RawDevelopCanvasView.Comparison(rawValue: comparison.selectedSegment) ?? .after
        let detail = zoom.selectedSegment == 1
        let clip: UInt32 = clipping.state == .on ? 3 : 0
        if detail != viewport.nativeDetail || clip != viewport.clipping {
            viewport.nativeDetail = detail
            viewport.clipping = clip
            horizontal.isEnabled = detail
            vertical.isEnabled = detail
            requestChange()
        } else { updateReadout() }
    }

    @objc private func locationChanged(_ sender: Any?) {
        viewport.center = CGPoint(x: horizontal.doubleValue, y: vertical.doubleValue)
        requestChange()
    }

    private func updateReadout() {
        guard let frame = previewFrame else { return }
        let before = comparison.selectedSegment == RawDevelopCanvasView.Comparison.before.rawValue
        let source = before ? frame.before : frame.after
        histogram.bins = source.bins
        let name = before ? "Before" : "After"
        let float = source.extendedMaster != nil
        histogramTitle.stringValue = "\(name) · entire \(source.image.width) × \(source.image.height) image · RGB"
            + (float ? " · stops −12…+6 EV" : "")
        func counts(_ keys: [String]) -> String {
            let rows = keys.compactMap { source.histogram[$0] as? [NSNumber] }
            guard rows.count == keys.count, rows.allSatisfy({ $0.count >= 3 }) else { return "—" }
            return (0..<3).map { c in String(rows.reduce(UInt64(0)) { $0 + $1[c].uint64Value }) }
                .joined(separator: " / ")
        }
        if float {
            // #680: a float side's endpoints are the float histogram's
            // inclusive counts, the clipping markers' rule.
            endpoints.stringValue = "RGB endpoints — ≤ 0: \(counts(["exact_zero_rgba", "below_zero_rgba"]))"
                + " · ≥ 1.0 (at or above white): \(counts(["exact_one_rgba", "above_white_rgba"]))"
        } else {
            endpoints.stringValue = "Exact RGB endpoints — 0: \(counts(["exact_zero_rgba"]))"
                + " · 65535: \(counts(["exact_max_rgba"]))"
        }
        let r = frame.detailRegion ?? frame.viewport.region(source.image)
        // An f32 side says whether it shows values above white (#680).
        let range: String
        if float {
            let peak = before ? frame.beforeExtendedPeak : frame.afterExtendedPeak
            range = peak.map { String(format: " · HDR, peak %.2f", $0) } ?? " · clipped to SDR"
        } else { range = "" }
        location.stringValue = frame.viewport.nativeDetail
            ? "100% · x \(r.x), y \(r.y) · \(r.width) × \(r.height) native pixels · drag to pan\(range)"
            : "Fit · entire image · histogram counts every native pixel\(range)"
    }
}
