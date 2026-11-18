import AppKit

/// `add_shape_layer` and `edit_shape_layer` on the `path` kind (ShapeOutline.swift), the agent
/// mirror of Paths panel ▸ Convert to Shape Layer (EditorViewController+PathCommands.swift), of
/// re-boxing a path shape in its re-edit session (EditorViewController+Shapes.swift) and of the
/// Pen, Path Selection and Direct Selection on the layer's outline (ShapeOutlineTarget.swift). The
/// outline is a Pen path: `subpaths` with an optional `fill_rule` in canvas pixels, or `path`, a
/// stored path's name (the Paths tools' own arguments, AgentServer+Paths.swift). It PLACES ITSELF
/// — the box is its outline's (`ShapeOutline.placed`; under a transform `replacingOutline`) — so
/// the box arguments do not apply to it.
extension AgentServer {
    /// The arguments that place a box kind and mean nothing to a path: a path shape's box is its
    /// outline's, and only a rect has a radius and a line a direction.
    private static let boxArguments = ["x", "y", "w", "h", "radius", "flipped"]
    /// The arguments that give a path shape its outline.
    private static let outlineArguments = ["subpaths", "path", "fill_rule"]

    /// The outline and box of a new `path` shape; nil for every other kind, which is refused any
    /// outline argument. A path shape is refused the box arguments by name, an empty path, and
    /// one the outline cannot hold.
    func pathShapePlacement(
        _ a: [String: Any], doc: RasterDocument, kind: String
    ) throws -> (outline: ShapeOutline, box: CGRect)? {
        guard kind == "path" else {
            if let key = Self.outlineArguments.first(where: { Self.present(a[$0]) }) {
                throw ToolError(
                    message: "\(key) goes with kind \"path\": a \(kind) is placed by x, y, w "
                        + "and h.")
            }
            return nil
        }
        if let key = Self.boxArguments.first(where: { Self.present(a[$0]) }) {
            throw ToolError(
                message: "A path shape places itself from its outline, so \(key) does not apply "
                    + "to kind \"path\": pass subpaths (canvas px) or path, and move or re-box the "
                    + "layer afterwards with edit_shape_layer.")
        }
        let shape = try pathArgument(a, doc, tool: "add_shape_layer with kind \"path\"")
        try refuseEmpty(shape, "draw")
        guard let placed = ShapeOutline.placed(shape) else {
            throw ToolError(
                message: "The path cannot be a shape: a shape's outline holds at most 4096 "
                    + "subpaths and 32768 anchors, each coordinate within ±1000000, and must have "
                    + "an extent.")
        }
        return placed
    }

    /// `edit_shape_layer`'s new outline for a path shape, with the anchor the layer takes for it
    /// (`ShapeLayerPayload.replacingOutline`, the canvas gestures' rule, under the layer's own
    /// transform): nil when no outline argument is given. Another kind is refused any outline
    /// argument, and a new outline is refused x, y, w, h and transform by name — it places
    /// itself under the layer's current map, so a move, re-box or re-map is a second call.
    func replacedPathOutline(
        _ a: [String: Any], doc: RasterDocument, current: ShapeLayerPayload, anchor: CGPoint
    ) throws -> (payload: ShapeLayerPayload, anchor: CGPoint)? {
        guard let given = Self.outlineArguments.first(where: { Self.present(a[$0]) }) else {
            return nil
        }
        guard current.kind == "path" else {
            throw ToolError(
                message: "\(given) goes with a path shape: a \(current.kind) is placed by x, y, w "
                    + "and h.")
        }
        if let key = ["x", "y", "w", "h", "transform"].first(where: { Self.present(a[$0]) }) {
            throw ToolError(
                message: "A new outline places itself under the layer's current transform, so "
                    + "\(key) does not apply with subpaths or path: replace the outline, then move, "
                    + "re-box or re-map the layer in a second edit_shape_layer call.")
        }
        let shape = try pathArgument(a, doc, tool: "edit_shape_layer with fill_rule")
        try refuseEmpty(shape, "draw")
        guard let replaced = current.replacingOutline(shape, anchor: anchor) else {
            throw ToolError(
                message: "The path cannot be this shape's outline: an outline holds at most 4096 "
                    + "subpaths and 32768 anchors, each coordinate within ±1000000, and must have "
                    + "an extent.")
        }
        return replaced
    }

    /// A path shape whose outline holds subpath operations (`subpaths[i].op`) takes no stroke and
    /// must be bounded (`ShapeLayerPayload.heldOpsRefusal`, which the canvas commits present as an
    /// alert); every other payload passes.
    static func validatePathShapeOps(_ payload: ShapeLayerPayload) throws {
        if let refusal = payload.heldOpsRefusal { throw ToolError(message: refusal) }
    }

    /// A path shape needs an area to fill: an outline under 1 px across on one axis (a straight
    /// open path) shows only a stroke.
    static func validatePathShapeArea(
        kind: String, w: Double, h: Double, stroke: NSColor?, strokeWidth: Double
    ) throws {
        guard kind == "path", min(w, h) < 1, stroke == nil || !(strokeWidth > 0) else { return }
        throw ToolError(
            message: "This path's outline is under 1 px across on one axis, so a fill alone shows "
                + "nothing: give it a stroke (stroke and a stroke_width above 0).")
    }

    /// A path shape's outline re-boxed to `w` × `h`: scaled about the box's origin by the new
    /// size over the old, so the box and its outline resize alike (`ShapeOutline.reboxed`, the
    /// re-edit session's rule). An axis the outline does not span (`ShapeOutline.scalingAxes`: a
    /// straight line's, whatever its box's size there) cannot be resized, as there is nothing to
    /// scale; nil for every other kind.
    static func reboxedOutline(_ current: ShapeLayerPayload, w: Double, h: Double) throws
        -> ShapeOutline? {
        guard current.kind == "path", let outline = current.path else { return nil }
        let old = CGSize(width: current.w, height: current.h)
        let axes = outline.scalingAxes(in: old)
        for (scales, size, wanted, axis, key) in [(axes.x, current.w, w, "width", "w"),
                                                  (axes.y, current.h, h, "height", "h")]
        where !scales && wanted != size {
            throw ToolError(
                message: "This path shape's outline is a straight line with no \(axis), so it "
                    + "cannot be given one: re-box it along its other axis and leave \(key) out.")
        }
        return outline.reboxed(from: old, to: CGSize(width: w, height: h), axes: axes)
    }

    /// A path shape's fields in the shape object: `fill_rule`, `anchors` (a count) and
    /// `subpath_count`, and with `outline` its `subpaths` relative to the box's top-left (the
    /// shape's `origin`) before the transform, in set_path's own form, and — when the anchor is
    /// known — `canvas_subpaths`, the outline where it lies on the canvas (`origin + T·local`,
    /// `canvasOutline`), the form `edit_shape_layer` takes back. Empty for every other kind.
    static func pathShapeFields(
        _ payload: ShapeLayerPayload, anchor: CGPoint?, outline: Bool
    ) -> [String: Any] {
        guard payload.kind == "path", let path = payload.path else { return [:] }
        var fields: [String: Any] = [
            "fill_rule": path.fillRule, "anchors": path.anchorCount,
            "subpath_count": path.subpaths.count,
        ]
        guard outline else { return fields }
        fields["subpaths"] = path.shape(origin: .zero).subpathsJSON
        if let anchor, let canvas = payload.canvasOutline(anchor: anchor) {
            fields["canvas_subpaths"] = canvas.subpathsJSON
        }
        return fields
    }
}
