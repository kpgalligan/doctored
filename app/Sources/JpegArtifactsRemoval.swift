import AppKit

// Filter ▸ Neural Filters ▸ JPEG Artifacts Removal… (M4): FBCNN over the core's 512² tiles
// on the active layer's own pixels at scale 1, Strength Low / Medium / High as FBCNN's quality
// factor 70 / 40 / 10, output to the layer itself or to a new layer "<name> JPEG Artifacts
// Removal" directly above it at the same offset. One
// `DocumentJobs` job (Remove's shape), one undo step; the byte result publishes through
// `withLayerPixels` / `addingImageLayer(above:…offset:)`, the 16-bit one through
// `dr_precision_doc_neural_layer_apply`. The selection is ignored: the filter outputs the whole
// layer. On a Lab Color document (#1 Lab phase 6b-iii) FBCNN reads the layer's
// Display P3 view (`NeuralFilterRun.modelInput`) and the core returns its result to Lab codes, the
// plan carrying both images (`NeuralFilterRun.labTerm`).

/// The Strength popup.
enum JpegArtifactsStrength: String, CaseIterable {
    case low, medium, high

    /// FBCNN's quality factor: the lower, the more it removes (the model takes 1 − QF/100).
    var qualityFactor: Int {
        switch self {
        case .low: return 70
        case .medium: return 40
        case .high: return 10
        }
    }

    var title: String { rawValue.capitalized }
}

/// The dialog's controls.
struct JpegArtifactsSettings: Equatable {
    var strength = JpegArtifactsStrength.medium
    var output = DepthBlurOutput.layer

    var arguments: [String: Any] { ["strength": strength.rawValue, "output": output.rawValue] }

    /// The MCP / Action arguments; absent keys keep their defaults.
    static func parse(_ a: [String: Any]) throws -> JpegArtifactsSettings {
        var settings = JpegArtifactsSettings()
        if let value = a["strength"] {
            guard let text = value as? String, let strength = JpegArtifactsStrength(rawValue: text) else {
                throw DoctoredCoreError(message: "strength must be low, medium or high.")
            }
            settings.strength = strength
        }
        if let value = a["output"] {
            guard let text = value as? String, let output = DepthBlurOutput(rawValue: text) else {
                throw DoctoredCoreError(message: "output must be layer or new_layer.")
            }
            settings.output = output
        }
        return settings
    }
}

enum JpegArtifactsRemovalRun {
    static let name = "JPEG Artifacts Removal"
    static let menu = "Filter ▸ Neural Filters ▸ JPEG Artifacts Removal…"
    static let capability = "jpeg_artifacts"

    /// The new layer's name for layer `layer`.
    static func layerName(_ doc: RasterDocument, layer: Int) -> String {
        (doc.layerInfo(layer)?.name ?? "Layer") + " JPEG Artifacts Removal"
    }

    /// The stage's figures over `layer`'s own pixels and the admitted plan: the core's plan +
    /// FBCNN's `inferenceBytes` + `hostBytesPerOutputPixel(bytesPerSample:)` × the layer's pixels
    /// (+ the Lab view and result on a Lab document).
    static func plan(_ doc: RasterDocument, layer: Int, entry: ModelCatalogEntry) throws
        -> (info: NeuralStageInfo, planned: UInt64) {
        guard let placement = doc.layerPlacement(layer), placement.width > 0, placement.height > 0 else {
            throw DoctoredCoreError(message: "jpeg artifacts removal needs visible pixels; layer \(layer) is empty")
        }
        let bytesPerSample = doc.isHighDepth ? 2 : 1
        let info = try NeuralFilterRun.info(NeuralFilterRun.jpegStage(), width: placement.width,
                                            height: placement.height, bytesPerSample: bytesPerSample)
        let pixels = UInt64(placement.width) * UInt64(placement.height)
        let host = NeuralFilterRun.hostBytesPerOutputPixel(bytesPerSample: bytesPerSample)
        let size = (placement.width, placement.height)
        let lab = NeuralFilterRun.labTerm(doc, input: size, output: size)
        return (info, NeuralFilterRun.plus(info.plannedBytes + entry.inferenceBytes + host * pixels, lab))
    }

