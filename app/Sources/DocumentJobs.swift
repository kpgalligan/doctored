import AppKit

// Final Content-Aware Fill, spot healing and the 8-bit Healing Brush and Patch commits
// (`healJob` / `healToNewLayerJob`, a display pin of the drag's picture cleared by `clearHealPin`)
// carry an owned Rust `RasterJobControl` for cooperative cancellation/progress. Their MCP twins
// `heal_stroke`/`patch_region` run as jobs too.
//
// A nonmodal job's panel appears only after `DocumentJobs.panelDelay` (0.25 s), and
// `DocumentJobs.refusesGesture` — settle-poll first, then refuse a worker-bound gesture while any
// job/worker runs and any other while a job runs on the same document — guards byte and 16-bit
// stroke begins and the Patch commit; completion checks monotonic document revision, immutable
// handle, captured target and selection, then uses the central mutation path in one explicit undo
// group; a result that path refuses (linear-light, persisted warp, a beep guard) ends the job
// `refused` with the sentence, never `completed` and never recorded, so a waiting MCP call answers
// an in-band error (an MCP job — `finishJobCall` sets `agentOrigin` — raises no alert; a UI job
// presents the sentence). `applyEdit`'s depth beep guard is answered before the group too — both
// through the shared `ImageDocument.preflightPublication` — so a refused result leaves no empty
// undo step (the post-group identity check is only a backstop).
//
// MCP `undo`/`redo`/`restore_history` refuse by name while any job runs on the document
// (`DocumentJobs.historyRefusal`, asked before the manager is touched) and while its undo group is
// open or deferred, a defensive refusal: no current path services a call inside one; `undo`/`redo`
// answer an error for a step that moved no history (`ImageDocument.stepHistory`); choosing another
// History state cancels the running UI job the way ⌘Z does.

/// The unpublished document a job computes on instead of the published one, and the selection
/// its publication leaves (a crop past the canvas with a Fill computes the fill on the
/// cropped document and publishes crop and fill as one canvas-geometry step).
struct DocumentJobBase {
    let document: RasterDocument
    let resultingSelection: SelectionHistoryState?
}

/// Main-thread owner of one final edit. A revision is monotonic even through
/// undo/revert; snapshot identity plus layer index/fingerprint binds the target
/// to that exact immutable stack, including otherwise identical layers.
final class DocumentJob {
    let id = UUID().uuidString
    let title: String
    let revision: UInt64
    let layer: Int
    let fingerprint: String?
    let droppedKind: DescribedKind?
    var details: [String: Any] = [:]
    let control: RasterJobControl
    let started = Date()
    let scratch: UInt64
    let renderScratch = JobScratch()
    var selection: SelectionHistoryState?
    let activeLayer: Int
    let selectedLayers: [Int]
    let paintTarget: PaintTarget
    let quickMask: Bool
    weak var document: ImageDocument?
    var source: RasterDocument?
    /// What `compute` ran on instead of `source`, published as a canvas-geometry step; released
    /// when the job finishes.
    var base: DocumentJobBase?
    var task: WorkerTask<PreparedDocumentEdit?>?
    var panel: JobProgressController?
    /// Whether this job shows the progress panel at all; the panel itself
    /// appears only once the job has run for `DocumentJobs.panelDelay`.
    var showProgress = false
    /// Set by `AgentServer.finishJobCall` (every MCP job passes through it on
    /// main before the job can finish): a publication refusal is answered in
    /// band only, with no alert.
    var agentOrigin = false
    var timer: Timer?
    var record: [ActionStep]
    var state = "running"
    var message: String?
    var finished: Date?
    var completion: ((DocumentJob) -> Void)?
    /// Edit ▸ Fade's hint for the edit this job commits, when its record
    /// cannot carry one (a stroke, an agent call); `finish` hands it to the
    /// document right before its `applyEdit`.
    var fade: FadeHint?

