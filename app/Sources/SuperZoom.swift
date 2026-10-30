import AppKit

// Filter ▸ Neural Filters ▸ Super Zoom… (M4): the active layer, on its canvas, zoomed ×2
// or ×4 into a NEW document. Enhance Image Details runs
// Real-ESRGAN x4plus over the core's tiles (a ×2 zoom is the mean of each 2×2 block of its ×4
// cells); off, the zoom is the classical Catmull-Rom resample. Remove JPEG Artifacts runs FBCNN at
// Medium on the source first. Noise Reduction and Sharpen are classical, in the core. The run is
// one admitted, cancellable `BackgroundWork` job, as Depth Blur's capture; the new document opens
// as Merge to HDR Pro's does and is noted as a duplicate, so a recording's later steps in it
// become the placeholder (`ActionRecorder.noteDuplicatedDocument`). On a Lab Color document
// (#1 Lab phase 6b-iii) the models read the layer's Display P3 view
// (`NeuralFilterRun.modelInput`) and the core returns the zoom to a Lab document by the Mode rule.

/// The dialog's controls, in the core's units.
struct SuperZoomSettings: Equatable {
    /// 2 or 4.
    var zoom = 2
    var enhanceDetails = true
    var removeJpegArtifacts = false
    /// 0…100.
    var noiseReduction = 0.0
    /// 0…100.
    var sharpen = 0.0

    static let zooms = [2, 4]

    var arguments: [String: Any] {
        ["zoom": zoom, "enhance_details": enhanceDetails, "remove_jpeg_artifacts": removeJpegArtifacts,
         "noise_reduction": ActionArgs.number(noiseReduction), "sharpen": ActionArgs.number(sharpen)]
    }

    /// The MCP / Action arguments; absent keys keep their defaults.
    static func parse(_ a: [String: Any]) throws -> SuperZoomSettings {
        var settings = SuperZoomSettings()
        if let zoom = try NeuralFilterRun.number(a, "zoom") {
            guard zooms.contains(where: { Double($0) == zoom }) else {
                throw DoctoredCoreError(message: "zoom must be 2 or 4.")
            }
            settings.zoom = Int(zoom)
        }
        if let value = try NeuralFilterRun.flag(a, "enhance_details") { settings.enhanceDetails = value }
        if let value = try NeuralFilterRun.flag(a, "remove_jpeg_artifacts") { settings.removeJpegArtifacts = value }
        for key in ["noise_reduction", "sharpen"] {
            guard let value = try NeuralFilterRun.number(a, key) else { continue }
            guard (0...100).contains(value) else { throw DoctoredCoreError(message: "\(key) must be in 0…100.") }
            if key == "sharpen" { settings.sharpen = value } else { settings.noiseReduction = value }
        }
        return settings
    }
}

/// One Super Zoom's stages, their figures and the admitted plan.
struct SuperZoomPlan {
    let stages: [(stage: [String: Any], info: NeuralStageInfo)]
    let plannedBytes: UInt64
    let width: Int
    let height: Int
    var tiles: Int { stages.reduce(0) { $0 + $1.info.tiles } }
}

/// A finished Super Zoom: the new document and its figures.
struct SuperZoomResult {
    let document: ImageDocument
    let plan: SuperZoomPlan
    let models: [ModelCatalogEntry]
}

enum SuperZoomRun {
    static let name = "Super Zoom"
    static let menu = "Filter ▸ Neural Filters ▸ Super Zoom…"
    static let capability = "super_resolution"

