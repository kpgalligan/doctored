import AppKit

/// The Tone rows of the Local Masks sidebar: one slider per `RawLocalTone` control.
/// `RawLocalControls` hosts the view, puts `toneJSON` in every update it sends, and fills the rows
/// from the selected local. These controls develop no camera variant (`RawLocalTone.swift`).
final class RawLocalToneControls: NSObject {
    let view = NSStackView()
    var onChange: (() -> Void)?
    private var rows: [String: CardControlRow] = [:]

    private static let labels: [String: (title: String, format: String)] = [
        "exposure": ("Exposure EV:", "%.2f"), "contrast": ("Contrast:", "%.0f"),
        "highlights": ("Highlights:", "%.0f"), "shadows": ("Shadows:", "%.0f"),
        "whites": ("Whites:", "%.0f"), "blacks": ("Blacks:", "%.0f"),
        "saturation": ("Saturation:", "%.0f"), "clarity": ("Clarity:", "%.0f"),
        "texture": ("Texture:", "%.0f"), "sharpness": ("Sharpness:", "%.0f"),
        "noise_reduction": ("Noise Reduction:", "%.0f")]

    override init() {
        super.init()
        view.orientation = .vertical; view.alignment = .leading; view.spacing = 10
        let note = NSTextField(wrappingLabelWithString:
            "Tone is applied to the global development inside the mask; Sharpness and Noise Reduction act last, at a fixed pixel radius. Camera exposure and white balance above develop the RAW again.")
        note.font = DS.sans(11); note.textColor = DS.textMuted
        note.preferredMaxLayoutWidth = 370
        note.widthAnchor.constraint(equalToConstant: 370).isActive = true
        view.addArrangedSubview(note)
        for (key, range) in RawLocalTone.fields {
            let label = Self.labels[key] ?? (key, "%.2f")
            let row = cardSliderRow(label: label.title, min: range.lowerBound, max: range.upperBound,
                value: 0, format: { String(format: label.format, $0) }) { [weak self] _ in self?.onChange?() }
            row.setAccessibilityIdentifier("raw-local-tone-" + key)
            rows[key] = row
            view.addArrangedSubview(row)
        }
    }

    /// Shows one local's tone (nil: every control at 0). Silent: no change is reported.
    func fill(_ tone: RawLocalTone?) {
        let tone = tone ?? RawLocalTone()
        for (key, _) in RawLocalTone.fields { rows[key]?.setValue(tone[key]) }
    }

    func setEnabled(_ enabled: Bool) {
        for row in rows.values { row.isEnabled = enabled }
    }

    /// The update's `tone` value: the tone object, or NSNull when every control is at 0.
    var toneJSON: Any {
        var tone = RawLocalTone()
        for (key, range) in RawLocalTone.fields {
            let value = rows[key]?.slider?.doubleValue ?? 0
            tone[key] = min(max(value, range.lowerBound), range.upperBound)
        }
        return tone.isNeutral ? NSNull() : tone.json
    }
}
