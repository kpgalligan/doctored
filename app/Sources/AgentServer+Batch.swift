import AppKit

/// `run_batch`, `batch_status`, `cancel_batch` — the headless mirror of the
/// Batch window (`BatchWindowController`).
///
/// **Why Batch has a tool now, having deliberately had none.** The original
/// argument was that Batch performs no edit — it is `open_document` +
/// `run_action` + `save_copy` in a loop, and an agent already has all three.
/// That is still true of the EDIT, and it is why nothing here re-implements
/// one. What an agent could not reproduce is the part that is not an edit: the
/// folder walk, the naming template, the containment guard, the claimed-set
/// skip that never overwrites, and the two halves of "stop on error". Written
/// by hand per call those become 200 chances to write outside the output
/// folder; `BatchRun` already had them, and this is the door to it.
///
/// **The modal window is NOT driven.** `run_batch` builds the same
/// `BatchSettings` the pane builds and starts the same `Timer`-driven
/// `BatchRun`, with no window at all: the run returns a `batch_id`
/// immediately, the caller polls `batch_status` and may `cancel_batch`. Trying
/// to drive the sheet would mean running a modal session from a tool call,
/// which cannot answer while it is up.
///
/// **While a batch runs, every OTHER tool is refused in band**
/// (`ActionPlayer.replayRefusal`, `BatchRun.allowedDuringBatch`): the run owns
/// the documents it opens and the recorder for its whole length, and an
/// outside edit would land in a file that is about to be written and closed.
/// The batch's own calls are exempt through `ActionPlayer.dispatchingOwnStep`,
/// which is why the clause lives in the player rather than in `execute`.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let batchHandlers: [String: ToolHandler] = [
        "run_batch": { $0.runBatch },
        "batch_status": { $0.batchStatus },
        "cancel_batch": { $0.cancelBatch },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let batchToolFacts: [String: ToolFacts] = [
        // Phase 12B: a batch plays actions (recording one would nest a
        // replay), and the other four read or write the action library —
        // none of them changes a picture.
        // None of these handlers resolves a document either — New
        // Document, the clipboard read and New from Clipboard, the action
        // library and recorder, and the batch and bundle tools (a batch opens
        // each file at its own depth and plays through this same gate).
        // Phase 12B's batch and bundle tools read no document, so they are
        // in `documentFreeTools`. `reorder_layers` is a TYPED tool
        // since 12E-1 (`typedTools` below): the core owns the typed reorder.
        // Phase 12C: a colour LABEL is metadata with a typed implementation
        // of its own (the `layer_label` edit), and `find_layers` only reads —
        // neither touches pixel storage. `new_document` and the three
        // document-preset tools read no document, so they are in
        // `documentFreeTools`.
        // Phase 12B's batch and bundle tools read no document, so they are
        // in `DocumentPrecision.documentFreeTools`. A float document's
        // own admitted set still gates every step a batch plays.
        "run_batch": [.nonRecordable, .documentFree],
        "batch_status": [.nonRecordable, .documentFree],
        "cancel_batch": [.nonRecordable, .documentFree],
    ]

    // MARK: - Starting

    /// Mirrors the Batch window's Run button.
    ///
    /// Everything that can be refused is refused BEFORE `BatchRun.start()`,
    /// so a rejected submission leaves no `batch_id`, no open document and an
    /// untouched output folder: a batch already running, a broken action, a
    /// parameter that will not bind, a preset whose destination folder does
    /// not exist, a template that cannot produce a name, an output folder
    /// that is the input folder, and an input folder holding nothing
    /// readable.
    func runBatch(_ a: [String: Any]) throws -> String {
        guard BatchRun.active == nil else {
            throw ToolError(
                message: "A batch is already running on this app. Poll it with batch_status "
                    + "or stop it with cancel_batch.")
        }
        guard let name = stringArg(a, "name") else {
            throw ToolError(message: "Missing required argument: name")
        }
        // `entry`, not `action(named:)`, for `runAction`'s reason: a file
        // that will not parse has to answer with WHY rather than with "there
        // is no action of that name".
        let action: Action
        switch ActionLibrary.entry(named: name) {
        case .ok(let decoded)?: action = decoded
        case .broken(let file, let reason)?: throw ToolError(message: "\(file): \(reason)")
        case nil:
            throw ToolError(message: "No action named \"\(name)\". Call list_actions.")
        }
        // The action-level refusal, at SUBMISSION: a parameter error says
        // nothing about any one picture, so it must never become 200
        // identical report rows (`ActionParameters`).
        var supplied: [String: Any] = [:]
        if let parsed = ActionParameters.argument(a["parameters"]) {
            switch parsed {
            case .success(let object): supplied = object
            case .failure(let error): throw ToolError(message: error.message)
            }
        }
        let bound: [String: Any]
        switch ActionParameters.bind(supplied, to: action) {
        case .success(let values): bound = values
        case .failure(let error): throw ToolError(message: error.message)
        }

        let input = try folder(a, "input", mustExist: true)
        let preset = try batchPreset(a)
        if let preset {
            try AgentPathGuard.shared.check(
                exportPreset: preset, argument: "export preset \"\(preset.name)\"")
        }
        // A preset run's destination IS the preset's folder, exactly as the
        // pane's own rule: the `output` argument, the template and
        // `overwrite` do not apply, because Quick Export owns the naming and
        // never overwrites.
        let output: URL
        if let preset = preset {
            // The Batch window's refusal, at submission: a missing folder
            // would otherwise fail every file in turn.
            do {
                output = try preset.requireDestinationFolder()
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
        } else {
            output = try folder(a, "output", mustExist: false)
        }
        guard !BatchRun.sameDirectory(input, output) else {
            throw ToolError(
                message: "The output folder must not be the input folder — a batch that wrote "
                    + "into the folder it reads would eat its own results.")
        }
        let template = (stringArg(a, "name_template") ?? "{name}")
            .trimmingCharacters(in: .whitespaces)
        if preset == nil, let problem = BatchRun.templateProblem(template) {
            throw ToolError(message: problem)
        }
        let files = BatchRun.imageURLs(in: input)
        guard !files.isEmpty else {
            throw ToolError(message: "\(input.path) holds no images Doctored can open.")
        }
        let format = try batchFormat(a)
        let policy = try batchMetadataPolicy(a)
        let settings = BatchSettings(
            action: action, inputFolder: input, outputFolder: output, format: format,
            jpegQuality: intArg(a, "jpeg_quality") ?? 90,
            embedProfile: boolArg(a, "embed_profile") ?? true,
            stripMetadata: boolArg(a, "strip_metadata") ?? false,
            nameTemplate: template,
            overwrite: preset == nil && (boolArg(a, "overwrite") ?? false),
            stopOnError: boolArg(a, "stop_on_error") ?? true,
            preset: preset, metadataPolicy: policy, parameters: bound)
        let run = BatchRun(settings: settings)
        // The finished report is kept so a caller that polled once after the
        // end still gets the rows; `BatchRun.active` is already nil by then.
        run.onFinish = { report in AgentServer.lastBatchReport = (run.id, report) }
        run.start()
        return try jsonResult([
            "ok": true, "batch_id": run.id, "files": run.files.count,
            "output_folder": output.path,
            "note": "The run is under way. Poll batch_status for progress and the per-file "
                + "report; cancel_batch stops it at the next file boundary. Every other tool "
                + "is refused while it runs.",
        ])
    }

    // MARK: - Polling and stopping

    /// Mirrors the Run pane's progress line and its report.
    ///
    /// Answers for the LIVE run, or — when none is live — for the last one
    /// that finished in this session, so a caller that polls once after the
    /// end still reads the rows instead of "there is no batch".
    func batchStatus(_ a: [String: Any]) throws -> String {
        if let run = BatchRun.active {
            try checkBatchID(a, run.id)
            return try jsonResult([
                "ok": true, "batch_id": run.id, "running": true,
                "processed": run.index, "total": run.files.count,
                "report": run.report.jsonObject(),
            ])
        }
        guard let last = AgentServer.lastBatchReport else {
            throw ToolError(
                message: "No batch has run in this session. Start one with run_batch.")
        }
        try checkBatchID(a, last.id)
        return try jsonResult([
            "ok": true, "batch_id": last.id, "running": false,
            "processed": last.report.files.count,
            "total": last.report.files.count + last.report.notProcessed,
            "report": last.report.jsonObject(),
        ])
    }

    /// Mirrors the Run pane's Cancel button: stops at the next file boundary.
    /// The file in flight finishes — a core call cannot be interrupted, and
    /// abandoning a half-written output is worse than one more file.
    func cancelBatch(_ a: [String: Any]) throws -> String {
        guard let run = BatchRun.active else {
            throw ToolError(message: "No batch is running.")
        }
        try checkBatchID(a, run.id)
        run.cancel()
        return try jsonResult([
            "ok": true, "batch_id": run.id, "processed": run.index,
            "total": run.files.count,
            "note": "The run stops at the next file boundary; the file in flight finishes. "
                + "Poll batch_status for the final report.",
        ])
    }

    /// The last finished run's report, so `batch_status` can answer after the
    /// end. One slot, not a history: a report holds one row per file and a
    /// caller that wanted the previous one had it.
    private(set) static var lastBatchReport: (id: String, report: BatchReport)?

    // MARK: - Arguments

    /// `batch_id` checked when it is given. Optional, because there is at most
    /// ONE batch app-wide and a caller that has just started it should not
    /// have to carry the id; refused when it names a different run, so a
    /// caller that DOES carry one is never answered about the wrong batch.
    private func checkBatchID(_ a: [String: Any], _ id: String) throws {
        guard let wanted = stringArg(a, "batch_id"), wanted != id else { return }
        throw ToolError(
            message: "batch_id \"\(wanted)\" is not the current batch (\"\(id)\").")
    }

    private func folder(_ a: [String: Any], _ key: String, mustExist: Bool) throws -> URL {
        guard let path = stringArg(a, key), !path.isEmpty else {
            throw ToolError(message: "Missing required argument: \(key)")
        }
        let url = URL(
            fileURLWithPath: (path as NSString).expandingTildeInPath, isDirectory: true)
        var isDirectory: ObjCBool = false
        let exists = FileManager.default.fileExists(
            atPath: url.path, isDirectory: &isDirectory)
        if exists {
            guard isDirectory.boolValue else {
                throw ToolError(message: "\(url.path) is a file, not a folder.")
            }
            return url
        }
        guard !mustExist else { throw ToolError(message: "\(url.path) is not there.") }
        // The OUTPUT folder may be created — the pane's own panel does it —
        // but only here, before the run: `save_copy` never creates one, and a
        // run that discovered the folder was missing at file 1 would fail all
        // 200.
        do {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        } catch {
            throw ToolError(
                message: "could not create \(url.path): \(error.localizedDescription)")
        }
        return url
    }

    private func batchFormat(_ a: [String: Any]) throws -> ExportFormat {
        guard let name = stringArg(a, "format") else { return .png }
        guard let format = ExportFormat.from(fileExtension: name),
              ExportFormat.flatSaveCases.contains(format) else {
            throw ToolError(
                message: "format must be one of "
                    + ExportFormat.flatSaveCases.map { $0.fileExtension }
                    .joined(separator: ", ") + " (got \"\(name)\")")
        }
        return format
    }

    private func batchPreset(_ a: [String: Any]) throws -> ExportPreset? {
        guard let name = stringArg(a, "preset") else { return nil }
        let entries = (try? ExportPresetLibrary.entries()) ?? []
        guard let preset = entries.first(where: {
            $0.name.compare(name, options: .caseInsensitive) == .orderedSame
        }) else {
            throw ToolError(
                message: "No export preset named \"\(name)\". Call list_export_presets.")
        }
        return preset
    }

    private func batchMetadataPolicy(_ a: [String: Any]) throws -> MetadataPolicy? {
        guard let name = stringArg(a, "metadata_policy") else { return nil }
        let entries = (try? MetadataPolicyLibrary.entries()) ?? []
        guard let entry = entries.first(where: {
            $0.name.compare(name, options: .caseInsensitive) == .orderedSame
        }) else {
            throw ToolError(
                message: "No metadata policy named \"\(name)\". Call list_metadata_policies.")
        }
        return entry.policy
    }
}
