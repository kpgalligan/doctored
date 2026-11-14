import AppKit

/// `add_shape_layer` and `edit_shape_layer` on the `path` kind (ShapeOutline.swift), the agent
/// mirror of Paths panel ▸ Convert to Shape Layer (EditorViewController+PathCommands.swift) and of
/// re-boxing a path shape in its re-edit session (EditorViewController+Shapes.swift). The outline
/// is a Pen path: `subpaths` with an optional `fill_rule` in canvas pixels, or `path`, a stored
/// path's name (the Paths tools' own arguments, AgentServer+Paths.swift). It PLACES ITSELF — the
/// box is its outline's (`ShapeOutline.placed`) — so the box arguments do not apply to it.
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
    /// shape's `origin`), in set_path's own form. Empty for every other kind.
    static func pathShapeFields(_ payload: ShapeLayerPayload, outline: Bool) -> [String: Any] {
        guard payload.kind == "path", let path = payload.path else { return [:] }
        var fields: [String: Any] = [
            "fill_rule": path.fillRule, "anchors": path.anchorCount,
            "subpath_count": path.subpaths.count,
        ]
        if outline { fields["subpaths"] = path.shape(origin: .zero).subpathsJSON }
        return fields
    }
}
