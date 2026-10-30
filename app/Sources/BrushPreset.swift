import Foundation

/// The paint tools' built-in tip presets: named combinations of the
/// options bar's tip fields (hardness, flow, spacing, angle, roundness,
/// airbrush). Selecting one applies its values to the active tool's
/// options; size and opacity are deliberately untouched — a preset shapes
/// the tip, not the hand. The popup's first row is "Custom", shown
/// whenever the current values match no preset; picking it changes
/// nothing.
struct BrushPreset {
    let name: String
    let hardness: Double
    let flow: Double
    let spacing: Double
    let angle: Double
    let roundness: Double
    let airbrush: Bool

    static let builtIns: [BrushPreset] = [
        BrushPreset(
            name: "Hard Round", hardness: 100, flow: 100, spacing: 25,
            angle: 0, roundness: 100, airbrush: false),
        BrushPreset(
            name: "Soft Round", hardness: 0, flow: 100, spacing: 25,
            angle: 0, roundness: 100, airbrush: false),
        BrushPreset(
            name: "Airbrush", hardness: 0, flow: 20, spacing: 15,
            angle: 0, roundness: 100, airbrush: true),
        BrushPreset(
            name: "Calligraphy", hardness: 95, flow: 100, spacing: 15,
            angle: -45, roundness: 30, airbrush: false),
        BrushPreset(
            name: "Flat Shader", hardness: 80, flow: 100, spacing: 20,
            angle: 90, roundness: 15, airbrush: false),
        BrushPreset(
            name: "Stippled", hardness: 100, flow: 100, spacing: 150,
            angle: 0, roundness: 100, airbrush: false),
    ]

    /// The popup's rows: "Custom" first, then every built-in.
    static let popupItems = ["Custom"] + builtIns.map { $0.name }

    /// The popup row the options currently show: a matching preset's row,
    /// or 0 (Custom). Values compare with a half-unit tolerance so a
    /// preset survives the store's Double round-trip.
    static func matchIndex(of options: PaintToolOptions) -> Int {
        for (i, preset) in builtIns.enumerated() where preset.matches(options) {
            return i + 1
        }
        return 0
    }

    private func matches(_ options: PaintToolOptions) -> Bool {
        abs(options.hardness - hardness) < 0.5
            && abs(options.flow - flow) < 0.5
            && abs(options.spacing - spacing) < 0.5
            && abs(options.angle - angle) < 0.5
            && abs(options.roundness - roundness) < 0.5
            && options.airbrush == airbrush
    }

    /// A preset-step key: , (previous) and . (next) wrap through the
    /// built-ins; Shift+, (first) and Shift+. (last) jump to either end.
    enum Step { case previous, next, first, last }

    /// The rung spacing of the { / } hardness ladder (0, 25, 50, 75, 100).
    static let hardnessKeyStep: Double = 25

    /// The hardness one { (down) or } (up) press lands on: the next ladder
    /// rung strictly in the pressed direction from the rounded, clamped
    /// input, pinned at 0 and 100.
    static func steppedHardness(_ hardness: Double, up: Bool) -> Double {
        let current = min(max(hardness.rounded(), 0), 100)
        let rungs = stride(from: 0, through: 100, by: hardnessKeyStep).map { $0 }
        if up { return rungs.first { $0 > current } ?? 100 }
        return rungs.last { $0 < current } ?? 0
    }

    /// The preset one step lands on from the options' current popup row.
    /// From Custom, next is the first built-in and previous the last.
    static func stepped(from options: PaintToolOptions, _ step: Step) -> BrushPreset {
        let presets = builtIns
        let count = presets.count
        let row = matchIndex(of: options)
        switch step {
        case .first:
            return presets[0]
        case .last:
            return presets[count - 1]
        case .next:
            return row == 0 ? presets[0] : presets[row % count]
        case .previous:
            return row == 0 ? presets[count - 1] : presets[(row - 2 + count) % count]
        }
    }

    /// The options with this preset's tip applied (size, opacity and the
    /// rest untouched).
    func applied(to options: PaintToolOptions) -> PaintToolOptions {
        var out = options
        out.hardness = hardness
        out.flow = flow
        out.spacing = spacing
        out.angle = angle
        out.roundness = roundness
        out.airbrush = airbrush
        return out
    }
}
