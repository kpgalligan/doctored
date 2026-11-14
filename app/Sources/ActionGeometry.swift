import Foundation

// `ActionGeometry.fields` is the explicit per-tool table of which ARGUMENTS are canvas coordinates:
// a non-absolute mode on a tool outside it is refused at DECODE by name, because remapping the
// wrong argument is silent.

/// Where a canvas anchor sits, as `canvas_size`'s own nine names.
///
/// The names are the catalog's — one vocabulary for "the top-left corner",
/// whether it is a Canvas Size dialog or a step's geometry mode — and the two
/// fractions are the whole definition: `center` is (½, ½), `top-left` is
/// (0, 0), `bottom-right` is (1, 1).
enum CanvasAnchor: String, CaseIterable {
    case center
    case top
    case bottom
    case left
    case right
    case topLeft = "top-left"
    case topRight = "top-right"
    case bottomLeft = "bottom-left"
    case bottomRight = "bottom-right"

    /// (fx, fy), each 0, ½ or 1 — the fraction of the canvas that lies to the
    /// LEFT of the anchor and ABOVE it.
    var fractions: (x: Double, y: Double) {
        switch self {
        case .center: return (0.5, 0.5)
        case .top: return (0.5, 0)
        case .bottom: return (0.5, 1)
        case .left: return (0, 0.5)
        case .right: return (1, 0.5)
        case .topLeft: return (0, 0)
        case .topRight: return (1, 0)
        case .bottomLeft: return (0, 1)
        case .bottomRight: return (1, 1)
        }
    }
}

/// How one step's canvas coordinates travel to a document of a different
/// size.
///
/// **Absolute is the default and is what every recording is.** The recorder
/// writes what the gesture did, in the pixels it did it in; deciding that a
/// crop should scale — or that a watermark should stay 20 px in from the
/// bottom-right corner however big the picture is — is a statement about
/// REUSE that only the person reusing the action can make. So this is an edit
/// (the Actions window's row menu), never something inferred.
///
/// The three, in the arithmetic they perform on a coordinate `v` recorded in
/// a `W × H` space and replayed in a `W' × H'` one:
///
/// - `.absolute` — `v`. The 12A behaviour, unchanged.
/// - `.relative` — `v · W' / W` (`H` for a y). EVERYTHING scales: positions,
///   extents and deltas alike, because "the same fraction of the picture" is
///   what proportional means.
/// - `.anchor(a)` — `f · W' + (v − f · W)`, where `f` is the anchor's
///   fraction. Distances FROM THE ANCHOR are preserved, so sizes and deltas
///   are unchanged and only positions move. That is what "keep it 20 px in
///   from the bottom-right" means, and it is why the two modes are separate
///   rather than one with a flag.
///
/// An affine MATRIX `[a, b, c, d, tx, ty]` (`transform_mask`,
/// `transform_layers`) follows one of two rules under `.relative`, with
/// `sx = W' / W` and `sy = H' / H`:
///
/// - With a recorded `pivot` `p = (px, py)` (every UI recording has one),
///   the linear part `a, b, c, d` is kept and the translation becomes
///   `tx · sx + c · (sx − sy) · py` and `ty · sy + b · (sy − sx) · px`, so the
///   angle, scale and skew are kept about the remapped pivot. That is
///   `transform_layer`'s own replay, `M'(x) = L(x − Rp) + Rp + S · (M(p) − p)`,
///   so the two tools agree; the `pivot` argument itself remaps as a position.
/// - Without one, the matrix is conjugated by the coordinate remap R,
///   `M' = R ∘ M ∘ R⁻¹`: `a` and `d` are kept, and `b · sy / sx`,
///   `c · sx / sy`, `tx · sx` and `ty · sy` are written. A rotated or skewed
///   matrix becomes its stretched equivalent, which is what the corner remaps
///   of `distort_layer` and `transform_layers`' own `source_rect` + `corners`
///   form already give — the two forms agree for Double-valued recordings
///   (every UI recording) to the corners' four-decimal quantization, while an
///   integer-written `source_rect` or `corners` rounds half away from zero and
///   can differ by up to 0.5 px.
///
/// With a uniform scale, or no rotation or skew, the two rules agree.
/// `.anchor(f)` uses one rule either way: it keeps the linear part and, with
/// `ox = fx · (W' − W)` and `oy = fy · (H' − H)`, writes
/// `tx + ox − (a · ox + c · oy)` and `ty + oy − (b · ox + d · oy)`. The
/// remapped entries are plain Doubles, never rounded: a translation of 12.5
/// is not a pixel count.
enum GeometryMode: Equatable {
    case absolute
    case relative
    case anchor(CanvasAnchor)

