import Foundation

// Generative Fill (M3b): the selection's crop is sent to a cloud provider's image model
// with the user's own key (`CloudProviders.swift`: OpenAI, Gemini, fal), and the core composites
// the first answer onto a NEW layer above the active one, masked by the selection and tagged as
// generated, keeping every answer as the layer's variations (`DoctoredCore+GenFill.swift`,
// `gen_fill.rs`), at 8 and 16 bits and on 8-bit CMYK, as one undo step. The request runs only
// inside the job's worker, between the core's input and apply (`request`, which the variations'
// "generate more" job shares, GenerativeVariations.swift). The sheet and the menu are
// GenerativeFillSheet.swift; the MCP twin is AgentServer+GenerativeFill.swift. Generative Expand
// is the same run over `GenerativeCoverage.transparency`: the core's coverage of
// what the composite leaves transparent (what Canvas Size adds) instead of the selection's.

/// Which generative edit a run is: its title, its menu path and its tag's `feature`.
enum GenerativeFeature: String {
    case fill = "generative_fill"
    case expand = "generative_expand"

    var title: String {
        switch self {
        case .fill: return "Generative Fill"
        case .expand: return "Generative Expand"
        }
    }

    var menuPath: String { "Edit \u{25B8} \(title)" }
}

/// What a run fills: the selection's canvas-sized u16 coverage (Generative Fill), every pixel
/// the composite leaves not fully opaque, read by the core in the job (Generative Expand), or a
/// given canvas-sized u16 area to expand into, framed on main like a selection (a crop past the
/// canvas with Generative Expand: its new area, `CropFillRun.newArea`).
enum GenerativeCoverage {
    case selection([UInt16])
    case transparency
    case area([UInt16])

    var feature: GenerativeFeature {
        switch self {
        case .selection: return .fill
        case .transparency, .area: return .expand
        }
    }
}

/// What one Generative Fill asks for.
struct GenerativeFillSettings {
    /// The prompt as typed; empty sends the provider's context prompt.
    var prompt = ""
    var provider = CloudProviderID.openai
    /// How many variations one run asks for (three).
    var variations = 3

    static let maximumPromptLength = 1000
    static let variationRange = 1...3

    var trimmedPrompt: String { prompt.trimmingCharacters(in: .whitespacesAndNewlines) }
    /// The prompt sent: the trimmed prompt, else the context prompt.
    var sentPrompt: String { trimmedPrompt.isEmpty ? OpenAIImageProvider.contextPrompt : trimmedPrompt }
}

/// One provider answer, decoded: straight RGBA8, row 0 = top.
struct GenerativePicture {
    let rgba: [UInt8]
    let width: Int
    let height: Int

    /// The core's results layout: the pictures back to back and their `(w, h)` pairs.
    static func flattened(_ pictures: [GenerativePicture]) -> ([UInt8], [UInt32]) {
        var bytes: [UInt8] = []
        bytes.reserveCapacity(pictures.reduce(0) { $0 + $1.rgba.count })
        var sizes: [UInt32] = []
        for picture in pictures {
            bytes.append(contentsOf: picture.rgba)
            sizes.append(contentsOf: [UInt32(picture.width), UInt32(picture.height)])
        }
        return (bytes, sizes)
    }
}

/// The generated-layer tag: the artwork description a Generative Layer carries — the byte
/// `meta` at 8 bits, the typed description at 16 — naming where its pixels came from. It never
/// re-renders (`LayerDescription.claimedKind` is nil for it, so no Doctored prompt appears), and
/// the core's Assign check ignores it (`artwork_document::rerenders`).
enum GeneratedLayerTag {
    static let type = "generated"

    static func make(provider: GenerativeFillProvider, prompt: String,
                     feature: GenerativeFeature = .fill) -> String {
        let object: [String: Any] = [
            "type": type, "version": 1, "feature": feature.rawValue,
            "provider": provider.id.rawValue, "model": provider.model, "prompt": prompt,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else {
            return "{\"type\":\"generated\",\"version\":1}"
        }
        return text
    }

    /// Whether `meta` is a generated-layer tag.
    static func isTag(_ meta: String?) -> Bool {
        guard let meta, let data = meta.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return false
        }
        return object["type"] as? String == type
    }
}

