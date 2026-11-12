import Foundation

/// The paint tools' tip presets: named combinations of the options bar's tip
/// fields (hardness, flow, spacing, angle, roundness, airbrush) — the
/// built-ins below, then the user's own (`BrushPresetLibrary`). Selecting one
/// applies its values to the active tool's options; size and opacity are
/// deliberately untouched — a preset shapes the tip, not the hand. The
/// popup's first row is "Custom", shown whenever the current values match no
/// preset; picking it changes nothing. A preset is identified on the popup
/// by its VALUES (`matchIndex`), so no two presets may hold the same six
/// values: the library refuses a save or an import that would repeat one.
struct BrushPreset {
    static let schemaVersion = 1

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

    /// Every preset the popup lists: the built-ins, then the user's library
    /// (`BrushPresetLibrary.presets`, cached; the built-ins alone when it
    /// cannot be read).
    static func all() -> [BrushPreset] {
        builtIns + BrushPresetLibrary.presets
    }

    /// The popup's preset rows: "Custom" first, then `all()`.
    static var popupItems: [String] { ["Custom"] + all().map { $0.name } }

    /// The popup row the options currently show: a matching preset's row,
    /// or 0 (Custom). Values compare with a half-unit tolerance so a
    /// preset survives the store's Double round-trip.
    static func matchIndex(of options: PaintToolOptions, in presets: [BrushPreset]? = nil) -> Int {
        for (i, preset) in (presets ?? all()).enumerated() where preset.matches(options) {
            return i + 1
        }
        return 0
    }

    /// The six values against `options`', each to within half a unit.
    private func matches(_ options: PaintToolOptions) -> Bool {
        abs(options.hardness - hardness) < 0.5
            && abs(options.flow - flow) < 0.5
            && abs(options.spacing - spacing) < 0.5
            && abs(options.angle - angle) < 0.5
            && abs(options.roundness - roundness) < 0.5
            && options.airbrush == airbrush
    }

    /// Whether `other` holds this preset's six values (`matchIndex`'s rule),
    /// which would leave one of the two unreachable on the popup.
    func repeats(_ other: BrushPreset) -> Bool {
        matches(other.applied(to: PaintToolOptions()))
    }

    /// A preset-step key: , (previous) and . (next) wrap through `all()`,
    /// built-in then the user's; Shift+, (first) and Shift+. (last) jump to
    /// either end.
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

    /// The preset one step lands on from the options' current popup row,
    /// through the built-ins and then the user's presets. From Custom, next
    /// is the first preset and previous the last.
    static func stepped(from options: PaintToolOptions, _ step: Step) -> BrushPreset {
        let presets = all()
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

    /// The six values a preset holds, in the options bar's (and
    /// `brush_stroke`'s) units and ranges, in the order `values` and the
    /// file schema list them.
    static let fieldNames = ["hardness", "flow", "spacing", "angle", "roundness", "airbrush"]
    private static let ranges: [(String, ClosedRange<Double>)] = [
        ("hardness", 0...100), ("flow", 1...100), ("spacing", 1...200), ("angle", -180...180),
        ("roundness", 1...100),
    ]

    /// A user preset: the name normalized and no built-in's, each value finite
    /// and in its range (the options bar's own field bounds).
    static func validated(name: String, values: [String: Any]) throws -> BrushPreset {
        let normalized = try PreferenceLibrary<BrushPresetCodec>.normalizedName(name)
        let key = PreferenceLibrary<BrushPresetCodec>.key
        guard !builtIns.contains(where: { key($0.name) == key(normalized) }) else {
            throw DoctoredCoreError(message: "“\(normalized)” is a built-in brush preset; choose another name.")
        }
        var numbers: [String: Double] = [:]
        for (field, range) in ranges {
            guard let number = NativeBrushStroke.jsonNumber(values[field]),
                  number.isFinite, range.contains(number) else {
                throw DoctoredCoreError(message: "A brush preset's \(field) must be a number from "
                    + "\(Int(range.lowerBound)) to \(Int(range.upperBound)).")
            }
            numbers[field] = number
        }
        guard let airbrush = NativeBrushStroke.jsonBoolean(values["airbrush"]) else {
            throw DoctoredCoreError(message: "A brush preset's airbrush must be true or false.")
        }
        return BrushPreset(name: normalized, hardness: numbers["hardness"] ?? 100,
            flow: numbers["flow"] ?? 100, spacing: numbers["spacing"] ?? 25,
            angle: numbers["angle"] ?? 0, roundness: numbers["roundness"] ?? 100, airbrush: airbrush)
    }

    /// The preset's library file, and one entry of a preset file.
    var json: [String: Any] {
        ["schema_version": Self.schemaVersion, "name": name, "hardness": hardness, "flow": flow,
         "spacing": spacing, "angle": angle, "roundness": roundness, "airbrush": airbrush]
    }

    /// The MCP row: `json`'s values without the schema.
    var fields: [String: Any] {
        ["name": name, "hardness": hardness, "flow": flow, "spacing": spacing, "angle": angle,
         "roundness": roundness, "airbrush": airbrush]
    }

    /// Strict: exactly `json`'s keys, `schema_version` 1, a normalized name and
    /// values `validated` accepts. Anything else throws, and the library lists
    /// the file as one `invalid_schema` row.
    static func decode(_ object: [String: Any]) throws -> BrushPreset {
        guard Set(object.keys) == Set(["schema_version", "name"] + fieldNames),
              NativeBrushStroke.jsonNumber(object["schema_version"]) == Double(schemaVersion),
              let name = object["name"] as? String else {
            throw DoctoredCoreError(message: "A brush preset holds schema_version 1, name, "
                + fieldNames.joined(separator: ", ") + " and nothing else.")
        }
        let preset = try validated(name: name, values: object)
        guard preset.name == name else {
            throw DoctoredCoreError(message: "A brush preset's name must be normalized.")
        }
        return preset
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
