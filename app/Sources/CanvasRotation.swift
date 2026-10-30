import AppKit

// The core plan owns classification and expanded extents; exact descriptions compose even when raw
// stored geometry is unchanged, and MCP returns `changed:false` without undo, dirtying or recording
// for a complete stored-state no-op. Float filtered rotation and retained-source whole-document
// rotation still refuse.

/// Phase 12F-1b3's host side of ARBITRARY CANVAS ROTATION — Image ▸ Rotate
/// Arbitrary…, the MCP `rotate` tool's non-quarter angles, and the recorded
/// `rotate` step that replays either.
///
/// **The core owns both rules.** `RasterDocument.canvasRotationPlan` calls
/// `dr_canvas_rotation_plan`, which classifies the angle (identity / an exact
/// quarter turn / filtered) and sizes the expanded canvas; nothing on this
/// side normalizes an angle, decides what "exact" means or computes a
/// rectangle. That matters because the same plan has to be read from three
/// places that share no state: the sheet (which has no edit yet), the MCP
/// handler (before it opens an undo group) and `ActionGeometry.canvasAfter`
/// (which has only a recorded canvas SIZE and no document at all).
///
/// **A quarter turn is delegated TWICE.** The core answers `rotate90()` /
/// `rotate180()` / `rotate270()` verbatim, and the host runs today's
/// `applyingDocumentGeometry(op)` path instead of the new op — so a text
/// or shape description COMPOSES with the turn and stays editable,
/// and the recorded Action step keeps its INTEGER `rotate {degrees: 90}`
/// form. A filtered angle cannot do either: it resamples, so every described
/// layer is rasterized inside the same edit.
struct CanvasRotationPlan {
    enum Kind: Equatable {
        /// A whole number of full turns: a successful no-op at every depth.
        case identity
        /// An exact quarter turn, carrying the existing whole-document
        /// geometry op that IS it.
        case quarter(DocumentGeometry)
        /// Anything else: the canvas expands and every plane resamples.
        case filtered

        /// Hand-written because `DocumentGeometry` is not `Equatable` and
        /// making it so would be a change to a file this feature has no
        /// other business in. Only the three quarter turns ever reach the
        /// `.quarter` case, and their action names are distinct.
        static func == (lhs: Kind, rhs: Kind) -> Bool {
            switch (lhs, rhs) {
            case (.identity, .identity), (.filtered, .filtered): return true
            case let (.quarter(left), .quarter(right)):
                return left.actionName == right.actionName
            default: return false
            }
        }
    }

    let kind: Kind
    /// The angle NORMALIZED into (−180, 180], as the core reports it.
    let degrees: Double
    /// The new canvas's rectangle in OLD-canvas coordinates. `width == 0`
    /// means a filtered angle whose expanded canvas the core refused.
    let left: Int
    let top: Int
    let width: Int
    let height: Int

    /// Where the OLD canvas's top-left lands in the new canvas — the origin
    /// the guides, the ruler and every offset move by.
    var origin: (x: Int, y: Int) { (x: -left, y: -top) }

    /// The old-canvas → new-canvas map of this rotation of a `canvasWidth ×
    /// canvasHeight` canvas: a turn by `degrees` about the old canvas's
    /// centre, then the shift into the expanded canvas. The map the rotation
    /// hands `ArtworkGeometryWork`, and the one the selection is carried by
    /// (SelectionCanvasMap.swift).
    func map(canvasWidth: Int, canvasHeight: Int) -> CGAffineTransform {
        let radians = degrees * .pi / 180
        let cosine = cos(radians), sine = sin(radians)
        let cx = Double(canvasWidth) / 2, cy = Double(canvasHeight) / 2
        return CGAffineTransform(a: cosine, b: sine, c: -sine, d: cosine,
            tx: cx - cosine * cx + sine * cy - Double(left),
            ty: cy - sine * cx - cosine * cy - Double(top))
    }
}

enum CanvasRotation {
    /// The ONE sentence a `degrees` that cannot be read as a finite number
    /// carries, at 8 bits, at 16 and at float alike.
    static let unreadable =
        "degrees must be a finite number of degrees; positive is clockwise"

    /// The plan for this request against `doc`'s current canvas, or nil when
    /// `degrees` is present but unreadable (`DocumentPrecision.rotateDegrees`
    /// answers `.nan`, which no canvas can be rotated by).
    static func plan(_ arguments: [String: Any], in doc: RasterDocument) -> CanvasRotationPlan? {
        RasterDocument.canvasRotationPlan(
            width: doc.width, height: doc.height,
            degrees: DocumentPrecision.rotateDegrees(arguments))
    }

    /// Whether this request would RESAMPLE. The two depth gates ask exactly
    /// this — never "is the angle 90" — so an unreadable angle falls THROUGH
    /// the gates and is refused once, by `unreadable`, in the handler.
    static func isFiltered(_ arguments: [String: Any], in doc: RasterDocument) -> Bool {
        plan(arguments, in: doc)?.kind == .filtered
    }
}

extension RasterDocument {
    /// The ONE host composition behind Image ▸ Rotate Arbitrary… and the MCP
    /// `rotate` tool's filtered angles. Editable artwork is captured before
    /// work and regenerated from its description after the core rotation.
    ///
    /// **PRECONDITION: a FILTERED angle.** An identity is answered as a
    /// successful no-op and a quarter turn runs `applyingDocumentGeometry`
    /// before anything calls here, because both of those keep descriptions
    /// alive and this one cannot.
    ///
    /// nil only when the core answers the identity (which the precondition
    /// excludes); a refusal is thrown with the core's own sentence and a
    /// Cancel as `JobCancelled`. Main thread only: the rotation, the artwork
    /// re-render and the display preparation run as ONE background job
    /// (`performingCanvasGeometry`), and the caller publishes the
    /// prepared edit. On an Indexed document an uncovered corner takes the
    /// entry nearest `indexedBackground` when there is no transparent index
    /// (#1 Indexed phase 3b); nil is white.
    func rotatingCanvas(degrees: Double, sampler: DrResizeFilter, described: [Int],
                        indexedBackground: NSColor? = nil) throws
        -> PreparedDocumentEdit?
    {
        guard let plan = RasterDocument.canvasRotationPlan(
            width: width, height: height, degrees: degrees),
              plan.kind == .filtered else {
            throw DoctoredCoreError(message: "A filtered canvas rotation is required.")
        }
        // The plan's own angle is `degrees` normalized, the same rotation.
        let map = plan.map(canvasWidth: width, canvasHeight: height)
        let artwork = try ArtworkGeometryWork(source: self, affineMap: map,
                                              canvas: (plan.width, plan.height))
        let command = try IndexedDocument.backgrounded(
            .rotateCanvas(degrees: degrees, filter: Self.precisionFilter(sampler)),
            indexedBackground, in: self)
        return try performingCanvasGeometry(
            command,
            title: "Rotate Canvas", artwork: artwork,
            dropping: described.filter { !hasArtworkDescription($0) })
    }
}