    /// The word the row menu and a refusal both use.
    var displayName: String {
        switch self {
        case .absolute: return "absolute"
        case .relative: return "relative"
        case .anchor(let anchor): return "\(anchor.rawValue) anchor"
        }
    }

    /// The file's form: `{"mode": "absolute" | "relative" | "anchor",
    /// "anchor": "top-left"}`. `.absolute` is never written (see
    /// `ActionStep.jsonObject`), so this is only ever asked of the other two.
    var jsonObject: [String: Any] {
        switch self {
        case .absolute: return ["mode": "absolute"]
        case .relative: return ["mode": "relative"]
        case .anchor(let anchor): return ["mode": "anchor", "anchor": anchor.rawValue]
        }
    }

    static func parse(_ value: Any) -> Result<GeometryMode, ActionError> {
        guard let fields = value as? [String: Any], let mode = fields["mode"] as? String else {
            return .failure(
                ActionError(
                    "geometry must be an object with \"mode\": \"absolute\", \"relative\" "
                        + "or \"anchor\""))
        }
        switch mode {
        case "absolute": return .success(.absolute)
        case "relative": return .success(.relative)
        case "anchor":
            guard let name = fields["anchor"] as? String,
                  let anchor = CanvasAnchor(rawValue: name)
            else {
                return .failure(
                    ActionError(
                        "geometry mode \"anchor\" needs \"anchor\": one of "
                            + CanvasAnchor.allCases.map { $0.rawValue }
                            .joined(separator: ", ")))
            }
            return .success(.anchor(anchor))
        default:
            return .failure(
                ActionError(
                    "geometry mode must be \"absolute\", \"relative\" or \"anchor\" "
                        + "(got \"\(mode)\")"))
        }
    }
}

/// What ONE argument of a tool means geometrically. The table below maps a
/// tool's argument names onto these, and the arithmetic reads only this.
enum GeometryField {
    /// A canvas x. Moves under both non-absolute modes.
    case x
    /// A canvas y.
    case y
    /// A width. Scales under `.relative`, unchanged under `.anchor`.
    case w
    /// A height.
    case h
    /// A horizontal DELTA. Scales under `.relative`, unchanged under
    /// `.anchor` — a nudge of 40 px stays a nudge of 40 px.
    case dx
    /// A vertical delta.
    case dy
    /// An `[[x, y], …]` polyline: every point is an (x, y) POSITION.
    case points
    /// A guide position, whose axis comes from the call's own
    /// `orientation` — a HORIZONTAL guide is a line of constant y, so its
    /// position is measured on the height axis (`doc_guide`'s convention).
    case guideAxis
    /// `[x, y]`: ONE position (`select_subjects`' `point`, a matrix's
    /// recorded `pivot`).
    case pair
    /// A `{"x", "y", "width", "height"}` object: a position and an extent.
    /// Any other key in it is kept as it is.
    case rect
    /// `[x, y, width, height]` (`transform_layers`' `source_rect`).
    case rectArray
    /// `[a, b, c, d, tx, ty]` in the `CGAffineTransform` convention, in
    /// canvas coordinates.
    case affine
}

