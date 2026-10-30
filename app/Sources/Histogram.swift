import AppKit

// `HistogramLoader` owns both coalescing read seams. The histogram and the scoped cursor name a
// read failure through `HistogramLoader.onFailure`: "Histogram unavailable" with the
// sentence as its tooltip and in the footnote, and "—" for the cursor. A `JobCancelled` — or any
// failure under a cancelled control (`HistogramLoader.reading(under:)`) — is delivered as nil, and
// a scoped histogram whose capture throws `supersede()`s the loader first, so no older request
// replaces the reason.

/// 256 bins each for red, green, blue and Rec. 709 luma over one image, plus
/// what a clipping warning needs. Built by the core
/// (`dr_image_histogram` through `RasterImage.histogram`): a pixel counts
/// when its alpha is non-zero and, with a selection, its coverage is 128 or
/// more — the 50 % contour rule the marquee uses everywhere else. `total` is
/// the number of pixels actually COUNTED, so a strided scan's proportions
/// read the same as a full one's.
struct HistogramBins {
    let red: [Double]
    let green: [Double]
    let blue: [Double]
    let luma: [Double]
    /// Complete scope mass when exact endpoint statistics are available.
    let total: Double
    let sampled: Bool
    let exactShadow: Double?
    let exactHighlight: Double?
    let completeEndpoints: Bool

    init?(flat: [UInt32], total: UInt64, sampled: Bool,
          exactShadow: UInt32? = nil, exactHighlight: UInt32? = nil) {
        self.init(values: flat.map(Double.init), total: Double(total), sampled: sampled,
                  exactShadow: exactShadow.map(Double.init),
                  exactHighlight: exactHighlight.map(Double.init), completeEndpoints: false)
    }

    init?(values: [Double], total: Double, sampled: Bool,
          exactShadow: Double? = nil, exactHighlight: Double? = nil,
          completeEndpoints: Bool = true) {
        guard values.count == 1024, values.allSatisfy({ $0.isFinite && $0 >= 0 }),
              total.isFinite, total >= 0 else { return nil }
        red = Array(values[0..<256]); green = Array(values[256..<512])
        blue = Array(values[512..<768]); luma = Array(values[768..<1024])
        self.total = total; self.sampled = sampled
        self.exactShadow = exactShadow; self.exactHighlight = exactHighlight
        self.completeEndpoints = completeEndpoints
    }

    var shadowClipped: Double { exactShadow ?? max(red[0], green[0], blue[0]) }
    var highlightClipped: Double { exactHighlight ?? max(red[255], green[255], blue[255]) }
    var plotPeak: Double {
        let peak = [red, green, blue, luma].flatMap { $0[1...254] }.max() ?? 0
        if peak > 0 { return peak }
        return max([red, green, blue, luma].flatMap { [$0[0], $0[255]] }.max() ?? 0, 1)
    }
}

/// Which pixels a histogram counts. "Selection" is a GATE on any of these,
/// not a fourth source.
/// `Equatable` because the Info panel compares scopes — every payload is a
/// layer index, so the synthesized comparison is exactly right.
enum HistogramSource: Equatable {
    /// The flattened document — what the canvas shows.
    case composite
    /// One layer's canvas-space image (0 outside the layer's rect).
    case layer(Int)
    /// The composite of everything BELOW `below` — what an adjustment layer
    /// at that index actually acts on, and therefore the only thing a plot
    /// behind its Levels or Curves controls may show. The composite would
    /// already have the correction (and every layer above it) baked in, so
    /// the handles would not line up with the tones they move.
    case backdrop(below: Int)
}

enum Histogram {
    /// The bins of one image, gated by `selection` (a canvas-sized coverage
    /// buffer) when given. Canvas-sized work: never call it on the main
    /// thread for a live panel — that is what `HistogramLoader` is for —
    /// and never per slider tick or per mouse-moved
    /// (`SliderSheetController`'s standing warning about canvas-sized reads
    /// ahead of a debounce).
    static func of(_ image: RasterImage, selection: [UInt8]?, stride: Int) -> HistogramBins? {
        let step = Swift.max(1, stride)
        guard let scan = image.histogram(mask: selection, stride: step) else { return nil }
        return HistogramBins(flat: scan.bins, total: scan.total, sampled: step > 1)
    }

