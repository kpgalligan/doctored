import Foundation

// `LayerInsertPosition`'s raw values are the MCP argument's spelling, `code` is the header's
// `DrInsertPosition`, `refusal(in:anchor:)` is a host PRE-CHECK so an MCP caller gets a sentence
// instead of a bare nil, and `landingIndex(in:anchor:)` is the "select what I just made" answer
// every caller used to ask `insertionIndex(above:)` for.
// `RasterDocument.addingLayer(anchor:position:name:)` and
// `addingAdjustmentLayer(anchor:position:…)` are the positional twins; `addingLayer(above:)`
// forwards with `.above`. The UI's New Layer and New Adjustment Layer ask
// `newEntryPosition(active:)` (below): into an active group, else above the active entry.
//
// **Phase 12E-2b** finishes the slot rule: `LayerInsertPosition.catalogProperties` /
// `catalogSentence` are THE schema and sentence the five layer-creating catalog entries merge in
// (`place_image`, `add_live_photo_layer`, `new_layer`, `add_text_layer`, `add_shape_layer`),
// `AgentServer.insertionArguments` is THE parser, and `typealias LayerInsertion = (anchor,
// position)` is the one value the three placement work enums take.

/// WHERE a newly created entry lands, relative to an ANCHOR entry — the host
/// side of the core's ONE slot rule (`core/src/layer_insert.rs`, whose module
/// doc IS the rule; the raw values are the MCP argument's spelling and `code`
/// the header's `DrInsertPosition`).
///
/// Nothing about what a position MEANS is decided here: the core resolves the
/// slot, joins a clip run and refuses a bad anchor. `refusal(in:anchor:)` is a
/// host PRE-CHECK only, so an MCP caller gets a sentence naming what it did
/// wrong instead of a bare nil from the byte path, and `landingIndex` is the
/// "select what I just made" answer every caller already asks
/// `insertionIndex(above:)` for.
enum LayerInsertPosition: String, CaseIterable {
    /// Above the anchor's whole SUBTREE, at the anchor's own level — the
    /// placement every existing insert has, and the UI's.
    case above
    /// The top of the ROOT level, whatever the anchor is.
    case top
    /// The anchor's TOP CHILD; the anchor must be a group.
    case intoGroup = "into_group"

    /// The header's `DrInsertPosition` value.
    var code: UInt32 {
        switch self {
        case .above: return UInt32(DR_INSERT_ABOVE.rawValue)
        case .top: return UInt32(DR_INSERT_TOP.rawValue)
        case .intoGroup: return UInt32(DR_INSERT_INTO_GROUP.rawValue)
        }
    }

    /// An MCP `position` argument; an omitted one is `above`, which is what
    /// every caller before 12E-2 meant.
    static func parse(_ value: Any?) throws -> LayerInsertPosition {
        guard let value else { return .above }
        guard let name = value as? String, let position = LayerInsertPosition(rawValue: name) else {
            throw AgentServer.ToolError(
                message: "position must be above, top or into_group.")
        }
        return position
    }

    /// The host pre-check, in the core's own words; nil when the position is
    /// admitted.
    func refusal(in doc: RasterDocument, anchor: Int) -> String? {
        guard anchor >= 0, anchor < doc.layerCount else { return "Invalid layer index." }
        if self == .intoGroup, !doc.layerIsGroup(anchor) {
            return "into_group requires a group layer."
        }
        return nil
    }

    /// The index the new entry occupies once the insert lands — the same
    /// question `insertionIndex(above:)` answers for `above`.
    func landingIndex(in doc: RasterDocument, anchor: Int) -> Int {
        switch self {
        case .above: return doc.insertionIndex(above: anchor)
        case .top: return doc.layerCount
        case .intoGroup: return anchor
        }
    }
}

extension LayerInsertPosition {
    /// THE catalog schema for the MCP `position`/`layer` pair, merged into
    /// every layer-CREATING tool's properties. Written once so the five
    /// entries cannot describe one argument five ways.
    static var catalogProperties: [String: Any] {
        [
            "position": [
                "type": "string",
                "enum": LayerInsertPosition.allCases.map(\.rawValue),
                "description":
                    "Where the new layer lands relative to `layer`: above it (default), at the top of the root level, or as the top child of a group.",
            ],
            "layer": [
                "type": "integer",
                "description":
                    "Anchor layer index (default: the active layer). A group is a legal anchor.",
            ],
        ]
    }

    /// THE sentence the same five entries append to their descriptions.
    static var catalogSentence: String {
        "Optional `layer` (anchor, default the active layer) and `position` "
            + "(`above` the anchor, `top` of the root level, or `into_group` for a group anchor, "
            + "default `above`) choose where it lands; the reply reports the `position` used."
    }
}

/// An MCP placement request: the anchor entry and the slot relative to it —
/// the pair `AgentServer.insertionArguments` parses, carried as one value so
/// a work enum takes one optional parameter rather than two.
typealias LayerInsertion = (anchor: Int, position: LayerInsertPosition)

extension RasterDocument {
    /// Where a UI New Layer / New Adjustment Layer lands relative to the
    /// ACTIVE entry: inside it, as its top child, when it is a group,
    /// else above it. The group's disclosure is left as it is.
    func newEntryPosition(active: Int) -> LayerInsertPosition {
        layerIsGroup(active) ? .intoGroup : .above
    }

    /// The index that new entry occupies once it lands, to select it.
    func newEntryLandingIndex(active: Int) -> Int {
        newEntryPosition(active: active).landingIndex(in: self, anchor: active)
    }
}