/// The geometry-mode machinery: which arguments of which tools are
/// coordinates, what a canvas-changing step does to the recorded space, and
/// the remap itself.
///
/// **Why the table is explicit and small.** Remapping the wrong argument is
/// silent — a `tolerance` scaled by 2 produces a picture nobody asked for and
/// no error anywhere — so a tool is in this table only when every listed key
/// is genuinely a canvas coordinate, and a tool that is not in it refuses a
/// non-absolute mode BY NAME at decode rather than accepting the setting and
/// ignoring it.
enum ActionGeometry {
    /// tool → (argument name → what it means).
    static let fields: [String: [String: GeometryField]] = [
        // Rect tools: an origin and an extent.
        "crop": ["x": .x, "y": .y, "width": .w, "height": .h],
        "select_rect": ["x": .x, "y": .y, "width": .w, "height": .h],
        "select_ellipse": ["x": .x, "y": .y, "width": .w, "height": .h],
        "red_eye": ["x": .x, "y": .y, "width": .w, "height": .h],
        // Polylines.
        "select_polygon": ["points": .points],
        "select_freehand": ["points": .points],
        // Localized Color Clusters' sample points.
        "select_color_range": ["points": .points],
        "brush_stroke": ["points": .points],
        "eraser_stroke": ["points": .points],
        "distort_layer": ["corners": .points],
        // Single points.
        "select_magic_wand": ["x": .x, "y": .y],
        "fill": ["x": .x, "y": .y],
        "auto_select_layer": ["x": .x, "y": .y],
        "set_ruler_origin": ["x": .x, "y": .y],
        "add_text_layer": ["x": .x, "y": .y],
        "add_text": ["x": .x, "y": .y],
        "place_image": ["x": .x, "y": .y],
        // A saved-payload paste's `at` position (`unsupported`
        // refuses a mode on any other paste). `.anchor` maps it field by
        // field; `.relative` maps it by `ClipboardSource.relativeOffset` over
        // the payload's size (`remap(…, extent:)`), so the payload's CENTRE
        // keeps its fraction of the canvas: Doctored's rule for its
        // saved-payload step.
        "paste": ["x": .x, "y": .y],
        // A point and a box.
        "add_shape_layer": ["x": .x, "y": .y, "w": .w, "h": .h],
        "edit_shape_layer": ["x": .x, "y": .y, "w": .w, "h": .h],
        // An absolute layer position…
        "set_layer_properties": ["offset_x": .x, "offset_y": .y],
        // …and the relative one.
        "move_layers": ["dx": .dx, "dy": .dy],
        "move_selected_pixels": ["dx": .dx, "dy": .dy],
        "transform_selected_pixels": [
            "pivot_x": .x, "pivot_y": .y, "translate_x": .dx, "translate_y": .dy,
            "corners": .points,
        ],
        // Free Transform: the pivot is a POSITION, the translation a DELTA.
        "transform_layer": [
            "pivot_x": .x, "pivot_y": .y, "translate_x": .dx, "translate_y": .dy,
        ],
        "transform_selection": [
            "anchor_x": .x, "anchor_y": .y, "translate_x": .dx, "translate_y": .dy,
        ],
        "gradient": ["x0": .x, "y0": .y, "x1": .x, "y1": .y],
        // Whole-canvas sizes: extents, so `.anchor` leaves them alone and
        // `.relative` scales them.
        "canvas_size": ["width": .w, "height": .h],
        "image_size": ["width": .w, "height": .h],
        "add_guide": ["position": .guideAxis],
        // remove_guide's position lives inside its `$guide` symbol; `remap`
        // reaches into the payload, which is why the key names the symbol
        // argument rather than a number.
        "remove_guide": ["guide": .guideAxis],
        // Select Objects' point or region. Its instance ids, mode and
        // feather address the picture, not the canvas.
        "select_subjects": ["point": .pair, "region": .rect],
        // Object Selection's dragged rectangle or lasso.
        "object_select": ["region": .rect, "points": .points],
        // Affine matrices, remapped about their recorded pivot or conjugated
        // by the remap without one (see `GeometryMode`).
        "transform_mask": ["matrix": .affine, "pivot": .pair],
        "transform_layers": [
            "matrix": .affine, "pivot": .pair, "source_rect": .rectArray, "corners": .points,
        ],
    ]