    /// Starts the job on `layer` (main thread): the depth refusal, the model, a retained source
    /// refused (`RetainedImagePolicy.rasterizeFirst`), the plan admitted by `DocumentJobs.start`,
    /// and a compute that runs the stage and the apply. The job's details carry
    /// `planned_bytes`, `tiles`, `quality_factor` and `model`.
    static func start(document: ImageDocument, layer: Int, settings: JpegArtifactsSettings,
                      record: [ActionStep], showProgress: Bool) throws -> DocumentJob {
        guard let source = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        if let reason = NeuralFilterRun.refusal(source, name: name) { throw DoctoredCoreError(message: reason) }
        guard let model = ModelGate.ready(capability) else {
            throw DoctoredCoreError(message: ModelGate.notInstalled(capability))
        }
        guard source.isValidIndex(layer) else {
            throw DoctoredCoreError(message: "jpeg artifacts removal layer \(layer) is out of range")
        }
        if source.hasRetainedSource(layer) { throw DoctoredCoreError(message: RetainedImagePolicy.rasterizeFirst) }
        let figures = try plan(source, layer: layer, entry: model.entry)
        let budget = try NeuralFilterRun.budget(source)
        let newLayer = settings.output == .newLayer
        let name = layerName(source, layer: layer)
        let compiled = model.compiled
        let quality = settings.strength.qualityFactor
        let placement = source.layerPlacement(layer)
        let job = try DocumentJobs.start(
            document: document, layer: layer, title: Self.name, scratch: figures.planned, record: record,
            showProgress: showProgress, rasterizesTarget: !newLayer,
            compute: { source, control in
                try control.beginCompoundOperation()
                let tiles = try NeuralTileModel(compiled: compiled, side: NeuralFilterRun.jpegTile,
                                                outputSide: NeuralFilterRun.jpegTile, qualityFactor: quality)
                let stage = NeuralFilterRun.jpegStage()
                if let precision = source.precision {
                    guard let pixels = precision.layerSource(layer) else {
                        throw DoctoredCoreError(message: "jpeg artifacts removal needs a raster layer; layer \(layer) is not one")
                    }
                    // A Lab layer's model reads its Display P3 view (#1 Lab phase 6b-iii).
                    let lab = source.isLab
                    var view = pixels
                    if lab {
                        view = try control.withChildOperation(from: 0, to: 0.05) { child in
                            try NeuralFilterRun.modelInput(pixels, document: source, limit: budget.limit,
                                                           other: budget.other, control: child)
                        }
                    }
                    let input = view
                    let out = try control.withChildOperation(from: lab ? 0.05 : 0, to: 0.9) { child in
                        try NeuralFilterRun.run(stage, model: tiles, image: .wide(input), limit: budget.limit,
                                                other: budget.other, control: child)
                    }
                    guard case .wide(let restored) = out, restored !== input else { return nil }
                    let applied = try control.withChildOperation(from: 0.9, to: 1) { child in
                        try precision.neuralLayerApply(layer, pixels: restored, newLayer: newLayer, name: name,
                                                       limit: budget.limit, other: budget.other, control: child)
                    }
                    return try applied.map { try RasterDocument(precision: $0) }
                }
                guard let pixels = source.layerImage(layer) else {
                    throw DoctoredCoreError(message: "jpeg artifacts removal needs a raster layer; layer \(layer) is not one")
                }
                let out = try control.withChildOperation(from: 0, to: 0.95) { child in
                    try NeuralFilterRun.run(stage, model: tiles, image: .byte(pixels), limit: budget.limit,
                                            other: budget.other, control: child)
                }
                guard case .byte(let restored) = out, restored !== pixels else { return nil }
                guard newLayer else { return source.withLayerPixels(layer, restored) }
                return source.addingImageLayer(above: layer, restored, name: name,
                                               offset: (x: placement?.x ?? 0, y: placement?.y ?? 0))
            })
        job.details["planned_bytes"] = figures.planned
        job.details["tiles"] = figures.info.tiles
        job.details["quality_factor"] = quality
        job.details["model"] = "\(model.entry.id) @ \(model.entry.version)"
        return job
    }
}
