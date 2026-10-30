import AppKit
import UniformTypeIdentifiers

// Batch's "Deliver with:" preset mode writes through the `quick_export` TOOL with `index`/`count`;
// it skips the flat path's containment and claimed-set checks, but pre-checks the exact name
// through `ImageDocument.quickExportDestination` (THE Quick Export naming, expanded BEFORE the
// encode) so an existing file is a `skipped` row rather than a halt, and a missing preset folder is
// refused at submission (`ExportPreset.requireDestinationFolder`) by the Batch window and
// `run_batch` alike; flat `save_copy` mode and its bytes are unchanged.

/// Running one action over a folder of images: the file list, the output
/// rules, and the driver that walks the list one file per run-loop turn.
///
/// **Its MCP twin is `run_batch`** (`AgentServer+Batch.swift`), which builds
/// the same `BatchSettings` and starts the same driver with no window. Batch
/// performs no edit of its own — it is `open_document` + `run_action` +
/// `save_copy` (or `quick_export`) in a loop. What it adds is the folder walk
/// and the output rules, and those are what this file is.
///
/// Every path rule is a **pure function** — `imageURLs(in:)`,
/// `sameDirectory(_:_:)`, `templateProblem(_:)`, `outputName(…)` — so the
/// guards that keep a run inside its output folder can be read one at a
/// time instead of inferred from the driver's state. The driver itself
/// holds only the list, the index, the cancel flag and the report.
///
/// Nothing here writes a byte: the output goes through the `save_copy`
/// tool, whose core helper `save_atomically` writes a temp file in the
/// destination directory and renames only on success, and which reports
/// what the chosen format actually carried. A batch that wrote bytes from
/// Swift would lose the atomicity, the profile, the EXIF packets and the
/// print resolution.

// MARK: - Settings

/// Everything the Setup pane collects, as one value handed to the driver.
struct BatchSettings {
    /// The action to play on every file. Its own optional `raw` object is
    /// what each camera RAW in the folder is developed with — the rule,
    /// stated once: **Batch uses the action's `raw` for the opens it
    /// performs; an `open_document` step inside an action carries its own
    /// `raw` in its own arguments and Batch never touches it.** No dialog
    /// can be shown per file, so without the action's own settings a folder
    /// of 200 RAWs could only be developed at each file's as-shot defaults.
    var action: Action
    var inputFolder: URL
    var outputFolder: URL
    var format: ExportFormat
    var jpegQuality: Int
    var embedProfile: Bool
    var stripMetadata: Bool
    /// `{name}` (the input's basename) and `{n}` (1-based, zero-padded to
    /// the digit width of the file count). The extension is always the
    /// chosen format's and is never taken from the template.
    var nameTemplate: String
    var overwrite: Bool
    /// Overrides every step's own `on_error` for the run, and decides what
    /// happens to the file a failure interrupted — see `BatchRun.process`.
    var stopOnError: Bool
    /// Phase 11B "Deliver with:": an 11A export preset. When it is set the
    /// run writes through the `quick_export` TOOL instead of `save_copy`, and
    /// the destination folder, the naming pattern and every delivery option
    /// come from the preset rather than from the pane. Quick Export never
    /// overwrites, so `overwrite` is forced off for a preset run.
    var preset: ExportPreset?
    /// An explicit metadata policy for the run, overriding the preset's own
    /// named one. Flat (`save_copy`) mode ignores it: `save_copy` has no
    /// policy argument and its bytes are deliberately unchanged.
    var metadataPolicy: MetadataPolicy?
    /// The action's parameters for this run — the BOUND dictionary, holding
    /// every declared name with its defaults already filled in
    /// (`ActionParameters.bind`).
    ///
    /// Bound BEFORE the run starts, never per file: a parameter error is a
    /// statement about the action and the submission, not about any one
    /// picture, so it is one refusal before the first file is opened rather
    /// than 200 identical rows in a report nobody would read.
    var parameters: [String: Any] = [:]
}

// MARK: - The report

/// What happened to one file. Every run ends with one of these per file, so
/// the report can say exactly what it did rather than a count.
struct BatchFileResult {
    enum Outcome {
        /// Written. `carried` is `save_copy`'s own account of what the
        /// format could and could not hold.
        case wrote(destination: String, carried: String?)
        /// Not attempted, and why — a name collision, an existing file, a
        /// name that would land outside the output folder.
        case skipped(reason: String)
        /// The open, the action or the write failed. `destination` is
        /// non-nil when the file was written anyway, which is what "Stop on
        /// error" being OFF means.
        case failed(reason: String, destination: String?)
    }

    let input: String
    let outcome: Outcome

