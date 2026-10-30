import AppKit

// Every 16-bit canvas stroke (Brush, Eraser, Dodge/Burn, Clone Stamp, Healing Brush, Spot Healing
// and scalar Brush/Eraser) and the 16-bit MCP `heal_stroke`/`spot_heal_stroke` commit as ONE job,
// as `DocumentJobs`' retouch commits do: this file's `PrecisionStrokeWork` rasterizes the
// footprint and runs the edit on the worker, each under its own child of the single-use control,
// and `ImageCanvasView+StrokeGhost.swift` pins the stroke's ghost (`StrokeGhostPin`, latched in
// `endStroke`) until `clearStrokeGhostPin`; the 16-bit Destination Patch places its region inside
// its job (`PatchNativeWork`). The 16-bit MCP brush/eraser/clone/dodge_burn layer strokes
// are jobs too (`background` returns the job_id); Quick Mask strokes keep their modal coverage wait.
// The work has a `Kind`: Blur / Sharpen / Sponge are ordinary `.edit`s (`retouch_brush`),
// Smudge runs along the stroke's own dabs and the History Brush paints its footprint from a state.
// A Smudge carries Sample All Layers, which its plan and run both take. The
// Art History Brush runs along the stroke's own dabs from a state, as Smudge does (no footprint).

/// A typed (16-bit) stroke as ONE nonmodal `DocumentJobs` job: the
/// canvas Brush, Eraser, Dodge/Burn, Clone Stamp, Healing Brush, Spot Healing
/// and scalar Brush/Eraser strokes, and MCP `heal_stroke` / `spot_heal_stroke`,
/// `brush_stroke` / `eraser_stroke` / `clone_stamp` / `dodge_burn` layer strokes.
/// Everything is captured on main; the worker rasterizes the stroke's
/// footprint, appends the captured selection coverage and runs the edit.
struct PrecisionStrokeWork {
    /// What the worker runs once it holds the stroke: the planned
    /// `command` over the rasterized footprint, a Smudge along the stroke's
    /// own dabs (no footprint), or the History Brush's footprint painted from
    /// a history state. `command` names the target layer for all three.
    enum Kind {
        case edit
        case smudge(strength: Double, sampleAllLayers: Bool)
        case historyBrush(source: RasterPrecisionDocument, sourceLayer: Int, opacity: Double)
        /// The Art History Brush's clicks along the stroke's dabs,
        /// painted from layer `sourceLayer` of `source`; `request` is
        /// `ArtHistoryOptions.request`.
        case artHistory(source: RasterPrecisionDocument, sourceLayer: Int, request: String)
    }

    let kind: Kind
    /// The layer a `.smudge` or `.historyBrush` edits; unused by `.edit`,
    /// whose `command` carries it.
    let layer: Int
    let source: RasterPrecisionDocument
    let stroke: NativeBrushStroke
    let command: PrecisionPixelCommand
    /// Canvas-sized u16 coverage appended after the footprint; nil = unselected.
    let selection: [UInt16]?
    let width: Int
    let height: Int
    /// The footprint plan's scratch plus the edit plan's.
    let scratchEstimate: UInt64
    let retainedPayload: PrecisionEditPayload

    /// The bytes the worker itself holds outside the core's reservation:
    /// the rasterized footprint and the joined footprint + selection samples.
    /// This, not `scratchEstimate`, is the job's `scratch` — the core reserves
    /// its plan scratch from the job's control.
    var heldBytes: UInt64 {
        let plane = UInt64(width) * UInt64(height) * 2
        // A Smudge rasterizes no footprint: it holds the selection alone.
        if case .smudge = kind { return selection == nil ? 0 : plane }
        // Nor does the Art History Brush.
        if case .artHistory = kind { return selection == nil ? 0 : plane }
        return plane + plane * (selection == nil ? 1 : 2)
    }

    init(source: RasterPrecisionDocument, stroke: NativeBrushStroke, command: PrecisionPixelCommand,
         selection: [UInt16]?, width: Int, height: Int) throws {
        try self.init(kind: .edit, layer: 0, source: source, stroke: stroke, command: command,
                      selection: selection, width: width, height: height)
    }

