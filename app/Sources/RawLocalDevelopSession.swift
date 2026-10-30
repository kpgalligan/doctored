import AppKit

// The local develop session owns its cumulative operation lease, cache and one coalescing worker;
// previews use queried native scratch, including effective feather. Apply publishes the exact
// prepared source once after original source/selection validation. Cancel/owner close clear visible
// and cached references immediately; parked tasks retain needed immutable inputs only until mailbox
// drain.

struct RawLocalDevelopResult {
    let document: RasterDocument
    let settings: RawDevelopSettings
    let operations: [[String: Any]]
    let retainedPayload: PrecisionEditPayload
}

/// One immutable original and one coalescing worker. Coverage and camera edits
/// share the production pipeline; viewport changes reuse its exact candidate.
final class RawLocalDevelopSession {
    private struct Request {
        let generation: UInt64
        let revision: UInt64
        let settings: RawDevelopSettings
        let operations: [[String: Any]]
        let viewport: RawDevelopViewport
        let selected: String?
        let overlay: Bool
        let canonicalEditing: Bool
    }
    private struct Candidate {
        let revision: UInt64
        let document: RasterDocument
        let state: RawLocalState
        let before: RawDevelopInspectionSource
        let after: RawDevelopInspectionSource
        let editing: RawDevelopInspectionSource
        /// The base development a range's overlay reads: a handle to the one the evaluation
        /// already made (the session cache holds it too), dropped with the candidate.
        let base: RasterDocument?
    }
    private struct Rendered {
        let candidate: Candidate
        let frame: RawDevelopPreviewFrame
        let overlay: CGImage?
    }
    let source: RasterDocument
    let layer: Int
    private(set) var state: RawLocalState
    private(set) var operations: [[String: Any]] = []
    private(set) var result: RawLocalDevelopResult?
    private let id = UUID()
    private let cache = RawLocalVariantCache()
    private let payload: PrecisionEditPayload
    private var candidate: Candidate?
    private var pending: Request?
    private var active: Request?
    private var task: WorkerTask<Rendered>?
    private var control: RasterJobControl?
    private var timer: Timer?
    private var deadline = Date.distantPast
    private var generation: UInt64 = 0
    private var revision: UInt64 = 0
    private var globalReviewedRevision: UInt64?
    private var settings = RawDevelopSettings()
    private var totalDabs = 0
    /// How many leading entries of `operations` a successful evaluation has included. A
    /// generating operation (a Subject or People mask) that refuses at an index below it was
    /// already previewed, so it is kept; at or above it this is its first run and it is dropped
    /// (`RawLocalGenerated.dropping`).
    private var previewed = 0
    /// The sentence of a dropped generation, shown ahead of each ready status until the next
    /// operation or Reset.
    private var notice: String?
    private var closed = false
    private static var activeSessions: [UUID: RawLocalDevelopSession] = [:]
    var onFrame: ((RawDevelopPreviewFrame, CGImage?, RawLocalState) -> Void)?
    var onStatus: ((String, Bool) -> Void)?
    var canApply: Bool {
        guard let candidate else { return false }
        let optics = candidate.state.descriptor.recipe.sourceGeometry
        return optics == nil || optics?.isNeutral == true || globalReviewedRevision == revision
    }

    init(source: RasterDocument, layer: Int) throws {
        self.source = source; self.layer = layer
        state = try RawLocalWork.state(source, layer: layer)
        // Dictionary/NSNumber storage and bounded JSON temporaries are covered
        // before any gesture arrays are retained. No pixel buffer is allocated.
        payload = try PrecisionEditPayload.admitted(bytes: UInt64(RawLocalWork.maximumOperationBytes) * 128 + 4 * 1024 * 1024)
    }

    func append(_ operation: [String: Any], dabs: Int = 0) throws {
        guard !closed, totalDabs + dabs <= RawLocalWork.maximumDabs else {
            throw DoctoredCoreError(message: "This Develop session exceeds its brush-dab limit. Apply and reopen before another stroke; no partial stroke was added.")
        }
        var desired = operations
        if operation["op"] as? String == "update", desired.last?["op"] as? String == "update",
           operation["id"] as? String == desired.last?["id"] as? String {
            // UI updates contain the complete selected node's control values;
            // keeping the last slider position avoids recording drag samples.
            desired[desired.count - 1] = operation
        } else { desired.append(operation) }
        let encoded = try JSONSerialization.data(withJSONObject: desired, options: [.sortedKeys])
        guard desired.count <= RawLocalWork.maximumOperations,
              encoded.count <= RawLocalWork.maximumOperationBytes else {
            throw DoctoredCoreError(message: "This Develop session exceeds its bounded Action payload. Apply and reopen before more edits; no partial operation was added.")
        }
        if encoded == (try JSONSerialization.data(withJSONObject: operations, options: [.sortedKeys])) { return }
        // The in-place replace above only ever swaps an `update`, never a generating operation,
        // so `previewed` still counts operations an evaluation included.
        operations = desired; totalDabs += dabs
        revision &+= 1; result = nil; notice = nil
    }

