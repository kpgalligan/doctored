import Foundation

// Generative Layer variations (M3b phase 2): a Generative Layer keeps every picture its
// fill asked for, persisted in the `.rz` (word 92, CMYK too). The
// user switches between them (`select`) and asks the provider for a new set (`startMore`) in
// Layer ▸ Generative Variations… (GenerativeVariationsSheet.swift) and over MCP
// (AgentServer+GenerativeFill.swift); the core's ops are DoctoredCore+GenVariations.swift. Each is
// one undo step, never recorded into an Action (replaying "more" would bill the key again).

/// A layer's live variations, as the core reports them.
struct GenerativeVariationsInfo {
    let count: Int
    let selected: Int
    let native: (width: Int, height: Int)
    /// The canvas area sent, `[x, y, w, h]`.
    let crop: [Int]
}

enum GenerativeVariationsRun {
    static let menuPath = "Layer ▸ Generative Variations"

    /// Layer `layer`'s live variations on `doc`, nil when it holds none.
    static func info(_ doc: RasterDocument?, layer: Int) -> GenerativeVariationsInfo? {
        guard let doc, doc.isValidIndex(layer) else { return nil }
        return try? doc.genFillVariations(layer: layer)
    }

    /// The prompt layer `layer`'s tag names (the selected variation's), empty when none.
    static func prompt(_ doc: RasterDocument?, layer: Int) -> String {
        guard let doc, doc.isValidIndex(layer), let meta = doc.layerMeta(layer),
              let data = meta.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return ""
        }
        return object["prompt"] as? String ?? ""
    }

    /// The provider layer `layer`'s tag names, nil when it names none this build knows.
    static func provider(_ doc: RasterDocument?, layer: Int) -> CloudProviderID? {
        guard let doc, doc.isValidIndex(layer), let meta = doc.layerMeta(layer),
              let data = meta.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let raw = object["provider"] as? String else {
            return nil
        }
        return CloudProviderID(rawValue: raw)
    }

    /// The feature layer `layer`'s tag names (Generative Fill or Expand), `.fill` when it names
    /// none this build knows, so a new set keeps the layer's own `feature`.
    static func feature(_ doc: RasterDocument?, layer: Int) -> GenerativeFeature {
        guard let doc, doc.isValidIndex(layer), let meta = doc.layerMeta(layer),
              let data = meta.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let raw = object["feature"] as? String,
              let feature = GenerativeFeature(rawValue: raw) else {
            return .fill
        }
        return feature
    }

    /// Adds `variations` and `selected_variation` (layer `layer`'s, read on main after the
    /// edit landed) to a finished job's details.
    static func describe(_ job: DocumentJob, document: ImageDocument?, layer: Int) {
        if let info = info(document?.doc, layer: layer) {
            job.details["variations"] = info.count
            job.details["selected_variation"] = info.selected
        }
    }

    /// Starts the job that re-renders layer `layer` from variation `index`: the core's plan for
    /// one stored picture plus `GenerativeFillRun.hostPlan`, admitted by `DocumentJobs.start`.
    /// No cloud request and no gate.
    static func select(document: ImageDocument, layer: Int, index: Int,
                       showProgress: Bool) throws -> DocumentJob {
        guard let source = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        if let reason = GenerativeFillRun.refusal(source) { throw DoctoredCoreError(message: reason) }
        guard let info = try source.genFillVariations(layer: layer) else {
            throw DoctoredCoreError(message: "generative layer \(layer) holds no variations")
        }
        guard (0..<info.count).contains(index) else {
            throw DoctoredCoreError(
                message: "generative variation \(index) is out of range (the layer holds \(info.count))")
        }
        let planned = try source.genFillPlan(native: info.native, variations: 1)
            + GenerativeFillRun.hostPlan(source, native: info.native, count: 1)
        let budget = try GenerativeFillRun.budget(source)
        let job = try DocumentJobs.start(
            document: document, layer: layer, title: "Generative Variation", scratch: planned,
            record: .unrecorded("Generative Variation"), showProgress: showProgress,
            rasterizesTarget: false,
            compute: { source, control in
                try source.genFillSelectVariation(layer: layer, index: index, limit: budget.limit,
                                                  other: budget.other, control: control)
            })
        job.details["planned_bytes"] = planned
        job.completion = { [weak document] finished in
            finished.details["changed"] = finished.state == "completed"
            finished.details["output_layer"] = layer
            describe(finished, document: document, layer: layer)
        }
        return job
    }

    /// Starts the job that asks `settings.provider` for a new set of layer `layer`'s variations:
    /// the core's more-input over the stored crop and native size (0–0.2), the shared
    /// `GenerativeFillRun.request` leg, then the core's add (0.8–1), which selects the first new
    /// picture. The caller has passed `GenerativeFillRun.gate`. Its details are Generative
    /// Fill's.
    static func startMore(_ settings: GenerativeFillSettings, document: ImageDocument, layer: Int,
                          showProgress: Bool) throws -> DocumentJob {
        let provider = CloudProviders.provider(settings.provider)
        guard let source = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        if let reason = GenerativeFillRun.refusal(source) { throw DoctoredCoreError(message: reason) }
        guard let info = try source.genFillVariations(layer: layer) else {
            throw DoctoredCoreError(message: "generative layer \(layer) holds no variations")
        }
        guard let key = CloudStore.key(provider.id) else {
            throw DoctoredCoreError(message: GenerativeFillRun.noKey(provider))
        }
        let count = min(max(settings.variations, GenerativeFillSettings.variationRange.lowerBound),
                        GenerativeFillSettings.variationRange.upperBound)
        let native = info.native
        let planned = try source.genFillPlan(native: native, variations: count)
            + GenerativeFillRun.hostPlan(source, native: native, count: count)
        let budget = try GenerativeFillRun.budget(source)
        let sent = settings.sentPrompt
        let tag = GeneratedLayerTag.make(provider: provider, prompt: settings.trimmedPrompt,
                                         feature: feature(source, layer: layer))
        let box = GenerativeFillBox()
        let job = try DocumentJobs.start(
            document: document, layer: layer, title: "Generative Variations", scratch: planned,
            record: .unrecorded("Generative Variations"), showProgress: showProgress,
            rasterizesTarget: false,
            compute: { source, control in
                try control.beginCompoundOperation()
                let input = try control.withChildOperation(from: 0, to: 0.2) { child in
                    try source.genFillMoreInput(layer: layer, native: native, limit: budget.limit,
                                                other: budget.other, control: child)
                }
                guard !control.isCancelled else { throw JobCancelled() }
                let answer = try GenerativeFillRun.request(
                    provider: provider, key: key, input: input, native: native, prompt: sent,
                    count: count, cancelled: { control.isCancelled })
                box.received = answer.pictures.count
                box.shortReason = answer.shortReason
                return try control.withChildOperation(from: 0.8, to: 1) { child in
                    try source.genFillAddVariations(layer: layer, results: answer.pictures, tag: tag,
                                                    limit: budget.limit, other: budget.other,
                                                    control: child)
                }
            })
        job.details["planned_bytes"] = planned
        job.details["provider"] = provider.id.rawValue
        job.details["model"] = provider.model
        job.details["native_size"] = [native.width, native.height]
        job.details["prompt"] = sent
        job.details["crop"] = info.crop
        job.completion = { [weak document] finished in
            let completed = finished.state == "completed"
            finished.details["changed"] = completed
            finished.details["output_layer"] = completed ? layer : NSNull()
            GenerativeFillRun.describeCount(finished, requested: count, box: box)
            if completed { describe(finished, document: document, layer: layer) }
        }
        return job
    }
}
