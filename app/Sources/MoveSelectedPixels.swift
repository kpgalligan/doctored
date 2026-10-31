import AppKit

// When a selection is active, the Move drag and arrow nudge (and `move_selected_pixels`) float the
// ACTIVE layer's pixels × the selection by an integer delta through the core's `move_selected`
// request at 8 and 16 bits and at 32-bit float, and `applyEdit(resultingSelection:)` moves
// the selection in the same "Move" step.
//
// `MoveSelectedPixels.prepared` refuses a stale `base`/`selection` before and after its worker.
//
// Every Move Selected request carries the selection's integral `bounds`, so each 8-bit
// preview request plans itself over the selection.
//
// `MoveSelectedPixels.run` admits the pixel edit alone, or `max(core, selection)` for a
// floating-transform commit whose selection stage re-admits itself on the worker, with no
// `W·H·24` display term (`PreparedDocumentEdit` admits its own).
//
// With a LAYER MASK or ALPHA CHANNEL as the edit target the drag, the nudge and MCP move
// that plane's selected coverage (`target`, `background`), exactly as ⌘T does: the
// vacated area takes the background swatch as coverage gray, at 8, 16 and 32-bit float.
// `planeRefusal` is the refusal both share; Quick Mask refuses.
//
// The RED, GREEN, BLUE or ALPHA plane takes the same plane branch with the layer path's lock
// alert (`planeLockKind`: Pixels, Alpha a merge edit) and rasterize prompt; the vacated area of
// R/G/B is the background swatch's 30/59/11 luma rounded to a code (the request's `swatch`),
// Alpha's transparent,
// at 8, 16 and 32-bit float. A lift from only transparent pixels refuses with
// `colorPlaneEmptyRefusal`.
//
// #1: on a CMYK document ONE INK PLATE (`PaintTarget.ink`, an ink row of the Channels panel or
// `"target":"cyan"` …) takes the same plane branch: the Pixels lock, `inkRefusal`, and the vacated
// area the background swatch's 30/59/11 gray read as ink (its sRGB codes as `"swatch"`).
// #1 Lab 6a-ii: a Lab document's Lightness, a, b and alpha planes take the same plane branch, the
// vacated value the background's code in that plane (`TransformSelectedPixels.planeBackground`).

/// Move Selected Pixels: the Move tool's drag and arrow nudge, and
/// `move_selected_pixels`, with a selection active. ONE pixel layer — the
/// active one — floats its pixels × the selection coverage, moves them by an
/// integer delta, leaves an `a·(1−c)` hole, composites back source-over, and
/// the selection moves with them, all in ONE undo step ("Move"). The core op
/// is `SelectedPixelsEdit` (`core/src/selected_pixels.rs`), the same request
/// at 8 and 16 bits, widened to Free Transform.
///
/// Decisions stated here and in the feature reference: a multi-layer panel selection
/// refuses by name, links are ignored, consecutive gestures and
/// nudges over one selection move ONE float lifted once
/// (`MoveFloat.swift`: each step is still one undo step, and pixels the
/// float passed over come back), and the hole is transparent, at 8, 16
/// and 32-bit float (f32, nothing quantized). ⌥ held at the PRESS
/// (latched, like the marquee modifiers) makes the drag a duplicate:
/// no hole, the copy lands source-over, the selection moves with
/// the copy, one "Duplicate" step.
/// An empty float refuses on the drag's first tick, as soon as the
/// background float query answers, rather than at mouse-up.
enum MoveSelectedPixels {
    static let actionName = "Move"
    static let duplicateActionName = "Duplicate"
    static let staleSentence = "The document changed while the move ran. Move again."
    static let emptyRefusal = "The selection holds no pixels to move."
    /// The core's `COLOR_EMPTY_AREA` (`selected_color_plane.rs`), verbatim —
    /// the sentence for a colour-channel lift with nothing in it.
    static let colorPlaneEmptyRefusal = "The selection holds no pixels in this colour channel."