    func reset() {
        operations = []; totalDabs = 0; revision &+= 1; result = nil
        previewed = 0; notice = nil
    }

    /// A generation that produced no mask. On its first run the operation is dropped and the
    /// request evaluated again without it. One that an evaluation already included (a global
    /// control moved to a setting whose base gives Vision nothing) keeps the whole list: the
    /// status says so, Apply stays off, and the next request evaluates the same list.
    private func refused(_ refusal: RawLocalGenerationRefusal, request failed: Request) {
        guard let kept = RawLocalGenerated.dropping(refusal, from: operations, previewed: previewed) else {
            onStatus?(refusal.message + RawLocalGenerated.keptSuffix, false)
            return
        }
        operations = kept; revision &+= 1; result = nil; notice = refusal.message
        request(settings: failed.settings, viewport: failed.viewport, selected: failed.selected,
            overlay: failed.overlay, canonicalEditing: failed.canonicalEditing)
        // After `request`, whose own progress line would otherwise replace the sentence.
        onStatus?(refusal.message, false)
    }

    func request(settings: RawDevelopSettings, viewport: RawDevelopViewport,
                 selected: String?, overlay: Bool, canonicalEditing: Bool = false) {
        assert(Thread.isMainThread)
        guard !closed else { return }
        if self.settings != settings { self.settings = settings; revision &+= 1 }
        generation &+= 1
        pending = Request(generation: generation, revision: revision, settings: settings,
            operations: operations, viewport: viewport, selected: selected, overlay: overlay,
            canonicalEditing: canonicalEditing)
        if active?.revision != revision { control?.cancel() }
        result = nil
        deadline = Date().addingTimeInterval(0.12)
        onStatus?("Updating native RAW development…", false)
        keepPolling()
    }

    func cancel() {
        assert(Thread.isMainThread)
        closed = true; pending = nil; result = nil; candidate = nil
        onFrame = nil; onStatus = nil; control?.cancel(); cache.clear()
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
        timer?.invalidate(); timer = nil
        if closed { cache.clear() }
        Self.activeSessions.removeValue(forKey: id)
    }