    /// The stages for `settings` over a `width × height` canvas at `bytesPerSample`: FBCNN at
    /// Medium first when asked, then Real-ESRGAN; refuses a zoom past the 100-megapixel limit by
    /// its size. P = each stage's core plan + its model's `inferenceBytes` (the FBCNN output held
    /// under the second) + the canvas source + `hostBytesPerOutputPixel(bytesPerSample:)` × the output.
    static func plan(width: Int, height: Int, bytesPerSample: Int, settings: SuperZoomSettings,
                     superResolution: ModelCatalogEntry, jpeg: ModelCatalogEntry?) throws -> SuperZoomPlan {
        var stages: [(stage: [String: Any], info: NeuralStageInfo)] = []
        let pixels = UInt64(width) * UInt64(height)
        let source = pixels * 4 * UInt64(bytesPerSample)
        var peak: UInt64 = 0
        if settings.removeJpegArtifacts, let jpeg {
            let stage = NeuralFilterRun.jpegStage()
            let info = try NeuralFilterRun.info(stage, width: width, height: height, bytesPerSample: bytesPerSample)
            stages.append((stage, info))
            peak = info.plannedBytes + jpeg.inferenceBytes
        }
        let stage = NeuralFilterRun.superResolutionStage(
            zoom: settings.zoom, enhance: settings.enhanceDetails,
            noiseReduction: settings.noiseReduction, sharpen: settings.sharpen)
        let info = try NeuralFilterRun.info(stage, width: width, height: height, bytesPerSample: bytesPerSample)
        stages.append((stage, info))
        let held = settings.removeJpegArtifacts ? source : 0
        peak = max(peak, info.plannedBytes + (settings.enhanceDetails ? superResolution.inferenceBytes : 0) + held)
        let output = UInt64(info.outputWidth) * UInt64(info.outputHeight)
        return SuperZoomPlan(stages: stages,
                             plannedBytes: peak + source
                                 + NeuralFilterRun.hostBytesPerOutputPixel(bytesPerSample: bytesPerSample) * output,
                             width: info.outputWidth, height: info.outputHeight)
    }

    /// The ready models, or the not-installed sentence: Real-ESRGAN always (even with Enhance
    /// off), FBCNN with Remove JPEG Artifacts.
    static func models(_ settings: SuperZoomSettings) throws
        -> (superResolution: (entry: ModelCatalogEntry, compiled: URL), jpeg: (entry: ModelCatalogEntry, compiled: URL)?) {
        guard let superResolution = ModelGate.ready(capability) else {
            throw DoctoredCoreError(message: ModelGate.notInstalled(capability))
        }
        guard settings.removeJpegArtifacts else { return (superResolution, nil) }
        guard let jpeg = ModelGate.ready(JpegArtifactsRemovalRun.capability) else {
            throw DoctoredCoreError(message: ModelGate.notInstalled(JpegArtifactsRemovalRun.capability))
        }
        return (superResolution, jpeg)
    }

    /// The new document's title: the source's name, then "Super Zoom ×N".
    static func title(_ document: ImageDocument, zoom: Int) -> String {
        let base = (document.displayName as NSString).deletingPathExtension
        return "\(base) Super Zoom ×\(zoom)"
    }

