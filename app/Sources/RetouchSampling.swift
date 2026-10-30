import Foundation

// `RetouchSampling`'s raw values are the MCP argument's spelling and its `code` values the core's
// `DR_RETOUCH_SAMPLE_*`; both are wire contracts.
//
// `sample_all_layers` survives as a legacy alias (true = all, false = current) with its per-tool
// default intact; co-presence with `sample` is refused only on CONTRADICTION, because the recorder
// writes both spellings so a step recorded now still replays on a build that predates `sample`.

/// WHICH pixels a retouch gesture samples — the Clone Stamp's, the Healing
/// Brush's, the Spot Healing Brush's, the Patch tool's and Content-Aware
/// Fill's, in both pixel modes and from the UI, MCP and a replayed Action
/// alike.
///
/// The raw values are the MCP argument's spelling and the `code` values are
/// the core's `DR_RETOUCH_SAMPLE_*`; both are wire contracts and neither may
/// be renamed. The core's `retouch_sampling` module owns what each mode
/// means, including the deliberate boundary that "ignore adjustment layers"
/// drops adjustment ENTRIES and not layer styles, Blend If or a plain raster
/// layer in a tone blend mode.
enum RetouchSampling: String, Codable, CaseIterable {
    /// The layer's own pixels, deliberately raw. The historic
    /// `sample_all_layers: false`.
    case current
    /// The projection with everything above the target contributing nothing.
    case currentAndBelow = "current_and_below"
    /// The whole projection. The historic `sample_all_layers: true`.
    case all

    var code: UInt32 {
        switch self {
        case .current: return 0
        case .currentAndBelow: return 1
        case .all: return 2
        }
    }

    /// The menu/label text.
    var displayName: String {
        switch self {
        case .current: return "Current Layer"
        case .currentAndBelow: return "Current & Below"
        case .all: return "All Layers"
        }
    }

    /// What the legacy `sample_all_layers` boolean would have to be for this
    /// mode, or nil for the mode that boolean cannot express.
    ///
    /// This is the whole of the compatibility story in one property: a mode
    /// with a truthful boolean keeps reporting one, and the mode without one
    /// reports no boolean at all rather than a lie.
    var legacyAllLayers: Bool? {
        switch self {
        case .current: return false
        case .all: return true
        case .currentAndBelow: return nil
        }
    }

    /// The mode the legacy boolean names.
    static func legacy(_ allLayers: Bool) -> RetouchSampling { allLayers ? .all : .current }

    /// The enum's spellings, for a schema or a refusal that lists them.
    static let vocabulary = RetouchSampling.allCases.map(\.rawValue)
}

/// Where a retouch gesture's result lands.
///
/// `newLayer` puts the repair on a fresh raster entry directly above the
/// layer it was aimed at, so the original pixels stay recoverable. It is not
/// available on every combination: the core refuses it when the target sits
/// inside a group with its own blending, opacity, mask or style, because the
/// repair would then be gated twice — once in the sampled picture and again
/// by the group.
enum RetouchOutput: String, Codable, CaseIterable {
    /// Retouch into the target layer's own pixels.
    case layer
    /// Retouch onto a new layer directly above the target.
    case newLayer = "new_layer"

    var displayName: String {
        switch self {
        case .layer: return "This Layer"
        case .newLayer: return "New Layer"
        }
    }

    static let vocabulary = RetouchOutput.allCases.map(\.rawValue)
}

/// One gesture's sampling choice, resolved once from whichever spellings the
/// caller used.
struct RetouchSamplingChoice: Equatable {
    var mode: RetouchSampling
    var ignoreAdjustments: Bool

    /// What a result reports back. `sample` is always present; the legacy
    /// boolean only when it can tell the truth (see
    /// `RetouchSampling.legacyAllLayers`).
    var report: [String: Any] {
        var out: [String: Any] = ["sample": mode.rawValue,
                                  "ignore_adjustments": ignoreAdjustments]
        if let legacy = mode.legacyAllLayers { out["sample_all_layers"] = legacy }
        return out
    }
}

enum RetouchSamplingArguments {
    /// The refusal text for an unrecognized `sample`.
    static func unknownMode(_ raw: String) -> String {
        "sample must be one of \(RetouchSampling.vocabulary.joined(separator: ", ")) — got "
            + "\"\(raw)\". current samples the layer's own pixels, current_and_below the "
            + "picture beneath everything above it, and all the whole composite."
    }

    /// The refusal text for a `sample` and a `sample_all_layers` that
    /// disagree. Co-presence alone is NOT a refusal: the app's own recorder
    /// writes both spellings for a 16-bit stroke, and every Action recorded
    /// before this release carries only the boolean, so the two must be able
    /// to travel together whenever they agree.
    static func contradiction(_ mode: RetouchSampling, _ legacy: Bool) -> String {
        let expected = mode.legacyAllLayers
        let detail = expected == nil
            ? "sample_all_layers cannot express \(mode.rawValue) at all"
            : "\(mode.rawValue) means sample_all_layers \(expected == true)"
        return "sample: \"\(mode.rawValue)\" and sample_all_layers: \(legacy) contradict each "
            + "other — \(detail). Send one of them, or send agreeing values."
    }
}
