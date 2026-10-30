import AppKit

// A 32-bit float (linear) development is inspected through its SDR-clipped u16 view
// (`RasterPrecisionImage.developInspection`) on a screen without headroom: Fit, 100 %, the
// clipping markers and Before/After show the development clipped to SDR, and a marker at 65535
// means "at or above white". On a screen with headroom, Fit and 100 %, Clipping on or off, draw
// each f32 side's extended image from its retained f32 master instead (`RawDevelopHDRPreview.swift`,
// #680): values above white show brighter than white, and Clipping marks samples at or below
// 0.0 and at or above 1.0. An f32 side's histogram reads its float samples (stop bins, above-white
// counts) either way; a u16 side's reads every genuine u16 source pixel. Fit colours are an
// explicitly approximate preview and clipping checks every source cell. A viewport-only frame
// charges `extendedFramePerPixel` per output pixel of each f32 side drawn extended. Native
// detail displays one source pixel per backing pixel, including Retina. One shared `WorkerTask`
// runs at a time, requests coalesce and generations reject stale results. Closing a modal clears display/master references; a cancelled
// window's task owner remains until its mailbox is consumed and the worker slot released. Do not
// add a second full decode reservation to the current worker's admission.

/// One native master and its whole-image readout. Viewport changes never
/// re-develop RAW or count a downsampled display as the image's histogram.
struct RawDevelopInspectionSource {
    /// The u16 master, or an f32 master's SDR-clipped u16 view.
    let image: RasterPrecisionImage
    /// The f32 master itself (#680), the same handle (no copy), which the
    /// HDR view and the float histogram read; nil for a u16 master.
    let extendedMaster: RasterPrecisionImage?
    let histogram: [String: Any]
    let bins: HistogramBins

    /// `image` is a u16 master, kept as it is, or an f32 one, kept as its
    /// SDR-clipped u16 view (made without the job token) beside the master.
    /// The histogram consumes the token: a u16 master's u16 histogram, an f32
    /// master's float one (stop bins, above-white counts).
    init(_ image: RasterPrecisionImage, control: RasterJobControl? = nil) throws {
        extendedMaster = image.isFloatStorage ? image : nil
        let view = try image.developInspection()
        self.image = view
        histogram = try (extendedMaster ?? view).developHistogram(control: control)
        guard let bins = Self.bins(histogram) else {
            throw DoctoredCoreError(message: "The native RAW histogram could not be read.")
        }
        self.bins = bins
    }

    static func bins(_ report: [String: Any]) -> HistogramBins? {
        if report["storage"] as? String == "rgba32_float" { return ScopedAnalysisRead.bins(report) }
        guard let values = report["bins"] as? [[NSNumber]], values.count == 4,
              values.allSatisfy({ $0.count == 256 }),
              let zero = report["exact_zero_rgba"] as? [NSNumber], zero.count == 4,
              let max = report["exact_max_rgba"] as? [NSNumber], max.count == 4,
              let total = report["total"] as? NSNumber else { return nil }
        let flat = values.flatMap { $0 }.map(\.uint32Value)
        return HistogramBins(flat: flat, total: total.uint64Value,
            sampled: (report["stride"] as? NSNumber)?.intValue != 1,
            exactShadow: zero.prefix(3).map(\.uint32Value).max(),
            exactHighlight: max.prefix(3).map(\.uint32Value).max())
    }
}

struct RawDevelopViewport: Equatable {
    var nativeDetail = false
    var center = CGPoint(x: 0.5, y: 0.5)
    var backingSize = CGSize(width: 1024, height: 680)
    var clipping: UInt32 = 0
    /// The window's screen has headroom (#680): Fit and 100 %, Clipping
    /// on or off, then draw an f32 side's extended image.
    var extendedRange = false

    func region(_ image: RasterPrecisionImage) -> (x: Int, y: Int, width: Int, height: Int, divisor: Int) {
        guard nativeDetail else {
            let divisor = max(1, Int(ceil(Double(max(image.width, image.height)) / 1600)))
            return (0, 0, image.width, image.height, divisor)
        }
        // The core bounds a detail crop to 1024 samples on each side. Smaller
        // panes request exactly their backing-pixel extent; larger panes center
        // a bounded crop without magnifying it or silently reducing detail.
        let width = min(image.width, max(1, min(1024, Int(backingSize.width.rounded(.down)))))
        let height = min(image.height, max(1, min(1024, Int(backingSize.height.rounded(.down)))))
        let x = min(max(0, Int((center.x * CGFloat(image.width)).rounded()) - width / 2), image.width - width)
        let y = min(max(0, Int((center.y * CGFloat(image.height)).rounded()) - height / 2), image.height - height)
        return (x, y, width, height, 1)
    }

