import AppKit

// `LayerLabel` is the palette of a PSD's `lclr` codes; `LayerKindFilter` and
// `LayerFilter` are the ONE question the filter bar and `find_layers` both ask, so the panel and
// the tool cannot answer it two ways.

/// Per-layer COLOUR LABELS and the layers panel's filter vocabulary.
///
/// A label is the mark a photographer puts on a row so they can find it again
/// in a stack of sixty: the palette and numbers of a PSD's `lclr` codes,
/// carried in the `.rz` at both depths and in a layered PSD both ways.
/// Nothing about it reaches the compositor, which is why it is exempt from the
/// layer locks exactly as a rename is.
///
/// `LayerKindFilter` and `LayerFilter` are the panel's and `find_layers`'
/// shared question — WHICH rows match — so the bar and the MCP tool can never
/// answer it two ways.
enum LayerLabel: UInt8, CaseIterable {
    case none = 0, red, orange, yellow, green, blue, violet, gray

    /// The core's `DR_LAYER_LABEL_MAX`, restated where Swift can read it.
    static let maximumCode: UInt8 = 7

    /// The wire spelling — the MCP argument and the Action step's value.
    var name: String {
        switch self {
        case .none: return "none"
        case .red: return "red"
        case .orange: return "orange"
        case .yellow: return "yellow"
        case .green: return "green"
        case .blue: return "blue"
        case .violet: return "violet"
        case .gray: return "gray"
        }
    }

    init?(name: String) {
        guard let match = Self.allCases.first(where: { $0.name == name }) else { return nil }
        self = match
    }

    /// The swatch the eye gutter is tinted with; nil for `none`, which draws
    /// nothing. Deliberately muted: the tint sits BEHIND the visibility eye,
    /// where a saturated colour would fight the thumbnail beside it.
    var color: NSColor? {
        switch self {
        case .none: return nil
        case .red: return NSColor(calibratedRed: 0.78, green: 0.25, blue: 0.25, alpha: 1)
        case .orange: return NSColor(calibratedRed: 0.82, green: 0.50, blue: 0.20, alpha: 1)
        case .yellow: return NSColor(calibratedRed: 0.78, green: 0.70, blue: 0.22, alpha: 1)
        case .green: return NSColor(calibratedRed: 0.32, green: 0.60, blue: 0.32, alpha: 1)
        case .blue: return NSColor(calibratedRed: 0.28, green: 0.47, blue: 0.78, alpha: 1)
        case .violet: return NSColor(calibratedRed: 0.55, green: 0.38, blue: 0.75, alpha: 1)
        case .gray: return NSColor(calibratedRed: 0.52, green: 0.52, blue: 0.55, alpha: 1)
        }
    }

    /// The menu/popup spelling.
    var displayName: String {
        switch self {
        case .none: return "None"
        default: return name.prefix(1).uppercased() + name.dropFirst()
        }
    }
}

/// What KIND a row is, for the panel's popup and `find_layers`. The order of
/// the cases is the order the decision is made in, and it matters: a group is
/// a group whatever it contains, an adjustment layer's pixels are ignored, a
/// text or shape layer is the rendering of its description, and "embedded" is
/// every layer whose pixels come from a retained original — a RAW master,
/// embedded or linked. Everything else is a plain
/// raster.
enum LayerKindFilter: String, CaseIterable {
    case raster, adjustment, text, shape, group, embedded

    var displayName: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }

    static func kind(of doc: RasterDocument, _ idx: Int) -> LayerKindFilter {
        if doc.layerInfo(idx)?.kind == DR_LAYER_GROUP { return .group }
        if doc.layerIsAdjustment(idx) { return .adjustment }
        if doc.textPayload(idx) != nil { return .text }
        if doc.shapePayload(idx) != nil { return .shape }
        if doc.hasRetainedSource(idx) { return .embedded }
        return .raster
    }
}

/// The layers panel's filter, and `find_layers`' query. Three independent
/// narrowings ANDed together; an empty filter matches everything and is the
/// state the panel ships in.
struct LayerFilter {
    var query: String = ""
    var kind: LayerKindFilter?
    var label: LayerLabel?

    var isActive: Bool { !trimmedQuery.isEmpty || kind != nil || label != nil }

    private var trimmedQuery: String {
        query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Case- and diacritic-insensitive SUBSTRING on the name, the kind and the
    /// label, all three ANDed. `label: .none` means "unlabelled", which is a
    /// real query a user asks (show me what I have not marked yet).
    func matches(_ doc: RasterDocument, _ idx: Int) -> Bool {
        let text = trimmedQuery
        if !text.isEmpty {
            guard let name = doc.layerInfo(idx)?.name,
                  name.range(of: text, options: [.caseInsensitive, .diacriticInsensitive]) != nil
            else { return false }
        }
        if let kind, LayerKindFilter.kind(of: doc, idx) != kind { return false }
        if let label, doc.layerLabel(idx) != label { return false }
        return true
    }
}

extension RasterDocument {
    /// Sets layer `idx`'s colour label; nil when the index is out of range or
    /// the entry already carries that label (the core's purity rule, so a
    /// no-op never registers an undo step).
    func withLayerLabel(_ idx: Int, _ label: LayerLabel) throws -> RasterDocument? {
        if isHighDepth {
            return try applyingPrecisionBookkeepingEdit(
                .init(["op": "layer_label", "layer": idx, "label": Int(label.rawValue)]))
        }
        return withLayerLabelHandle(idx, UInt32(label.rawValue))
    }

    /// Layer `idx`'s colour label; `.none` for a bad index.
    func layerLabel(_ idx: Int) -> LayerLabel {
        if let precisionMetadata {
            return LayerLabel(rawValue: precisionMetadata.layer(idx)?.label ?? 0) ?? .none
        }
        return LayerLabel(rawValue: UInt8(truncatingIfNeeded: layerLabelCode(idx))) ?? .none
    }

    /// Every entry matching `filter`, in bottom-first index order. The panel's
    /// walk and `find_layers`' walk are this one function, so a row the panel
    /// shows and a row the tool reports can never disagree.
    func layers(matching filter: LayerFilter) -> [Int] {
        (0..<layerCount).filter { filter.matches(self, $0) }
    }
}