    /// Which pixels a dialog's plot counts, as VALUES: the layer being
    /// filtered for a destructive sheet, and the BACKDROP BELOW the layer
    /// for an adjustment-layer sheet — `.edit` the layer's own index,
    /// `.create` the index the new layer will be inserted at. `.create`
    /// also gates on the selection it captured, because that is exactly
    /// where the new layer's mask will let the adjustment act. A new layer
    /// landing above every entry has the whole stack below it, which is the
    /// composite itself (the backdrop's cut would exclude nothing), so that
    /// plan is `.composite` — the scope that can count the retained
    /// projection instead of rendering.
    ///
    /// Decided on the main thread because it reads the editor's active
    /// layer; nothing it returns is a handle, so the scan it names runs on
    /// a background queue.
    static func sheetPlan(
        _ document: ImageDocument, mode: AdjustmentSheetMode, destructiveLayer: Int?
    ) -> (source: HistogramSource, selection: [UInt8]?) {
        switch mode {
        case .destructive:
            return (.layer(destructiveLayer ?? document.activeLayerIndex), nil)
        case .create(let selection):
            let below = document.doc?.newEntryLandingIndex(active: document.activeLayerIndex)
                ?? document.activeLayerIndex + 1
            if let doc = document.doc, below >= doc.layerCount { return (.composite, selection) }
            return (.backdrop(below: below), selection)
        case .edit(let layer, _):
            return (.backdrop(below: layer), nil)
        }
    }

