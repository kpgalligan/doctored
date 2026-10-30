import AppKit

// The main-thread coordinator captures committed states, uses the existing bounded worker, and
// publishes complete generations through an atomic manifest. File leases exclude other live app
// instances; a failed write keeps the prior copy.
//
// Startup offers recover/discard, recovery creates a dirty native untitled copy, confirmed saves
// clean up only when no newer edits or session snapshots need protection, and actual close
// invalidates late publication.

extension Notification.Name {
    static let imageDocumentRecoveryDidChange = Notification.Name("ImageDocumentRecoveryDidChange")
}

/// AppKit owns scheduling, capture and installation. Only immutable archive
/// values cross BackgroundWork; recovery never invokes NSDocument's save path,
/// changes a source URL, or marks the captured document saved.
final class RecoveryCoordinator {
    static let shared = RecoveryCoordinator()

    final class Record {
        weak var document: ImageDocument?
        let id: String
        var serial: UInt64 = 1
        var capturedSerial: UInt64 = 0
        var lastState: EditingState?
        var neededSince = ProcessInfo.processInfo.systemUptime
        var nextAttempt: TimeInterval = 0
        var manifest: RecoveryStore.Manifest?
        var lease: RecoveryLease?
        var gate = RecoveryPublicationGate()
        var task: WorkerTask<RecoveryStore.Manifest?>?
        var waiting = false
        var closed = false
        var error: String?
        /// Set only by `fail`, from a working-memory `RecoveryCapacity.Refusal`.
        var refusal: RecoveryCapacity.Refusal?
        var notifiedError = false
        var completions: [(Error?) -> Void] = []

        init(document: ImageDocument, id: String = UUID().uuidString) {
            self.document = document
            self.id = id
        }

        var report: [String: Any] {
            var result: [String: Any] = ["recovery_id": id, "serial": serial,
                "captured_serial": capturedSerial, "pending": serial != capturedSerial,
                "state": task != nil ? "writing" : (error != nil ? "failed" : (manifest != nil ? "protected" : "pending"))]
            if let document { result["revision"] = document.revision; result["title"] = document.displayName ?? "Untitled" }
            if let manifest {
                result["captured_revision"] = manifest.current.revision
                result["captured_at"] = ISO8601DateFormatter().string(from: manifest.current.captured)
                result["generation"] = manifest.current.folder
            }
            if let error { result["error"] = error }
            if let refusal { result["memory"] = refusal.report }
            return result
        }
    }

    let root: URL
    let interval: TimeInterval
    private(set) var started = false
    private var records: [ObjectIdentifier: Record] = [:]
    private var timer: Timer?
    private var observers: [NSObjectProtocol] = []
    private var installing = false
    /// `systemUptime` of the latest `begin`: `tick` yields the free slot
    /// to a waiting caller only while a write began within `interval`, so a
    /// steady stream of calls cannot starve recovery.
    private var lastBegin: TimeInterval = -.infinity
    private var panel: RecoveryWindowController?
    private(set) var discoveryError: String?

    private init() {
        let environment = ProcessInfo.processInfo.environment
        if let path = environment["DR_RECOVERY_DIR"], !path.isEmpty {
            root = URL(fileURLWithPath: path, isDirectory: true)
        } else {
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
                ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support")
            root = support.appendingPathComponent("Doctored/Recovery", isDirectory: true)
        }
        interval = max(1, min(3600, Double(environment["DR_RECOVERY_INTERVAL_SECONDS"] ?? "") ?? 30))
    }

    func start() {
        assert(Thread.isMainThread)
        guard !started else { return }
        started = true
        observers.append(NotificationCenter.default.addObserver(
            forName: .imageDocumentImageDidChange, object: nil, queue: nil) { [weak self] note in
                guard note.userInfo?["isLive"] as? Bool != true,
                      let document = note.object as? ImageDocument else { return }
                self?.documentDidChange(document)
            })
        observers.append(NotificationCenter.default.addObserver(
            forName: .imageDocumentSnapshotsDidChange, object: nil, queue: nil) { [weak self] note in
                guard let document = note.object as? ImageDocument else { return }
                self?.documentDidChange(document, snapshotsChanged: true)
            })
        let timer = Timer(timeInterval: 0.5, repeats: true) { [weak self] _ in self?.tick() }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
        tick()
        if !UserDefaults.standard.bool(forKey: "RecoveryDiscoverySuppressed") {
            do {
                if try !available().isEmpty { showRecoveryWindow(nil) }
            } catch {
                // An inaccessible root is different from an empty recovery
                // tree. The nonmodal panel shows the error and offers Refresh.
                showRecoveryWindow(nil)
            }
        }
    }