extension RasterDocument {
    /// Whether layer `index` is a Generative Layer (its description is the generated tag).
    func isGeneratedLayer(_ index: Int) -> Bool {
        guard isValidIndex(index) else { return false }
        return GeneratedLayerTag.isTag(layerMeta(index))
    }
}

/// What the request leg answers: the decoded pictures and, when they are fewer than asked, why.
struct GenerativeFillResult {
    let pictures: [GenerativePicture]
    let shortReason: String?
}

/// The worker's results, read on main by the job's completion (Generative Fill's and the
/// variations' "more" job's).
final class GenerativeFillBox {
    private let lock = NSLock()
    private var cropValue: [Int]?
    private var layerValue: Int?
    private var nativeValue: [Int]?
    private var receivedValue: Int?
    private var shortReasonValue: String?
    var crop: [Int]? {
        get { lock.lock(); defer { lock.unlock() }; return cropValue }
        set { lock.lock(); cropValue = newValue; lock.unlock() }
    }
    var layer: Int? {
        get { lock.lock(); defer { lock.unlock() }; return layerValue }
        set { lock.lock(); layerValue = newValue; lock.unlock() }
    }
    /// Expand's native size, framed in the worker once the coverage is known.
    var native: [Int]? {
        get { lock.lock(); defer { lock.unlock() }; return nativeValue }
        set { lock.lock(); nativeValue = newValue; lock.unlock() }
    }
    /// The pictures decoded this run.
    var received: Int? {
        get { lock.lock(); defer { lock.unlock() }; return receivedValue }
        set { lock.lock(); receivedValue = newValue; lock.unlock() }
    }
    /// Why `received` is below the count asked for, nil when it is not.
    var shortReason: String? {
        get { lock.lock(); defer { lock.unlock() }; return shortReasonValue }
        set { lock.lock(); shortReasonValue = newValue; lock.unlock() }
    }
}

enum GenerativeFillRun {
    static let menuPath = "Edit ▸ Generative Fill"
    /// The host's own term per canvas pixel beside the core's plan: the selection's u16 coverage
    /// the job keeps (2), its byte rasterization (1), and the published result's display
    /// preparation. Raised 2 → 9, the least whole value clearing the 8-bit 6000 × 4000 case of
    /// `scripts/generative-fill-admission/measure.py` (receipt admission-1: E 476.5 MB, need
    /// 595.6 MB); the residual scales with the canvas, not the native cells. Raised 9 → 10 for a
    /// crop past the canvas with Generative Expand (`crop-extend-admission`'s
    /// fillgen8 at 6000 × 4000, max E 698.4 MB, need 873.0 MB, P 861.8 MB at 9), whose new-area
    /// coverage — the polygon's bytes, its inverse and grown mask, the u16 copy — also scales with
    /// the canvas.
    static let hostBytesPerPixel: UInt64 = 10
    /// The host's own term per native cell AND per variation: the core's two RGBA8 pictures
    /// (8), their PNGs and the request body that copies them, the reply's JSON and base64 text,
    /// its decoded PNG, and the decoded RGBA8 answer. Measured by
    /// `scripts/generative-fill-admission/measure.py`.
    static let hostBytesPerCell: UInt64 = 40

    /// Why `feature` cannot run on `doc`, nil when it can: a 32-bit or a 16-bit CMYK document.
    /// A Lab Color document runs since #1 Lab phase 6b-iii: the provider sees the plate's
    /// Display P3 view and the core returns each picture to Lab codes by the Mode rule. A Bitmap
    /// document refuses first, for good (#1 Bitmap phase 2a-i): the Crop tool's Fill popup
    /// reaches Generative Expand there, before any provider call.
    static func refusal(_ doc: RasterDocument, feature: GenerativeFeature = .fill) -> String? {
        if doc.isBitmap { return "\(feature.title) is not available in Bitmap mode." }
        if doc.isFloat { return "\(feature.title) is not available in a 32-bit document." }
        if doc.isCmyk, CmykAdjustFilter.depth(of: doc) == 16 {
            return "\(feature.title) is not available in a 16-bit CMYK document."
        }
        return nil
    }

