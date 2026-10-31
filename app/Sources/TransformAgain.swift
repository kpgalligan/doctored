import AppKit

// `ImageDocument.lastFreeTransform` (a `RepeatableTransform`: the canvas-space matrix, pivot
// included, and the sampler — or a true perspective's `Perspective(sourceRect, quad)`) is a
// view of the ONE application-wide `TransformAgainMemory`, never persisted or undone; it is
// assigned only after a PUBLISHED change:
// `commitTransformSession()` wraps `commitTransformSessionEdit()` and stores the session (a
// ⌘-corner parallelogram as its affine; a true perspective as its corners), and MCP stores it after
// `performTransformJob`, `transformDescribedLayer`, `transform_layers` (both branches) and
// `commitSelectedPixelsTransform` publish.
//
// A perspective memory re-sends the same rectangle and corners (the core re-solves the same canvas
// homography; no Swift homography), takes ⌘T's true-perspective prompts (MCP: `rasterize_artwork`,
// `AgentServer.truePerspectiveRefusal`), skips the 8-bit described compose, records
// `rasterize_artwork: true`, and a layer or copy target past its horizon refuses with the one
// sentence `RepeatableTransform.perspectiveRefusal` (`replayRefusal`, a geometry-plan preflight);
// replies carry `appliedFields`.

/// Layer ▸ Transform Again (⇧⌘T) and Transform Again with Copy (⌥⇧⌘T):
/// the application remembers the last committed Free Transform and
/// re-applies it as ONE undo step. MCP `transform_again {duplicate}` is the twin
/// (`AgentServer+TransformAgain.swift`); the controller half is
/// `EditorViewController+TransformAgain.swift`.
///
/// Decisions stated here and in the feature reference:
/// 1. What gets recorded. The memory is the canvas-space matrix, pivot
///    included, plus the sampler — or, for a TRUE PERSPECTIVE, its
///    four corners over the source rectangle they were dragged from — and it
///    is assigned only after a PUBLISHED change, with the canvas size of the
///    document it was committed in. A cancel, a refusal, an identity commit
///    or a `changed: false` leaves it untouched; undo/redo, history,
///    snapshots, recovery, `.rz`, Warp, Transform Selection, Transform Mask,
///    canvas Rotate/Flip/Crop/Size and closing a document never touch it. The UI
///    stores it in `commitTransformSession()` — every ⌘T and ⌥⌘T commit at
///    both depths, over layers or selected pixels; MCP stores it after
///    `transform_layer`, `transform_layers`, `distort_layer` and
///    `transform_selected_pixels` publish. A ⌘-corner parallelogram stores its
///    affine; a true perspective stores `Perspective(sourceRect, quad)`.
/// 2. How it is re-applied. The matrix is applied VERBATIM in
///    canvas space: no pivot, box or bounds is re-derived from the new target.
///    ⌘T's M = T(P+t)·R·S·T(−P), so repeating it on the same content (or on
///    its copy) gives M·M — a 15° turn about P becomes 30° about P, a 10 px
///    move becomes 20 px, and 90% + 15° about a moved reference point spirals
///    into that point. A different target turns about the same canvas point.
///    A perspective re-sends the SAME rectangle and corners to the core, which
///    re-solves the same canvas homography H and maps each target's whole
///    buffer, float or selection through it: H verbatim in canvas space, so
///    the same content gives H·H: Again repeats Distort and Perspective. No
///    homography is solved here and no convexity re-checked (the quad was
///    convex when stored); a target the map folds past its horizon refuses
///    with a sentence and changes nothing. The stored sampler is used; the
///    options bar's sampler is ignored.
/// 3. Target, as ⌘T picks it. With a selection: the active layer's selected
///    pixels (with ⌥, a pixel copy). Without one: the expanded layer
///    selection (with ⌥, a layer copy, the copies selected after
///    publication). The step is named `actionName(duplicate:)`; no session
///    opens.
/// 4. Scope and availability (measured in Photoshop 26.10.0, takes 1–3
///    of `scripts/photoshop-probe/transform_again_after_edits.py`). The memory
///    is ONE for the application (`TransformAgainMemory`), not per document:
///    a transform committed in any document replaces it, and closing a
///    document keeps it. It repeats only in a document whose canvas is the
///    size it was committed on, and it is never remapped: an edit that
///    changes the canvas size makes it unavailable, undoing that edit (or
///    sizing the canvas back) makes it available again, and Rotate 180°, the
///    flips and a quarter turn of a square canvas keep it. A canvas of
///    another size refuses with `sizeRefusal`; Straighten follows Crop at an
///    angle and Rotate Arbitrary follows a 30° rotation (both resize).
struct RepeatableTransform: Equatable {
    static let actionName = "Transform Again"
    static let copyActionName = "Transform Again with Copy"
    static let emptyRefusal = "There is no Free Transform to repeat: none has been committed yet."
    /// A layer or copy replay whose perspective the core refused (a raster
    /// the map folds past its horizon), in place of the core's own sentence.
    static let perspectiveRefusal = "Transform Again can't repeat this perspective on the "
        + "layer: it would fold the layer's pixels past the horizon."
    /// The core's geometric refusal wording — the only plan errors
    /// `replayRefusal` rewrites into `perspectiveRefusal`: the byte
    /// `mapped_bounds` sentence (doc_source_geometry.rs), the byte set's
    /// (doc_geometry_set.rs) and the typed plan's and set's
    /// (precision_geometry_plan.rs, precision_geometry_set.rs). Every other
    /// plan error (admission or resource limits) keeps its own; a commit's
    /// plan carries no pixel-work refusal.
    static let geometricRefusalMarkers = ["horizon", "exceeds supported bounds"]