    /// Tools that DO carry canvas coordinates — clone/heal sources and
    /// strokes, dodge/burn strokes, patch regions, red-eye discs, Quick
    /// Selection seeds — but whose remapping has not been qualified, so a
    /// non-absolute mode on one is refused naming what is missing rather
    /// than claiming the step has no coordinates.
    static let unqualified: Set<String> = [
        "clone_stamp", "heal_stroke", "spot_heal_stroke",
        "dodge_burn", "patch_region", "red_eye_auto", "quick_select",
        // The retouch brushes' strokes (history_brush_stroke is never
        // recorded, but a hand-written step carries canvas points).
        "blur_stroke", "sharpen_stroke", "smudge_stroke", "sponge_stroke", "history_brush_stroke",
        // The Art History Brush's stroke.
        "art_history_brush_stroke",
        // The Remove tool's stroke, and Content-Aware Move's region and offset.
        "remove_stroke", "content_aware_move",
        // Never recorded (it records `select_freehand`, which IS
        // remapped), but a hand-written step carries canvas points.
        "select_magnetic",
        // Filters ▸ Liquify…'s strokes.
        "liquify",
        // Edit ▸ Puppet Warp's pins.
        "puppet_warp",
        // Edit ▸ Perspective Warp's quads (source corners and pins).
        "perspective_warp",
        // A path's anchors and handles (`subpaths`) are canvas
        // coordinates; a step that names its path instead carries none, but
        // the shape it names was written by a step that does.
        "set_path", "path_to_selection", "fill_path", "stroke_path",
        // …and so is a vector mask's shape.
        "set_vector_mask",
        // The Object Finder's prompt points and box.
        "object_finder_select",
    ]

    /// Why `tool` may not carry a non-absolute mode, or nil when it may.
    ///
    /// The reasons are different fixes. The tool's remapping is not
    /// qualified (`warp_layer`, `unqualified`, and
    /// `content_aware_fill` with a `source_mask`); `content_aware_fill`
    /// without one has no coordinates of its own, because it fills the
    /// selection an earlier step made, and `select_subjects` without a
    /// `point` or `region` has none either (its instance ids and whole-image
    /// form address the picture, not the canvas); a canvas-size-locked payload
    /// (`quick_mask_stroke`) is its own reason, because it replays only on the
    /// size it was recorded for; a tool outside the `fields` table is
    /// absolute-only whether or not its arguments carry coordinates; or it is
    /// a stroke recorded as native canonical dabs — which carry their own
    /// pressure- and time-stamped geometry in a packed blob this layer cannot
    /// rewrite, so remapping only the fallback `points` would replay a stroke
    /// that disagreed with itself.
    static func unsupported(tool: String, _ arguments: [String: Any]) -> String? {
        if tool == "warp_layer" {
            return "\"warp_layer\" supports absolute geometry only; relative and anchor "
                + "lattice remapping is not qualified."
        }
        if tool == "quick_mask_stroke" {
            return "\"quick_mask_stroke\" supports absolute geometry only: its coverage is recorded "
                + "for a canvas of exactly its width × height and replays only on that size, so "
                + "relative and anchor remapping cannot apply. Re-record it on the target size"
        }
        if tool == "paste", arguments["placement"] as? String != "at" {
            return "\"paste\" has no canvas coordinates unless its placement is at — a live "
                + "paste lands by the replaying document's own placement rule — so put the "
                + "geometry mode on a step that carries coordinates"
        }
        if tool == "select_subjects",
           arguments["point"].map({ $0 is NSNull }) ?? true,
           arguments["region"].map({ $0 is NSNull }) ?? true {
            return "\"select_subjects\" has no canvas coordinates without a point or region — "
                + "its instance ids and whole-image form address the picture, not the canvas — "
                + "so put the geometry mode on a step that carries coordinates"
        }
        // A path shape's outline is a path's anchors and handles, which the
        // path tools above do not remap either.
        if unqualified.contains(tool)
            || (tool == "add_shape_layer" && ["subpaths", "path"].contains { arguments[$0] != nil }) {
            return "\"\(tool)\" supports absolute geometry only; relative and anchor "
                + "remapping of its canvas coordinates is not qualified."
        }
        if tool == "content_aware_fill" {
            if let mask = arguments["source_mask"], !(mask is NSNull) {
                return "\"content_aware_fill\" supports absolute geometry only; relative and "
                    + "anchor remapping of its source_mask is not qualified."
            }
            return "\"content_aware_fill\" has no canvas coordinates of its own — it fills the "
                + "current selection — so put the geometry mode on the selection step that "
                + "defines the fill region"
        }
        guard let table = fields[tool] else {
            return "\"\(tool)\" supports absolute geometry only: it is not in the geometry "
                + "remap table. If its arguments carry no canvas coordinates, remove the mode "
                + "or put it on the step that carries them"
        }
        if table["points"] != nil, arguments["native_stroke"] != nil {
            return "\"\(tool)\" was captured as a native stroke, whose canonical dabs carry "
                + "their own geometry — remapping only the fallback points would replay a "
                + "stroke that disagreed with itself. Re-record it on the target size"
        }
        return nil
    }

