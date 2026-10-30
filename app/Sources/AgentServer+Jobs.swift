import AppKit

// A handler that ends in `finishJobCall` must be listed in `AgentServer.documentJobTools`, or its
// `background` call records twice and its waiting call records no resolved `layer`; the 16-bit
// brush/eraser/clone/dodge_burn layer strokes are `precisionStrokeJobTools` instead, which
// `defersRecording` reads only for a 16-bit document, so their 8-bit calls record as before.
// A `background` call records exactly what the same waiting call records: `finishJobCall`
// pins the job's `layer` only for `documentJobTools`, the set `execute` injects it for.

/// MCP mirrors the progress panel's status/Cancel and the resource diagnostics.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let jobsHandlers: [String: ToolHandler] = [
        "job_status": { $0.jobStatus },
        "cancel_job": { $0.cancelJob },
        "resource_usage": { $0.resourceUsage },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let jobsToolFacts: [String: ToolFacts] = [
        // None of these handlers resolves a document.
        "job_status": [.nonRecordable, .documentFree],
        "cancel_job": [.nonRecordable, .documentFree],
        "resource_usage": [.nonRecordable, .documentFree],
    ]

    /// The handlers that commit through a `DocumentJobs` job and reply
    /// through `finishJobCall`. `execute` reads it twice: it injects the
    /// resolved `layer` into a call that named none (so the recorded step
    /// names the layer the job edited), and it defers recording a
    /// `background` call to the job's completion, which `finishJobCall`
    /// records once, pinning the job's `layer` for these tools only.
    static let documentJobTools: Set<String> = [
        "content_aware_fill", "spot_heal_stroke", "heal_stroke", "patch_region",
        "remove_stroke", "content_aware_move",
    ]

    /// Tools whose 16-bit layer strokes commit through a `DocumentJobs` job
    /// and reply through `finishJobCall`. Unlike `documentJobTools`,
    /// neither `execute` nor `finishJobCall` injects a `layer` for them, so
    /// 8-bit, waiting 16-bit and `background` 16-bit calls record the same
    /// arguments and replay onto the active layer.
    static let precisionStrokeJobTools: Set<String> = ["brush_stroke", "eraser_stroke", "clone_stamp", "dodge_burn",
        "blur_stroke", "sharpen_stroke", "smudge_stroke", "sponge_stroke", "history_brush_stroke",
        "art_history_brush_stroke"]

    /// Whether `execute` leaves recording a call to the job's completion,
    /// which `finishJobCall` records once: a `background` call of a job tool
    /// outside an Action replay.
    static func defersRecording(tool: String, arguments: [String: Any], document: ImageDocument?) -> Bool {
        guard shared.boolArg(arguments, "background") == true, !ActionPlayer.isPlaying else { return false }
        return documentJobTools.contains(tool)
            || (document?.doc?.isHighDepth == true && precisionStrokeJobTools.contains(tool))
    }

    func jobStatus(_ a: [String: Any]) throws -> String {
        guard let id = stringArg(a, "job_id"), let job = DocumentJobs.find(id) else {
            throw ToolError(message: "Unknown job_id. Completed jobs retain their last 16 reports.")
        }
        DocumentJobs.poll(job)
        return try jsonResult(job.report)
    }

    func cancelJob(_ a: [String: Any]) throws -> String {
        guard let id = stringArg(a, "job_id"), let job = DocumentJobs.find(id) else {
            throw ToolError(message: "Unknown job_id.")
        }
        if job.state == "running" { job.control.cancel() }
        DocumentJobs.poll(job)
        return try jsonResult(job.report)
    }

    func resourceUsage(_ a: [String: Any]) throws -> String {
        let job = DocumentJobs.active
        var report = try RasterResources.shared.report(
            scratch: (job?.scratch ?? 0) + (job?.control.scratchBytes ?? 0) + (job?.renderScratch.bytes ?? 0) + BackgroundWork.scratchEstimate,
            evict: boolArg(a, "evict_caches") ?? false)
        if let job { report["active_job"] = job.report }
        report["editing_state_retention"] = EditingResourceReport.collect()
        return try jsonResult(report)
    }

    /// Async calls record at successful commit. Waiting calls use the existing
    /// execute seam after completion, so cancelled/refused jobs record nothing.
    /// An async call records what the waiting call records: `background`
    /// removed, and the job's `layer` pinned only for `documentJobTools`, the
    /// set `execute` injects it for; a caller's own `layer` is kept otherwise.
    /// `unchanged` answers a waiting job that ended `unchanged` (a no-op)
    /// in place of the thrown error, so the tool replies `changed: false`
    /// and the no-op records like `heal_stroke`'s.
    func finishJobCall(_ job: DocumentJob, tool: String, arguments: [String: Any],
                       unchanged: (() throws -> String)? = nil) throws -> String {
        job.agentOrigin = true
        job.fade = FadeStep.hint(tool: tool, arguments: arguments)
        if boolArg(arguments, "background") == true, !ActionPlayer.isPlaying {
            let prior = job.completion
            var capturedArguments = arguments
            capturedArguments.removeValue(forKey: "background")
            if Self.documentJobTools.contains(tool) {
                capturedArguments["layer"] = job.layer
            }
            job.completion = { completed in
                prior?(completed)
                if completed.state == "completed" {
                    ActionRecorder.shared.recordAgentCall(tool, capturedArguments, on: completed.document)
                }
            }
            return try jsonResult(job.report)
        }
        var waitError: Error?
        do { try DocumentJobs.wait(job) } catch { waitError = error }
        if let unchanged, job.state == "unchanged" { return try unchanged() }
        if let waitError { throw waitError }
        var result = job.report
        result["ok"] = true
        return try jsonResult(result)
    }
}