    init(document: ImageDocument, layer: Int, title: String, scratch: UInt64,
         record: [ActionStep], control: RasterJobControl, rasterizesTarget: Bool) {
        self.activeLayer = document.activeLayerIndex
        self.selectedLayers = document.selectedLayerIndices
        self.paintTarget = document.planeEditTarget
        self.quickMask = document.quickMaskActive
        self.selection = document.selectionHistory
        self.document = document
        self.source = document.doc
        self.revision = document.revision
        self.layer = layer
        self.fingerprint = document.doc?.layerFingerprint(layer)
        self.droppedKind = rasterizesTarget ? LayerDescription.claimedKind(of: document.doc?.layerMeta(layer)) : nil
        self.title = title
        self.scratch = scratch
        self.record = record
        self.control = control
    }

    var report: [String: Any] {
        var result: [String: Any] = ["job_id": id, "action": title, "state": state,
            "revision": revision, "layer": layer, "progress": control.progress,
            "cancel_requested": control.isCancelled,
            "elapsed_ms": Int((finished ?? Date()).timeIntervalSince(started) * 1000),
            "scratch_estimate_bytes": state == "running" ? scratch + control.scratchBytes + renderScratch.bytes : 0]
        result.merge(details) { existing, _ in existing }
        if state == "completed", let droppedKind {
            switch droppedKind {
            case .text: result["rasterized_text"] = true
            case .shape: result["rasterized_shape"] = true
            case .fill: result["rasterized_fill"] = true
            }
            result["note"] = "This edit rasterized the layer's source description. Undo restores it."
        }
        if let message { result["message"] = message }
        return result
    }
}

enum DocumentJobs {
    /// How long a job runs before its progress panel appears: a heal dab that
    /// solves in a frame or two never flashes a panel.
    static let panelDelay: TimeInterval = 0.25
    private(set) static var active: DocumentJob?
    private static var recent: [DocumentJob] = []

    static func find(_ id: String) -> DocumentJob? {
        if active?.id == id { return active }
        return recent.first { $0.id == id }
    }