    /// The canvas the document has AFTER `tool` ran with these arguments,
    /// given the space it ran in — nil for every tool that leaves the canvas
    /// alone.
    ///
    /// This is what keeps a running recorded space honest: an action whose
    /// first step is a crop puts every later coordinate in the CROPPED space,
    /// so remapping them against the size the recording started at would be
    /// wrong by exactly the crop. The player advances the space with this
    /// after each step that SUCCEEDS.
    ///
    /// **The `from:` parameter is not decoration**: `rotate` swaps the two
    /// numbers rather than naming them, so the answer cannot be read out of
    /// the arguments alone.
    static func canvasAfter(
        tool: String, arguments: [String: Any], from space: (width: Int, height: Int)
    ) -> (width: Int, height: Int)? {
        switch tool {
        case "crop", "image_size", "canvas_size":
            guard let width = integer(arguments["width"]), let height = integer(arguments["height"]),
                  width > 0, height > 0
            else { return nil }
            return (width: width, height: height)
        case "rotate":
            // Phase 12F-1b3: the CORE answers this, through the plan export
            // that takes no document handle — so the quarter turns swap
            // exactly as they always did, an identity leaves the space alone,
            // and a FILTERED angle reports the EXPANDED canvas. A `degrees`
            // that is neither a number nor a numeric string, and a plan whose
            // canvas the core refused, both answer nil, which is what the
            // player reads as "this step did not move the recorded space".
            let raw = arguments["degrees"]
            guard let degrees = (raw as? NSNumber)?.doubleValue
                ?? (raw as? String).flatMap(Double.init)
            else { return nil }
            guard let plan = RasterDocument.canvasRotationPlan(
                width: space.width, height: space.height, degrees: degrees),
                plan.width > 0, plan.height > 0
            else { return nil }
            return (width: plan.width, height: plan.height)
        default:
            return nil
        }
    }