    /// The bins a dialog plots behind its controls, captured ONCE at sheet
    /// open at stride 1 — but OFF the main thread, `apply` landing back on
    /// it when they arrive (the plot simply draws empty until then).
    ///
    /// Doing it inline in a controller's `init` is what this exists to
    /// stop: `.layer` is a canvas-sized `dr_doc_layer_canvas_image` and
    /// `.backdrop` a clone per layer above the target plus a full flatten,
    /// so on a 100 MP document Levels froze the UI for about half a second
    /// before the sheet appeared and Curves on an adjustment layer for
    /// about one. The sheet these replaced did no canvas-sized work at
    /// open, so an inline capture was a regression, not a cost of the
    /// feature.
    ///
    /// The loader is a one-shot here rather than a debouncer — it is used
    /// instead of a bare `DispatchQueue` because it already owns the hop
    /// out and the hop back — and it is parked in `inFlight` for the single
    /// round trip. That is deliberate: it means a caller needs no stored
    /// property, which is what keeps the two callers inside `Sheets.swift`
    /// (frozen) down to one wiring line each.
    ///
    /// A read that FAILS is not an empty picture: with `onFailure`
    /// set, the reason a throwing pre-flight or the read itself gave is
    /// delivered there INSTEAD of `apply`, so the sheet can say "Histogram
    /// unavailable" and why. Without it the failure is `apply(nil)`, as
    /// before. A stale document or capture is state, not a failure, and is
    /// always `apply(nil)`; so is a cancel. An 8-bit sheet with a
    /// `destructiveSource` falls back to scanning it instead of failing.
    /// A 32-bit float source is read on unit bins (`floatBins: .unit`), the
    /// linear 0…1 axis the sheet's handles sit on.
    static func loadForSheet(
        _ document: ImageDocument, mode: AdjustmentSheetMode, destructiveLayer: Int?,
        destructiveSource: RasterImage? = nil, precisionCapture: PrecisionEditorCapture? = nil,
        then apply: @escaping (HistogramBins?) -> Void,
        onFailure: ((String) -> Void)? = nil
    ) {
        guard let doc = document.doc else {
            apply(nil)
            return
        }
        let nativeCapture = precisionCapture ?? (doc.isHighDepth ? try? PrecisionEditorCapture(document) : nil)
        if let nativeCapture, (try? nativeCapture.requireCurrent(document)) == nil { apply(nil); return }
        let plan = sheetPlan(document, mode: mode, destructiveLayer: destructiveLayer)
        let fail: (Error) -> Void = { error in
            if !(error is JobCancelled), let onFailure {
                onFailure(error.localizedDescription)
            } else {
                apply(nil)
            }
        }
        let selected: CanvasSelection?
        let capturedCoverage: [UInt8]?
        let scalarSource: RasterImage?
        switch mode {
        case .destructive:
            selected = nativeCapture != nil ? nativeCapture?.selection : document.canvasSelection
            capturedCoverage = nil
            scalarSource = !doc.isHighDepth && document.planeEditTarget != .layer
                ? destructiveSource : nil
        case .create(let coverage):
            selected = nativeCapture?.selection
            capturedCoverage = nativeCapture == nil ? coverage : nil
            scalarSource = nil
        case .edit:
            selected = nil
            capturedCoverage = nil
            scalarSource = nil
        }
        // The 8-bit composite a failed scoped read falls back to.
        let byteFallback = doc.isHighDepth ? nil : destructiveSource
        var nativeScalar: NativeScalarStatisticsRead?
        if case .destructive = mode, let nativeCapture, nativeCapture.target.isCoverage {
            do {
                nativeScalar = try NativeScalarStatisticsRead(source: nativeCapture.facade,
                    target: nativeCapture.target, layer: destructiveLayer ?? nativeCapture.layer,
                    selection: nativeCapture.selection, preview: true)
            } catch { fail(error); return }
        }
        var scoped: ScopedAnalysisRead?
        if scalarSource == nil && nativeScalar == nil {
            do {
                // A composite plan counts the retained projection;
                // every other scope drops it.
                scoped = try ScopedAnalysisRead(source: doc, scope: plan.source,
                    selection: selected, preview: true, coverage: capturedCoverage,
                    projection: document.retainedPrecisionProjection,
                    byteProjection: document.retainedByteProjection, floatBins: .unit)
            } catch {
                if byteFallback == nil { fail(error); return }
            }
        }
        let loader = HistogramLoader()
        nextTicket += 1
        let ticket = nextTicket
        inFlight[ticket] = loader
        // The closures capture the TICKET, not the loader: capturing the
        // loader would be a cycle through its own `onBins`, and the parking
        // slot is what keeps it alive instead.
        loader.onBins = { [weak document] bins in
            defer { inFlight.removeValue(forKey: ticket) }
            if let nativeCapture {
                guard let document, (try? nativeCapture.requireCurrent(document)) != nil else { apply(nil); return }
            }
            apply(bins)
        }
        // A failure suppresses `onBins`, so this frees the slot itself.
        if let onFailure {
            loader.onFailure = { [weak document] message in
                inFlight.removeValue(forKey: ticket)
                if let nativeCapture {
                    guard let document, (try? nativeCapture.requireCurrent(document)) != nil else { apply(nil); return }
                }
                onFailure(message)
            }
        }
        let scanned = scoped
        let scalarRead = nativeScalar
        loader.request {
            if let scalarRead { return ScopedAnalysisRead.bins(try scalarRead.read()) }
            // A byte scalar sheet owns a raw mask/channel/plane image; reading
            // its layer RGB instead would show unrelated tones behind controls.
            if let scalarSource { return of(scalarSource, selection: nil, stride: 1) }
            if let scanned {
                do { return ScopedAnalysisRead.bins(try scanned.read()) } catch {
                    guard byteFallback != nil else { throw error }
                }
            }
            guard let image = byteFallback else { return nil }
            return of(image, selection: plan.selection, stride: 1)
        }
    }

    /// One-shot sheet captures with a delivery still in flight, held only
    /// for the hop out and back. Main-thread only, like everything else on
    /// this side; `HistogramLoader` always delivers exactly once, so a slot
    /// is never left behind.
    private static var inFlight: [Int: HistogramLoader] = [:]
    private static var nextTicket = 0

    /// The stride the LIVE panel scans at: enough pixels for a stable shape
    /// (~4 M) and no more, so a 24 MP document does not re-scan itself on
    /// every brush tick.
    ///
    /// It is then bumped upward until it shares no factor with the row
    /// length: a stride that divides the width lands on the same columns in
    /// every row forever, which on anything with vertical structure — a
    /// gradient, a border, a picket fence — samples a biased slice of the
    /// image rather than a thinner copy of it.
    static func liveStride(pixels: Int, width: Int) -> Int {
        guard pixels > 4_000_000, width > 0 else { return 1 }
        var stride = Swift.max(1, pixels / 4_000_000)
        while stride > 1 && gcd(stride, width) != 1 { stride += 1 }
        return stride
    }

    private static func gcd(_ a: Int, _ b: Int) -> Int {
        var (x, y) = (abs(a), abs(b))
        while y != 0 { (x, y) = (y, x % y) }
        return x
    }
}