    @discardableResult
    static func start(document: ImageDocument, layer: Int, title: String,
                      scratch: UInt64, record: [ActionStep], showProgress: Bool,
                      rasterizesTarget: Bool = true,
                      base: DocumentJobBase? = nil,
                      compute: @escaping (RasterDocument, RasterJobControl) throws -> RasterDocument?,
                      completion: ((DocumentJob) -> Void)? = nil) throws -> DocumentJob {
        assert(Thread.isMainThread)
        guard !document.isLiveEditing, let source = document.doc else {
            throw DoctoredCoreError(message: "Finish the current gesture before starting this job.")
        }
        guard active == nil, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Another background operation is running. Wait or cancel it first.")
        }
        let available = try RasterResources.shared.admit(scratch: scratch, own: try source.ownRetainedBytes())
        let control = try RasterJobControl()
        guard control.setScratchLimit(available) else {
            throw DoctoredCoreError(message: "Could not reserve working memory for this job.")
        }
        let job = DocumentJob(document: document, layer: layer, title: title,
                              scratch: scratch, record: record, control: control, rasterizesTarget: rasterizesTarget)
        let described = rasterizesTarget && document.layerDescribesSource(layer)
        let renderScratch = job.renderScratch
        job.base = base
        let input = base?.document ?? source
        job.task = try WorkerTask {
            guard let result = try compute(input, control) else { return nil }
            if control.isCancelled { throw JobCancelled() }
            let output = described ? result.withLayerMeta(layer, nil) ?? result : result
            return PreparedDocumentEdit(output, scratch: renderScratch)
        }
        job.completion = completion
        job.showProgress = showProgress
        active = job
        let timer = Timer(timeInterval: 0.025, repeats: true) { [weak job] _ in
            guard let job else { return }
            poll(job)
        }
        job.timer = timer
        RunLoop.main.add(timer, forMode: .common)
        return job
    }

    static func poll(_ job: DocumentJob) {
        guard job.state == "running", !ActionPlayer.isPlaying else { return }
        if let result = job.task?.take() {
            finish(job, result: result)
            return
        }
        // Only a job still running past the delay gets its panel, once.
        if job.showProgress, job.panel == nil,
           Date().timeIntervalSince(job.started) >= panelDelay {
            job.panel = JobProgressController(title: job.title, cancel: { [weak job] in job?.control.cancel() })
        }
        job.panel?.update(control: job.control)
    }

    /// The gesture guard: settles a finished job first (so its result is
    /// applied before the gesture captures anything), then refuses when that
    /// settle ended the job `failed` or `stale` — its alert is on its way —
    /// and otherwise refuses a `workerBound` gesture (one that commits
    /// through the one worker) while any job or worker runs, and any other
    /// gesture only while a job runs on this same document, which it would
    /// make stale.
    static func refusesGesture(on document: ImageDocument, workerBound: Bool) -> Bool {
        if let job = active {
            poll(job)
            if job.state == "failed" || job.state == "stale" { return true }
        }
        if workerBound { return active != nil || BackgroundWork.occupied }
        return active?.document === document
    }

    static func wait(_ job: DocumentJob) throws {
        guard let task = job.task else { return }
        job.timer?.invalidate()
        do {
            let result = try BackgroundWork.wait(task, title: job.title, control: job.control,
                                                revealAfter: panelDelay)
            finish(job, result: .success(result))
        } catch { finish(job, result: .failure(error)) }
        if job.state == "cancelled" { throw JobCancelled() }
        guard job.state == "completed" else {
            throw DoctoredCoreError(message: job.message ?? "The job changed nothing.")
        }
    }

    private static func finish(_ job: DocumentJob, result: Result<PreparedDocumentEdit?, Error>) {
        defer {
            job.finished = Date()
            job.timer?.invalidate()
            job.timer = nil
            job.panel?.close()
            job.panel = nil
            job.source = nil
            job.base = nil
            job.selection = nil
            job.task = nil
            job.record = []
            active = nil
            recent.insert(job, at: 0)
            recent = Array(recent.prefix(16))
            let completion = job.completion
            job.completion = nil
            completion?(job)
        }
        if job.control.isCancelled {
            job.state = "cancelled"
            job.message = JobCancelled.message
            return
        }
        do {
            guard let updated = try result.get() else {
                job.state = "unchanged"
                job.message = "The operation reproduced the existing pixels or missed the layer."
                return
            }
            guard let document = job.document, document.revision == job.revision,
                  document.doc === job.source, !document.isLiveEditing,
                  !document.isPublishingEditingState,
                  document.selectionHistory == job.selection,
                  document.activeLayerIndex == job.activeLayer,
                  document.selectedLayerIndices == job.selectedLayers,
                  document.planeEditTarget == job.paintTarget,
                  document.quickMaskActive == job.quickMask,
                  document.doc?.layerFingerprint(job.layer) == job.fingerprint else {
                job.state = "stale"
                job.message = "The document changed while this job ran. Run the operation again."
                return
            }
            // `applyEdit`'s own refusals (`ImageDocument.preflightPublication`:
            // the publication refusals, then the depth guard), asked before its
            // undo group opens: refused inside the group, it would still leave
            // an empty undo step behind. `preparedEdit` is pre-set, so
            // the display-preparation branch never runs. The job ends
            // `refused`, never `completed`, so nothing is recorded. A UI job
            // beeps or presents the sentence as `applyEdit` would; an agent
            // job answers it in band only (a waiting call's error, a
            // background job's `message`) and never raises an alert that a
            // waiting call would block on.
            document.preparedEdit = updated
            do {
                try document.preflightPublication(updated.document, record: job.record)
            } catch {
                job.state = "refused"
                job.message = error.localizedDescription
                if !job.agentOrigin {
                    if error is SilentEditRefusal { NSSound.beep() } else { document.presentError(error) }
                }
                return
            }
            withUndoGroup(document.undoManager) {
                document.fadeHint = job.fade
                document.applyEdit(job.title, record: job.record,
                                   changesCanvasGeometry: job.base != nil,
                                   resultingSelection: job.base?.resultingSelection) { _ in
                    updated.document
                }
            }
            // A backstop only: every `applyEdit` refusal is answered
            // above, before the undo group. Should one still refuse here, it
            // published nothing, so the job must not read as completed.
            guard document.doc === updated.document else {
                job.state = "refused"
                job.message = "The edit was refused and nothing changed."
                return
            }
            job.state = "completed"
        } catch {
            job.state = "failed"
            job.message = error.localizedDescription
        }
    }

    /// A recording boundary drains the pending command before changing the
    /// session, so late completion cannot omit it or enter a newer recording.
    static func finishBeforeRecordingChange() {
        if let active { try? wait(active) }
    }

    static func cancel(for document: ImageDocument) {
        if active?.document === document { active?.control.cancel() }
    }

    /// The document's running UI-started job (`showProgress` is the UI-origin
    /// marker: every UI start passes true, every MCP start false), cancelled
    /// or not. No `poll`.
    static func runningUserJob(on document: ImageDocument) -> DocumentJob? {
        guard let job = active, job.document === document, job.showProgress,
              job.state == "running" else { return nil }
        return job
    }

    /// The job ⌘Z/⇧⌘Z and Esc cancel: the document's running
    /// UI-started job not yet cancelled. No `poll`.
    static func userCancellableJob(on document: ImageDocument) -> DocumentJob? {
        guard let job = runningUserJob(on: document), !job.control.isCancelled else { return nil }
        return job
    }

    /// The panel's Cancel on that job; its completion still runs through
    /// `finish` as `cancelled`. False when there is none.
    @discardableResult
    static func cancelUserJob(on document: ImageDocument) -> Bool {
        guard let job = userCancellableJob(on: document) else { return false }
        job.control.cancel()
        return true
    }

    /// Esc's rule wherever it lands: true while the
    /// document's UI job is still running, cancelling it on the first press
    /// and swallowing every later one while it drains, so Esc does not also
    /// deselect or close. False for nil or no running UI job.
    static func escapeCancelsUserJob(on document: ImageDocument?) -> Bool {
        guard let document, runningUserJob(on: document) != nil else { return false }
        cancelUserJob(on: document)
        return true
    }

    /// MCP `undo`/`redo` refuse by name while any job, whatever its origin,
    /// still runs on the document: the reply is decided before the
    /// manager is touched, and `cancel_job` is the UI cancel's twin.
    static func historyStepRefusal(on document: ImageDocument, redo: Bool) -> String? {
        historyRefusal(on: document, then: redo ? "redo" : "undo")
    }

    /// The same refusal for any MCP history move (`restore_history`
    /// too), ending with `verb`. It also refuses while the document's undo
    /// group is open or deferred, where the manager's `undo()` would
    /// raise. That refusal is defensive: no current path services a call
    /// inside an open group.
    static func historyRefusal(on document: ImageDocument, then verb: String) -> String? {
        if let job = active, job.document === document, job.state == "running",
           !job.control.isCancelled {
            return "\(job.title) is still running on this document (job_id \(job.id)). Cancel it with "
                + "cancel_job, or wait until job_status no longer reports it running, then \(verb)."
        }
        guard let manager = document.undoManager,
              manager.groupingLevel > 0
                || (manager as? HistoryUndoManager)?.defersOuterGroup == true else { return nil }
        return "Another edit is still being recorded on this document. Wait for it to finish, "
            + "then \(verb)."
    }
}

/// Worker-owned projection scratch estimate, readable by main-thread diagnostics.
final class JobScratch {
    private let lock = NSLock()
    private var value: UInt64 = 0
    var bytes: UInt64 {
        get { lock.lock(); defer { lock.unlock() }; return value }
        set { lock.lock(); value = newValue; lock.unlock() }
    }
}