    func documentDidChange(_ document: ImageDocument, snapshotsChanged: Bool = false) {
        assert(Thread.isMainThread)
        guard started, !installing, !ActionPlayer.isPlaying, !document.isLiveEditing,
              let state = document.captureEditingState() else { return }
        let key = ObjectIdentifier(document)
        let record = records[key] ?? Record(document: document)
        records[key] = record
        // Removing session snapshots is the refusal's remedy; retry at the next tick.
        if snapshotsChanged, record.refusal != nil { record.nextAttempt = 0 }
        if snapshotsChanged || record.lastState.map({ !state.equivalent(to: $0) }) == true {
            if record.serial == record.capturedSerial { record.neededSince = ProcessInfo.processInfo.systemUptime }
            record.serial &+= 1
        }
        record.lastState = state
        if !needsRecovery(document) { clear(record) }
    }

    /// A newer revision must retain its checkpoint even when an older captured
    /// save succeeds. Session snapshots remain recoverable until normal close.
    func documentDidSave(_ document: ImageDocument, captured: RasterDocument?) {
        documentDidChange(document)
        guard let record = records[ObjectIdentifier(document)] else { return }
        if !needsRecovery(document), captured == nil || captured === document.doc { clear(record) }
        else { record.serial &+= 1; record.nextAttempt = 0; record.neededSince = 0 }
    }

    func documentDidClose(_ document: ImageDocument) {
        // Closing a document frees memory a refused capture asked for.
        for (key, other) in records where key != ObjectIdentifier(document) && other.refusal != nil {
            other.nextAttempt = 0
        }
        guard let record = records[ObjectIdentifier(document)] else { return }
        record.closed = true
        clear(record)
        if record.task == nil { records.removeValue(forKey: ObjectIdentifier(document)) }
    }

    func status(for document: ImageDocument) -> [String: Any] {
        documentDidChange(document)
        guard let record = records[ObjectIdentifier(document)] else {
            return ["state": "not_needed", "interval_seconds": interval]
        }
        poll(record)
        var result = record.report
        result["interval_seconds"] = interval
        result["needed"] = needsRecovery(document)
        if !needsRecovery(document), record.task == nil, record.error == nil { result["state"] = "not_needed" }
        result["deferred_by_worker"] = record.task == nil && BackgroundWork.occupied
        return result
    }

    func available() throws -> [RecoveryStore.Candidate] {
        do { let values = try RecoveryStore.discover(root: root); discoveryError = nil; return values }
        catch { discoveryError = error.localizedDescription; throw error }
    }

    func checkpoint(_ document: ImageDocument, wait: Bool, completion: ((Error?) -> Void)? = nil) throws {
        guard started else { completion?(nil); return }
        documentDidChange(document)
        guard needsRecovery(document), let record = records[ObjectIdentifier(document)] else { completion?(nil); return }
        if let completion { record.completions.append(completion) }
        if record.task == nil {
            guard !BackgroundWork.occupied else {
                record.neededSince = 0
                if wait { throw failure("Another background operation is running. Recovery is queued and will retry.") }
                return
            }
            do { try begin(record) }
            catch {
                fail(record, error: error)
                if completion == nil { throw error }
                return
            }
        }
        guard wait, let task = record.task else { return }
        record.waiting = true
        defer { record.waiting = false }
        let result = Result { try BackgroundWork.wait(task, title: "Saving Recovery") }
        finish(record, result: result)
        _ = try result.get()
    }