    /// The host plan beside the core's for `count` variations at `native` on `doc`.
    static func hostPlan(_ doc: RasterDocument, native: (width: Int, height: Int), count: Int) -> UInt64 {
        let cells = UInt64(native.width * native.height)
        let pixels = UInt64(doc.width) * UInt64(doc.height)
        return hostBytesPerPixel * pixels + hostBytesPerCell * cells * UInt64(max(1, count))
    }

    /// The working-memory context a job measures on main.
    static func budget(_ doc: RasterDocument) throws -> (limit: UInt64, other: UInt64) {
        if let precision = doc.precision {
            let context = try PrecisionWorkContext(source: precision)
            return (context.limit, context.otherRetained)
        }
        return try doc.legacyPrecisionWorkContext()
    }

    /// The request leg both jobs share (worker thread): the core's two pictures at `native` to
    /// PNG (the mask in the provider's style), the provider's request for `count` variations
    /// with `prompt`, then each answer decoded. A provider answering fewer pictures than asked
    /// keeps what came (at least one), and the result's `shortReason` says why: the provider's
    /// own sentence, else `CloudProviders.shortSentence`. Polls `cancelled` between the stages.
    static func request(provider: GenerativeFillProvider, key: String,
                        input: (image: [UInt8], mask: [UInt8]), native: (width: Int, height: Int),
                        prompt: String, count: Int,
                        cancelled: @escaping () -> Bool) throws -> GenerativeFillResult {
        let request = GenerativeFillRequest(
            // The core's image is transparent on the hole. OpenAI is sent it RGBA, like the mask:
            // its edit reads transparent image pixels as the area to generate (an opaque black
            // Expand border came back black). Gemini and fal read a white-on-black mask and are
            // sent the image opaque.
            image: try CloudImageCodec.png(rgba: input.image, width: native.width,
                                           height: native.height,
                                           opaque: provider.maskStyle == .whiteOnBlack),
            mask: try CloudImageCodec.maskPNG(rgba: input.mask, width: native.width,
                                              height: native.height, style: provider.maskStyle),
            prompt: prompt, width: native.width, height: native.height)
        let answer = try provider.fill(request, count: count, key: key, cancelled: cancelled)
        guard !cancelled() else { throw JobCancelled() }
        let requested = max(1, count)
        var pictures: [GenerativePicture] = []
        for answer in answer.pictures.prefix(requested) {
            let picture = try CloudImageCodec.decode(answer)
            pictures.append(GenerativePicture(rgba: picture.rgba, width: picture.width,
                                              height: picture.height))
            guard !cancelled() else { throw JobCancelled() }
        }
        guard !pictures.isEmpty else {
            throw CloudProviderError(message: "\(provider.displayName)'s reply holds no image.")
        }
        guard pictures.count < requested else {
            return GenerativeFillResult(pictures: pictures, shortReason: nil)
        }
        return GenerativeFillResult(
            pictures: pictures,
            shortReason: answer.shortReason
                ?? CloudProviders.shortSentence(provider: provider.displayName,
                                                received: pictures.count, requested: requested))
    }

    /// Adds `variations_requested` (`requested`, the clamped count), `variations_received` (the
    /// pictures decoded this run, 0 when the request never answered) and, when fewer came than
    /// asked, `short_reason` to a finished job's details. Main thread, in the job's completion.
    static func describeCount(_ job: DocumentJob, requested: Int, box: GenerativeFillBox) {
        let received = box.received ?? 0
        job.details["variations_requested"] = requested
        job.details["variations_received"] = received
        if received < requested, let reason = box.shortReason {
            job.details["short_reason"] = reason
        }
    }