    /// The report's line for this file.
    var line: String {
        switch outcome {
        case .wrote(let destination, let carried):
            let tail = carried.map { " (\($0))" } ?? ""
            return "ok       \(input) → \(destination)\(tail)"
        case .skipped(let reason):
            return "skipped  \(input) — \(reason)"
        case .failed(let reason, let destination):
            guard let destination = destination else {
                return "failed   \(input) — \(reason); nothing was written"
            }
            return "failed   \(input) — \(reason); written anyway to \(destination)"
        }
    }

    /// One row as `batch_status` reports it — the same three outcomes
    /// `line` spells in prose, with the fields named so a caller can branch
    /// on them instead of parsing a sentence.
    func jsonObject() -> [String: Any] {
        switch outcome {
        case .wrote(let destination, let carried):
            var row: [String: Any] = [
                "input": input, "outcome": "wrote", "destination": destination,
            ]
            if let carried = carried { row["carried"] = carried }
            return row
        case .skipped(let reason):
            return ["input": input, "outcome": "skipped", "reason": reason]
        case .failed(let reason, let destination):
            var row: [String: Any] = ["input": input, "outcome": "failed", "reason": reason]
            // Present only when the file was written ANYWAY, which is what
            // "Stop on error" being off means; its absence says nothing was.
            if let destination = destination { row["destination"] = destination }
            return row
        }
    }

    var isFailure: Bool {
        if case .failed = outcome { return true }
        return false
    }

    var isSkip: Bool {
        if case .skipped = outcome { return true }
        return false
    }
}

/// The whole run's account of itself: one row per file plus the two things
/// that are true of the run rather than of any one file.
struct BatchReport {
    var action: String = ""
    var outputFolder: String = ""
    var files: [BatchFileResult] = []
    /// Things that are true of the run rather than of one file, each said
    /// ONCE however many files earned it — a canvas-size mismatch, which is
    /// a warning and not a failure, would otherwise repeat 200 times.
    var warnings: [String] = []
    /// The sentence a "Stop on error" halt earns, naming the file, the step
    /// and the reason. Nil when the run reached the end of the list.
    var stopped: String?
    var cancelled = false
    /// Files never reached, because the run stopped or was cancelled.
    var notProcessed = 0

    var written: Int {
        files.filter { if case .wrote = $0.outcome { return true } else { return false } }.count
    }
    var skipped: Int { files.filter { $0.isSkip }.count }
    var failed: Int { files.filter { $0.isFailure }.count }

    /// The one-line headline the Run pane shows when the run ends.
    ///
    /// A halted run leads with the halt sentence and the count of files it
    /// never reached, because that is the thing the user has to know before
    /// any tally: the output folder holds part of a job.
    var summary: String {
        var parts = ["\(written) written"]
        if skipped > 0 { parts.append("\(skipped) skipped") }
        if failed > 0 { parts.append("\(failed) failed") }
        let counts = parts.joined(separator: ", ")
        let unreached = "\(notProcessed) file\(notProcessed == 1 ? "" : "s") not processed"
        if let stopped = stopped { return "\(stopped); \(unreached) (\(counts))" }
        if cancelled { return "Cancelled — \(counts), \(unreached)" }
        return "Finished — \(counts)"
    }

    /// The whole report as the JSON object `batch_status` answers with — the
    /// mirror of `text()` below, field for field, so a headless caller and
    /// the window's Copy Report never disagree about what happened.
    ///
    /// Per-file rows rather than counts: "failed" with no reason and no input
    /// name is the one thing a caller cannot act on, and the counts are
    /// derivable from the rows anyway.
    func jsonObject() -> [String: Any] {
        var object: [String: Any] = [
            "action": action, "output_folder": outputFolder, "summary": summary,
            "written": written, "skipped": skipped, "failed": failed,
            "not_processed": notProcessed, "cancelled": cancelled,
            "files": files.map { $0.jsonObject() },
        ]
        if !warnings.isEmpty { object["warnings"] = warnings }
        if let stopped = stopped { object["stopped"] = stopped }
        return object
    }

    /// The whole report as plain text: the headline, then one line per
    /// file, in the order they were processed. This is what Copy Report
    /// puts on the pasteboard, so it has to stand on its own.
    func text() -> String {
        var lines = [
            "Batch: \(action)",
            "Output: \(outputFolder)",
            summary,
        ]
        lines.append(contentsOf: warnings.map { "warning: \($0)" })
        lines.append("")
        lines.append(contentsOf: files.map { $0.line })
        return lines.joined(separator: "\n")
    }
}

// MARK: - The driver