    private func poll() {
        if let task, let completed = task.take() {
            self.task = nil; control = nil
            let request = active; active = nil
            if !closed {
                switch completed {
                case .success(let value):
                    if request?.revision == revision {
                        candidate = value.candidate; previewed = operations.count
                    }
                    if request?.generation == generation, let request {
                        state = value.candidate.state
                        if !request.canonicalEditing { globalReviewedRevision = revision }
                        result = RawLocalDevelopResult(document: value.candidate.document,
                            settings: request.settings, operations: request.operations, retainedPayload: payload)
                        onFrame?(value.frame, value.overlay, state)
                        let float = value.candidate.state.descriptor.recipe.isLinearFloat
                        let ready = canApply
                            ? (float ? "32-bit float preview ready · Apply uses this exact result"
                                : "Native development ready · Apply uses this exact result")
                            : "Canonical editing preview · review the final Global image before Apply"
                        onStatus?(notice.map { "\($0) · \(ready)" } ?? ready, true)
                    }
                case .failure(let error):
                    if request?.generation == generation, let request {
                        if let refusal = error as? RawLocalGenerationRefusal {
                            refused(refusal, request: request)
                        } else { onStatus?(error.localizedDescription, false) }
                    }
                }
            }
        }
        if closed { if task == nil { stopPolling() }; return }
        guard task == nil, let request = pending, Date() >= deadline else {
            if task == nil && pending == nil { stopPolling() }
            return
        }
        guard !BackgroundWork.occupied else { return }
        pending = nil
        do {
            // A range's overlay reads the base development. The evaluation is told the overlay is
            // on (`needsBase`) and narrows that by the nodes it applies, never by the last
            // displayed state: after Reset those nodes are the saved ones, which may hold a range
            // the displayed state no longer does. A candidate at this revision already holds
            // those nodes, so it is evaluated again only when they hold a range and it was made
            // without a base (nothing had changed while the overlay was off).
            let known: Candidate?
            if let candidate, candidate.revision == request.revision,
               !request.overlay || candidate.base != nil
                   || !candidate.state.nodes.contains(where: { $0.range != nil }) {
                known = candidate
            } else { known = nil }
            let previousBefore = candidate?.before
            // A superseded candidate is not held beside the new evaluation.
            if known == nil { candidate = nil }
            let plan = known == nil ? try RawLocalWork.request(source: source, layer: layer,
                settings: request.settings, operations: request.operations, cache: cache) : nil
            var displayScratch: UInt64 = 64 * 1024 * 1024
            if let known, request.overlay, let selected = request.selected,
               let index = known.state.nodes.firstIndex(where: { $0.id == selected }) {
                let region = request.viewport.region(known.editing.image)
                let node = known.state.nodes[index]
                displayScratch = try PrecisionDocumentDescriptor.sum(displayScratch,
                    known.state.masks[index].previewScratch(x: region.x, y: region.y,
                        width: region.width, height: region.height, divisor: region.divisor,
                        density: node.density, feather: node.feather, effective: true))
            }
            // A 32-bit float development is inspected through its SDR-clipped
            // u16 view (`developInspection`): three copies (before, after,
            // editing) at 8 B/px, held with the candidate.
            if let plan, plan.desired.isLinearFloat || state.descriptor.recipe.isLinearFloat {
                displayScratch = try PrecisionDocumentDescriptor.sum(displayScratch,
                    UInt64(state.width) * UInt64(state.height) * 24)
            }
            let scratch = try PrecisionDocumentDescriptor.sum(plan?.scratchEstimate ?? 0, displayScratch)
            _ = try RasterResources.shared.admit(scratch: scratch)
            let control = try RasterJobControl()
            guard control.setScratchLimit(scratch) else { throw DoctoredCoreError(message: "Could not admit local RAW preview stages.") }
            let source = self.source, layer = self.layer, payload = self.payload
            active = request; self.control = control
            task = try WorkerTask(scratchEstimate: scratch) {
                try withExtendedLifetime(payload) {
                    let current: Candidate
                    if let known { current = known }
                    else {
                        let evaluated = try control.withChildOperation(from: 0, to: 0.75) {
                            try plan?.evaluatePreview(control: $0, needsBase: request.overlay)
                                ?? (document: source, editingSource: source, base: nil)
                        }
                        let document = evaluated.document
                        let state = try RawLocalWork.state(document, layer: layer)
                        let before = try control.withChildOperation(from: 0.75, to: 0.8) {
                            if let previousBefore { return previousBefore }
                            return try RawDevelopInspectionSource(source.sourceMaster(layer), control: $0)
                        }
                        let after = try control.withChildOperation(from: 0.8, to: 0.85) {
                            if document === source { return before }
                            return try RawDevelopInspectionSource(document.sourceMaster(layer), control: $0)
                        }
                        let editing = try RawDevelopInspectionSource(
                            evaluated.editingSource.precision?.layerSource(0)
                                ?? source.sourceMaster(layer))
                        current = Candidate(revision: request.revision, document: document,
                            state: state, before: before, after: after, editing: editing,
                            base: evaluated.base)
                    }
                    let displayAfter = request.canonicalEditing ? current.editing : current.after
                    let frame = try control.withChildOperation(from: 0.85, to: 0.95) {
                        try RawDevelopPreviewFrame(before: current.before, after: displayAfter,
                            viewport: request.viewport, control: $0)
                    }
                    let overlay: CGImage?
                    if request.overlay, let selected = request.selected,
                       let index = current.state.nodes.firstIndex(where: { $0.id == selected }) {
                        let region = request.viewport.region(current.editing.image)
                        let node = current.state.nodes[index]
                        // Shape times range, as the development blends; the shape alone when
                        // this candidate has no base to read.
                        let range: String? = try current.base == nil ? nil
                            : node.range.map { try RawLocalJSON.string($0.coreJSON()) }
                        overlay = try control.withChildOperation(from: 0.95, to: 1) {
                            let raster = try current.state.masks[index].preview(x: region.x, y: region.y,
                                width: region.width, height: region.height, divisor: region.divisor,
                                density: node.density, feather: node.feather, effective: true,
                                base: range == nil ? nil : current.base, range: range, control: $0)
                            guard let image = raster.makeCGImage(in: WorkingSpace.sRGB.cgSpace) else {
                                throw DoctoredCoreError(message: "The native local-mask overlay could not be displayed.")
                            }
                            return image
                        }
                    } else { overlay = nil }
                    guard !control.isCancelled else { throw JobCancelled() }
                    return Rendered(candidate: current, frame: frame, overlay: overlay)
                }
            }
        } catch {
            active = nil; control = nil
            onStatus?(error.localizedDescription, false)
        }
    }
}