    static let agentSwitchOff = "Cloud tools are off for MCP agents. The user can turn on "
        + "\u{201C}Allow MCP agents to use cloud tools\u{201D} in Settings \u{25B8} Cloud."

    static func noConsent(_ provider: GenerativeFillProvider) -> String {
        "\(provider.displayName)'s terms have not been accepted, so nothing is sent. The user accepts "
            + "them in Settings \u{25B8} Cloud or by running Edit \u{25B8} Generative Fill\u{2026} once."
    }

    static func noKey(_ provider: GenerativeFillProvider) -> String {
        "Add your \(provider.displayName) API key in Settings \u{25B8} Cloud."
    }

    /// The checks before anything is sent, in order: the agent switch (agent only), the consent
    /// (the UI shows the card, whose Cancel throws `JobCancelled` and sends nothing; the agent is
    /// refused), the key. Main thread.
    static func gate(_ provider: GenerativeFillProvider, agent: Bool) throws {
        if agent, !CloudStore.agentMayUseCloudTools {
            throw DoctoredCoreError(message: agentSwitchOff)
        }
        if !CloudStore.hasConsent(provider.id, termsVersion: provider.terms.version) {
            if agent { throw DoctoredCoreError(message: noConsent(provider)) }
            guard CloudConsentCard.present(provider) else { throw JobCancelled() }
            try CloudStore.recordConsent(provider.id, termsVersion: provider.terms.version)
        }
        guard CloudStore.key(provider.id) != nil else { throw DoctoredCoreError(message: noKey(provider)) }
    }

    /// The new layer's name: the trimmed prompt (its first 64 characters), else "Generative
    /// Layer N" with N the smallest positive integer no layer name uses.
    static func layerName(prompt: String, in doc: RasterDocument) -> String {
        let trimmed = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty { return String(trimmed.prefix(64)) }
        let names = Set((0..<doc.layerCount).compactMap { doc.layerInfo($0)?.name })
        var n = 1
        while names.contains("Generative Layer \(n)") { n += 1 }
        return "Generative Layer \(n)"
    }