/// Walks the file list, one file per main-run-loop turn.
///
/// One file per turn, driven by a `Timer` in **`.common` modes**: Batch
/// runs inside `NSApp.runModal`, and a timer scheduled in `.default` would
/// never fire there. The airbrush repeat (`ImageCanvasView`) is the house
/// precedent for a main-run-loop timer, and this is the same shape for the
/// same reason — no GCD, no async/await, everything on the main thread
/// where `ImageDocument` and the Dr handles have to be.
///
/// Cancellation is checked **between** files. A core call cannot be
/// interrupted, so cancellation is between units of work, exactly as the
/// assistant checks its cancel flag only at tool and API boundaries.
final class BatchRun {
    /// The run that is in flight, or nil.
    ///
    /// **One at a time, app-wide.** A batch opens and closes documents, holds
    /// the recorder suspended and drives the whole tool surface through
    /// `AgentServer.execute`; two of them interleaving on the main run loop
    /// would each see the other's documents. `ActionPlayer.replayRefusal`
    /// reads this to refuse every OUTSIDE call while one runs, and the Batch
    /// window's Run button refuses a second.
    ///
    /// Set by `start()` and cleared by `finish()`, so it is exactly the
    /// interval during which files are being processed.
    private(set) static var active: BatchRun?

    /// The tools a caller may still reach while a batch runs — the ones they
    /// need precisely BECAUSE it is running.
    ///
    /// Deliberately tiny and deliberately read-only-or-cancel: poll the run,
    /// stop it, and the three job/resource readouts that say what it is
    /// costing. Anything that edits a document is refused, because the batch
    /// owns every document it has open and an outside edit would land in a
    /// file that is about to be written and closed.
    static let allowedDuringBatch: Set<String> = [
        "batch_status", "cancel_batch", "job_status", "cancel_job", "resource_usage",
    ]

    let settings: BatchSettings
    /// This run's id, the handle `batch_status` and `cancel_batch` take. A
    /// UUID rather than a counter: a caller that kept an id across a restart
    /// must not have it answer for a different run.
    let id: String = UUID().uuidString
    private(set) var files: [URL]
    /// How many files have been taken off the list — also the 1-based
    /// number of the file being processed while one is in flight.
    private(set) var index = 0
    private(set) var report = BatchReport()

    /// Called with (1-based number, count, file name) in the run-loop turn
    /// BEFORE the one that processes that file — see `tick`, which spends
    /// one turn announcing and the next working, so the status line the
    /// user reads is the file being worked on rather than the one before it.
    var onProgress: ((Int, Int, String) -> Void)?
    var onFinish: ((BatchReport) -> Void)?

    private var cancelled = false
    private var timer: Timer?
    private var recorderWasSuspended = false
    /// The file `onProgress` has already been called for, so each file gets
    /// its announcing turn exactly once.
    private var announced = -1
    /// Destinations this run has already claimed, lower-cased. Two inputs
    /// that produce one output name is a refusal for the SECOND file, never
    /// an overwrite — and the comparison is case-insensitive because the
    /// common macOS volume is: on APFS as shipped, `A.tiff` and `a.tiff`
    /// are one file. A case-sensitive volume gets one conservative refusal
    /// it did not strictly need, which is the safe direction to be wrong in.
    private var claimed: Set<String> = []

    init(settings: BatchSettings) {
        self.settings = settings
        files = BatchRun.imageURLs(in: settings.inputFolder)
        report.action = settings.action.name
        report.outputFolder = settings.outputFolder.path
    }

    // MARK: Lifecycle