    @discardableResult
    func recover(id: String, show: Bool) throws -> (ImageDocument, String?) {
        guard !BackgroundWork.occupied else { throw failure("Wait for the current background operation to finish.") }
        let directory = try RecoveryStore.directory(root: root, id: id)
        guard let lease = try RecoveryLease.acquire(directory: directory, create: false) else {
            throw failure("That recovery is in use by another open document.")
        }
        let candidate = RecoveryStore.Candidate(id: id, directory: directory,
                                               manifest: try RecoveryStore.readManifest(at: directory), failure: nil)
        let scratch = BackgroundWork.scratch
        let task = try WorkerTask { () -> (RecoveryStore.Loaded, PreparedDocumentEdit) in
            let loaded = try RecoveryStore.read(candidate)
            return (loaded, PreparedDocumentEdit(loaded.state.document, scratch: scratch))
        }
        let (loaded, prepared) = try BackgroundWork.wait(task, title: "Opening Recovery")
        let state = try loaded.state.editingState()
        let document = ImageDocument()
        installing = true
        defer { installing = false }
        document.preparedEdit = prepared
        document.installEditingState(state, markSaved: false)
        try document.namedSnapshots.installArchive(loaded.snapshots)
        document.fileURL = nil
        document.fileType = ImageDocument.nativeTypeIdentifier
        document.displayName = ((loaded.manifest.title as NSString).deletingPathExtension) + " (Recovered)"
        let record = Record(document: document, id: id)
        record.lease = lease
        record.serial = loaded.generation.serial
        record.capturedSerial = loaded.generation.serial
        record.lastState = state
        record.manifest = RecoveryStore.Manifest(version: 1, id: id, title: loaded.manifest.title,
            sourceURL: loaded.manifest.sourceURL, sourceType: loaded.manifest.sourceType,
            current: loaded.generation, previous: nil)
        records[ObjectIdentifier(document)] = record
        NSDocumentController.shared.addDocument(document)
        if show { document.makeWindowControllers(); document.showWindows() }
        changed(document)
        return (document, loaded.warning)
    }

    func discard(id: String) throws {
        let directory = try RecoveryStore.directory(root: root, id: id)
        guard let lease = try RecoveryLease.acquire(directory: directory, create: false) else {
            throw failure("That recovery belongs to an open document. Close the document to discard it.")
        }
        defer { lease.release() }
        try RecoveryStore.discard(directory: directory)
        panel?.reload()
    }

    @objc func showRecoveryWindow(_ sender: Any?) {
        if panel == nil { panel = RecoveryWindowController(coordinator: self) }
        panel?.reload()
        if let panel { BackgroundLaunch.show(panel) }
    }

    /// Main thread, no side effects. Non-nil only while `document` needs recovery, its record is open,
    /// nothing is writing, and the latest attempt failed. text == "Recovery off".
    func offNotice(for document: ImageDocument) -> (text: String, detail: String)? {
        guard let record = records[ObjectIdentifier(document)], !record.closed, record.task == nil,
              let error = record.error, needsRecovery(document) else { return nil }
        let copy = record.manifest.map {
            "Its recovery copy from \(DateFormatter.localizedString(from: $0.current.captured, dateStyle: .medium, timeStyle: .short)) is kept."
        } ?? "It has no recovery copy."
        return ("Recovery off", error + " " + copy)
    }

    /// "<title>: <detail>" for every open document whose recovery is off, sorted by title.
    var offNotices: [String] {
        records.values.compactMap { record -> (title: String, detail: String)? in
            guard let document = record.document, let notice = offNotice(for: document) else { return nil }
            return (document.displayName ?? "Untitled", notice.detail)
        }
        .sorted { ($0.title, $0.detail) < ($1.title, $1.detail) }
        .map { "\($0.title): \($0.detail)" }
    }

    private func needsRecovery(_ document: ImageDocument) -> Bool {
        document.isDocumentEdited || !document.namedSnapshots.entries.isEmpty
    }

    private func tick() {
        guard started else { return }
        for record in Array(records.values) { poll(record) }
        for document in NSDocumentController.shared.documents.compactMap({ $0 as? ImageDocument }) {
            documentDidChange(document)
        }
        let now = ProcessInfo.processInfo.systemUptime
        for (key, record) in records where record.document == nil && record.task == nil { records.removeValue(forKey: key) }
        // A write begun in the same turn that polled the last one free
        // would keep a waiting caller from ever seeing the slot free.
        let yields = BackgroundWork.slotRequested && now - lastBegin < interval
        guard !BackgroundWork.occupied, !yields, !ActionPlayer.isPlaying else { return }
        let due = records.values.filter { record in
            guard let document = record.document else { return false }
            return !record.closed && !document.isLiveEditing && needsRecovery(document)
                && record.serial != record.capturedSerial && record.task == nil
                && now >= record.nextAttempt && now - record.neededSince >= interval
        }.sorted { $0.neededSince < $1.neededSince }
        guard let record = due.first else { return }
        do { try begin(record) }
        catch { fail(record, error: error) }
    }

