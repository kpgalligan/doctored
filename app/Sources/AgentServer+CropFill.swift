import AppKit

/// `crop`'s `fill`: the agent mirror of the Crop tool's Fill popup on a box past
/// the canvas — the Crop tool's Return with Fill set to Generative Expand or Content-Aware Fill
/// (`EditorViewController.commitCropFill`, CropFill.swift's shared run). No tool of its own and
/// no table entry: `crop` routes an extended window with a fill other than `background` here,
/// after its own argument and size refusals and before any edit.
///
/// The reply is the job's report (`extended`, `fill`, `planned_bytes`), always waited for: `crop`
/// is not a document-job tool, so a `background: true` call would record twice. The execute seam
/// records the `crop` call itself, so the job records nothing; a `generative_expand` crop is
/// recorded nowhere (`ActionCatalogFacts.billsCloud`), as `generative_expand` is not.
extension AgentServer {
    func cropFill(_ a: [String: Any], document: ImageDocument, request: CropFillRequest,
                  fill: CropFill) throws -> String {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let started: (job: DocumentJob, plannedBytes: UInt64)
        switch fill {
        case .background:
            throw ToolError(message: CropFillRun.insideWindow)
        case .contentAware:
            if a["delete_cropped"] != nil, boolArg(a, "delete_cropped") == false {
                throw ToolError(message: CropFillRun.contentAwareKeepsPixels)
            }
            guard doc.hasQualifyingBackground() else {
                throw ToolError(message: CropFillRun.contentAwareScope)
            }
            if let reason = DocumentPrecision.refusal(
                tool: "content_aware_fill", arguments: ContentAwareFillSettings().arguments(layer: 0),
                document: document) {
                throw ToolError(message: reason)
            }
            started = try toolErrors {
                try CropFillRun.startContentAware(request, document: document, record: .notACommand,
                                                  showProgress: false)
            }
        case .generativeExpand:
            guard !ActionPlayer.isPlaying else { throw ToolError(message: CropFillRun.notReplayed) }
            _ = try generativeDocument(
                document, "Generative Expand writes a new layer above a layer; target the layer, "
                    + "not a mask, colour plane or channel.", feature: .expand)
            let settings = try generativeSettings(a, GenerativeFillSettings())
            started = try toolErrors {
                try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: true)
                return try CropFillRun.startGenerativeExpand(settings, request, document: document,
                                                             showProgress: false)
            }
        }
        started.job.details["extended"] = true
        started.job.details["fill"] = fill.rawValue
        started.job.details["planned_bytes"] = started.plannedBytes
        var arguments = a
        arguments.removeValue(forKey: "background")
        return try finishJobCall(started.job, tool: "crop", arguments: arguments)
    }

    /// `body`'s error as a `ToolError`, kept as is when it already is one.
    private func toolErrors<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }
}