    /// Whether a core plan error is a geometric refusal (`geometricRefusalMarkers`).
    static func isGeometricRefusal(_ message: String) -> Bool {
        geometricRefusalMarkers.contains { message.contains($0) }
    }

    /// A true perspective: the corners the source rectangle was dragged to,
    /// both in canvas space.
    struct Perspective: Equatable {
        let sourceRect: CGRect
        let quad: [CGPoint]
    }

    /// Canvas space, pivot included (`LayerTransform.matrix`'s space);
    /// `.identity` for a perspective memory.
    let matrix: CGAffineTransform
    /// nil for an affine memory.
    let perspective: Perspective?
    let sampler: DrResizeFilter

    init(matrix: CGAffineTransform, sampler: DrResizeFilter) {
        self.matrix = matrix
        self.perspective = nil
        self.sampler = sampler
    }

    init(perspective: Perspective, sampler: DrResizeFilter) {
        self.matrix = .identity
        self.perspective = perspective
        self.sampler = sampler
    }

    /// quad nil → `matrix`; a parallelogram quad → `LayerTransform.
    /// parallelogramAffine(of: sourceRect, onto: quad)`; any other quad → a
    /// `Perspective`. nil only for a quad with no rect or not four points.
    init?(matrix: CGAffineTransform, quad: [CGPoint]?, sourceRect: CGRect?,
          sampler: DrResizeFilter) {
        guard let quad else {
            self.init(matrix: matrix, sampler: sampler)
            return
        }
        guard let sourceRect, quad.count == 4 else { return nil }
        if let affine = LayerTransform.parallelogramAffine(of: sourceRect, onto: quad) {
            self.init(matrix: affine, sampler: sampler)
        } else {
            self.init(perspective: Perspective(sourceRect: sourceRect, quad: quad),
                      sampler: sampler)
        }
    }

    /// A Free Transform session's commit: the warped quad iff the box has
    /// corner offsets, else its matrix.
    init?(session transform: LayerTransform, sourceRect: CGRect, sampler: DrResizeFilter) {
        let quad = transform.hasCornerOffsets ? transform.warpedQuad(of: sourceRect) : nil
        self.init(matrix: transform.matrix, quad: quad, sourceRect: sourceRect, sampler: sampler)
    }