    func comparisonRegion(before: RasterPrecisionImage, after: RasterPrecisionImage)
        throws -> (x: Int, y: Int, width: Int, height: Int, divisor: Int) {
        let region = self.region(before)
        guard nativeDetail else { return region }
        let width = max(1, min(1024, Int(backingSize.width.rounded(.down))))
        let height = max(1, min(1024, Int(backingSize.height.rounded(.down))))
        guard region.width == width, region.height == height,
              region.x + width <= after.width, region.y + height <= after.height else {
            throw DoctoredCoreError(message: "The requested 100% region \(width)×\(height) at "
                + "(\(region.x), \(region.y)) does not fit both saved \(before.width)×\(before.height) "
                + "and proposed \(after.width)×\(after.height) RAW images. Choose Fit or another region.")
        }
        return region
    }
}

struct RawDevelopPreviewFrame {
    let before: RawDevelopInspectionSource
    let after: RawDevelopInspectionSource
    let beforeDisplay: CGImage
    let afterDisplay: CGImage
    let viewport: RawDevelopViewport
    let detailRegion: (x: Int, y: Int, width: Int, height: Int, divisor: Int)?
    /// The largest RGB value of the extended images drawn; nil when
    /// none is, so the canvas engages EDR exactly while one shows.
    let extendedPeak: Float?
    /// Each side's extended peak (#680); nil for a side drawn SDR.
    let beforeExtendedPeak: Float?
    let afterExtendedPeak: Float?

    init(before: RawDevelopInspectionSource, after: RawDevelopInspectionSource,
         viewport: RawDevelopViewport, control: RasterJobControl? = nil) throws {
        let sharedDetail: (x: Int, y: Int, width: Int, height: Int, divisor: Int)? =
            viewport.nativeDetail && (before.image.width != after.image.width
                || before.image.height != after.image.height)
                ? try viewport.comparisonRegion(before: before.image, after: after.image) : nil
        // An f32 side on a screen with headroom draws its extended image of
        // the same region at any zoom, with Clipping's float markers. A side
        // whose extended image cannot be made falls back to its SDR view and
        // is left out of the peak; the fallback then runs without the token
        // the refused attempt consumed. A cancel is not a refusal: it ends
        // the frame rather than falling back.
        func display(_ source: RawDevelopInspectionSource,
                     token: RasterJobControl?) throws -> (CGImage, Float?) {
            let r = sharedDetail ?? viewport.region(source.image)
            var token = token
            if viewport.extendedRange, let master = source.extendedMaster {
                let made: RawDevelopExtendedImage?
                do {
                    made = try RawDevelopExtendedImage.make(master, region: r,
                        clipping: viewport.clipping, control: token)
                } catch {
                    if token?.isCancelled == true { throw error }
                    made = nil
                }
                if let made, let cgImage = made.image(profileData: source.image.profileData) {
                    return (cgImage, made.peak)
                }
                token = nil
            }
            let image = try source.image.developPreview(x: r.x, y: r.y, width: r.width,
                height: r.height, divisor: r.divisor, clipping: viewport.clipping, control: token)
            guard let space = CGColorSpace(iccData: source.image.profileData as CFData),
                  let cgImage = image.makeCGImage(in: space) else {
                throw DoctoredCoreError(message: "The RAW detail preview could not be displayed.")
            }
            return (cgImage, nil)
        }
        self.before = before
        self.after = after
        self.viewport = viewport
        detailRegion = sharedDetail
        let beforeShown: (CGImage, Float?), afterShown: (CGImage, Float?)
        if let control {
            beforeShown = try control.withChildOperation(from: 0, to: 0.5) {
                try display(before, token: $0)
            }
            afterShown = try control.withChildOperation(from: 0.5, to: 1) {
                try display(after, token: $0)
            }
        } else {
            beforeShown = try display(before, token: nil)
            afterShown = try display(after, token: nil)
        }
        beforeDisplay = beforeShown.0
        afterDisplay = afterShown.0
        beforeExtendedPeak = beforeShown.1
        afterExtendedPeak = afterShown.1
        extendedPeak = [beforeShown.1, afterShown.1].compactMap { $0 }.max()
    }

    /// The output pixels of one f32 side's extended image for `viewport`
    /// (#680): `ceil(w / divisor) × ceil(h / divisor)` of its Fit region, or
    /// at 100 % the pane's detail extent (at most 1024 per side), which bounds
    /// every detail region the frame can draw. 0 for a u16 image or a screen
    /// without headroom.
    static func extendedOutputPixels(_ image: RasterPrecisionImage?,
                                     viewport: RawDevelopViewport) -> UInt64 {
        guard viewport.extendedRange, let image, image.isFloatStorage else { return 0 }
        if viewport.nativeDetail {
            let width = max(1, min(1024, Int(viewport.backingSize.width.rounded(.down))))
            let height = max(1, min(1024, Int(viewport.backingSize.height.rounded(.down))))
            return UInt64(width) * UInt64(height)
        }
        let r = viewport.region(image)
        return UInt64((r.width + r.divisor - 1) / r.divisor) * UInt64((r.height + r.divisor - 1) / r.divisor)
    }
}

