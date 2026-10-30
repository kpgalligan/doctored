import AppKit

// The Crop tool's Fill: a crop past the canvas fills its new area with Background
// (Default), Generative Expand or Content-Aware Fill. Both fills first run the crop exactly as
// Background (Default) — `cropped`, `extendedCrop` / `straightenedCrop` with the same
// window and background — WITHOUT publishing it, then run the existing fill job on that base
// (`DocumentJobs.start(base:)`), whose one `applyEdit` publishes the crop and the fill as ONE undo
// step. The coverage is the old canvas's polygon mapped by the crop (`CropSession.canvasMap`),
// inverted and grown by the resample's support (`seamRadius`), so a turned crop's pad-tinted seam
// inside the old edge is filled too. Content-Aware fills record 0 only, the qualifying Background
// (take1's R6); Generative Expand lands a Generative Layer above the active layer with three
// variations. The UI path is EditorViewController+CropFill.swift; the MCP twin is `crop`'s `fill`
// (AgentServer+CropFill.swift).

/// The Crop options bar's Fill popup, in popup order.
enum CropFill: String, CaseIterable {
    case background
    case generativeExpand = "generative_expand"
    case contentAware = "content_aware"

    var title: String {
        switch self {
        case .background: return "Background (Default)"
        case .generativeExpand: return "Generative Expand"
        case .contentAware: return "Content-Aware Fill"
        }
    }

    /// The popup's `index`, clamped into `allCases`.
    init(index: Int) {
        let all = Self.allCases
        self = all[min(max(index, 0), all.count - 1)]
    }
}

/// One crop past the canvas, as both fills run it: the ROUNDED window, the straighten angle and
/// sampler, Delete Cropped Pixels, the Background (Default) fill and the described layers the
/// straighten regenerates.
struct CropFillRequest {
    let rect: CGRect
    let angle: Double
    let sampler: DrResizeFilter
    let deleteCropped: Bool
    let background: CropBackground
    let described: [Int]
}

enum CropFillRun {
    static let contentAwareScope = "Content-Aware Fill on a crop fills the Background: the bottom "
        + "layer must be a visible, Normal, 100%, unmasked, unstyled raster covering the canvas. "
        + "Choose Background (Default) or Generative Expand."
    static let contentAwareKeepsPixels = "fill content_aware deletes the cropped pixels: omit "
        + "delete_cropped or pass true."
    static let insideWindow = "fill applies only to a rectangle past the canvas; this one lies "
        + "inside it: omit fill."
    static let notReplayed = "A crop with Generative Expand is not replayed from an Action: it "
        + "would bill the provider key again."
    static let noNewArea = "The crop adds no area past the picture to fill."

    /// The crop alone, exactly as Background (Default) commits it, NOT published: at angle 0
    /// `extendedCrop`, turned `straightenedCrop`'s document. nil when it changed nothing.
    static func cropped(_ doc: RasterDocument, _ request: CropFillRequest) throws -> RasterDocument? {
        let rect = request.rect
        let (x, y, w, h) = (Int(rect.minX), Int(rect.minY), Int(rect.width), Int(rect.height))
        guard request.angle != 0 else {
            return try doc.extendedCrop(x: x, y: y, w: w, h: h, deleteCropped: request.deleteCropped,
                                        background: request.background)
        }
        let matrix = AgentServer.cropMatrix(x: x, y: y, w: w, h: h, angle: request.angle)
        return try doc.straightenedCrop(
            matrix, sampler: request.sampler, x: x, y: y, w: w, h: h,
            described: request.described, deleteCropped: request.deleteCropped,
            background: request.background)?.document
    }

    /// How far a turned crop's resample spreads the pad inside the old edge: the kernel's
    /// support (bilinear 1, bicubic 2, Lanczos3 3), 0 at angle 0 or Nearest, which resample
    /// nothing across the edge.
    static func seamRadius(_ sampler: DrResizeFilter, angle: Double) -> Double {
        guard angle != 0 else { return 0 }
        switch sampler {
        case DR_FILTER_NEAREST: return 0
        case DR_FILTER_BILINEAR: return 1
        case DR_FILTER_CATMULL_ROM: return 2
        default: return 3
        }
    }