    func start() {
        assert(Thread.isMainThread)
        guard timer == nil else { return }
        // Published BEFORE the first turn, so the exclusion is in force from
        // the moment the run is accepted rather than from its first file.
        BatchRun.active = self
        // A batch is a replay: nothing it does may be recorded. ActionPlayer
        // suspends the recorder for each action run, but the opens and the
        // save_copy calls happen outside it, so the flag is held for the
        // whole run.
        recorderWasSuspended = ActionRecorder.shared.isSuspended
        ActionRecorder.shared.isSuspended = true
        // Interval 0: each turn does one whole file, which is where all the
        // time goes, and the run loop still drains events (the Cancel
        // button, the progress redraw) between turns.
        let timer = Timer(timeInterval: 0, repeats: true) { [weak self] _ in
            self?.tick()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    /// Stops at the next file boundary. The file in flight finishes: its
    /// core calls cannot be interrupted, and abandoning a half-written
    /// output is worse than one more file.
    func cancel() {
        cancelled = true
    }

    private func tick() {
        guard !cancelled else { return finish() }
        guard index < files.count else { return finish() }
        let position = index
        let url = files[position]
        // Two turns per file. The processing turn does not end until the
        // file has been opened, played and written — seconds, for a big
        // photograph — so a status line set in that same turn would not be
        // drawn until the work it describes was already done. Announcing in
        // its own turn lets AppKit draw it, with no display-forcing.
        guard announced == position else {
            announced = position
            onProgress?(position + 1, files.count, url.lastPathComponent)
            return
        }
        index += 1
        // Held per file rather than across the run because that is what the
        // scoped API gives, and it is the honest scope: the assertion exists
        // to stop App Nap demoting an app that hogs its main thread in the
        // background, and the only main-thread work is inside a file.
        let outcome = AppActivity.userInitiated("running the \(settings.action.name) batch") {
            process(url, at: position)
        }
        report.files.append(outcome.result)
        if let halt = outcome.halt {
            report.stopped = halt
            finish()
        }
    }

    private func finish() {
        timer?.invalidate()
        timer = nil
        // Only ever clears THIS run: a `finish` that fired for a run that had
        // already been replaced must not unlock the app for the new one.
        if BatchRun.active === self { BatchRun.active = nil }
        ActionRecorder.shared.isSuspended = recorderWasSuspended
        report.cancelled = cancelled && report.stopped == nil
        report.notProcessed = max(0, files.count - index)
        onFinish?(report)
    }

    // MARK: One file

    /// Opens `url`, plays the action on it, and writes the result — or
    /// says why it did none of those.
    ///
    /// `halt` is non-nil when the run must stop here. **"Stop on error"
    /// says two things, and the checkbox alone says neither:** ON (the
    /// default) a step failure halts the whole run AND the current file is
    /// not written; OFF the file IS written in whatever state the action
    /// left it and every failed step is listed against it. Without the
    /// first half a file whose action died at step 7 would sit in the
    /// output folder half-edited with nothing in the report saying so.
    private func process(_ url: URL, at position: Int) -> (result: BatchFileResult, halt: String?) {
        let name = url.lastPathComponent
        // Preset mode names its own output through Quick Export's naming
        // (`quickExportDestination`), which expands the pattern BEFORE the
        // encode — {width}/{height} come from the core's output size — so
        // `processWithPreset` pre-checks the name here and an existing file
        // is SKIPPED, not a halt. The containment and claimed-set checks
        // below stay flat-mode only: the preset's folder is the output, and
        // Quick Export never overwrites. The delivery is the tool's resolved
        // form (settings + destination + pattern), which refuses a taken name
        // rather than suffixing it (`QuickExportCollision.refuse`), so a
        // batch never writes a `-2` copy. The one gap is a {date} pattern run
        // across midnight: the pre-check and the delivery may expand
        // different days, and the worst case is Quick Export's own exists
        // refusal as a `failed` row — never an overwrite.
        guard settings.preset == nil else {
            return processWithPreset(url, at: position)
        }
        let destination = BatchRun.outputURL(
            for: url, index: position, of: files.count, settings: settings)

        // Containment, checked per file and not only per run: the name the
        // template produced must land in the output folder itself. The same
        // identity test the Setup pane uses for "a different folder", so
        // there is one rule and one implementation.
        guard BatchRun.sameDirectory(
            destination.deletingLastPathComponent(), settings.outputFolder)
        else {
            return (
                BatchFileResult(
                    input: name,
                    outcome: .skipped(
                        reason: "the name “\(destination.lastPathComponent)” would write "
                            + "outside the output folder")), nil)
        }
        guard claimed.insert(destination.path.lowercased()).inserted else {
            return (
                BatchFileResult(
                    input: name,
                    outcome: .skipped(
                        reason: "another file in this run already claimed "
                            + "\(destination.lastPathComponent) — add {n} to the naming "
                            + "template to keep them apart")), nil)
        }
        var isDirectory: ObjCBool = false
        if FileManager.default.fileExists(atPath: destination.path, isDirectory: &isDirectory) {
            if isDirectory.boolValue {
                return (
                    BatchFileResult(
                        input: name,
                        outcome: .skipped(
                            reason: "\(destination.lastPathComponent) is a folder")), nil)
            }
            if !settings.overwrite {
                return (
                    BatchFileResult(
                        input: name,
                        outcome: .skipped(
                            reason: "\(destination.lastPathComponent) already exists — turn on "
                                + "“Overwrite existing files” to replace it")), nil)
            }
        }

        let document: ImageDocument
        do {
            document = try open(url)
        } catch {
            let reason = "could not open it: \(error.localizedDescription)"
            return (
                BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                settings.stopOnError ? "stopped at \(name): \(reason)" : nil)
        }
        // close() does NOT prompt — canClose is what prompts — so a batch
        // document that the action left dirty goes away silently, and the
        // next open never inherits its window, its undo stack or its id.
        defer { document.close() }

        let played = ActionPlayer.run(
            settings.action, on: document, parameters: settings.parameters,
            stopOnError: settings.stopOnError)
        if let mismatch = played.canvasMismatch, !report.warnings.contains(mismatch) {
            report.warnings.append(mismatch)
        }
        if settings.stopOnError, let failure = played.failures.first {
            let reason = "step \(failure.step) (\(failure.tool)): \(failure.reason)"
            return (
                BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                "stopped at \(name), step \(failure.step): \(failure.reason)")
        }

        switch write(document, to: destination) {
        case .failure(let failure):
            let reason = failure.message
            return (
                BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                settings.stopOnError ? "stopped at \(name): \(reason)" : nil)
        case .success(let carried):
            guard played.failures.isEmpty else {
                // Stop on error is OFF (the ON case returned above): the
                // file was written, so the row says where it went AND
                // lists every step that failed on the way.
                let reasons = played.failures
                    .map { "step \($0.step) (\($0.tool)): \($0.reason)" }
                    .joined(separator: "; ")
                return (
                    BatchFileResult(
                        input: name,
                        outcome: .failed(
                            reason: reasons, destination: destination.lastPathComponent)), nil)
            }
            return (
                BatchFileResult(
                    input: name,
                    outcome: .wrote(
                        destination: destination.lastPathComponent, carried: carried)), nil)
        }
    }

    /// The preset path: open, play, then deliver through the `quick_export`
    /// TOOL, which owns the naming, the refusals and the recorded-step rule.
    /// Nothing about the delivery is re-implemented here.
    private func processWithPreset(_ url: URL, at position: Int)
        -> (result: BatchFileResult, halt: String?) {
        let name = url.lastPathComponent
        let document: ImageDocument
        do {
            document = try open(url)
        } catch {
            let reason = "could not open it: \(error.localizedDescription)"
            return (
                BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                settings.stopOnError ? "stopped at \(name): \(reason)" : nil)
        }
        defer { document.close() }

        let played = ActionPlayer.run(
            settings.action, on: document, parameters: settings.parameters,
            stopOnError: settings.stopOnError)
        if let mismatch = played.canvasMismatch, !report.warnings.contains(mismatch) {
            report.warnings.append(mismatch)
        }
        if settings.stopOnError, let failure = played.failures.first {
            let reason = "step \(failure.step) (\(failure.tool)): \(failure.reason)"
            return (
                BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                "stopped at \(name), step \(failure.step): \(failure.reason)")
        }

        let resolved: ExportSettings
        do {
            resolved = try resolvedDeliverySettings()
        } catch {
            let reason = error.localizedDescription
            return (
                BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                settings.stopOnError ? "stopped at \(name): \(reason)" : nil)
        }
        // The pre-check: the exact name Quick Export will write, expanded
        // now, so a file an earlier run left is a SKIP with a reason rather
        // than a refusal that halts the run.
        if let preset = settings.preset, let doc = document.doc {
            let destination: URL
            do {
                destination = try document.quickExportDestination(
                    preset: preset, settings: resolved, sourceSize: (doc.width, doc.height),
                    ordinal: (position + 1, files.count), date: Date())
            } catch {
                let reason = error.localizedDescription
                return (
                    BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                    settings.stopOnError ? "stopped at \(name): \(reason)" : nil)
            }
            if FileManager.default.fileExists(atPath: destination.path) {
                return (
                    BatchFileResult(
                        input: name,
                        outcome: .skipped(
                            reason: "\(destination.lastPathComponent) already exists — an export "
                                + "preset never overwrites a file")), nil)
            }
        }

        switch deliver(document, resolved: resolved, at: position) {
        case .failure(let failure):
            let reason = failure.message
            return (
                BatchFileResult(input: name, outcome: .failed(reason: reason, destination: nil)),
                settings.stopOnError ? "stopped at \(name): \(reason)" : nil)
        case .success(let written):
            guard played.failures.isEmpty else {
                let reasons = played.failures
                    .map { "step \($0.step) (\($0.tool)): \($0.reason)" }
                    .joined(separator: "; ")
                return (
                    BatchFileResult(
                        input: name,
                        outcome: .failed(reason: reasons, destination: written.0)), nil)
            }
            return (
                BatchFileResult(
                    input: name,
                    outcome: .wrote(destination: written.0, carried: written.1)), nil)
        }
    }

    /// The settings a preset run delivers with: the preset's own, resolved
    /// (its named policy read now), with the run's explicit policy winning.
    /// Resolved ONCE per file, so the pre-check and the delivery name the
    /// same file.
    private func resolvedDeliverySettings() throws -> ExportSettings {
        guard let preset = settings.preset else {
            throw BatchWriteFailure("this run has no export preset")
        }
        var resolved = try preset.resolvedSettings()
        if let policy = settings.metadataPolicy { resolved.metadata = policy }
        return resolved
    }

    /// One delivery through the `quick_export` tool. `index`/`count` feed
    /// `{index}`, so a preset pattern can number the run's outputs.
    private func deliver(_ document: ImageDocument, resolved: ExportSettings, at position: Int)
        -> Result<(String, String?), BatchWriteFailure> {
        guard let preset = settings.preset else {
            return .failure(BatchWriteFailure("this run has no export preset"))
        }
        let arguments: [String: Any] = [
            "document_id": AgentServer.shared.documentID(for: document),
            "settings": resolved.json,
            "destination": preset.destination,
            "pattern": preset.pattern,
            "preset": preset.name,
            "index": position + 1,
            "count": files.count,
        ]
        guard JSONSerialization.isValidJSONObject(arguments),
              let data = try? JSONSerialization.data(withJSONObject: arguments)
        else {
            return .failure(BatchWriteFailure("the export arguments could not be encoded"))
        }
        // The batch's OWN dispatch: `ActionPlayer.replayRefusal` refuses every
        // outside call while `BatchRun.active` is set, and this delivery is
        // not one — without the marker every preset-mode file would fail in
        // band naming the batch that was making the call.
        let result = ActionPlayer.dispatchingOwnStep {
            AgentServer.shared.execute(
                tool: "quick_export", argumentsJSON: String(decoding: data, as: UTF8.self))
        }
        guard let parsed = try? JSONSerialization.jsonObject(with: Data(result.utf8)),
              let object = parsed as? [String: Any]
        else {
            return .failure(BatchWriteFailure("the export returned a result that could not be read"))
        }
        let content = (object["content"] as? [[String: Any]] ?? [])
            .compactMap { $0["text"] as? String }
        guard (object["isError"] as? Bool) != true else {
            let text = content.joined(separator: " ")
            return .failure(
                BatchWriteFailure(text.isEmpty ? "the file could not be written" : text))
        }
        guard let payload = content.first,
              let fields = (try? JSONSerialization.jsonObject(with: Data(payload.utf8)))
                as? [String: Any],
              let path = fields["path"] as? String
        else {
            return .failure(BatchWriteFailure("the export reported no path"))
        }
        var note: String?
        if let carried = fields["carried"] as? [String], !carried.isEmpty {
            note = "wrote " + carried.joined(separator: ", ")
        }
        return .success(((path as NSString).lastPathComponent, note))
    }

    /// Opens one file as a document that has window controllers but is
    /// never shown.
    ///
    /// The window controllers are not optional: `AgentServer.editor(_:)`
    /// finds the `EditorViewController` through them, and every selection
    /// step goes through it — without one, a select-then-fill action would
    /// silently fill the whole canvas. Not showing them keeps the screen
    /// still, and `noteNewRecentDocumentURL` is deliberately not called, so
    /// a 200-file run does not flush Open Recent.
    ///
    /// A file the user already has open gets its own second document here,
    /// read from disk, rather than the open one: a batch runs over what is
    /// IN the folder, and playing an action on the window someone is working
    /// in — then closing it — is the one thing a folder run must never do.
    private func open(_ url: URL) throws -> ImageDocument {
        let previousPSD = PSDImportRequest.mode
        PSDImportRequest.mode = .refuse
        defer { PSDImportRequest.mode = previousPSD }
        let controller = NSDocumentController.shared
        let type = try controller.typeForContents(of: url)
        // A camera RAW develops with the ACTION's settings and shows
        // nothing: no dialog is possible per file, and the agent path sets
        // the same mode for the same reason (AgentServer.openDocument).
        RawImportRequest.mode = .headless(settings.action.raw ?? RawDevelopSettings())
        defer { RawImportRequest.mode = .ask }
        guard let document = try controller.makeDocument(withContentsOf: url, ofType: type)
            as? ImageDocument
        else {
            throw ActionError("\(url.lastPathComponent) did not open as an image document")
        }
        controller.addDocument(document)
        document.makeWindowControllers()
        return document
    }

    /// Writes the played document through the `save_copy` tool. On success
    /// the payload's own account of what the format carried; on failure the
    /// tool's in-band message.
    private func write(
        _ document: ImageDocument, to destination: URL
    ) -> Result<String?, BatchWriteFailure> {
        // Every option is explicit — the format beats the extension, and the
        // three carry options are the pane's, not the document's defaults,
        // because a batch document was created seconds ago and its defaults
        // are the app's rather than anything the user chose. jpeg_quality
        // rides along for every format; save_copy reads it only for JPEG.
        let arguments: [String: Any] = [
            "document_id": AgentServer.shared.documentID(for: document),
            "path": destination.path,
            "format": settings.format.fileExtension,
            "jpeg_quality": settings.jpegQuality,
            "embed_profile": settings.embedProfile,
            "strip_metadata": settings.stripMetadata,
        ]
        // `isValidJSONObject` first: `data(withJSONObject:)` RAISES rather
        // than throws on a value it cannot write, and a raise inside a batch
        // would take the app down mid-run (AgentServer.saveAction says the
        // same thing at the same seam).
        guard JSONSerialization.isValidJSONObject(arguments),
              let data = try? JSONSerialization.data(withJSONObject: arguments)
        else {
            return .failure(BatchWriteFailure("the save arguments could not be encoded"))
        }
        // The batch's own dispatch, for `deliver`'s reason.
        let result = ActionPlayer.dispatchingOwnStep {
            AgentServer.shared.execute(
                tool: "save_copy", argumentsJSON: String(decoding: data, as: UTF8.self))
        }
        guard let parsed = try? JSONSerialization.jsonObject(with: Data(result.utf8)),
              let object = parsed as? [String: Any]
        else {
            return .failure(BatchWriteFailure("the save returned a result that could not be read"))
        }
        let content = (object["content"] as? [[String: Any]] ?? [])
            .compactMap { $0["text"] as? String }
        guard (object["isError"] as? Bool) != true else {
            let text = content.joined(separator: " ")
            return .failure(
                BatchWriteFailure(text.isEmpty ? "the file could not be written" : text))
        }
        return .success(BatchRun.carriedNote(content.first))
    }

    /// "wrote color_profile, exif; dropped iptc" out of `save_copy`'s
    /// payload — the same two lists `save_copy` reports to an agent, so the
    /// report never has to guess whether a profile survived the format.
    private static func carriedNote(_ payload: String?) -> String? {
        guard let payload = payload,
              let parsed = try? JSONSerialization.jsonObject(with: Data(payload.utf8)),
              let object = parsed as? [String: Any]
        else { return nil }
        var parts: [String] = []
        if let wrote = object["wrote"] as? [String], !wrote.isEmpty {
            parts.append("wrote " + wrote.joined(separator: ", "))
        }
        if let dropped = object["dropped"] as? [String], !dropped.isEmpty {
            parts.append("dropped " + dropped.joined(separator: ", "))
        }
        return parts.isEmpty ? nil : parts.joined(separator: "; ")
    }
}

/// The write half's failure, kept as its own type so `write` can report a
/// reason through `Result` without borrowing `ActionError`, which is the
/// Actions feature's own vocabulary.
struct BatchWriteFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }

    init(_ message: String) {
        self.message = message
    }
}

// MARK: - The path rules (pure)

extension BatchRun {
    /// The images in `folder`, **top level only** — a batch that recursed
    /// would walk into an output folder nested under its input and eat its
    /// own results.
    ///
    /// The predicate is the identical one `FileDropView.readableImageURLs`
    /// uses for a drag: a UTType that conforms to one of
    /// `ImageDocument.readableTypes`. Live Photo pair collapsing is
    /// deliberately NOT applied — a batch of stills is a batch of stills,
    /// and a folder holding both halves of a pair has two files in it.
    static func imageURLs(in folder: URL) -> [URL] {
        let keys: [URLResourceKey] = [.contentTypeKey, .isDirectoryKey]
        let entries =
            (try? FileManager.default.contentsOfDirectory(
                at: folder, includingPropertiesForKeys: keys,
                options: [.skipsHiddenFiles, .skipsSubdirectoryDescendants])) ?? []
        let readable = ImageDocument.readableTypes.compactMap(UTType.init)
        return entries.filter { url in
            let values = try? url.resourceValues(forKeys: Set(keys))
            guard values?.isDirectory != true else { return false }
            guard let type = values?.contentType ?? UTType(filenameExtension: url.pathExtension)
            else { return false }
            return readable.contains { type.conforms(to: $0) }
        }
        .sorted {
            $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending
        }
    }

