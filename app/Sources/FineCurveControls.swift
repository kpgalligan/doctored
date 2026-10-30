import AppKit

/// Curves retain their historical 0…255 Action coordinates. Only this control
/// converts normalized/native-code entry to that continuous coordinate space.
final class FineCurveControls: NSStackView {
    private weak var curve: CurveEditorView?
    private let points = NSPopUpButton(frame: .zero, pullsDown: false)
    private var entry: FineToneControls?

    init(curve: CurveEditorView, native: Bool) {
        self.curve = curve
        super.init(frame: .zero)
        orientation = .vertical; alignment = .leading; spacing = 5
        points.target = self; points.action = #selector(pointChanged(_:))
        points.setAccessibilityIdentifier("tone.curves.point")
        addArrangedSubview(points)
        entry = FineToneControls(id: "curves", native: native,
            parameters: [.init(key: "input", title: "Input", range: 0...1), .init(key: "output", title: "Output", range: 0...1)],
            read: { [weak curve] in
                guard let curve, !curve.points.isEmpty else { return [0, 0] }
                let point = curve.points[min(curve.selectedPointIndex, curve.points.count - 1)]
                return [point.x / 255, point.y / 255]
            }, write: { [weak curve] values in
                guard let curve, curve.points.indices.contains(curve.selectedPointIndex) else { return false }
                let index = curve.selectedPointIndex, x = values[0] * 255
                guard (index == 0 || x > curve.points[index - 1].x),
                      (index == curve.points.count - 1 || x < curve.points[index + 1].x) else { return false }
                curve.points[index] = CurvePoint(x: x, y: values[1] * 255)
                curve.onPointsChanged?(curve.points)
                return true
            })
        if let entry { addArrangedSubview(entry) }
        refresh()
    }
    @available(*, unavailable) required init?(coder: NSCoder) { fatalError("FineCurveControls does not support NSCoder") }

    func accept() -> Bool { entry?.accept() ?? true }

    func refresh() {
        guard let curve else { return }
        points.removeAllItems()
        points.addItems(withTitles: curve.points.indices.map { "Point \($0 + 1)" })
        curve.selectedPointIndex = min(curve.selectedPointIndex, max(0, curve.points.count - 1))
        points.selectItem(at: curve.selectedPointIndex)
        entry?.refresh()
    }
    @objc private func pointChanged(_ sender: Any?) {
        curve?.selectedPointIndex = max(0, points.indexOfSelectedItem)
        entry?.refresh()
    }
}