    private func begin(_ record: Record) throws {
        guard !ActionPlayer.isPlaying else { throw failure("Recovery waits until the Action finishes.") }
        guard let document = record.document, !document.isLiveEditing,
              let state = document.captureEditingState() else { throw failure("Finish the current gesture before saving recovery.") }
        let directory = try RecoveryStore.directory(root: root, id: record.id)
        if record.lease == nil {
            guard let lease = try RecoveryLease.acquire(directory: directory, create: true) else {
                throw failure("Recovery is in use by another app instance.")
            }
            record.lease = lease
        }
        let captured = RecoveryStore.Captured(id: record.id, title: document.displayName ?? "Untitled",
            sourceURL: document.fileURL?.absoluteString ?? record.manifest?.sourceURL,
            sourceType: document.fileType ?? record.manifest?.sourceType,
            generation: RecoveryStore.Generation(folder: UUID().uuidString, revision: document.revision,
                serial: record.serial, captured: Date()), previous: record.manifest,
            state: try EditingStateArchive.capture(state), snapshots: try document.namedSnapshots.captureArchive())
        // A reopen refusal neither evicts nor takes the worker slot.
        try RecoveryCapacity.validate(captured)
        do {
            _ = try RasterResources.shared.admit(scratch: captured.scratchEstimate,
                                                 own: captured.state.ownRetainedBytes,
                                                 remedy: captured.state.refusalRemedy)
        } catch {
            // The fast refusal skips admit's eviction, so the periodic retry must recapture without live views.
            let report = try? RasterResources.shared.report(evict: true)
            guard error is DoctoredCoreError,
                  let retained = (report?["unique_retained_bytes"] as? NSNumber)?.uint64Value,
                  let previews = (report?["selection_preview_scratch_bytes"] as? NSNumber)?.uint64Value else { throw error }
            throw RecoveryCapacity.captureRefusal(scratch: captured.scratchEstimate,
                own: captured.state.ownRetainedBytes, inUse: retained + previews,
                limit: RasterResources.shared.workingLimit, snapshots: !captured.snapshots.isEmpty,
                otherDocuments: NSDocumentController.shared.documents.count > 1,
                remedy: captured.state.refusalRemedy)
        }
        record.gate = RecoveryPublicationGate()
        let gate = record.gate
        record.error = nil
        record.refusal = nil
        record.task = try WorkerTask(scratchEstimate: captured.scratchEstimate) {
            try RecoveryStore.write(captured, directory: directory, gate: gate)
        }
        lastBegin = ProcessInfo.processInfo.systemUptime
        changed(document)
    }

    private func poll(_ record: Record) {
        guard !record.waiting, let result = record.task?.take() else { return }
        finish(record, result: result)
    }

    private func finish(_ record: Record, result: Result<RecoveryStore.Manifest?, Error>) {
        record.task = nil
        if record.closed {
            let completions = record.completions; record.completions.removeAll()
            completions.forEach { $0(nil) }
            clear(record)
            if let document = record.document { records.removeValue(forKey: ObjectIdentifier(document)) }
            return
        }
        switch result {
        case .success(let manifest):
            if let manifest {
                record.manifest = manifest
                record.capturedSerial = manifest.current.serial
                record.neededSince = ProcessInfo.processInfo.systemUptime
                record.error = nil
                record.refusal = nil
                record.notifiedError = false
            }
            let completions = record.completions; record.completions.removeAll()
            completions.forEach { $0(nil) }
        case .failure(let error): fail(record, error: error)
        }
        if let document = record.document { changed(document) }
    }

    private func fail(_ record: Record, error: Error) {
        record.error = error.localizedDescription
        record.refusal = error as? RecoveryCapacity.Refusal
        record.nextAttempt = ProcessInfo.processInfo.systemUptime + interval
        let completions = record.completions; record.completions.removeAll()
        completions.forEach { $0(error) }
        if !record.notifiedError && !UserDefaults.standard.bool(forKey: "RecoveryDiscoverySuppressed") {
            record.notifiedError = true
            showRecoveryWindow(nil)
        }
        if let document = record.document { changed(document) }
    }

    private func clear(_ record: Record) {
        record.refusal = nil
        let hadError = record.error != nil
        record.gate.invalidate()
        let completions = record.completions; record.completions.removeAll()
        completions.forEach { $0(nil) }
        do {
            try RecoveryStore.discard(directory: try RecoveryStore.directory(root: root, id: record.id))
            record.manifest = nil
            record.capturedSerial = record.serial
            record.error = nil
            record.lease = nil
        } catch { record.error = "Could not remove recovery: \(error.localizedDescription)" }
        // Only a reset error changes what the editor shows; `tick` clears every half second.
        if hadError, record.error == nil, let document = record.document { changed(document) }
    }

    private func changed(_ document: ImageDocument) {
        NotificationCenter.default.post(name: .imageDocumentRecoveryDidChange, object: document)
        panel?.reload()
    }

    private func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }
}