    /// True when two URLs name the same directory.
    ///
    /// `fileResourceIdentifier` is the direct question — "is this the same
    /// object on this volume?" — where a path compare is a proxy for it.
    /// Measured on this machine, `resolvingSymlinksInPath().standardizedFileURL`
    /// *does* case-canonicalize on APFS (`…/RASTERIZE/samples` resolved to
    /// `…/rasterize/samples` and compared equal), so the string form is a
    /// sound fast path and stays as the fallback for anything the identity
    /// key cannot be read for — a folder that vanished between the panel
    /// and the run, say.
    static func sameDirectory(_ a: URL, _ b: URL) -> Bool {
        let first = try? a.resourceValues(forKeys: [.fileResourceIdentifierKey])
            .fileResourceIdentifier
        let second = try? b.resourceValues(forKeys: [.fileResourceIdentifierKey])
            .fileResourceIdentifier
        if let first = first, let second = second { return first.isEqual(second) }
        // Compared as PATHS, not as URLs: a URL built from a file's parent
        // carries a trailing slash and one built from a chosen folder does
        // not, and `standardizedFileURL` keeps the difference — measured,
        // `/tmp/out/` and `/tmp/out` compare unequal as URLs and equal as
        // paths. This half only runs when a folder cannot be read at all,
        // where a false "different" is the safe way to be wrong.
        return a.resolvingSymlinksInPath().standardizedFileURL.path
            == b.resolvingSymlinksInPath().standardizedFileURL.path
    }

