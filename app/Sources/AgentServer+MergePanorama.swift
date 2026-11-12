import AppKit

/// Merge Panorama for the agent:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `merge_panorama` | File ▸ Automate ▸ Merge Panorama… (`AppDelegate.mergePanorama`, AppDelegate+MergePanorama.swift) |
///
/// `merge_panorama` CREATES a document and resolves none, so it is
/// `documentFree`; unlike `load_files_into_stack` it is recordable, as
/// `duplicate_to_new_document` is: the step replays as a new panorama from
/// the same files, and edits recorded on the document it made become ONE
/// placeholder (`ActionRecorder.noteCreatedDocument`). It keeps the caller's
/// `paths` order, where the dialog keeps its list's.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let mergePanoramaHandlers: [String: ToolHandler] = [
        "merge_panorama": { $0.mergePanorama },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let mergePanoramaToolFacts: [String: ToolFacts] = [
        // Creates a document and resolves none; recordable.
        "merge_panorama": [.documentFree],
    ]

    /// Mirrors File ▸ Automate ▸ Merge Panorama…: `MergePanorama.run` (the stack,
    /// planned and admitted before any file is decoded, then the one job),
    /// `openMergePanorama`, and the document noted for an Action being
    /// recorded. A refusal names its stage and opens nothing.
    func mergePanorama(_ a: [String: Any]) throws -> String {
        guard BatchRun.active == nil else {
            throw ToolError(
                message: "Merge Panorama cannot run inside a Batch: each file would leave a new "
                    + "document open.")
        }
        guard let raw = a["paths"] as? [Any] else {
            throw ToolError(
                message: "paths must be an array of at least \(MergePanorama.minimumFiles) file "
                    + "paths.")
        }
        let urls = try raw.map { entry -> URL in
            guard let path = entry as? String, !path.isEmpty else {
                throw ToolError(message: "paths must hold file paths (non-empty strings).")
            }
            return URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        }
        var options = MergePanorama.Options()
        if let given = a["layout"] {
            let accepted = MergePanorama.layouts.map(\.projectionName).joined(separator: ", ")
            guard let name = given as? String,
                  let layout = AutoAlign.Registration(projectionName: name) else {
                throw ToolError(message: "layout must be one of \(accepted).")
            }
            options.layout = layout
        }
        options.blend = try mergePanoramaSwitch(a, "blend", default: true)
        options.vignette = try mergePanoramaSwitch(a, "vignette", default: false)
        options.distortion = try mergePanoramaSwitch(a, "distortion", default: false)
        options.fill = try mergePanoramaSwitch(a, "fill", default: false)
        let outcome: MergePanorama.Outcome
        let document: ImageDocument
        do {
            outcome = try MergePanorama.run(urls, options: options)
            document = try ImageDocument.openMergePanorama(outcome, name: stringArg(a, "name"))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        ActionRecorder.shared.noteCreatedDocument(document)
        let made = outcome.document
        var reply: [String: Any] = [
            "ok": true,
            "document": summary(document),
            "depth": made.precisionName,
            "profile": outcome.load.profileName,
            "layout": options.layout.projectionName,
            "canvas": ["width": made.width, "height": made.height],
            "layers": stackLayerRows(outcome.load),
            "align": autoAlignReplyFields(outcome.aligned),
            "working_memory_plan_bytes": outcome.planBytes,
        ]
        if let blend = outcome.blend {
            reply["blend"] = [
                "layers": blend.layers.map { layer -> [String: Any] in
                    var row: [String: Any] = [
                        "layer": layer.layer, "name": layer.name, "share": layer.share,
                    ]
                    if let gain = layer.gain { row["gain"] = gain }
                    return row
                },
            ] as [String: Any]
            if let filled = blend.fill {
                reply["fill"] = [
                    "layer": filled.layer.map { $0 as Any } ?? NSNull(),
                    "covered_pixels": filled.coveredPixels,
                ] as [String: Any]
            }
        }
        if let notice = outcome.notice { reply["note"] = notice }
        return try jsonResult(reply)
    }

    /// A boolean argument that may be absent: `value` when it is. Anything
    /// that is not a boolean (or its string spelling) is refused by name.
    private func mergePanoramaSwitch(_ a: [String: Any], _ key: String, default value: Bool) throws
        -> Bool
    {
        guard a[key] != nil else { return value }
        guard let given = boolArg(a, key) else {
            throw ToolError(message: "\(key) must be true or false.")
        }
        return given
    }
}