    /// `arguments` with every coordinate this tool declares remapped from the
    /// `recorded` space into the `live` one under `mode`.
    ///
    /// `.absolute` returns the arguments untouched — literally the same
    /// dictionary — so the default path costs nothing and cannot round a
    /// value it was not asked to change. `extent` is a `paste` step's
    /// payload size (the player reads it from the step's `.drcb`), which
    /// its `.relative` remap needs.
    static func remap(
        _ arguments: [String: Any], tool: String, mode: GeometryMode,
        recorded: (width: Int, height: Int), live: (width: Int, height: Int),
        extent: (width: Int, height: Int)? = nil
    ) -> Result<[String: Any], ActionError> {
        guard mode != .absolute else { return .success(arguments) }
        guard recorded.width > 0, recorded.height > 0, live.width > 0, live.height > 0 else {
            return .failure(
                ActionError("a geometry remap needs both canvas sizes, and one of them is empty"))
        }
        if let reason = unsupported(tool: tool, arguments) {
            return .failure(ActionError(reason))
        }
        if tool == "paste", mode == .relative {
            guard let extent else {
                return .failure(ActionError("a relative paste needs its payload's size"))
            }
            guard let x = ActionJSON.number(arguments["x"]),
                  let y = ActionJSON.number(arguments["y"])
            else { return .success(arguments) }
            let at = ClipboardSource.relativeOffset(
                of: (x.intValue, y.intValue), size: extent, from: recorded, into: live)
            var out = arguments
            out["x"] = at.x
            out["y"] = at.y
            return .success(out)
        }
        guard let table = fields[tool] else { return .success(arguments) }
        var out = arguments
        for (key, field) in table {
            guard let value = arguments[key] else { continue }
            switch field {
            case .x, .y, .w, .h, .dx, .dy:
                guard let number = ActionJSON.number(value) else { continue }
                out[key] = mapped(number, field: field, mode: mode, recorded: recorded, live: live)
            case .points:
                switch mappedPoints(value, mode: mode, recorded: recorded, live: live) {
                case .success(let points): out[key] = points
                case .failure(let error): return .failure(error)
                }
            case .guideAxis:
                switch mappedGuide(
                    key: key, value: value, arguments: arguments, mode: mode, recorded: recorded,
                    live: live)
                {
                case .success(let replacement): out[key] = replacement
                case .failure(let error): return .failure(error)
                }
            case .pair:
                switch mappedPair(value, key: key, mode: mode, recorded: recorded, live: live) {
                case .success(let replacement): out[key] = replacement
                case .failure(let error): return .failure(error)
                }
            case .rect:
                switch mappedRect(value, key: key, mode: mode, recorded: recorded, live: live) {
                case .success(let replacement): out[key] = replacement
                case .failure(let error): return .failure(error)
                }
            case .rectArray:
                switch mappedRectArray(value, key: key, mode: mode, recorded: recorded, live: live) {
                case .success(let replacement): out[key] = replacement
                case .failure(let error): return .failure(error)
                }
            case .affine:
                switch mappedAffine(
                    value, key: key, pivot: arguments["pivot"], mode: mode, recorded: recorded,
                    live: live)
                {
                case .success(let replacement): out[key] = replacement
                case .failure(let error): return .failure(error)
                }
            }
        }
        return .success(out)
    }

    // MARK: - The arithmetic

    /// One number, in the field's own axis and class.
    private static func mapped(
        _ number: NSNumber, field: GeometryField, mode: GeometryMode,
        recorded: (width: Int, height: Int), live: (width: Int, height: Int)
    ) -> Any {
        let horizontal: Bool
        switch field {
        case .x, .w, .dx: horizontal = true
        default: horizontal = false
        }
        let from = Double(horizontal ? recorded.width : recorded.height)
        let to = Double(horizontal ? live.width : live.height)
        let isPosition: Bool
        switch field {
        case .x, .y, .points, .guideAxis: isPosition = true
        case .w, .h, .dx, .dy: isPosition = false
        // The nested kinds map their scalars as `.x`/`.y`/`.w`/`.h` (and the
        // matrix not through here at all), so none of them reaches this.
        case .pair, .rect, .rectArray, .affine: isPosition = true
        }
        let value = number.doubleValue
        let result: Double
        switch mode {
        case .absolute:
            result = value
        case .relative:
            // Everything scales — a proportional step keeps the same fraction
            // of the picture, extents and deltas included.
            result = value * to / from
        case .anchor(let anchor):
            guard isPosition else { return number }
            let fraction = horizontal ? anchor.fractions.x : anchor.fractions.y
            result = fraction * to + (value - fraction * from)
        }
        return quantized(result, like: number)
    }

    /// `[[x, y], …]`, each point a position on its own axis.
    private static func mappedPoints(
        _ value: Any, mode: GeometryMode, recorded: (width: Int, height: Int),
        live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        guard let points = value as? [Any] else { return .success(value) }
        var out: [Any] = []
        out.reserveCapacity(points.count)
        for point in points {
            guard let pair = point as? [Any], pair.count == 2,
                  let px = ActionJSON.number(pair[0]), let py = ActionJSON.number(pair[1])
            else {
                return .failure(
                    ActionError("a remapped polyline needs [[x, y], …] pairs of numbers"))
            }
            out.append([
                mapped(px, field: .x, mode: mode, recorded: recorded, live: live),
                mapped(py, field: .y, mode: mode, recorded: recorded, live: live),
            ])
        }
        return .success(out)
    }