    /// What is wrong with a naming template, or nil when it is usable.
    ///
    /// Refused at edit time rather than at run time: a template that cannot
    /// produce a file name is a mistake the user can see and fix in the
    /// field, and refusing 200 files one at a time would be a report nobody
    /// wants to read.
    static func templateProblem(_ template: String) -> String? {
        let trimmed = template.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return "A naming template cannot be empty." }
        // "/" is a path separator and ":" is what Finder shows as one, so
        // either would let a template reach out of the output folder or
        // produce a name the user did not read as a name.
        if trimmed.contains("/") { return "A naming template cannot contain “/”." }
        if trimmed.contains(":") { return "A naming template cannot contain “:”." }
        // An unknown token is a typo ("{names}"), and a template that kept
        // it verbatim would write the same literal name for every file.
        var scanner = trimmed[...]
        while let open = scanner.firstIndex(of: "{") {
            guard let close = scanner[open...].firstIndex(of: "}") else {
                return "A naming template has a “{” with no “}”."
            }
            let token = String(scanner[open...close])
            guard token == "{name}" || token == "{n}" else {
                return "Unknown token “\(token)” — the template takes {name} and {n}."
            }
            scanner = scanner[scanner.index(after: close)...]
        }
        return nil
    }

    /// The file name one input produces: the template with its tokens
    /// filled in, plus the chosen format's extension — never the
    /// template's, which is why a template may not carry one.
    ///
    /// `{n}` is zero-padded to the digit width of the file count, so a
    /// hundred files sort as 001…100 in Finder rather than 1, 10, 100, 11.
    static func outputName(
        template: String, input: URL, index: Int, of count: Int, format: ExportFormat
    ) -> String {
        let base = input.deletingPathExtension().lastPathComponent
        let width = String(max(count, 1)).count
        let number = String(format: "%0\(width)d", index + 1)
        var name = sanitised(
            template
                .replacingOccurrences(of: "{name}", with: base)
                .replacingOccurrences(of: "{n}", with: number))
        // A template that survived `templateProblem` can still produce an
        // unusable name from the INPUT's own name (a file called ".", or
        // one long enough to exceed the 255-byte limit once the extension
        // is on it). Falling back to the basename, and then to a constant,
        // means a file is never skipped for want of a name.
        if name.isEmpty { name = sanitised(base) }
        if name.isEmpty { name = "image" }
        return name + "." + format.fileExtension
    }

    /// Where one input file's output goes.
    static func outputURL(
        for input: URL, index: Int, of count: Int, settings: BatchSettings
    ) -> URL {
        settings.outputFolder.appendingPathComponent(
            outputName(
                template: settings.nameTemplate, input: input, index: index, of: count,
                format: settings.format))
    }

    /// A file name with everything that is not one taken out: the two path
    /// characters, control characters, a leading dot (which would hide the
    /// file), and anything past 200 UTF-8 BYTES — the filesystem's limit is
    /// 255 bytes, not characters, so a name of emoji would blow a
    /// character cap four times over and leaves room for the extension.
    private static func sanitised(_ name: String) -> String {
        var out = name
            .components(separatedBy: CharacterSet(charactersIn: "/:"))
            .joined(separator: "-")
            .components(separatedBy: .controlCharacters)
            .joined()
            .trimmingCharacters(in: .whitespaces)
        while out.hasPrefix(".") { out.removeFirst() }
        while out.utf8.count > 200 { out.removeLast() }
        return out.trimmingCharacters(in: .whitespaces)
    }
}