    /// The shared core request: `{"op":"move_selected","layer":N,"dx":…,"dy":…}`,
    /// plus `"duplicate":true` for the ⌥-drag and the selection's integral
    /// `bounds` (the core plans over the selection, not the layer). A
    /// mask or channel target adds `"target"` and `"background"`, a
    /// colour plane `"target"` and its `"swatch"`; a layer target
    /// sends neither.
    static func command(layer: Int, dx: Int, dy: Int, duplicate: Bool, bounds: CGRect?,
                        target: PaintTarget = .layer,
                        background: PlaneBackground? = nil) throws -> PrecisionPixelCommand {
        var fields: [String: Any] = ["op": "move_selected", "layer": layer, "dx": dx, "dy": dy]
        if duplicate { fields["duplicate"] = true }
        addPlaneTarget(&fields, target: target, background: background)
        if let bounds { fields["bounds"] = requestBounds(bounds) }
        return try PrecisionPixelCommand(fields)
    }

    /// A plane target's request fields, shared with ⌘T's plane transform:
    /// `"target":"mask"` or `{"channel":N}`, and for those the `"background"`
    /// coverage gray when one is given; a colour plane names itself
    /// ("red" … "alpha") and, for R/G/B, sends the background's `"swatch"`
    /// codes. A layer target writes nothing.
    static func addPlaneTarget(_ fields: inout [String: Any], target: PaintTarget,
                               background: PlaneBackground?) {
        switch target {
        case .mask: fields["target"] = "mask"
        case .channel(let index): fields["target"] = ["channel": index]
        case .plane(let plane): fields["target"] = plane.agentName
        case .ink(let plate): fields["target"] = plate.key
        case .layer: break
        }
        if target == .mask || target.isChannel, let background {
            fields["background"] = Int(background.gray)
        }
        if target.isPlane || target.isInk, let swatch = background?.swatch {
            fields["swatch"] = swatch.map(Int.init)
        }
    }

    /// The lock kind a colour-plane edit asks, the core's own — Pixels
    /// for Red, Green and Blue, Merge for Alpha (it writes the layer's
    /// transparency); nil for any other target.
    static func planeLockKind(_ target: PaintTarget) -> DrEditKind? {
        // #1: one ink plate writes the layer's pixels, as Red, Green and Blue do.
        if target.isInk { return DR_EDIT_PIXELS }
        guard case .plane(let plane) = target else { return nil }
        return plane == .alpha ? DR_EDIT_MERGE : DR_EDIT_PIXELS
    }

    /// `CanvasSelection.bounds` (integral, clamped to the canvas) as the
    /// core request's `[x, y, w, h]`.
    static func requestBounds(_ bounds: CGRect) -> [Int] {
        [Int(bounds.minX), Int(bounds.minY), Int(bounds.width), Int(bounds.height)]
    }

    /// The multi-layer + marquee refusal. ⌘T calls it with verb "transform".
    static func multiLayerRefusal(verb: String, count: Int) -> String {
        "You can \(verb) the selected pixels of only one layer at a time, and \(count) layers "
            + "are selected. Select one layer in the Layers panel, or Deselect (⌘D) to \(verb) "
            + "the layers whole."
    }

    /// Why moving `layer`'s selected pixels is refused, in order: Quick Mask,
    /// a multi-layer panel selection (skipped when `panelSelection` is false —
    /// an MCP call naming its layer explicitly), a group or
    /// adjustment, a non-pixel target, then the shared pixel-only scope (the
    /// retained source and the lock). Free Transform of the selected
    /// pixels asks the same with `verb` "transform" / `gerund` "Transforming".
    static func refusal(_ document: ImageDocument, layer: Int, target: PaintTarget,
                        panelSelection: Bool = true, verb: String = "move",
                        gerund: String = "Moving") -> String? {
        guard let doc = document.doc else { return "No image loaded." }
        if document.quickMaskActive { return OperationScope.quickMaskRefusal }
        if panelSelection, document.layerSelection.isMultiple {
            return multiLayerRefusal(verb: verb, count: document.layerSelection.count)
        }
        let name = doc.layerInfo(layer)?.name ?? "this layer"
        if doc.layerIsGroup(layer) || doc.layerIsAdjustment(layer) {
            let kind = doc.layerIsGroup(layer) ? "a group" : "an adjustment layer"
            return "\(gerund) selected pixels needs a pixel layer, and “\(name)” is \(kind). "
                + "Deselect (⌘D) to \(verb) it whole."
        }
        if target != .layer {
            return "\(gerund) selected pixels works on layer pixels only. Target the layer's "
                + "pixels (not its mask or a channel), or Deselect (⌘D) to \(verb) the whole "
                + "layer."
        }
        return OperationScope.refusalReason(.pixelOnly, target: target, doc: doc, layerIndex: layer)
    }