    /// `[x, y]`: one position.
    private static func mappedPair(
        _ value: Any, key: String, mode: GeometryMode, recorded: (width: Int, height: Int),
        live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        if value is NSNull { return .success(value) }
        guard let pair = value as? [Any], pair.count == 2,
              let px = ActionJSON.number(pair[0]), let py = ActionJSON.number(pair[1])
        else {
            return .failure(ActionError("a remapped \"\(key)\" needs [x, y] numbers"))
        }
        return .success([
            mapped(px, field: .x, mode: mode, recorded: recorded, live: live),
            mapped(py, field: .y, mode: mode, recorded: recorded, live: live),
        ])
    }

    /// `{"x", "y", "width", "height"}`: a position and an extent. Any other
    /// key is kept as it is.
    private static func mappedRect(
        _ value: Any, key: String, mode: GeometryMode, recorded: (width: Int, height: Int),
        live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        if value is NSNull { return .success(value) }
        guard var rect = value as? [String: Any],
              let x = ActionJSON.number(rect["x"]), let y = ActionJSON.number(rect["y"]),
              let width = ActionJSON.number(rect["width"]),
              let height = ActionJSON.number(rect["height"])
        else {
            return .failure(
                ActionError(
                    "a remapped \"\(key)\" needs an object with \"x\", \"y\", \"width\" and "
                        + "\"height\" numbers"))
        }
        rect["x"] = mapped(x, field: .x, mode: mode, recorded: recorded, live: live)
        rect["y"] = mapped(y, field: .y, mode: mode, recorded: recorded, live: live)
        rect["width"] = mapped(width, field: .w, mode: mode, recorded: recorded, live: live)
        rect["height"] = mapped(height, field: .h, mode: mode, recorded: recorded, live: live)
        return .success(rect)
    }

    /// `[x, y, width, height]`: a position and an extent.
    private static func mappedRectArray(
        _ value: Any, key: String, mode: GeometryMode, recorded: (width: Int, height: Int),
        live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        if value is NSNull { return .success(value) }
        guard let rect = value as? [Any], rect.count == 4,
              let x = ActionJSON.number(rect[0]), let y = ActionJSON.number(rect[1]),
              let width = ActionJSON.number(rect[2]), let height = ActionJSON.number(rect[3])
        else {
            return .failure(
                ActionError("a remapped \"\(key)\" needs [x, y, width, height] numbers"))
        }
        return .success([
            mapped(x, field: .x, mode: mode, recorded: recorded, live: live),
            mapped(y, field: .y, mode: mode, recorded: recorded, live: live),
            mapped(width, field: .w, mode: mode, recorded: recorded, live: live),
            mapped(height, field: .h, mode: mode, recorded: recorded, live: live),
        ])
    }

