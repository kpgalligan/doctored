import AppKit

// One coalescing `WorkerTask` prepares the candidate and native/display projection, and Apply
// publishes that same owner through central history. Cancelled workers remain parked only until
// mailbox drain.

/// Coalescing final-quality work. A cancelled owner remains parked until its
/// mailbox is drained, releasing the shared worker slot and scratch together.
final class ContentAwareFillReview {
    let id = UUID().uuidString
    let capture: ImageSourceCapture
    let layer: Int
    weak var document: ImageDocument?
    private let coverage: [UInt8]
    private let nativeCoverage: [UInt16]?
    private let payload: PrecisionEditPayload
    private var task: WorkerTask<ContentAwareFillCandidate>?
    private var control: RasterJobControl?
    private var timer: Timer?
    private var closeObserver: NSObjectProtocol?
    private var generation: UInt64 = 0
    private var activeGeneration: UInt64 = 0
    private var queued: ContentAwareFillSettings?
    private var closed = false
    private var deadline = Date.distantPast
    private static var parked: [String: ContentAwareFillReview] = [:]
    /// A review is preparing (debounced or on the worker) or draining a
    /// cancelled task: the MCP worker wait never waits on it.
    static var isRunning: Bool {
        parked.values.contains { $0.task != nil || $0.state == "running" }
    }
    private(set) var candidate: ContentAwareFillCandidate?
    private(set) var settings = ContentAwareFillSettings()
    private(set) var state = "idle"
    private(set) var message = "Choose a source area and prepare the final result."
    var onChange: (() -> Void)?