    /// Why moving or transforming the selected area of a mask,
    /// channel or colour-plane `target` is refused, in order: Quick
    /// Mask, a multi-layer panel selection on a mask or a colour plane
    /// (skipped when `panelSelection` is false), then the shared fill scope
    /// (mask presence, locks, channel existence; a colour plane's retained
    /// source, group or adjustment layer, Pixels lock and, for Alpha, Lock
    /// Transparency).
    static func planeRefusal(_ document: ImageDocument, layer: Int, target: PaintTarget,
                             panelSelection: Bool = true, verb: String,
                             subject: String) -> String? {
        guard let doc = document.doc else { return "No image loaded." }
        if document.quickMaskActive { return OperationScope.quickMaskRefusal }
        if target == .mask || target.isPlane || target.isInk, panelSelection,
           document.layerSelection.isMultiple {
            return multiLayerRefusal(verb: verb, count: document.layerSelection.count)
        }
        if case .ink(let plate) = target { return inkRefusal(doc, layer: layer, plate: plate) }
        return OperationScope.refusalReason(.fill, target: target, doc: doc, layerIndex: layer)
    }

    /// #1: why one ink plate's selected pixels cannot move — not a CMYK document, a group or
    /// adjustment record, a retained source, then the Pixels lock (the core's own order,
    /// `cmyk_selected_plate`). Painting one plate refuses elsewhere (`OperationScope`).
    static func inkRefusal(_ doc: RasterDocument, layer: Int, plate: CmykPlane) -> String? {
        guard doc.isCmyk else { return "The \(plate.key) channel requires a CMYK document." }
        guard let info = doc.layerInfo(layer) else { return "The target layer no longer exists." }
        if info.isGroup || doc.layerIsAdjustment(layer) {
            return "Moving one plate's selected pixels needs a pixel layer, and “\(info.name)” is "
                + (info.isGroup ? "a group." : "an adjustment layer.")
        }
        if doc.hasRetainedSource(layer) { return RetainedImagePolicy.rasterizeFirst }
        return doc.lockRefusal(layer, kind: DR_EDIT_PIXELS)?.message
    }

    /// The recorded step; a plane move adds its `target` and
    /// `background` hex (a colour plane: its background swatch's hex,
    /// none for Alpha).
    static func record(_ doc: RasterDocument, layer: Int, dx: Int, dy: Int, duplicate: Bool,
                       target: PaintTarget = .layer, background: String? = nil) -> [ActionStep] {
        let plane = target != .layer
        return .moveSelectedPixelsStep(
            layer: doc.layerRef(layer), dx: dx, dy: dy, duplicate: duplicate,
            target: plane ? target.agentName(in: doc) : nil,
            background: plane && target != .plane(.alpha) ? background : nil)
    }

    /// The 8-bit commit shared by the drag, the nudge and MCP. Refuses with
    /// `stale` unless `base` and `selection` are still the document's and no
    /// live edit is open — the drag no longer holds a live edit, so an MCP
    /// edit landing between press and release would otherwise be overwritten.
    /// Plans, admits and runs `editingSelectedPixels` in a `WorkerTask` under
    /// `BackgroundWork.wait`, then refuses again if the document changed.
    /// nil = identity.
    static func prepared(_ document: ImageDocument, source: RasterDocument, request: String,
                         coverage: [UInt16], title: String, base: RasterDocument,
                         selection: SelectionHistoryState,
                         stale: String = staleSentence) throws -> PreparedDocumentEdit? {
        try run(document, source: source, request: request, coverage: coverage, title: title,
                base: base, selection: selection, stale: stale, transformedSelection: nil)?.edit
    }