    /// `kind` over `layer`. `command` is the planned request of an `.edit`
    /// and only names the stroke for the other two kinds.
    init(kind: Kind, layer: Int, source: RasterPrecisionDocument, stroke: NativeBrushStroke,
         command: PrecisionPixelCommand, selection: [UInt16]?, width: Int, height: Int) throws {
        guard !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
        }
        let context = try PrecisionWorkContext(source: source)
        let plan: [String: Any]
        let pixels = UInt64(width) * UInt64(height)
        let planned: UInt64
        switch kind {
        case .edit:
            plan = try source.pixelEditPlan(request: command.request, workingLimit: context.limit,
                                            otherRetained: context.otherRetained)
            planned = pixels * (selection == nil ? 1 : 2)
        case .smudge(_, let sampleAllLayers):
            plan = try source.smudgePlan(layer: layer, request: stroke.request, dabs: stroke.dabs,
                selection: selection != nil, sampleAllLayers: sampleAllLayers,
                workingLimit: context.limit, otherRetained: context.otherRetained)
            planned = selection == nil ? 0 : pixels
        case .historyBrush(let history, let historyLayer, _):
            plan = try source.historyBrushPlan(layer: layer, source: history,
                sourceLayer: historyLayer, selection: selection != nil,
                workingLimit: context.limit, otherRetained: context.otherRetained)
            planned = pixels * (selection == nil ? 1 : 2)
        case .artHistory(let history, let historyLayer, let request):
            plan = try source.artHistoryBrushPlan(layer: layer, source: history,
                sourceLayer: historyLayer, request: request, brushRequest: stroke.request,
                dabs: stroke.dabs, selection: selection != nil, workingLimit: context.limit,
                otherRetained: context.otherRetained)
            planned = selection == nil ? 0 : pixels
        }
        guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == planned,
              selection.map({ $0.count == width * height }) ?? true else {
            throw DoctoredCoreError(message: "High-depth input samples do not match the planned edit.")
        }
        self.kind = kind
        self.layer = layer
        self.source = source
        self.stroke = stroke
        self.command = command
        self.selection = selection
        self.width = width
        self.height = height
        retainedPayload = try PrecisionEditPayload.admitted(
            bytes: UInt64(selection?.count ?? 0) * 2 + UInt64(command.request.utf8.count) * 2 + 512)
        let editScratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        switch kind {
        case .smudge, .artHistory:
            scratchEstimate = editScratch
        case .edit, .historyBrush:
            scratchEstimate = try PrecisionDocumentDescriptor.sum(
                stroke.footprintPlan(width: width, height: height), editScratch)
        }
    }

    /// Worker. A job control is single-use at the core, so the footprint and
    /// the edit each run under their own child of the job's control.
    func compute(control: RasterJobControl) throws -> RasterPrecisionDocument? {
        if case .smudge(let strength, let sampleAllLayers) = kind {
            let context = try PrecisionWorkContext(source: source)
            return try source.smudging(layer: layer, request: stroke.request, dabs: stroke.dabs,
                strength: strength, selection: selection, sampleAllLayers: sampleAllLayers,
                workingLimit: context.limit, otherRetained: context.otherRetained,
                control: control)
        }
        if case .artHistory(let history, let historyLayer, let request) = kind {
            let context = try PrecisionWorkContext(source: source)
            return try source.artHistoryBrushing(layer: layer, source: history,
                sourceLayer: historyLayer, request: request, brushRequest: stroke.request,
                dabs: stroke.dabs, selection: selection, workingLimit: context.limit,
                otherRetained: context.otherRetained, control: control)
        }
        let footprint = try control.withChildOperation(from: 0, to: 0.2) { child in
            try stroke.footprint(width: width, height: height, control: child)
        }
        if control.isCancelled { throw JobCancelled() }
        return try control.withChildOperation(from: 0.2, to: 1) { child in
            let context = try PrecisionWorkContext(source: source)
            if case .historyBrush(let history, let historyLayer, let opacity) = kind {
                return try source.historyBrushing(layer: layer, source: history,
                    sourceLayer: historyLayer, opacity: opacity,
                    samples: footprint + (selection ?? []), workingLimit: context.limit,
                    otherRetained: context.otherRetained, control: child)
            }
            return try source.editingPixels(request: command.request, samples: footprint + (selection ?? []),
                workingLimit: context.limit, otherRetained: context.otherRetained, control: child)
        }
    }
}

extension DocumentJobs {
    /// Starts a captured typed stroke, refusing a capture the document has
    /// already moved past.
    @discardableResult
    static func startPrecisionStroke(_ work: PrecisionStrokeWork, document: ImageDocument,
                                     capture: PrecisionEditorCapture, layer: Int, title: String,
                                     record: [ActionStep], showProgress: Bool, rasterizesTarget: Bool,
                                     completion: ((DocumentJob) -> Void)? = nil) throws -> DocumentJob {
        try capture.requireCurrent(document)
        return try start(document: document, layer: layer, title: title, scratch: work.heldBytes,
            record: record, showProgress: showProgress, rasterizesTarget: rasterizesTarget,
            compute: { _, control in
                try work.compute(control: control).map { try RasterDocument(precision: $0) }
            }, completion: completion)
    }
}