    init(document: ImageDocument, layer: Int, coverage: [UInt8]) throws {
        capture = try ImageSourceCapture(document)
        self.document = document; self.layer = layer; self.coverage = coverage
        nativeCoverage = document.doc?.isHighDepth == true ? document.canvasSelection?.maskSamples16() : nil
        // Before/After providers retain full display RGBA planes. Reserve
        // their lifecycle plus the coverage/geometry graphs, not native RGB.
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(coverage.count) * 9 + 2 * 1024 * 1024)
        closeObserver = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
            object: document, queue: nil) { [weak self] _ in self?.cancel() }
    }
    deinit { if let closeObserver { NotificationCenter.default.removeObserver(closeObserver) } }

    var report: [String: Any] {
        var result = settings.arguments(layer: layer)
        result["preview_id"] = id; result["state"] = state; result["message"] = message
        result["progress"] = control?.progress ?? (candidate == nil ? 0 : 1)
        result["stage"] = control?.stage ?? 0
        result["scratch_bytes"] = control?.scratchBytes ?? 0
        result["selected_pixels"] = coverage.reduce(0) { $0 + ($1 > 0 ? 1 : 0) }
        result["revision"] = capture.revision
        result["comparison"] = "Before then After: display images of the exact captured final candidate. Apply performs no second synthesis."
        return result
    }

    func invalidate() {
        guard !closed else { return }
        generation &+= 1; queued = nil; candidate = nil; control?.cancel()
        state = "idle"; message = "Settings changed. Prepare a new final result."
        onChange?()
    }
    func request(_ settings: ContentAwareFillSettings) {
        guard !closed else { return }
        generation &+= 1; self.settings = settings; queued = settings
        candidate = nil; control?.cancel(); deadline = Date().addingTimeInterval(0.10)
        state = "running"; message = "Preparing final result…"
        keepPolling(); onChange?()
    }
    func cancel() {
        closed = true; generation &+= 1; queued = nil; candidate = nil
        state = "cancelled"; message = "Preview discarded; no edit was applied."
        control?.cancel(); onChange?(); onChange = nil
        ContentAwareFillTickets.release(id)
        if task == nil { stopPolling() }
    }
    func wait() throws {
        // Same event-servicing wait seam as final jobs. Poll starts only after
        // any cancelled predecessor has drained, so one worker stays app-wide.
        deadline = .distantPast; poll()
        guard let task else { throw DoctoredCoreError(message: message) }
        do { finish(.success(try BackgroundWork.wait(task, title: "Preparing Content-Aware Fill", control: control))) }
        catch { finish(.failure(error)) }
        guard state == "ready" else { throw DoctoredCoreError(message: message) }
    }
    func requireCandidate() throws -> ContentAwareFillCandidate {
        guard !closed, let document else { throw DoctoredCoreError(message: "This fill preview is closed.") }
        do { try capture.requireCurrent(document) }
        catch {
            // A rejected acceptance cannot become current again: release the
            // unusable owner immediately instead of waiting for ticket expiry.
            cancel()
            throw error
        }
        guard state == "ready", let candidate else { throw DoctoredCoreError(message: message) }
        return candidate
    }
    private func keepPolling() {
        Self.parked[id] = self
        guard timer == nil else { return }
        let timer = Timer(timeInterval: 0.025, repeats: true) { [weak self] _ in
            guard !BackgroundWork.isWaiting else { return }
            self?.poll()
        }
        self.timer = timer
        RunLoop.main.add(timer, forMode: .common); RunLoop.main.add(timer, forMode: .modalPanel)
    }
    private func stopPolling() {
        timer?.invalidate(); timer = nil; Self.parked.removeValue(forKey: id)
    }
    private func finish(_ completion: Result<ContentAwareFillCandidate, Error>) {
        task = nil; control = nil
        if !closed, activeGeneration == generation {
            do {
                guard let document else { throw DoctoredCoreError(message: "The document closed.") }
                try capture.requireCurrent(document)
                candidate = try completion.get(); state = "ready"
                message = "Ready · Apply uses this exact result"
            } catch { candidate = nil; state = "failed"; message = error.localizedDescription }
            onChange?()
        }
        if queued == nil { stopPolling() }
    }
    func poll() {
        if let completion = task?.take() { finish(completion) }
        if closed { if task == nil { stopPolling() }; return }
        if let control, state == "running" {
            let stages = ["Preparing", "Reading pixels", "Planning", "Filling", "Blending", "Finishing", "Rendering"]
            let status = control.isCancelled ? "Updating settings…" :
                "\(stages[min(Int(control.stage), stages.count - 1)])… · \(Int(control.progress * 100))%"
            if message != status { message = status; onChange?() }
        }
        guard task == nil, let settings = queued, Date() >= deadline, !BackgroundWork.occupied else { return }
        queued = nil
        do {
            guard let document else { throw DoctoredCoreError(message: "The document closed.") }
            try capture.requireCurrent(document)
            let source = capture.source, payload = self.payload
            let work = try ContentAwareFillWork(source: source, layer: layer, coverage: coverage, settings: settings, nativeCoverage: nativeCoverage)
            let estimate = max(work.scratch, try source.projectionScratchEstimate())
            _ = try RasterResources.shared.admit(scratch: estimate)
            let token = try RasterJobControl()
            guard token.setScratchLimit(estimate) else { throw DoctoredCoreError(message: "Could not configure fill memory.") }
            activeGeneration = generation; control = token
            task = try WorkerTask(scratchEstimate: estimate) {
                try withExtendedLifetime(payload) {
                    let updated = try token.withChildOperation(from: 0, to: 0.9) { try work.compute(control: $0) }
                    guard let updated else { throw DoctoredCoreError(message: "The fill produced no change.") }
                    let before = PreparedDocumentEdit(source)
                    if let error = before.error { throw DoctoredCoreError(message: error) }
                    guard !token.isCancelled else { throw JobCancelled() }
                    let after = PreparedDocumentEdit(updated)
                    if let error = after.error { throw DoctoredCoreError(message: error) }
                    guard let image = before.preview, after.preview != nil else {
                        throw DoctoredCoreError(message: "The exact fill comparison could not be displayed.")
                    }
                    guard !token.isCancelled else { throw JobCancelled() }
                    return ContentAwareFillCandidate(prepared: after, before: image, payload: payload)
                }
            }
        } catch { state = "failed"; message = error.localizedDescription; stopPolling(); onChange?() }
    }
}

/// One expiring app-wide review, matching the RAW migration review contract.
enum ContentAwareFillTickets {
    private static var current: ContentAwareFillReview?
    private static var expiry: Timer?
    static func store(_ value: ContentAwareFillReview) {
        current?.cancel(); expiry?.invalidate(); current = value
        let timer = Timer(timeInterval: 300, repeats: false) { _ in discard() }
        expiry = timer; RunLoop.main.add(timer, forMode: .common); RunLoop.main.add(timer, forMode: .modalPanel)
    }
    static func find(_ id: String) throws -> ContentAwareFillReview {
        guard let current, current.id == id else { throw DoctoredCoreError(message: "Unknown or expired fill preview_id.") }
        return current
    }
    static func release(_ id: String) {
        guard current?.id == id else { return }
        current = nil; expiry?.invalidate(); expiry = nil
    }
    static func discard() {
        let review = current; current = nil; expiry?.invalidate(); expiry = nil; review?.cancel()
    }
}
