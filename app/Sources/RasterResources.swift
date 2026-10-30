import Foundation

// `RasterResources` deduplicates one full live set through core diagnostics and includes reserved
// scratch and the explicit list of unaccounted platform allocations. Admission uses the process
// working limit (`WorkingLimit.current.effectiveBytes`, fixed for the process lifetime; its private
// `init` hands it to core through `DoctoredCoreProcess.setWorkingLimit`, and `report()` adds the
// `working_limit` object); core caches over working limit / 8 (all-or-nothing) are
// evicted after projection refresh (the trim compares `cache_bytes − contiguous_view_bytes`:
// 13B-2b's contiguous layer views are excluded because every projection re-reads them, and they
// stay bounded by the core's own 1 GiB view cache); that check runs at most
// once per second on main — a refresh inside the interval schedules one trailing check on the main
// run loop's `.default` mode, so it waits for the next `.default` turn and a long drag or modal
// wait can postpone it, while `admit` still evicts and remeasures before any refusal — and
// registration compacts dead weak boxes only when a list doubles. Job admission preserves caches
// when the full retained-plus-scratch peak fits, otherwise evicts and remeasures before refusal.
// These are working estimates and cache policy, not an OS memory ceiling. `liveHandleSummary()`
// lists each live registered wrapper by kind and size ("doc16 W×H", "precisionImage W×H", "image
// W×H", …) plus the typed-owner, RAW-mask and payload counts, retaining nothing — the jobs probe's
// handle snapshots.

/// Weak registration follows actual ownership, including NSUndoManager closures,
/// live-edit bases, previews and jobs. It never keeps a discarded snapshot alive.
/// Registry access is synchronized because wrappers are also created by workers.
final class RasterResources {
    static let shared = RasterResources()
    private final class Weak<T: AnyObject> {
        weak var value: T?
        init(_ value: T) { self.value = value }
    }
    /// Dead boxes are dropped when the list reaches `threshold`, which
    /// then doubles past the live count, so registration is amortised O(1).
    private struct WeakList<T: AnyObject> {
        private var boxes: [Weak<T>] = []
        private var threshold: Int = RasterResources.compactionFloor

        mutating func append(_ value: T) {
            if boxes.count >= threshold {
                boxes.removeAll { $0.value == nil }
                threshold = max(RasterResources.compactionFloor, 2 * boxes.count)
            }
            boxes.append(Weak(value))
        }

        var live: [T] { boxes.compactMap(\.value) }
    }
    private static let compactionFloor = 64
    private let lock = NSLock()
    private var documents = WeakList<RasterDocument>()
    private var images = WeakList<RasterImage>()
    private var precisionDocuments = WeakList<RasterPrecisionDocument>()
    private var precisionImages = WeakList<RasterPrecisionImage>()
    private var rawLocalMasks = WeakList<RasterRawLocalMask>()
    private var precisionPayloads = WeakList<PrecisionEditPayload>()

    // The RAM-scaled policy (`WorkingLimit`): fixed for the process lifetime.
    let workingLimit: UInt64
    /// The post-projection trim threshold, core's `dr_process_cache_trim_limit`.
    let cacheLimit: UInt64
    private static let trimSpacing: TimeInterval = 1
    private var lastTrimCheck: TimeInterval?  // main thread only
    private var trailingTrim: Timer?  // main thread only
    /// Visible documents: written on main by `ImageDocument`, read by
    /// `admit` off-main to choose its remainder sentence. Never gates admission.
    private var openDocumentCount = 0  // guarded by `lock`

    /// Core's process limit follows the host's, so internal admissions with
    /// no caller limit refuse at the same bound.
    private init() {
        let limit = WorkingLimit.current.effectiveBytes
        workingLimit = limit
        DoctoredCoreProcess.setWorkingLimit(limit)
        cacheLimit = DoctoredCoreProcess.cacheTrimLimit()
    }

    func noteDocumentOpened() {
        lock.lock()
        openDocumentCount += 1
        lock.unlock()
    }

    func noteDocumentClosed() {
        lock.lock()
        openDocumentCount = max(0, openDocumentCount - 1)
        lock.unlock()
    }

    func register(_ document: RasterDocument) {
        lock.lock()
        documents.append(document)
        lock.unlock()
    }

    func register(_ image: RasterImage) {
        lock.lock()
        images.append(image)
        lock.unlock()
    }

    func register(_ document: RasterPrecisionDocument) {
        lock.lock()
        precisionDocuments.append(document)
        lock.unlock()
    }

    func register(_ image: RasterPrecisionImage) {
        lock.lock()
        precisionImages.append(image)
        lock.unlock()
    }

    func register(_ payload: PrecisionEditPayload) {
        lock.lock()
        precisionPayloads.append(payload)
        lock.unlock()
    }
    func register(_ mask: RasterRawLocalMask) {
        lock.lock()
        rawLocalMasks.append(mask)
        lock.unlock()
    }