/// Debounced, coalescing background scanner for the LIVE panel: the same
/// shape as `PreviewRenderer` (Sheets.swift) — which is frozen and typed to
/// `CGImage`, so it cannot be reused directly — hopping back to main with
/// the bins.
///
/// Capture the `RasterImage` and the mask bytes by VALUE in the closure
/// (both are immutable, and `CanvasSelection` is a struct), never the
/// document: the handle must be touched by one queue at a time, and the
/// document's is the main thread's.
final class HistogramLoader {
    private let queue = DispatchQueue(label: "com.kgalligan.doctored.histogram")
    private var pending: (() -> HistogramLoadResult)?
    private var isLoading = false
    private var generation: UInt64 = 0
    private var pendingGeneration: UInt64 = 0
    private var debounce: DispatchWorkItem?

    private enum HistogramLoadResult {
        case bins(HistogramBins?)
        case report([String: Any]?)
        /// A thrown read, and whether it was a `request` (bins) or a
        /// `requestReport`.
        case failed(Error, bins: Bool)
    }
    var onBins: ((HistogramBins?) -> Void)?
    var onReport: (([String: Any]?) -> Void)?
    /// A read's thrown sentence, for its generation. Unset, a failure
    /// is delivered as `onBins(nil)` / `onReport(nil)` by request kind, so a
    /// one-shot caller still gets exactly one delivery. A `JobCancelled` is
    /// always delivered as nil, never here.
    var onFailure: ((String) -> Void)?

    func request(_ compute: @escaping () throws -> HistogramBins?) {
        enqueue {
            do { return .bins(try compute()) } catch { return .failed(error, bins: true) }
        }
    }

    func requestReport(_ compute: @escaping () throws -> [String: Any]?) {
        enqueue {
            do { return .report(try compute()) } catch { return .failed(error, bins: false) }
        }
    }

    /// Runs one read under `control`. A failure thrown once the control is
    /// cancelled IS the cancel, whatever sentence the kernel phrased it in
    /// (`BackgroundWork.wait`'s rule), so it is delivered as nil and never
    /// replaces a shown failure.
    static func reading<Value>(under control: RasterJobControl,
                               _ body: () throws -> Value) throws -> Value {
        do {
            return try body()
        } catch {
            guard control.isCancelled else { throw error }
            throw error as? JobCancelled ?? JobCancelled(kernelMessage: error.localizedDescription)
        }
    }

    private func enqueue(_ compute: @escaping () -> HistogramLoadResult) {
        generation &+= 1
        let requestedGeneration = generation
        debounce?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self = self else { return }
            self.pending = compute
            self.pendingGeneration = requestedGeneration
            self.drain()
        }
        debounce = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.12, execute: work)
    }

    /// Drops every earlier request — a pending debounce and any in-flight
    /// result — while keeping the callbacks, so nothing older can land after
    /// a failure the caller is about to show. Never blocks.
    func supersede() {
        generation &+= 1
        debounce?.cancel()
        debounce = nil
        pending = nil
    }

    func cancel() {
        generation &+= 1
        debounce?.cancel()
        debounce = nil
        pending = nil
        onBins = nil
        onReport = nil
        onFailure = nil
        // Wait out any in-flight scan, exactly as PreviewRenderer does: Dr
        // handles are not thread-safe and the caller may touch the same one
        // the moment this returns. The queue only hops back to main
        // asynchronously, so this cannot deadlock.
        queue.sync {}
    }

    private func drain() {
        guard !isLoading, let compute = pending else { return }
        pending = nil
        let requestedGeneration = pendingGeneration
        isLoading = true
        queue.async { [weak self] in
            let bins = compute()
            DispatchQueue.main.async {
                guard let self = self else { return }
                self.isLoading = false
                if self.generation == requestedGeneration {
                    switch bins {
                    case .bins(let bins): self.onBins?(bins)
                    case .report(let report): self.onReport?(report)
                    case .failed(let error, let bins):
                        if !(error is JobCancelled), let onFailure = self.onFailure {
                            onFailure(error.localizedDescription)
                        } else if bins {
                            self.onBins?(nil)
                        } else {
                            self.onReport?(nil)
                        }
                    }
                }
                self.drain()
            }
        }
    }
}