/// The shared final worker runs at most one native decode/inspection. Pending
/// slider and pan requests coalesce, and a generation check rejects old frames.
/// A cancelled window keeps the task owner alive until its mailbox is consumed,
/// releasing the global worker slot and all captured inputs even after UI close.
final class RawDevelopPreviewSession {
    private struct Request {
        let generation: UInt64
        let descriptor: RawSourceDescriptor?
        let whiteBalanceChanged: Bool
        let viewport: RawDevelopViewport
    }
    private struct ResultValue {
        let before: RawDevelopInspectionSource
        let after: RawDevelopInspectionSource
        let frame: RawDevelopPreviewFrame
    }
    private struct Camera {
        let probe: RawProbe
        let descriptor: RawSourceDescriptor
        let snapshot: RawSourceSnapshot
    }

    private let id = UUID()
    private let camera: Camera?
    private let initialBefore: RasterPrecisionImage?
    private let fixedAfter: RasterPrecisionImage?
    private var before: RawDevelopInspectionSource?
    private var after: RawDevelopInspectionSource?
    private var afterDescriptor: RawSourceDescriptor?
    private var pending: Request?
    private var active: Request?
    private var task: WorkerTask<ResultValue>?
    private var timer: Timer?
    private var generation: UInt64 = 0
    private var deadline = Date.distantPast
    private var closed = false
    private let payload: PrecisionEditPayload

    // Main-thread-only parking. The timer callback can be weak without losing
    // an in-flight WorkerTask when a modal controller closes.
    private static var activeSessions: [UUID: RawDevelopPreviewSession] = [:]
    /// The viewport-only frame's extended-image term (#680): bytes per output
    /// pixel of each f32 side's extended image, beside the fixed 48 MiB. This
    /// session's own term, not a shared constant: ⌈⌈1.25 × max E⌉ / output
    /// pixels⌉ over the VF32 cases (`scripts/raw-decode-probe`, mode `frame`,
    /// mirrored there). Measured in Release (one case per process,
    /// MallocLargeCache=0, 3 runs + twin, every twin E 0) on the NEF, D24
    /// and D850: max E is 12.0 B per output pixel at Fit (VF32F) and 1024²
    /// 100 % (VF32D), so ⌈1.25 × max E⌉ needs 16 per pixel; the whole charge
    /// ÷ max E is 2.723/2.724/2.989 (VF32F) and 3.327/3.329/3.329 (VF32D).
    static let extendedFramePerPixel: UInt64 = 16
    var onFrame: ((RawDevelopPreviewFrame) -> Void)?
    var onStatus: ((String, Bool) -> Void)?

    init(probe: RawProbe, descriptor: RawSourceDescriptor, snapshot: RawSourceSnapshot,
         beforeSource: RasterPrecisionImage?) throws {
        camera = Camera(probe: probe, descriptor: descriptor, snapshot: snapshot)
        initialBefore = beforeSource
        fixedAfter = nil
        payload = try PrecisionEditPayload.admitted(bytes: 256 * 1024)
    }

    init(before: RasterPrecisionImage, after: RasterPrecisionImage) throws {
        camera = nil
        initialBefore = before
        fixedAfter = after
        payload = try PrecisionEditPayload.admitted(bytes: 256 * 1024)
    }

    func request(descriptor: RawSourceDescriptor? = nil, whiteBalanceChanged: Bool = false,
                 viewport: RawDevelopViewport) {
        assert(Thread.isMainThread)
        guard !closed else { return }
        generation &+= 1
        pending = Request(generation: generation, descriptor: descriptor,
            whiteBalanceChanged: whiteBalanceChanged, viewport: viewport)
        if afterDescriptor != descriptor { after = nil }
        deadline = Date().addingTimeInterval(0.12)
        onStatus?("Updating native preview…", false)
        keepPolling()
    }

    func cancel() {
        assert(Thread.isMainThread)
        closed = true
        pending = nil
        onFrame = nil
        onStatus = nil
        before = nil
        after = nil
        if task == nil { stopPolling() }
    }

    private func keepPolling() {
        Self.activeSessions[id] = self
        guard timer == nil else { return }
        let timer = Timer(timeInterval: 0.02, repeats: true) { [weak self] _ in self?.poll() }
        self.timer = timer
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
    }

