import AppKit

/// The `on_path` argument add_text_layer and edit_text_layer take — the agent mirror of
/// Layer ▸ Place Text on Path and Remove Text from Path (EditorViewController+TextOnPath.swift,
/// which run edit_text_layer through the dispatch) — and the `on_path` object their replies and
/// get_document's text object carry. The value is stored in the layer's SOURCE space
/// (TextOnPath.swift); the argument and the report speak canvas px.
extension AgentServer {
    static let onPathLineBreakRefusal = "text on a path is one line: remove the line break from text"
    static let onPathWrapRefusal = "text on a path does not wrap: leave wrap_width out"
    static let onPathSubpathRefusal = "on_path takes one subpath of at most 1024 anchors with a "
        + "length: pass subpath to pick one"
    static let onPathSideRefusal = "on_path.side must be \"left\" or \"right\""
    static let onPathBothRefusal = "pass subpaths or path_name in on_path, not both"
    static let onPathNoneRefusal = "this layer is not on a path: on_path needs subpaths or path_name"

    /// `on_path` read against `doc`: nil when the argument is absent; `(nil, anchor)` for JSON
    /// null (off the path); else the value in the source space of a layer at `anchor` under
    /// `transform` — the call's RESULTING map — with the anchor used. `{subpaths | path_name,
    /// subpath?, offset?, side?}`: `subpaths` in set_path's form or `path_name` a stored path's
    /// name (never `path`, which the path guard reads as a file), `subpath` the index of the
    /// one subpath (required when the source holds more), `offset` and `side` keeping
    /// `current`'s (else 0 and "left"). With neither `subpaths` nor `path_name` the current
    /// outline is kept. A nil `anchor` (add_text_layer with no x, y) is the subpath's first
    /// anchor, placed by the whole-pixel rule.
    func textOnPathArg(
        _ a: [String: Any], doc: RasterDocument, current: TextOnPath?, anchor: CGPoint?,
        transform: LinearMap
    ) throws -> (value: TextOnPath?, anchor: CGPoint?)? {
        guard let raw = a["on_path"] else { return nil }
        if raw is NSNull { return (nil, anchor) }
        guard let object = raw as? [String: Any] else {
            throw ToolError(
                message: "on_path must be an object {subpaths | path_name, subpath?, offset?, "
                    + "side?}, or null to take the text off its path")
        }
        let side = stringArg(object, "side") ?? current?.side ?? "left"
        guard TextOnPath.sides.contains(side) else { throw ToolError(message: Self.onPathSideRefusal) }
        let offset: Double
        if Self.present(object["offset"]) {
            guard !Self.isJSONBoolean(object["offset"] as Any), let given = doubleArg(object, "offset"),
                  given.isFinite, abs(given) <= 1e7
            else { throw ToolError(message: "on_path.offset must be a number of px") }
            offset = given
        } else {
            offset = current?.offset ?? 0
        }
        let named = Self.present(object["path_name"]), explicit = Self.present(object["subpaths"])
        if named, explicit { throw ToolError(message: Self.onPathBothRefusal) }
        guard named || explicit else {
            guard let current else { throw ToolError(message: Self.onPathNoneRefusal) }
            return (TextOnPath(outline: current.outline, offset: offset, side: side), anchor)
        }
        if named, !(object["path_name"] is String) {
            throw ToolError(message: "on_path.path_name must be a string — a path's name.")
        }
        var source: [String: Any] = [:]
        source["path"] = object["path_name"]
        source["subpaths"] = object["subpaths"]
        let shape = try pathArgument(source, doc, tool: "on_path")
        let index: Int
        if Self.present(object["subpath"]) {
            guard let given = intArg(object, "subpath") else {
                throw ToolError(message: Self.onPathSubpathRefusal)
            }
            index = given
        } else {
            guard shape.subpaths.count == 1 else { throw ToolError(message: Self.onPathSubpathRefusal) }
            index = 0
        }
        guard shape.subpaths.indices.contains(index) else {
            throw ToolError(message: Self.onPathSubpathRefusal)
        }
        let subpath = shape.subpaths[index]
        guard subpath.anchors.count <= TextOnPath.maxAnchors, let first = subpath.anchors.first,
              (try? RasterPathCore.pathLength(PathShape(subpaths: [subpath]), subpath: 0)) != nil
        else { throw ToolError(message: Self.onPathSubpathRefusal) }
        let placed = anchor ?? DescribedLayer.placementAnchor(first.point, transform: transform)
        guard let value = TextOnPath(canvas: subpath, anchor: placed, transform: transform,
                                     offset: offset, side: side)
        else { throw ToolError(message: Self.onPathSubpathRefusal) }
        return (value, placed)
    }

    /// The refusals a text on a path answers before any edit: one line, never wrapped.
    func refuseTextOnPathLayout(_ a: [String: Any], text: String) throws {
        if TextOnPath.hasLineBreak(text) { throw ToolError(message: Self.onPathLineBreakRefusal) }
        if case .width? = try textBoxArg(a) { throw ToolError(message: Self.onPathWrapRefusal) }
    }

    /// The `on_path` object of a text object: the subpath on the canvas (`canvas_subpaths`, in
    /// set_path's form, when the anchor is known), `offset`, `side`, the subpath's `length` in
    /// source px and `hidden_glyphs`, the glyphs past an open subpath's ends. nil off a path.
    static func textOnPathFields(_ payload: TextLayerPayload, anchor: CGPoint?) -> [String: Any]? {
        guard let onPath = payload.onPath else { return nil }
        var fields: [String: Any] = ["offset": onPath.offset, "side": onPath.side]
        if let anchor {
            fields["canvas_subpaths"] =
                onPath.canvasShape(anchor: anchor, transform: payload.transform).subpathsJSON
        }
        if let metrics = TextLayer.onPathMetrics(payload) {
            fields["length"] = metrics.length
            fields["hidden_glyphs"] = metrics.hiddenGlyphs
        }
        return fields
    }
}
