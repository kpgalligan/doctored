import AppKit

/// Merge to HDR for the agent:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `mergeToHDR` | File ▸ Merge to HDR… (`AppDelegate.mergeToHDR`, AppDelegate+MergeHDR.swift) |
/// | `mergeLayersToHDR` | Layer ▸ Merge Layers to HDR… (`EditorViewController.mergeLayersToHDR`, EditorViewController+MergeHDR.swift) |
///
/// Both CREATE a document and edit none, so both are `nonRecordable`
/// (`load_files_into_stack`'s reason: a replay would pop a second window
/// beside the run's target). `merge_to_hdr` resolves no document and is
/// `documentFree`; unlike the menu, which sorts the chosen files by name, it
/// keeps the caller's `paths` order. `merge_layers_to_hdr` reads an open 8- or
/// 16-bit document and is a `commonTool` without `floatTool`, so a 32-bit
/// float document refuses it by the float table. Where the card would ask for
/// an EV the estimate could not find, the handler refuses with the same
/// sentence (`MergeHDR.Prepared.notice`) and the caller passes `evs`.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let mergeHDRHandlers: [String: ToolHandler] = [
        "merge_to_hdr": { $0.mergeToHDR },
        "merge_layers_to_hdr": { $0.mergeLayersToHDR },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let mergeHDRToolFacts: [String: ToolFacts] = [
        // Creates a document and resolves none.
        "merge_to_hdr": [.nonRecordable, .documentFree],
        // Creates a document from an 8- or 16-bit one; a float document
        // has nothing to merge to.
        "merge_layers_to_hdr": [.nonRecordable, .commonTool],
    ]

    /// `evs` when given: one finite number per frame, in the frames' order.
    private func exposureValues(_ a: [String: Any], tool: String) throws -> [Double]? {
        guard let value = a["evs"] else { return nil }
        guard let raw = value as? [Any] else {
            throw ToolError(message: "\(tool): evs must be an array of numbers, one per frame.")
        }
        return try raw.map { entry -> Double in
            guard NativeBrushStroke.jsonBoolean(entry) == nil,
                  let number = Self.finiteNumber(entry) else {
                throw ToolError(message: "\(tool): evs must hold finite numbers.")
            }
            return number
        }
    }

    /// The shared second half: the options from the arguments, the one job,
    /// the new document and the reply. An unknown EV with no `evs` refuses
    /// before anything is merged or opened.
    private func mergedReply(_ prepared: consuming MergeHDR.Prepared, _ a: [String: Any],
                             evs: [Double]?) throws -> [String: Any] {
        let frames = prepared.frames
        let known = frames.compactMap(\.ev)
        guard known.count == frames.count else {
            throw ToolError(
                message: (prepared.notice ?? "The frames' exposures could not be estimated")
                    + ". Pass evs: one number per frame, in stops.")
        }
        let options = MergeHDR.Options(
            evs: known, evsEntered: evs != nil, align: boolArg(a, "align") ?? true,
            deghost: boolArg(a, "deghost") ?? false)
        let outcome = try MergeHDR.merge(consume prepared, options: options)
        let document = try ImageDocument.openMergedHDR(outcome, name: stringArg(a, "name"))
        var reasons = outcome.leftOut.makeIterator()
        let listed = outcome.frames.map { frame -> [String: Any] in
            let used = frame.alignment != .unregistered
            var fields: [String: Any] = [
                "index": frame.layer, "name": frame.name, "ev": frame.ev ?? NSNull(),
                "ev_source": frame.source.rawValue, "used": used,
            ]
            if let path = frame.path { fields["path"] = path }
            if let alignment = frame.alignment { fields["alignment"] = alignment.rawValue }
            if !used, let left = reasons.next() { fields["reason"] = left.reason }
            return fields
        }
        func count(_ key: String) -> Any { outcome.report[key] ?? 0 }
        var reply: [String: Any] = [
            "ok": true,
            "document": summary(document),
            "depth": "rgba32_float",
            "profile": outcome.document.profileName,
            "canvas": ["width": outcome.document.width, "height": outcome.document.height],
            "frames": listed,
            "reference": outcome.reference,
            "aligned": options.align,
            "deghost": options.deghost,
            "all_clipped_pixels": count("all_clipped_pixels"),
            "all_dark_pixels": count("all_dark_pixels"),
            "deghosted_pixels": count("deghosted_pixels"),
            "working_memory_plan_bytes": outcome.planBytes,
        ]
        let raw = outcome.frames.contains { frame in
            frame.path.map { RawImage.isRawFile(URL(fileURLWithPath: $0)) } ?? false
        }
        if raw { reply["raw_development"] = MergeHDR.rawDevelopmentNote }
        if let notice = outcome.notice { reply["note"] = notice }
        return reply
    }

    /// Mirrors File ▸ Merge to HDR…. The stack is planned and admitted before
    /// any file is decoded (`StackLoad.plan`) and the merge before its job, so
    /// a refusal opens nothing.
    func mergeToHDR(_ a: [String: Any]) throws -> String {
        guard let raw = a["paths"] as? [Any] else {
            throw ToolError(
                message: "paths must be an array of at least \(MergeHDR.minimumFrames) file paths.")
        }
        let urls = try raw.map { entry -> URL in
            guard let path = entry as? String, !path.isEmpty else {
                throw ToolError(message: "paths must hold file paths (non-empty strings).")
            }
            return URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        }
        let evs = try exposureValues(a, tool: "merge_to_hdr")
        do {
            let prepared = try MergeHDR.prepare(files: urls, evs: evs)
            return try jsonResult(mergedReply(consume prepared, a, evs: evs))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors Layer ▸ Merge Layers to HDR…: `MergeHDR.prepare(layers:in:)`'s
    /// eligibility rules, then the same job. The source document is not
    /// edited: its history cursor and its pixels do not move. `layers` keeps
    /// the caller's order, which is the order `evs` is read in.
    func mergeLayersToHDR(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        guard let raw = a["layers"] as? [Any], !raw.isEmpty else {
            throw ToolError(message: "layers must be a non-empty array of layer indices")
        }
        var layers: [Int] = []
        for entry in raw {
            guard NativeBrushStroke.jsonBoolean(entry) == nil,
                  let number = Self.finiteNumber(entry), number == number.rounded(),
                  abs(number) < 1e9 else {
                throw ToolError(message: "layers must hold layer indices (integers)")
            }
            let index = Int(number)
            guard index >= 0, index < source.layerCount else {
                throw ToolError(
                    message: "Layer \(index) is out of range (0..\(source.layerCount - 1))")
            }
            guard !layers.contains(index) else {
                throw ToolError(message: "Layer \(index) is listed twice in layers")
            }
            layers.append(index)
        }
        let evs = try exposureValues(a, tool: "merge_layers_to_hdr")
        do {
            let prepared = try MergeHDR.prepare(layers: layers, in: source, evs: evs)
            var reply = try mergedReply(consume prepared, a, evs: evs)
            reply["source_document"] = summary(document)
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