    private func stopPolling() {
        timer?.invalidate()
        timer = nil
        Self.activeSessions.removeValue(forKey: id)
    }

    private func poll() {
        if let task, let result = task.take() {
            self.task = nil
            let request = active
            active = nil
            if !closed {
                switch result {
                case .success(let value):
                    before = value.before
                    if request?.generation == generation {
                        after = value.after
                        afterDescriptor = request?.descriptor
                        onFrame?(value.frame)
                        onStatus?(request?.descriptor?.recipe.isLinearFloat == true
                            ? "32-bit float preview ready"
                            : "Native 16-bit preview ready", true)
                    }
                case .failure(let error):
                    if request?.generation == generation {
                        onStatus?(error.localizedDescription, false)
                    }
                }
            }
        }
        if closed { if task == nil { stopPolling() }; return }
        guard task == nil, let request = pending, Date() >= deadline else {
            if task == nil && pending == nil { stopPolling() }
            return
        }
        // Another app operation may have started since the preview request.
        // Wait for its existing serial worker instead of allocating alongside it.
        guard !BackgroundWork.occupied else { return }
        pending = nil
        do {
            let scratch: UInt64
            if let camera, after == nil {
                // A float development is charged at the float decode
                // figure plus 24 B/px for its inspection copies.
                let float = request.descriptor?.recipe.isLinearFloat == true
                    || camera.descriptor.recipe.isLinearFloat
                let width = Int(camera.probe.pixelSize.width)
                let height = Int(camera.probe.pixelSize.height)
                scratch = try PrecisionDocumentDescriptor.sum(
                    RawSourceWork.scratch(width: width, height: height,
                        encoded: UInt64(camera.snapshot.bytes.count), float: float),
                    float ? UInt64(width) * UInt64(height) * 24 : 0)
            } else {
                // #680: a viewport-only frame of known sides draws each f32
                // side's extended image on a screen with headroom, charged
                // per output pixel beside the fixed figure.
                let shownAfter = self.afterDescriptor == request.descriptor ? self.after : nil
                let sides = [self.before.map { $0.extendedMaster ?? $0.image } ?? self.initialBefore,
                             shownAfter.map { $0.extendedMaster ?? $0.image } ?? self.fixedAfter]
                let pixels = sides.reduce(UInt64(0)) {
                    $0 + RawDevelopPreviewFrame.extendedOutputPixels($1, viewport: request.viewport)
                }
                scratch = 48 * 1024 * 1024 + Self.extendedFramePerPixel * pixels
            }
            _ = try RasterResources.shared.admit(scratch: scratch)
            let camera = self.camera
            let knownBefore = before
            let knownAfter = afterDescriptor == request.descriptor ? after : nil
            let initialBefore = self.initialBefore
            let fixedAfter = self.fixedAfter
            let payload = self.payload
            active = request
            task = try WorkerTask(scratchEstimate: scratch) {
                try withExtendedLifetime(payload) {
                    let before: RawDevelopInspectionSource
                    if let knownBefore { before = knownBefore }
                    else if let initialBefore { before = try RawDevelopInspectionSource(initialBefore) }
                    else if let camera {
                        let developed = try RetainedRawDecoder.develop(url: camera.snapshot.url,
                            descriptor: camera.descriptor, maximumScratch: scratch)
                        guard let image = developed.0.precision?.layerSource(0) else {
                            throw DoctoredCoreError(message: "The RAW before image has no native samples.")
                        }
                        before = try RawDevelopInspectionSource(image)
                    } else { throw DoctoredCoreError(message: "The RAW before image is unavailable.") }

                    let after: RawDevelopInspectionSource
                    if let knownAfter { after = knownAfter }
                    else if let fixedAfter { after = try RawDevelopInspectionSource(fixedAfter) }
                    else if let camera, let descriptor = request.descriptor {
                        if descriptor == camera.descriptor { after = before }
                        else {
                            let developed = try RetainedRawDecoder.develop(url: camera.snapshot.url,
                                descriptor: descriptor, maximumScratch: scratch,
                                whiteBalanceChanged: request.whiteBalanceChanged)
                            guard let image = developed.0.precision?.layerSource(0) else {
                                throw DoctoredCoreError(message: "The RAW after image has no native samples.")
                            }
                            after = try RawDevelopInspectionSource(image)
                        }
                    } else { throw DoctoredCoreError(message: "The RAW development request is unavailable.") }
                    let frame = try RawDevelopPreviewFrame(before: before, after: after, viewport: request.viewport)
                    return ResultValue(before: before, after: after, frame: frame)
                }
            }
        } catch {
            active = nil
            onStatus?(error.localizedDescription, false)
            stopPolling()
        }
    }
}