    func report(scratch: UInt64 = 0, evict: Bool = false) throws -> [String: Any] {
        lock.lock()
        let docs = documents.live
        let rasters = images.live
        let typedDocs = precisionDocuments.live
        let typedImages = precisionImages.live
        let masks = rawLocalMasks.live
        let typedPayloads = precisionPayloads.live
        lock.unlock()
        if evict { _ = try RasterDocument.evictResourceCaches(docs, precisionDocuments: typedDocs) }
        let previews = SelectionWorkBudget.scratchBytes
        let (totalScratch, overflow) = scratch.addingReportingOverflow(previews)
        guard !overflow else { throw DoctoredCoreError(message: "Working-memory estimate exceeds its supported range.") }
        var report = try RasterDocument.mixedResourceReport(
            documents: docs, images: rasters, precisionDocuments: typedDocs,
            precisionImages: typedImages, rawLocalMasks: masks, scratch: totalScratch, limit: workingLimit)
        report["precision_accounting"] = "One allocation union across byte and typed handles, including shared source masters and profiles. This report is not process RSS."
        let payloadBytes = try Self.mergePrecisionPayloads(typedPayloads, into: &report, limit: workingLimit)
        report["selection_preview_scratch_bytes"] = previews
        report["host_tracking"] = "all live wrappers: documents, undo/redo, jobs, projections, previews"
        report["host_document_handles"] = docs.count
        report["host_legacy_document_handles"] = docs.filter { !$0.isHighDepth }.count
        report["host_precision_document_facades"] = docs.filter(\.isHighDepth).count
        report["host_image_handles"] = rasters.count
        report["host_precision_document_handles"] = typedDocs.count
        report["host_precision_image_handles"] = typedImages.count
        report["host_raw_local_mask_handles"] = masks.count
        report["host_precision_capture_payloads"] = typedPayloads.count
        report["precision_capture_payload_bytes"] = payloadBytes
        report["cache_limit_bytes"] = cacheLimit
        report["working_limit"] = WorkingLimit.current.report()
        return report
    }

    /// Instrumentation (the jobs probe's transform admission halves):
    /// one line per live registered wrapper — "doc8 W×H", "doc16 W×H",
    /// "doc32 W×H", "precisionImage W×H", "image W×H" — then the counts of
    /// typed owners, RAW local masks and capture payloads. Reads the weak
    /// lists only and retains nothing past the call.
    func liveHandleSummary() -> [String] {
        lock.lock()
        let docs = documents.live
        let rasters = images.live
        let typedDocs = precisionDocuments.live.count
        let typedImages = precisionImages.live
        let masks = rawLocalMasks.live.count
        let payloads = precisionPayloads.live.count
        lock.unlock()
        var lines = docs.map { doc -> String in
            let depth = doc.isFloat ? "doc32" : doc.isHighDepth ? "doc16" : "doc8"
            return "\(depth) \(doc.width)×\(doc.height)"
        }
        lines += typedImages.map { "precisionImage \($0.width)×\($0.height)" }
        lines += rasters.map { "image \($0.width)×\($0.height)" }
        lines += ["precisionDocuments \(typedDocs)", "rawLocalMasks \(masks)", "payloads \(payloads)"]
        return lines
    }