    /// `[a, b, c, d, tx, ty]`, remapped about the step's ORIGINAL recorded
    /// `pivot` when it has one and conjugated by the coordinate remap when it
    /// does not (both rules are on `GeometryMode`). Remapped entries are plain
    /// Doubles, never quantized or rounded; an entry the mode leaves alone
    /// keeps its recorded number.
    private static func mappedAffine(
        _ value: Any, key: String, pivot pivotValue: Any?, mode: GeometryMode,
        recorded: (width: Int, height: Int), live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        if value is NSNull { return .success(value) }
        var pivot: (x: Double, y: Double)?
        if let pivotValue, !(pivotValue is NSNull) {
            guard let pair = pivotValue as? [Any], pair.count == 2,
                  let px = ActionJSON.number(pair[0]), let py = ActionJSON.number(pair[1])
            else {
                return .failure(ActionError("a remapped \"pivot\" needs [x, y] numbers"))
            }
            pivot = (px.doubleValue, py.doubleValue)
        }
        let numbers = (value as? [Any])?.compactMap { ActionJSON.number($0) } ?? []
        guard (value as? [Any])?.count == 6, numbers.count == 6,
              numbers.allSatisfy({ $0.doubleValue.isFinite })
        else {
            return .failure(
                ActionError("a remapped \"\(key)\" needs six numbers [a, b, c, d, tx, ty]"))
        }
        let a = numbers[0].doubleValue, b = numbers[1].doubleValue
        let c = numbers[2].doubleValue, d = numbers[3].doubleValue
        let tx = numbers[4].doubleValue, ty = numbers[5].doubleValue
        switch mode {
        case .absolute:
            return .success(value)
        case .relative:
            let sx = Double(live.width) / Double(recorded.width)
            let sy = Double(live.height) / Double(recorded.height)
            if let pivot {
                return .success([
                    numbers[0], numbers[1], numbers[2], numbers[3],
                    tx * sx + c * (sx - sy) * pivot.y, ty * sy + b * (sy - sx) * pivot.x,
                ] as [Any])
            }
            return .success([
                numbers[0], b * sy / sx, c * sx / sy, numbers[3], tx * sx, ty * sy,
            ] as [Any])
        case .anchor(let anchor):
            let ox = anchor.fractions.x * Double(live.width - recorded.width)
            let oy = anchor.fractions.y * Double(live.height - recorded.height)
            return .success([
                numbers[0], numbers[1], numbers[2], numbers[3],
                tx + ox - (a * ox + c * oy), ty + oy - (b * ox + d * oy),
            ] as [Any])
        }
    }

    /// A guide position, on the axis its ORIENTATION names. `add_guide`
    /// carries the orientation and the position as siblings; `remove_guide`
    /// carries both inside its `$guide` symbol, and the rewritten symbol goes
    /// back where it was so ordinary resolution still finds the guide.
    private static func mappedGuide(
        key: String, value: Any, arguments: [String: Any], mode: GeometryMode,
        recorded: (width: Int, height: Int), live: (width: Int, height: Int)
    ) -> Result<Any, ActionError> {
        if let number = ActionJSON.number(value) {
            guard let orientation = arguments["orientation"] as? String else {
                return .failure(
                    ActionError("a remapped guide needs its \"orientation\" to know its axis"))
            }
            return .success(
                mapped(
                    number, field: orientation == "horizontal" ? .y : .x, mode: mode,
                    recorded: recorded, live: live))
        }
        guard let symbol = value as? [String: Any],
              var payload = symbol["$guide"] as? [String: Any],
              let orientation = payload["orientation"] as? String,
              let position = ActionJSON.number(payload["position"])
        else {
            return .failure(
                ActionError(
                    "a remapped \"\(key)\" must be a number or a $guide symbol carrying "
                        + "\"orientation\" and \"position\""))
        }
        payload["position"] = mapped(
            position, field: orientation == "horizontal" ? .y : .x, mode: mode,
            recorded: recorded, live: live)
        return .success(["$guide": payload])
    }

    /// The remapped number in the shape the recorded one had: an INTEGER
    /// argument stays an integer (`crop` takes whole pixels and a `1279.5`
    /// would be refused by the tool), anything else goes through the
    /// four-decimal quantization every recorded coordinate already uses.
    private static func quantized(_ value: Double, like number: NSNumber) -> Any {
        guard value.isFinite else { return number }
        guard wasInteger(number) else { return ActionArgs.number(value) }
        let rounded = value.rounded()
        guard let integer = Int(exactly: rounded) else { return ActionArgs.number(value) }
        return integer
    }

    /// True when the recorded value was written as a whole number rather than
    /// as a decimal. `objCType` is the direct question — `JSONSerialization`
    /// produces `q` for an integer literal and `d` for one with a decimal
    /// point — where `v == v.rounded()` would silently turn a recorded `40.0`
    /// into an `Int` and change the file on the next save.
    private static func wasInteger(_ number: NSNumber) -> Bool {
        let type = String(cString: number.objCType)
        return type != "d" && type != "f"
    }

    private static func integer(_ value: Any?) -> Int? {
        guard let number = ActionJSON.number(value) else { return nil }
        return Int(exactly: number.doubleValue.rounded())
    }
}