    /// The floating-transform commit: the same, with the transformed
    /// selection computed in the SAME job after the pixel edit (one wait,
    /// with Cancel/Esc) and its history state assembled on main. nil = the
    /// pixel edit changed nothing; the selection is then never computed.
    static func prepared(_ document: ImageDocument, source: RasterDocument, request: String,
                         coverage: [UInt16], title: String, base: RasterDocument,
                         selection: SelectionHistoryState, stale: String,
                         transformedSelection: TransformedSelection)
        throws -> (edit: PreparedDocumentEdit, selection: SelectionHistoryState)?
    {
        guard let result = try run(
            document, source: source, request: request, coverage: coverage, title: title,
            base: base, selection: selection, stale: stale,
            transformedSelection: transformedSelection)
        else { return nil }
        return (result.edit, try transformedSelection.state(result.samples))
    }

    /// Both overloads' body. Without `transformedSelection` it is the move's
    /// uncontrolled job; with one, a compound job (`GeometrySet`'s pattern)
    /// whose first child is the pixel edit and second the selection.
    /// Per-stage admission (`CanvasGeometryJob`'s model): the parent is
    /// admitted at the larger of the pixel edit and the selection, the
    /// selection stage re-admits itself on the worker against what is then
    /// live, and `PreparedDocumentEdit` admits its own display.
    private static func run(_ document: ImageDocument, source: RasterDocument, request: String,
                            coverage: [UInt16], title: String, base: RasterDocument,
                            selection: SelectionHistoryState, stale: String,
                            transformedSelection: TransformedSelection?)
        throws -> (edit: PreparedDocumentEdit, samples: [UInt16]?)?
    {
        assert(Thread.isMainThread)
        guard document.doc === base, document.selectionHistory == selection,
              !document.isLiveEditing else {
            throw DoctoredCoreError(message: stale)
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let plan = try source.selectedPixelsPlan(request: request)
        let core = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        guard let transformedSelection else {
            _ = try RasterResources.shared.admit(scratch: core)
            let budget = try base.legacyPrecisionWorkContext()
            let task = try WorkerTask(scratchEstimate: core) {
                guard let moved = try source.editingSelectedPixels(
                    request: request, coverage: coverage, budget: budget)
                else { return nil as (edit: PreparedDocumentEdit, samples: [UInt16]?)? }
                return (PreparedDocumentEdit(moved, scratch: BackgroundWork.scratch), nil)
            }
            let result = try BackgroundWork.wait(task, title: title,
                                                 revealAfter: DocumentJobs.panelDelay)
            guard document.revision == revision, document.doc === base, !document.isLiveEditing,
                  document.isCurrentEditingState(state) else {
                throw DoctoredCoreError(message: stale)
            }
            return result
        }
        let selectionWork = transformedSelection
        let scratch = max(core, selectionWork.scratchEstimate)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let budget = try base.legacyPrecisionWorkContext()
        let control = try RasterJobControl()
        guard control.setScratchLimit(scratch) else {
            throw DoctoredCoreError(message: "Could not bound the selected pixels transform.")
        }
        let task = try WorkerTask(scratchEstimate: scratch) {
            try control.beginCompoundOperation()
            let split = selectionWork.work == nil ? 1.0 : 0.5
            guard let moved = try control.withChildOperation(from: 0, to: split, { child in
                try source.editingSelectedPixels(
                    request: request, coverage: coverage, budget: budget, control: child)
            }) else { return nil as (edit: PreparedDocumentEdit, samples: [UInt16]?)? }
            var samples: [UInt16]?
            if selectionWork.work != nil {
                _ = try RasterResources.shared.admit(scratch: selectionWork.scratchEstimate)
                samples = try control.withChildOperation(from: 0.5, to: 1) { child in
                    try selectionWork.compute(control: child)
                }
            }
            return (PreparedDocumentEdit(moved, scratch: BackgroundWork.scratch), samples)
        }
        let result = try BackgroundWork.wait(task, title: title, control: control,
                                             revealAfter: DocumentJobs.panelDelay)
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.revision == revision, document.doc === base, !document.isLiveEditing,
              document.isCurrentEditingState(state) else {
            throw DoctoredCoreError(message: stale)
        }
        return result
    }
}

/// One Move drag over a selection, from press to release. Captured at the
/// press; nothing is presented or opened until the first non-zero tick, so a
/// click stays inert.
struct MoveSelectedSession {
    let base: RasterDocument
    let layer: Int
    let selection: SelectionHistoryState
    /// ⌥ at the press: the drag duplicates instead of moving.
    let duplicate: Bool
    /// The plane the drag moves — the layer's pixels, its mask or an
    /// alpha channel; set at the press.
    var target: PaintTarget = .layer
    /// A plane target's vacated-area colour, set on the first tick.
    var background: PlaneBackground? = nil
    /// `base`, with the layer's description dropped when `described`; set on
    /// the first tick.
    var source: RasterDocument? = nil
    var coverage: [UInt16] = []
    var budget: (limit: UInt64, other: UInt64)? = nil
    /// The first tick's background float-rect query (both depths): an empty
    /// answer refuses the drag mid-gesture.
    var floatQuery: SelectedPixelsFloatQuery? = nil
    /// 8-bit only; the 16-bit drag uses `precisionMoveGesture`.
    var preview: TransformMaskPreview? = nil
    var described = false
    var opened = false
    var refused = false
    /// The DRAG delta of the last tick.
    var applied = (x: 0, y: 0)
    /// The float this drag continues (set at the press), whose source,
    /// coverage, bounds and background the drag uses; nil = a fresh lift.
    var continuing: (float: MoveFloat, at: Int)? = nil
    /// The float's total at the press; a tick's total is `start` + its drag delta.
    var start = (x: 0, y: 0)
}

extension CanvasSelection {
    /// The selection moved by a whole-pixel delta: geometric shapes offset
    /// exactly, coverage shifts with a zero fill. nil when nothing is left
    /// on the canvas.
    func translated(dx: Int, dy: Int) -> CanvasSelection? {
        let (fx, fy) = (CGFloat(dx), CGFloat(dy))
        let shifted: Shape
        switch shape {
        case .rect(let rect): shifted = .rect(rect.offsetBy(dx: fx, dy: fy))
        case .ellipse(let rect): shifted = .ellipse(rect.offsetBy(dx: fx, dy: fy))
        case .polygon(let points):
            shifted = .polygon(points.map { CGPoint(x: $0.x + fx, y: $0.y + fy) })
        case .mask(let bytes): shifted = .mask(Self.shift(bytes, dx, dy, canvasWidth, canvasHeight))
        case .mask16(let samples):
            shifted = .mask16(Self.shift(samples, dx, dy, canvasWidth, canvasHeight))
        }
        return CanvasSelection(shape: shifted, canvasWidth: canvasWidth, canvasHeight: canvasHeight)
    }

    private static func shift<T: FixedWidthInteger>(
        _ samples: [T], _ dx: Int, _ dy: Int, _ width: Int, _ height: Int
    ) -> [T] {
        var out = [T](repeating: 0, count: samples.count)
        guard abs(dx) < width, abs(dy) < height else { return out }
        for y in max(0, dy)..<min(height, height + dy) {
            let from = (y - dy) * width
            let to = y * width
            for x in max(0, dx)..<min(width, width + dx) {
                out[to + x] = samples[from + x - dx]
            }
        }
        return out
    }
}

extension SelectionHistoryState {
    /// The selection after a move: the translated selection, the old one kept
    /// for Reselect when it differs, and Quick Mask off.
    func moved(dx: Int, dy: Int) -> SelectionHistoryState {
        var next = self
        let moved = selection?.translated(dx: dx, dy: dy)
        if !Self.equivalent(moved, selection), let old = selection { next.previous = old }
        next.selection = moved
        next.quickMaskActive = false
        return next
    }
}
