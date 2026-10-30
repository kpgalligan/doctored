import AppKit

// `LevelsMeta` holds the canonical form (black/white/gamma always, the output pair only when not
// (0, 1), a channel object only when its stage is not the identity), `merged` (a re-edit keeps keys
// this build does not read) and `sameEffect` (an unchanged Apply registers no undo step at either
// depth).
//
// The core applies each channel's stage before the composite's (`core/src/adjust_levels.rs`).

/// The stages a Levels adjustment holds: the composite (the params' top
/// level) and one per colour channel (`r`, `g`, `b` — or, in a CMYK document
/// (#547), `c`, `m`, `y`, `k` — each an optional object). The core applies a
/// channel's stage FIRST and the composite's second (`core/src/adjust_levels.rs`,
/// the order measured against Photoshop; `core/src/cmyk_adjust.rs` on
/// `1 − ink`). `rgb` and `cmyk` are the one composite, stored under `.rgb`
/// (`slot`).
enum LevelsChannel: String, CaseIterable {
    case rgb, r, g, b, cmyk, c, m, y, k

    /// The channels a dialog lists: RGB, Red, Green, Blue, or CMYK, Cyan,
    /// Magenta, Yellow, Black in a CMYK document, or Lightness (the
    /// composite), a and b (the g and b stages) in a Lab Color one (#547 Lab
    /// phase 2c).
    static func cases(cmyk: Bool, lab: Bool = false) -> [LevelsChannel] {
        if lab { return [.rgb, .g, .b] }
        return cmyk ? [.cmyk, .c, .m, .y, .k] : [.rgb, .r, .g, .b]
    }

    /// The dialog's name for this stage: Lightness, a and b in a Lab Color
    /// document, else `displayName`.
    func title(lab: Bool) -> String {
        guard lab else { return displayName }
        switch self {
        case .rgb, .r: return "Lightness"
        case .g: return "a"
        case .b: return "b"
        default: return displayName
        }
    }

    /// The per-channel stages, every mode's (absent = identity).
    static let channels: [LevelsChannel] = [.r, .g, .b, .c, .m, .y, .k]

    /// The stage's storage key: the composite is one stage whatever its name.
    var slot: LevelsChannel { self == .cmyk ? .rgb : self }

    var displayName: String {
        switch self {
        case .rgb: return "RGB"
        case .r: return "Red"
        case .g: return "Green"
        case .b: return "Blue"
        case .cmyk: return "CMYK"
        case .c: return "Cyan"
        case .m: return "Magenta"
        case .y: return "Yellow"
        case .k: return "Black"
        }
    }

    /// What the dialog's histogram shows while this stage is edited.
    var histogramDisplay: HistogramView.Display {
        switch self {
        case .rgb, .cmyk, .c, .m, .y, .k: return .luminosity
        case .r: return .red
        case .g: return .green
        case .b: return .blue
        }
    }
}

/// One stage's five numbers, in the meta's normalized units: the input
/// black/white points and gamma (`gamma > 1` brightens), then the output
/// black/white points (either order; black above white inverts).
struct LevelsStage: Equatable {
    var black = 0.0
    var white = 1.0
    var gamma = 1.0
    var outputBlack = 0.0
    var outputWhite = 1.0

    static let identity = LevelsStage()

    var isIdentity: Bool { self == .identity }

    init() {}

    /// Defensive read of one stage object, the way every dialog prefills:
    /// a missing, non-numeric or non-finite value takes its default, each
    /// value is clamped into its range, and an inverted or empty input range
    /// is repaired by pulling black below white.
    init(object: [String: Any]) {
        func number(_ key: String, _ fallback: Double, _ range: ClosedRange<Double>) -> Double {
            guard let value = (object[key] as? NSNumber)?.doubleValue, value.isFinite else {
                return fallback
            }
            return min(max(value, range.lowerBound), range.upperBound)
        }
        black = number("black", 0, 0...1)
        white = number("white", 1, 0...1)
        if black >= white {
            if white > 0 { black = max(white - 0.01, 0) } else { white = 0.01 }
        }
        gamma = number("gamma", 1, 0.1...10)
        outputBlack = number("output_black", 0, 0...1)
        outputWhite = number("output_white", 1, 0...1)
    }

    /// The canonical object: the input keys always, the output pair only
    /// when it is not the full range.
    var object: [String: Any] {
        var object: [String: Any] = ["black": black, "white": white, "gamma": gamma]
        if outputBlack != 0 || outputWhite != 1 {
            object["output_black"] = outputBlack
            object["output_white"] = outputWhite
        }
        return object
    }
}

/// The Levels params ↔ stages mapping every writer shares — the dialog, the
/// MCP validator and (in the core) the PSD decode all write this CANONICAL
/// form, so equal adjustments are equal JSON.
enum LevelsMeta {
    /// Every top-level key a levels meta reads (`c`/`m`/`y`/`k` in a CMYK
    /// document, #547).
    static let keys = ["black", "white", "gamma", "output_black", "output_white", "r", "g", "b",
                       "c", "m", "y", "k"]

    /// The stages a params object describes (absent channel = identity): the
    /// composite under `.rgb` and every channel of `LevelsChannel.channels`.
    static func stages(in params: [String: Any]) -> [LevelsChannel: LevelsStage] {
        var stages: [LevelsChannel: LevelsStage] = [.rgb: LevelsStage(object: params)]
        for channel in LevelsChannel.channels {
            stages[channel] = (params[channel.rawValue] as? [String: Any])
                .map(LevelsStage.init(object:)) ?? .identity
        }
        return stages
    }

    /// The canonical params: the composite's keys at the top level and a
    /// channel object only for a channel whose stage is not the identity.
    static func params(_ stages: [LevelsChannel: LevelsStage]) -> [String: Any] {
        var params = (stages[.rgb] ?? .identity).object
        for channel in LevelsChannel.channels {
            if let stage = stages[channel], !stage.isIdentity {
                params[channel.rawValue] = stage.object
            }
        }
        return params
    }

    /// `original` with every levels key replaced by the canonical params,
    /// so a re-edit keeps any key this build does not read.
    static func merged(
        _ stages: [LevelsChannel: LevelsStage], into original: [String: Any]
    ) -> [String: Any] {
        var merged = original
        for key in keys { merged.removeValue(forKey: key) }
        for (key, value) in params(stages) { merged[key] = value }
        return merged
    }

    /// Whether two params objects describe the same four stages — what an
    /// unchanged Apply compares, so a differently spelled identical
    /// adjustment registers no undo step and no dirty flag.
    static func sameEffect(_ a: [String: Any], _ b: [String: Any]) -> Bool {
        stages(in: a) == stages(in: b)
    }
}