    @discardableResult
    static func mergePrecisionPayloads(_ payloads: [PrecisionEditPayload],
                                      into report: inout [String: Any], limit: UInt64) throws -> UInt64 {
        var owners: [ObjectIdentifier: PrecisionEditPayload] = [:]
        for payload in payloads { owners[ObjectIdentifier(payload)] = payload }
        let bytes = try owners.values.reduce(UInt64(0)) { try PrecisionDocumentDescriptor.sum($0, $1.bytes) }
        guard !owners.isEmpty else { return 0 }
        for key in ["unique_retained_bytes", "referenced_bytes"] {
            report[key] = try PrecisionDocumentDescriptor.sum(PrecisionDocumentDescriptor.integer(report, key), bytes)
        }
        report["allocation_count"] = try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.integer(report, "allocation_count"), UInt64(owners.count))
        let working = try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes"),
            PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes"))
        report["estimated_working_bytes"] = working
        report["within_working_limit"] = limit == 0 || working <= limit
        var categories = report["categories"] as? [String: Any] ?? [:]
        categories["precision.host_capture_payload"] = ["unique_bytes": bytes,
            "referenced_bytes": bytes, "allocation_count": UInt64(owners.count)]
        report["categories"] = categories
        return bytes
    }

    /// A leading check, then at most one trailing check per
    /// `trimSpacing` on the main run loop's `.default` mode.
    func trimCachesIfNeeded() {
        guard Thread.isMainThread else {
            checkCaches()
            return
        }
        let now = ProcessInfo.processInfo.systemUptime
        if let last = lastTrimCheck, now - last < Self.trimSpacing {
            guard trailingTrim == nil else { return }
            let delay = last + Self.trimSpacing - now
            let timer = Timer(timeInterval: delay, repeats: false) { [weak self] _ in
                guard let self else { return }
                self.trailingTrim = nil
                self.lastTrimCheck = ProcessInfo.processInfo.systemUptime
                self.checkCaches()
            }
            trailingTrim = timer
            RunLoop.main.add(timer, forMode: .default)
            return
        }
        trailingTrim?.invalidate()
        trailingTrim = nil
        lastTrimCheck = now
        checkCaches()
    }

    private func checkCaches() {
        // Contiguous layer views are excluded: every projection re-reads them,
        // so trimming them after each projection would only re-materialize.
        guard let current = try? report(),
              let bytes = (current["cache_bytes"] as? NSNumber)?.uint64Value else { return }
        let views = (current["contiguous_view_bytes"] as? NSNumber)?.uint64Value ?? 0
        guard bytes - min(views, bytes) > cacheLimit else { return }
        _ = try? report(evict: true)
    }

    static let smallerJobRemedy = "Work on a smaller image or region."
    static let depthJobRemedy =
        "Use a smaller image, or lower its depth first with Image ▸ Mode ▸ Convert Precision…."

    /// `own` is the job's non-evictable input already retained by its own
    /// document. When scratch plus `own` reaches the whole working
    /// limit, closing other documents cannot help, so the refusal says so.
    func admit(scratch: UInt64, own: UInt64 = 0,
               remedy: String = RasterResources.smallerJobRemedy) throws -> UInt64 {
        let (sum, overflow) = scratch.addingReportingOverflow(own)
        let need = overflow ? UInt64.max : sum
        if need >= workingLimit {
            func memory(_ bytes: UInt64) -> String {
                ByteCountFormatter.string(fromByteCount: Int64(min(bytes, UInt64(Int64.max))),
                                          countStyle: .memory)
            }
            throw DoctoredCoreError(message: "This job needs \(memory(need)) of working memory on its own, "
                + "at least the \(memory(workingLimit)) working limit, so closing other documents "
                + "will not help. \(remedy)")
        }
        func remaining(_ report: [String: Any]) throws -> UInt64? {
            // Rust owns the complete allocation union, including live caches.
            guard let measured = report["unique_retained_bytes"] as? NSNumber else {
                throw DoctoredCoreError(message: "Resource accounting did not return retained bytes.")
            }
            let retained = measured.uint64Value
            let totalScratch = (report["transient_estimate_bytes"] as? NSNumber)?.uint64Value ?? UInt64.max
            guard retained <= workingLimit, totalScratch < workingLimit - retained else { return nil }
            return workingLimit - retained - totalScratch
        }
        // Disposable core caches survive when the full requested peak
        // fits. Pressure evicts and remeasures before refusing; post-projection
        // cache trimming retains its independent policy: working limit / 8
        // (all-or-nothing).
        if let available = try remaining(report(scratch: scratch)) { return available }
        if let available = try remaining(report(scratch: scratch, evict: true)) { return available }
        lock.lock()
        let others = openDocumentCount > 1
        lock.unlock()
        guard others else {
            throw DoctoredCoreError(message: "This job needs more working memory than the current budget, "
                + "and no other document is open to close. \(remedy)")
        }
        throw DoctoredCoreError(message: "This job needs more working memory than the current budget. "
            + "Close other documents or reduce the region before trying again.")
    }

    /// nil when `admit(scratch:own:)` would admit (same evict-and-remeasure); else the bytes over the limit, ≥ 1.
    /// The estimate behind undo, redo and History navigation's warning, which never refuse.
    func overLimitBytes(scratch: UInt64, own: UInt64) throws -> UInt64? {
        let (sum, overflow) = scratch.addingReportingOverflow(own)
        let need = overflow ? UInt64.max : sum
        if need >= workingLimit { return max(1, need - workingLimit) }
        func over(_ report: [String: Any]) throws -> UInt64? {
            guard let measured = report["unique_retained_bytes"] as? NSNumber else {
                throw DoctoredCoreError(message: "Resource accounting did not return retained bytes.")
            }
            let retained = measured.uint64Value
            let totalScratch = (report["transient_estimate_bytes"] as? NSNumber)?.uint64Value ?? UInt64.max
            if retained <= workingLimit, totalScratch < workingLimit - retained { return nil }
            let (total, totalOverflow) = retained.addingReportingOverflow(totalScratch)
            return max(1, (totalOverflow ? UInt64.max : total) - workingLimit)
        }
        guard try over(report(scratch: scratch)) != nil else { return nil }
        return try over(report(scratch: scratch, evict: true))
    }
}

extension RasterDocument {
    /// The document's own non-evictable retained bytes, either depth:
    /// `unique_retained_bytes − cache_bytes` of a single-document resource
    /// report, which covers a typed document through its precision handle.
    /// A lower bound of what the open document keeps through a job.
    func ownRetainedBytes() throws -> UInt64 {
        let inventory = try RasterDocument.resourceReport(documents: [self], images: [], scratch: 0, limit: 0)
        guard let retained = (inventory["unique_retained_bytes"] as? NSNumber)?.uint64Value,
              let caches = (inventory["cache_bytes"] as? NSNumber)?.uint64Value else {
            throw DoctoredCoreError(message: "Could not estimate the document's own memory.")
        }
        return retained - min(retained, caches)
    }
}