    /// Runs Super Zoom on `layer` of `document` and opens the new document (main thread): the
    /// depth refusal, the models, the plan, then one admitted, cancellable worker job.
    static func run(document: ImageDocument, layer: Int, settings: SuperZoomSettings) throws -> SuperZoomResult {
        guard let source = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        if let reason = NeuralFilterRun.refusal(source, name: name) { throw DoctoredCoreError(message: reason) }
        let models = try models(settings)
        guard source.isValidIndex(layer) else { throw DoctoredCoreError(message: "super zoom layer \(layer) is out of range") }
        let bytesPerSample = source.isHighDepth ? 2 : 1
        let base = try plan(width: source.width, height: source.height, bytesPerSample: bytesPerSample,
                            settings: settings, superResolution: models.superResolution.entry,
                            jpeg: models.jpeg?.entry)
        // A Lab document's model reads the Display P3 view and its result returns to Lab codes
        // (#1 Lab phase 6b-iii): the plan carries both images.
        let labTerm = NeuralFilterRun.labTerm(source, input: (source.width, source.height),
                                              output: (base.width, base.height))
        let plan = SuperZoomPlan(stages: base.stages,
                                 plannedBytes: NeuralFilterRun.plus(base.plannedBytes, labTerm),
                                 width: base.width, height: base.height)
        let budget = try NeuralFilterRun.budget(source)
        _ = try RasterResources.shared.admit(scratch: plan.plannedBytes)
        let control = try RasterJobControl()
        let stages = plan.stages
        let jpeg = models.jpeg, superResolution = models.superResolution
        let enhance = settings.enhanceDetails
        let made = try BackgroundWork.wait(title: name, control: control, revealAfter: DocumentJobs.panelDelay) {
            try WorkerTask(scratchEstimate: plan.plannedBytes) { () -> RasterDocument in
                try control.beginCompoundOperation()
                var image: NeuralImage = try control.withChildOperation(from: 0, to: 0.04) { child in
                    if let precision = source.precision {
                        return .wide(try precision.layerCanvasImage(
                            layer, workingLimit: budget.limit, otherRetained: budget.other, control: child))
                    }
                    guard let canvas = source.layerCanvasImage(layer) else {
                        throw DoctoredCoreError(message: "The layer could not be read for Super Zoom.")
                    }
                    return .byte(canvas)
                }
                if source.isLab, case .wide(let canvas) = image {
                    image = .wide(try control.withChildOperation(from: 0.04, to: 0.05) { child in
                        try NeuralFilterRun.modelInput(canvas, document: source, limit: budget.limit,
                                                       other: budget.other, control: child)
                    })
                }
                var start = 0.05
                for (index, entry) in stages.enumerated() {
                    let last = index == stages.count - 1
                    let end = last ? 0.95 : 0.35
                    let model: NeuralTileModel?
                    if !last, let jpeg {
                        model = try NeuralTileModel(compiled: jpeg.compiled, side: NeuralFilterRun.jpegTile,
                                                    outputSide: NeuralFilterRun.jpegTile,
                                                    qualityFactor: JpegArtifactsStrength.medium.qualityFactor)
                    } else if enhance {
                        model = try NeuralTileModel(
                            compiled: superResolution.compiled, side: NeuralFilterRun.superResolutionTile,
                            outputSide: NeuralFilterRun.superResolutionTile * NeuralFilterRun.superResolutionScale,
                            qualityFactor: nil)
                    } else {
                        model = nil
                    }
                    let input = image
                    image = try control.withChildOperation(from: start, to: end) { child in
                        try NeuralFilterRun.run(entry.stage, model: model, image: input, limit: budget.limit,
                                                other: budget.other, control: child)
                    }
                    start = end
                }
                let zoomed = image
                return try control.withChildOperation(from: 0.95, to: 1) { child in
                    switch zoomed {
                    case .byte(let pixels): return try source.superZoomDocument(pixels)
                    case .wide(let pixels):
                        guard let precision = source.precision else {
                            throw DoctoredCoreError(message: "Super Zoom lost its document.")
                        }
                        return try RasterDocument(precision: precision.superZoomDocument(
                            pixels, limit: budget.limit, other: budget.other, control: child))
                    }
                }
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        let opened = ImageDocument.openSuperZoom(made, name: title(document, zoom: settings.zoom))
        ActionRecorder.shared.noteDuplicatedDocument(opened, from: document)
        return SuperZoomResult(document: opened, plan: plan,
                               models: [superResolution.entry] + (jpeg.map { [$0.entry] } ?? []))
    }
}

extension ImageDocument {
    /// Super Zoom's new document, shown (`openMergedHDR`'s steps): native type, dirty, its one
    /// layer active, titled `name`.
    static func openSuperZoom(_ made: RasterDocument, name: String) -> ImageDocument {
        let document = ImageDocument()
        document.doc = made
        document.activeLayerIndex = 0
        document.refreshProjection()
        document.fileType = Self.nativeTypeIdentifier
        document.displayName = name
        document.countEditChange(.changeDone)
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        return document
    }
}