    /// Starts the job for `coverage` anchored at `layer`: the depth refusal, the plan (the
    /// core's for `settings.variations` pictures, plus `hostPlan`) admitted by
    /// `DocumentJobs.start`, and a compute that runs the core's input (to 0.2), the shared
    /// `request` leg and the core's apply (0.8–1), polling the cancel between the stages. For a
    /// selection the core's frame is read on main and the plan is at its native size. For
    /// `.transparency` (Generative Expand) the compute first reads the core's coverage of what the
    /// composite leaves transparent (0–0.1; a picture with none refuses by name) and frames it,
    /// so the plan is at the provider's native size with the most cells: the coverage op's plate
    /// term is within the core's plan and its u16 array within `hostBytesPerPixel`. The caller
    /// has passed `gate`. The job's details carry `provider`, `model`, `native_size`,
    /// `planned_bytes` and the prompt sent; its completion adds `crop`, `changed`,
    /// `output_layer`, `variations`, `selected_variation` and `describeCount`'s keys (and
    /// Expand's `native_size`), and
    /// makes the new layer active. With a `base` (a crop past the canvas) every step reads
    /// the unpublished cropped document instead, the job is titled "Crop", and its publication
    /// is the crop and the fill as one canvas-geometry step.
    static func start(_ settings: GenerativeFillSettings, coverage: GenerativeCoverage,
                      document: ImageDocument, layer: Int, showProgress: Bool,
                      base: DocumentJobBase? = nil) throws -> DocumentJob {
        let feature = coverage.feature
        let provider = CloudProviders.provider(settings.provider)
        guard let source = base?.document ?? document.doc else {
            throw DoctoredCoreError(message: "Document has no image.")
        }
        let title = base == nil ? feature.title : "Crop"
        if let reason = refusal(source, feature: feature) { throw DoctoredCoreError(message: reason) }
        guard let key = CloudStore.key(provider.id) else { throw DoctoredCoreError(message: noKey(provider)) }
        let count = min(max(settings.variations, GenerativeFillSettings.variationRange.lowerBound),
                        GenerativeFillSettings.variationRange.upperBound)
        let framed: (width: Int, height: Int)?
        let planNative: (width: Int, height: Int)
        switch coverage {
        case .selection(let samples), .area(let samples):
            let native = try source.genFillFrame(coverage: samples, sizes: provider.nativeSizes).native
            framed = native
            planNative = native
        case .transparency:
            framed = nil
            guard let largest = provider.nativeSizes.max(by: { $0.width * $0.height < $1.width * $1.height })
            else { throw DoctoredCoreError(message: "\(provider.displayName) declares no native size.") }
            planNative = largest
        }
        let planned = try source.genFillPlan(native: planNative, variations: count)
            + hostPlan(source, native: planNative, count: count)
        let budget = try budget(source)
        let sent = settings.sentPrompt
        let name = layerName(prompt: settings.prompt, in: source)
        let tag = GeneratedLayerTag.make(provider: provider, prompt: settings.trimmedPrompt,
                                         feature: feature)
        let sizes = provider.nativeSizes
        let box = GenerativeFillBox()
        let job = try DocumentJobs.start(
            document: document, layer: layer, title: title, scratch: planned,
            record: .unrecorded(title), showProgress: showProgress,
            rasterizesTarget: false, base: base,
            compute: { source, control in
                try control.beginCompoundOperation()
                let samples: [UInt16]
                let native: (width: Int, height: Int)
                let inputFrom: Double
                switch coverage {
                case .selection(let selected), .area(let selected):
                    samples = selected
                    native = framed ?? planNative
                    inputFrom = 0
                case .transparency:
                    samples = try control.withChildOperation(from: 0, to: 0.1) { child in
                        try source.genExpandCoverage(layer: layer, limit: budget.limit,
                                                     other: budget.other, control: child)
                    }
                    guard !control.isCancelled else { throw JobCancelled() }
                    native = try source.genFillFrame(coverage: samples, sizes: sizes).native
                    box.native = [native.width, native.height]
                    inputFrom = 0.1
                }
                let input = try control.withChildOperation(from: inputFrom, to: 0.2) { child in
                    try source.genFillInput(layer: layer, coverage: samples, native: native,
                                            limit: budget.limit, other: budget.other, control: child)
                }
                box.crop = input.crop
                guard !control.isCancelled else { throw JobCancelled() }
                let answer = try request(
                    provider: provider, key: key, input: (input.image, input.mask), native: native,
                    prompt: sent, count: count, cancelled: { control.isCancelled })
                box.received = answer.pictures.count
                box.shortReason = answer.shortReason
                let applied = try control.withChildOperation(from: 0.8, to: 1) { child in
                    try source.genFillApply(
                        layer: layer, coverage: samples, native: native, results: answer.pictures,
                        name: name, tag: tag, limit: budget.limit, other: budget.other,
                        control: child)
                }
                box.layer = applied?.layer
                return applied?.document
            })
        job.details["planned_bytes"] = planned
        job.details["provider"] = provider.id.rawValue
        job.details["model"] = provider.model
        if let framed { job.details["native_size"] = [framed.width, framed.height] }
        job.details["prompt"] = sent
        job.completion = { [weak document] finished in
            if let crop = box.crop { finished.details["crop"] = crop }
            if let native = box.native { finished.details["native_size"] = native }
            let completed = finished.state == "completed"
            finished.details["changed"] = completed
            describeCount(finished, requested: count, box: box)
            if completed, let index = box.layer {
                finished.details["output_layer"] = index
                GenerativeVariationsRun.describe(finished, document: document, layer: index)
                document?.activeLayerIndex = index
            } else {
                finished.details["output_layer"] = NSNull()
            }
        }
        return job
    }
}