    /// The new area on the cropped canvas: the old canvas's corners mapped by the crop, as a
    /// polygon, inverted, then grown by `seamRadius`; the whole new canvas when the polygon misses
    /// it. Refuses `noNewArea` when the old canvas covers the whole window.
    static func newArea(_ request: CropFillRequest, from canvas: CGSize) throws -> CanvasSelection {
        let map = CropSession.canvasMap(window: request.rect, angle: request.angle)
        let corners = [CGPoint(x: 0, y: 0), CGPoint(x: canvas.width, y: 0),
                       CGPoint(x: canvas.width, y: canvas.height), CGPoint(x: 0, y: canvas.height)]
            .map { $0.applying(map) }
        let (w, h) = (Int(request.rect.width), Int(request.rect.height))
        let area: CanvasSelection?
        if let old = CanvasSelection(shape: .polygon(corners), canvasWidth: w, canvasHeight: h) {
            area = old.inverted()
        } else {
            area = CanvasSelection(shape: .rect(CGRect(x: 0, y: 0, width: w, height: h)),
                                   canvasWidth: w, canvasHeight: h)
        }
        guard var selection = area else { throw DoctoredCoreError(message: noNewArea) }
        let radius = seamRadius(request.sampler, angle: request.angle)
        if radius > 0, let grown = selection.grown(by: radius) { selection = grown }
        return selection
    }

    /// The crop's own plan, as phase 1's `crop` reports it: the straighten request's
    /// `scratch_bytes`.
    static func cropPlan(_ doc: RasterDocument, _ request: CropFillRequest) throws -> UInt64 {
        let rect = request.rect
        let (x, y, w, h) = (Int(rect.minX), Int(rect.minY), Int(rect.width), Int(rect.height))
        let command = try PrecisionPixelCommand.straighten(
            matrix: AgentServer.cropMatrix(x: x, y: y, w: w, h: h, angle: request.angle),
            filter: request.angle == 0 ? .nearest : RasterDocument.precisionFilter(request.sampler),
            x: x, y: y, width: w, height: h, deleteCropped: request.deleteCropped,
            background: request.background.fields)
        return try PrecisionDocumentDescriptor.integer(
            doc.geometryEditPlan(request: command.request), "scratch_bytes")
    }

    /// The crop, then its base for the fill: the cropped document, the new area on it and the
    /// selection the publication leaves.
    private static func prepare(_ request: CropFillRequest, document: ImageDocument) throws
        -> (doc: RasterDocument, base: RasterDocument, area: CanvasSelection,
            selection: SelectionHistoryState?)
    {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        guard let base = try cropped(doc, request) else {
            throw DoctoredCoreError(message: "Crop was refused and nothing changed.")
        }
        let area = try newArea(request, from: CGSize(width: doc.width, height: doc.height))
        let selection = document.selectionCarried(
            by: CropSession.canvasMap(window: request.rect, angle: request.angle),
            width: Int(request.rect.width), height: Int(request.rect.height))
        return (doc, base, area, selection)
    }

    /// Content-Aware Fill over the new area of record 0 (the caller has checked
    /// `hasQualifyingBackground` and forced Delete Cropped Pixels, so the Background is padded),
    /// with the fill's default settings, as one "Crop" job. `plannedBytes` is the larger of the
    /// crop's plan and the fill's beside the cropped base.
    static func startContentAware(_ request: CropFillRequest, document: ImageDocument,
                                  record: [ActionStep], showProgress: Bool) throws
        -> (job: DocumentJob, plannedBytes: UInt64)
    {
        let prepared = try prepare(request, document: document)
        let base = prepared.base
        let work = try ContentAwareFillWork(
            source: base, layer: 0, coverage: prepared.area.maskBytes(),
            settings: ContentAwareFillSettings(),
            nativeCoverage: base.isHighDepth ? prepared.area.maskSamples16() : nil)
        // As `content_aware_fill` admits it: the native forecast once, the job's own host
        // allocation none.
        _ = try RasterResources.shared.admit(scratch: work.scratch)
        let planned = max(try cropPlan(prepared.doc, request),
                          work.scratch + (try base.ownRetainedBytes()))
        let job = try DocumentJobs.start(
            document: document, layer: 0, title: "Crop", scratch: 0, record: record,
            showProgress: showProgress, rasterizesTarget: true,
            base: DocumentJobBase(document: base, resultingSelection: prepared.selection),
            // A fill that reproduces the pad still publishes the crop.
            compute: { _, control in try work.compute(control: control) ?? base })
        return (job, planned)
    }

    /// Generative Expand over the new area, anchored at the active layer: the Generative Layer
    /// lands above it with the run's variations, as one "Crop" job. The caller has passed
    /// `GenerativeFillRun.gate`.
    static func startGenerativeExpand(_ settings: GenerativeFillSettings, _ request: CropFillRequest,
                                      document: ImageDocument, showProgress: Bool) throws
        -> (job: DocumentJob, plannedBytes: UInt64)
    {
        let prepared = try prepare(request, document: document)
        let job = try GenerativeFillRun.start(
            settings, coverage: .area(prepared.area.maskSamples16()), document: document,
            layer: document.activeLayerIndex, showProgress: showProgress,
            base: DocumentJobBase(document: prepared.base, resultingSelection: prepared.selection))
        let fill = (job.details["planned_bytes"] as? UInt64) ?? 0
        let planned = max(try cropPlan(prepared.doc, request),
                          fill + (try prepared.base.ownRetainedBytes()))
        return (job, planned)
    }
}
