import AppKit

/// How full the assistant conversation's context is: tokens in play after
/// the last response, against the model's context window, both reported
/// by the helper (`context_usage`).
struct AssistantContextUsage {
    var used = 0
    /// The model's context window; nil until the helper has looked it up,
    /// or when the Models API did not say.
    var limit: Int?
    /// False until the helper has reported at all.
    var reported = false
}

/// The Assistant panel's context panel content: a bar from 0 to the context
/// window with the used part filled, and the numbers under it. Shown in an
/// `OverflowPanel` hung from the panel's context button; `update` refreshes
/// it live while it is open.
final class AssistantContextMeterView: NSView {
    private let bar = ContextBar()
    private let figures = NSTextField(labelWithString: "")

    init(usage: AssistantContextUsage) {
        super.init(frame: NSRect(x: 0, y: 0, width: 260, height: 78))
        let title = NSTextField(labelWithString: "")
        title.attributedStringValue = DS.microLabel("Context")
        figures.font = DS.mono(11)
        figures.textColor = DS.textMuted
        figures.lineBreakMode = .byTruncatingTail

        for view in [title, bar, figures] as [NSView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        NSLayoutConstraint.activate([
            title.topAnchor.constraint(equalTo: topAnchor, constant: 12),
            title.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),

            bar.topAnchor.constraint(equalTo: title.bottomAnchor, constant: 8),
            bar.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            bar.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            bar.heightAnchor.constraint(equalToConstant: 8),

            figures.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 8),
            figures.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            figures.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
        ])
        update(usage)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("AssistantContextMeterView does not support NSCoder")
    }

    func update(_ usage: AssistantContextUsage) {
        let used = usage.used.formatted()
        switch (usage.reported, usage.limit) {
        case (false, _):
            bar.fraction = 0
            figures.stringValue = "Nothing reported yet"
        case (true, let limit?) where limit > 0:
            let fraction = min(1, Double(usage.used) / Double(limit))
            bar.fraction = fraction
            let percent = (fraction * 100).formatted(.number.precision(.fractionLength(0...1)))
            figures.stringValue = "\(used) of \(limit.formatted()) tokens · \(percent)%"
        case (true, _):
            bar.fraction = 0
            figures.stringValue = "\(used) tokens (window size unknown)"
        }
    }
}

/// A square-cornered track with the used fraction filled in the accent;
/// the fill turns to the warning colour past 80%.
private final class ContextBar: NSView {
    var fraction: Double = 0 {
        didSet { needsDisplay = true }
    }

    override func draw(_ dirtyRect: NSRect) {
        DS.hoverFill.setFill()
        bounds.fill()
        guard fraction > 0 else { return }
        (fraction > 0.8 ? DS.warningText : DS.accent).setFill()
        // At least a sliver, so a small conversation still shows as used.
        let width = max(2, bounds.width * CGFloat(fraction))
        NSRect(x: 0, y: 0, width: width, height: bounds.height).fill()
    }
}