    /// The refusal for a canvas of another size than the remembered one.
    static func sizeRefusal(committed: CGSize, canvas: CGSize) -> String {
        "Transform Again repeats the last Free Transform only on a canvas the size it was "
            + "committed on (\(Int(committed.width)) × \(Int(committed.height)) px); this canvas "
            + "is \(Int(canvas.width)) × \(Int(canvas.height)) px."
    }

    static func actionName(duplicate: Bool) -> String {
        duplicate ? copyActionName : actionName
    }

    /// The perspective's corners; nil for an affine memory.
    var quad: [CGPoint]? { perspective?.quad }

    /// The rectangle a replay maps from: the perspective's own, else `box`.
    func sourceRect(over box: CGRect) -> CGRect {
        perspective?.sourceRect ?? box
    }

    /// `[a, b, c, d, tx, ty]`, `transform_layers`' `matrix` spelling.
    var matrixArray: [Double] {
        [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init)
    }

    /// What a reply reports as applied: `matrix`, or a perspective's
    /// `source_rect` and `corners` (`transform_layers`' spelling).
    var appliedFields: [String: Any] {
        guard let perspective else { return ["matrix": matrixArray] }
        let rect = perspective.sourceRect
        return [
            "source_rect": [rect.minX, rect.minY, rect.width, rect.height].map(Double.init),
            "corners": perspective.quad.map { [Double($0.x), Double($0.y)] },
        ]
    }

    static func == (lhs: RepeatableTransform, rhs: RepeatableTransform) -> Bool {
        lhs.matrix == rhs.matrix && lhs.perspective == rhs.perspective
            && lhs.sampler == rhs.sampler
    }
}

extension RepeatableTransform {
    /// A perspective replay over `layers` of `source` that the core's geometry
    /// plan refuses. A geometric refusal (a raster the map folds past its
    /// horizon, whose destination the core then cannot bound) becomes ONE
    /// Transform Again sentence (`perspectiveRefusal`) in place of the core's;
    /// any other plan error answers its own message — the layer and copy
    /// arms' refusal, in the UI and over MCP. nil for an affine memory
    /// and for a plan the core admits.
    func replayRefusal(source: RasterDocument, layers: [Int]) -> String? {
        guard let perspective else { return nil }
        let request = GeometrySetRequest.perspective(
            layers: layers, sourceRect: perspective.sourceRect, quad: perspective.quad,
            sampler: sampler)
        do {
            _ = try source.geometryEditPlan(request: request.command().request)
            return nil
        } catch {
            let message = error.localizedDescription
            return Self.isGeometricRefusal(message) ? Self.perspectiveRefusal : message
        }
    }
}

/// The ONE Transform Again memory, shared by every document (decision 4
/// above): the last committed Free Transform and the canvas size of the
/// document it was committed in. Main thread only, as every writer and reader
/// is; never persisted and never undone. `ImageDocument.lastFreeTransform` is
/// each document's view of it.
struct TransformAgainMemory {
    let transform: RepeatableTransform
    let canvasSize: CGSize

    /// nil until the first commit of the session.
    static var last: TransformAgainMemory?

    /// The memory a canvas of `size` may repeat: nil with none, or when it
    /// was committed on a canvas of another size.
    static func repeatable(on size: CGSize) -> RepeatableTransform? {
        guard let last, last.canvasSize == size else { return nil }
        return last.transform
    }

    /// Why a canvas of `size` has nothing to repeat: `emptyRefusal` with no
    /// memory, else `sizeRefusal`.
    static func refusal(on size: CGSize) -> String {
        guard let last else { return RepeatableTransform.emptyRefusal }
        return RepeatableTransform.sizeRefusal(committed: last.canvasSize, canvas: size)
    }
}
