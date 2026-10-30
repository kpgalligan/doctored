import AppKit

/// The agent mirror of Edit ▸ Generative Fill…, Edit ▸ Generative Expand… and Layer ▸ Generative
/// Variations… (M3b):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `generativeFill` | Edit ▸ Generative Fill… ▸ Generate (`GenerativeFillSheetController`, `GenerativeFillRun.start`) |
/// | `generativeExpand` | Edit ▸ Generative Expand… ▸ Generate (`showGenerativeExpand`, `GenerativeFillRun.start` over `.transparency`) |
/// | `generativeVariations` | Layer ▸ Generative Variations…'s "Variation k of N" (`GenerativeVariationsSheetController.refresh`) |
/// | `selectGenerativeVariation` | Layer ▸ Generative Variations… ▸ Previous / Next (`GenerativeVariationsRun.select`) |
/// | `generateVariations` | Layer ▸ Generative Variations… ▸ Generate (`GenerativeVariationsRun.startMore`) |
///
/// `generative_fill` sends the shared selection's crop to the provider with the user's own key
/// and puts the first answer on a new masked layer above `layer` as one undo step, keeping every
/// answer as its variations; `generative_expand` does the same over the area the picture leaves
/// transparent, ignoring the selection; `generate_variations` asks for a new set over the same area. Both are
/// refused before anything is sent unless the user turned on Settings ▸ Cloud's "Allow MCP agents
/// to use cloud tools", accepted the provider's terms and saved a key (`GenerativeFillRun.gate`,
/// agent order); the read and the select send nothing and need no gate. None is recorded into an
/// Action: replaying a fill would bill the user's key again.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let generativeFillHandlers: [String: ToolHandler] = [
        "generative_fill": { $0.generativeFill },
        "generative_expand": { $0.generativeExpand },
        "generative_variations": { $0.generativeVariations },
        "select_generative_variation": { $0.selectGenerativeVariation },
        "generate_variations": { $0.generateVariations },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. 8 and 16 bits and 8-bit
    /// CMYK (32 bits and 16-bit CMYK refuse by name); not recordable.
    static let generativeFillToolFacts: [String: ToolFacts] = [
        "generative_fill": [.nonRecordable, .typedTool, .cmykTool],
        "generative_expand": [.nonRecordable, .typedTool, .cmykTool],
        "generative_variations": [.nonRecordable, .typedTool, .cmykTool],
        "select_generative_variation": [.nonRecordable, .typedTool, .cmykTool],
        "generate_variations": [.nonRecordable, .typedTool, .cmykTool],
    ]

    static let variationsPlaneRefusal = "Generative Variations re-render a Generative Layer; target "
        + "the layer, not a mask, colour plane or channel."

    /// Refuses a key outside `allowed`, in `tool`'s name.
    private func refuseUnknown(_ a: [String: Any], _ allowed: Set<String>, _ tool: String) throws {
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown \(tool) parameter: \(unexpected).")
        }
    }

    /// The document's own refusal: no image, Quick Mask, a non-layer plane, the depth rule.
    func generativeDocument(_ document: ImageDocument, _ planeRefusal: String,
                                    feature: GenerativeFeature = .fill) throws -> RasterDocument {
        guard !document.quickMaskActive else { throw ToolError(message: OperationScope.quickMaskRefusal) }
        guard document.planeEditTarget == .layer else { throw ToolError(message: planeRefusal) }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if let reason = GenerativeFillRun.refusal(doc, feature: feature) { throw ToolError(message: reason) }
        return doc
    }

    /// The prompt, provider and variations arguments over `settings`.
    func generativeSettings(_ a: [String: Any], _ settings: GenerativeFillSettings) throws
        -> GenerativeFillSettings {
        var settings = settings
        if let value = a["prompt"] {
            guard let prompt = value as? String else { throw ToolError(message: "prompt must be a string.") }
            guard prompt.count <= GenerativeFillSettings.maximumPromptLength else {
                throw ToolError(message: "prompt must be at most \(GenerativeFillSettings.maximumPromptLength) characters.")
            }
            settings.prompt = prompt
        }
        if let value = a["provider"] {
            guard let name = value as? String, let id = CloudProviderID(rawValue: name) else {
                let names = CloudProviderID.allCases.map(\.rawValue).joined(separator: ", ")
                throw ToolError(message: "provider must be one of: \(names).")
            }
            settings.provider = id
        }
        if a["variations"] != nil {
            let range = GenerativeFillSettings.variationRange
            guard let count = intArg(a, "variations"), range.contains(count) else {
                throw ToolError(message: "variations must be \(range.lowerBound) to \(range.upperBound).")
            }
            settings.variations = count
        }
        return settings
    }

    /// generative_fill — mirrors Edit ▸ Generative Fill… and its Generate.
    func generativeFill(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "layer", "prompt", "provider", "variations"], "generative_fill")
        try refuseNullOrBooleanNumbers(a, ["layer", "variations"], in: "generative_fill")
        let index = try structuralLayerIndex(a, document)
        _ = try generativeDocument(
            document, "Generative Fill writes a new layer above a layer; target the layer, not a "
                + "mask, colour plane or channel.")
        let settings = try generativeSettings(a, GenerativeFillSettings())
        guard let selection = editor(document)?.agentSelection else {
            throw ToolError(
                message: "generative_fill needs a selection: make one first with select_rect, "
                    + "select_ellipse, select_polygon, select_magic_wand or select_subject.")
        }
        let job: DocumentJob
        do {
            try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: true)
            job = try GenerativeFillRun.start(
                settings, coverage: .selection(selection.maskSamples16()), document: document,
                layer: index, showProgress: false)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try finishJobCall(job, tool: "generative_fill", arguments: a)
    }

    /// generative_expand — mirrors Edit ▸ Generative Expand… and its Generate. The shared
    /// selection is neither required nor changed.
    func generativeExpand(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "layer", "prompt", "provider", "variations"],
                          "generative_expand")
        try refuseNullOrBooleanNumbers(a, ["layer", "variations"], in: "generative_expand")
        let index = try structuralLayerIndex(a, document)
        _ = try generativeDocument(
            document, "Generative Expand writes a new layer above a layer; target the layer, not a "
                + "mask, colour plane or channel.", feature: .expand)
        let settings = try generativeSettings(a, GenerativeFillSettings())
        let job: DocumentJob
        do {
            try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: true)
            job = try GenerativeFillRun.start(settings, coverage: .transparency, document: document,
                                              layer: index, showProgress: false)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try finishJobCall(job, tool: "generative_expand", arguments: a)
    }

    /// The live variations of `layer`, else the refusal by name.
    private func liveVariations(_ doc: RasterDocument, _ index: Int) throws -> GenerativeVariationsInfo {
        do {
            guard let info = try doc.genFillVariations(layer: index) else {
                throw ToolError(message: "generative layer \(index) holds no variations")
            }
            return info
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// generative_variations — mirrors Layer ▸ Generative Variations…'s "Variation k of N".
    func generativeVariations(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "layer"], "generative_variations")
        try refuseNullOrBooleanNumbers(a, ["layer"], in: "generative_variations")
        let index = try structuralLayerIndex(a, document)
        let doc = try generativeDocument(document, Self.variationsPlaneRefusal)
        let info = try liveVariations(doc, index)
        return try jsonResult([
            "ok": true, "layer": index, "count": info.count, "selected": info.selected,
            "native_size": [info.native.width, info.native.height], "crop": info.crop,
            "prompt": GenerativeVariationsRun.prompt(doc, layer: index),
        ])
    }

    /// select_generative_variation — mirrors Layer ▸ Generative Variations… ▸ Previous / Next.
    func selectGenerativeVariation(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "layer", "index"], "select_generative_variation")
        try refuseNullOrBooleanNumbers(a, ["layer", "index"], in: "select_generative_variation")
        // `index` is the variation here, never the layer (`structuralLayerIndex` reads either).
        var layerArguments = a
        layerArguments.removeValue(forKey: "index")
        let layer = try structuralLayerIndex(layerArguments, document)
        let doc = try generativeDocument(document, Self.variationsPlaneRefusal)
        let info = try liveVariations(doc, layer)
        guard let index = intArg(a, "index") else {
            throw ToolError(message: "select_generative_variation needs index.")
        }
        guard (0..<info.count).contains(index) else {
            throw ToolError(
                message: "generative variation \(index) is out of range (the layer holds \(info.count))")
        }
        if index == info.selected {
            return try jsonResult(["ok": true, "changed": false, "layer": layer,
                                   "selected": info.selected, "count": info.count])
        }
        do {
            let job = try GenerativeVariationsRun.select(document: document, layer: layer, index: index,
                                                         showProgress: false)
            job.agentOrigin = true
            try DocumentJobs.wait(job)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let after = GenerativeVariationsRun.info(document.doc, layer: layer)
        return try jsonResult(["ok": true, "changed": true, "layer": layer,
                               "selected": after?.selected ?? index, "count": after?.count ?? info.count])
    }

    /// generate_variations — mirrors Layer ▸ Generative Variations… ▸ Generate.
    func generateVariations(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try refuseUnknown(a, ["document_id", "layer", "prompt", "provider", "variations"],
                          "generate_variations")
        try refuseNullOrBooleanNumbers(a, ["layer", "variations"], in: "generate_variations")
        let index = try structuralLayerIndex(a, document)
        let doc = try generativeDocument(document, Self.variationsPlaneRefusal)
        _ = try liveVariations(doc, index)
        var defaults = GenerativeFillSettings()
        defaults.prompt = GenerativeVariationsRun.prompt(doc, layer: index)
        if let provider = GenerativeVariationsRun.provider(doc, layer: index) { defaults.provider = provider }
        let settings = try generativeSettings(a, defaults)
        let job: DocumentJob
        do {
            try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: true)
            job = try GenerativeVariationsRun.startMore(settings, document: document, layer: index,
                                                        showProgress: false)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try finishJobCall(job, tool: "generate_variations", arguments: a)
    }
}
