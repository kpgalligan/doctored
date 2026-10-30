import AppKit

// Filter ▸ Neural Filters ▸ Colorize… (M4 phase 2): DDColor predicts Lab chroma from the
// layer's luminance once at 512², and the core (`neural_colorize.rs`) adjusts it with the
// classical Saturation, Cyan/Red, Magenta/Green, Yellow/Blue and Color Artifact Reduction and
// carries it onto the layer's own full-resolution L* in the document's profile, so the source's
// luminance and detail stay exact. Output to the layer itself, a new layer "<name> Colorize"
// directly above it (JPEG Artifacts Removal's `DocumentJobs` job, one undo step), or a NEW
// document "<doc> Colorize" (Super Zoom's `BackgroundWork` job and `openSuperZoom`, noted as a
// duplicate). The selection is ignored: the filter outputs the whole layer;
// Focal Points are not built (DDColor takes no colour hints). On a Lab Color document (#547 Lab
// phase 6b-iii) it runs on the layer's Display P3 view in that profile
// (`NeuralFilterRun.modelInput` / `modelProfile`: the twin is no matrix/TRC profile), and the core
// returns the result to Lab codes by the Mode rule.

/// The Output To popup.
enum ColorizeOutput: String, CaseIterable {
    case layer, newLayer = "new_layer", newDocument = "new_document"

    var title: String {
        switch self {
        case .layer: return "Current Layer"
        case .newLayer: return "New Layer"
        case .newDocument: return "New Document"
        }
    }
}

/// The dialog's controls, in the core's units.
struct ColorizeSettings: Equatable {
    /// −50…50 each.
    var saturation = 0.0
    var cyanRed = 0.0
    var magentaGreen = 0.0
    var yellowBlue = 0.0
    /// 0…100.
    var colorArtifactReduction = 0.0
    var output = ColorizeOutput.layer

    /// The sliders' MCP / Action keys, in the sheet's order, with their ranges.
    static let sliders: [(key: String, range: ClosedRange<Double>)] = [
        ("saturation", -50...50), ("cyan_red", -50...50), ("magenta_green", -50...50),
        ("yellow_blue", -50...50), ("color_artifact_reduction", 0...100),
    ]

    /// The core's `adjust_json`.
    var adjustments: [String: Any] {
        ["saturation": saturation, "cyan_red": cyanRed, "magenta_green": magentaGreen,
         "yellow_blue": yellowBlue, "color_artifact_reduction": colorArtifactReduction]
    }

    var arguments: [String: Any] {
        var arguments = adjustments.mapValues { value -> Any in
            ActionArgs.number((value as? Double) ?? 0)
        }
        arguments["output"] = output.rawValue
        return arguments
    }

    /// The slider value for `key`.
    func value(_ key: String) -> Double {
        switch key {
        case "saturation": return saturation
        case "cyan_red": return cyanRed
        case "magenta_green": return magentaGreen
        case "yellow_blue": return yellowBlue
        default: return colorArtifactReduction
        }
    }

    mutating func set(_ key: String, _ value: Double) {
        switch key {
        case "saturation": saturation = value
        case "cyan_red": cyanRed = value
        case "magenta_green": magentaGreen = value
        case "yellow_blue": yellowBlue = value
        default: colorArtifactReduction = value
        }
    }

    /// The MCP / Action arguments; absent keys keep their defaults.
    static func parse(_ a: [String: Any]) throws -> ColorizeSettings {
        var settings = ColorizeSettings()
        for slider in sliders {
            guard let value = try NeuralFilterRun.number(a, slider.key) else { continue }
            guard slider.range.contains(value) else {
                let low = Int(slider.range.lowerBound), high = Int(slider.range.upperBound)
                throw DoctoredCoreError(message: "\(slider.key) must be in \(low)…\(high).")
            }
            settings.set(slider.key, value)
        }
        if let value = a["output"] {
            guard let text = value as? String, let output = ColorizeOutput(rawValue: text) else {
                throw DoctoredCoreError(message: "output must be layer, new_layer or new_document.")
            }
            settings.output = output
        }
        return settings
    }
}

enum ColorizeRun {
    static let name = "Colorize"
    static let menu = "Filter ▸ Neural Filters ▸ Colorize…"
    static let capability = "colorize"

    /// The new layer's name for layer `layer`.
    static func layerName(_ doc: RasterDocument, layer: Int) -> String {
        (doc.layerInfo(layer)?.name ?? "Layer") + " Colorize"
    }

    /// The new document's title: the source's name, then "Colorize".
    static func title(_ document: ImageDocument) -> String {
        "\((document.displayName as NSString).deletingPathExtension) Colorize"
    }

    /// The admitted plan: the core's plan over the layer's own pixels (the canvas for New
    /// Document) + DDColor's `inferenceBytes` + `hostBytesPerOutputPixel(bytesPerSample:)` × those
    /// pixels, plus the canvas source the new document is made from (and on a Lab document the
    /// Display P3 view and the Lab result, `NeuralFilterRun.labTerm`).
    static func plan(_ doc: RasterDocument, layer: Int, output: ColorizeOutput, entry: ModelCatalogEntry)
        throws -> UInt64 {
        let bytesPerSample = doc.isHighDepth ? 2 : 1
        let width: Int, height: Int
        if output == .newDocument {
            (width, height) = (doc.width, doc.height)
        } else {
            guard let placement = doc.layerPlacement(layer), placement.width > 0, placement.height > 0 else {
                throw DoctoredCoreError(message: "colorize needs visible pixels; layer \(layer) is empty")
            }
            (width, height) = (placement.width, placement.height)
        }
        let core = try RasterNeural.colorizePlan(width: width, height: height, side: NeuralFilterRun.colorizeSide,
                                                 bytesPerSample: bytesPerSample)
        let pixels = UInt64(width) * UInt64(height)
        let host = NeuralFilterRun.hostBytesPerOutputPixel(bytesPerSample: bytesPerSample)
        let canvas = output == .newDocument ? pixels * 4 * UInt64(bytesPerSample) : 0
        let lab = NeuralFilterRun.labTerm(doc, input: (width, height), output: (width, height))
        return NeuralFilterRun.plus(core + entry.inferenceBytes + host * pixels + canvas, lab)
    }

    /// The loaded DDColor model. Worker threads only.
    static func model(_ compiled: URL) throws -> NeuralTileModel {
        try NeuralTileModel(compiled: compiled, side: NeuralFilterRun.colorizeSide,
                            outputSide: NeuralFilterRun.colorizeSide, qualityFactor: nil, outputChannels: 2)
    }

    /// The model's chroma for `image` (`2·side²` a*, b*): the core's gray input, then DDColor.
    /// `control`, when given, is a begun compound parent. Worker thread only.
    static func chroma(_ image: NeuralImage, profile: Data, model: NeuralTileModel, limit: UInt64,
                       other: UInt64, control: RasterJobControl?) throws -> [Float] {
        let side = NeuralFilterRun.colorizeSide
        let input: [Float] = try step(control, 0, 0.1) { child in
            switch image {
            case .byte(let pixels):
                return try pixels.neuralColorizeInput(profile: profile, side: side, limit: limit,
                                                      other: other, control: child)
            case .wide(let pixels):
                return try pixels.neuralColorizeInput(profile: profile, side: side, limit: limit,
                                                      other: other, control: child)
            }
        }
        if control?.isCancelled == true { throw JobCancelled() }
        var chroma = [Float](repeating: 0, count: 2 * side * side)
        try input.withUnsafeBufferPointer { source in
            try chroma.withUnsafeMutableBufferPointer { plane in
                try autoreleasepool { try model.predict(source, into: plane) }
            }
        }
        if control?.isCancelled == true { throw JobCancelled() }
        return chroma
    }

    /// `chroma` applied to `image` with `settings`; nil when nothing changes. Worker thread only.
    static func apply(_ image: NeuralImage, profile: Data, chroma: [Float], settings: ColorizeSettings,
                      limit: UInt64, other: UInt64, control: RasterJobControl?, from: Double = 0.15,
                      to: Double = 1) throws -> NeuralImage? {
        let side = NeuralFilterRun.colorizeSide
        let adjustments = settings.adjustments
        return try step(control, from, to) { child in
            switch image {
            case .byte(let pixels):
                return try pixels.neuralColorizeApply(profile: profile, side: side, chroma: chroma,
                                                      adjustments: adjustments, limit: limit, other: other,
                                                      control: child).map(NeuralImage.byte)
            case .wide(let pixels):
                return try pixels.neuralColorizeApply(profile: profile, side: side, chroma: chroma,
                                                      adjustments: adjustments, limit: limit, other: other,
                                                      control: child).map(NeuralImage.wide)
            }
        }
    }

    /// Colorize over `image` through `model`: input, the model, the apply; nil when nothing
    /// changes. `control`, when given, is a begun compound parent. Worker thread only.
    static func colorize(_ image: NeuralImage, profile: Data, model: NeuralTileModel, settings: ColorizeSettings,
                         limit: UInt64, other: UInt64, control: RasterJobControl?) throws -> NeuralImage? {
        let chroma = try chroma(image, profile: profile, model: model, limit: limit, other: other,
                                control: control)
        return try apply(image, profile: profile, chroma: chroma, settings: settings, limit: limit,
                         other: other, control: control, from: 0.15, to: 0.9)
    }

    /// `body` under a child of `control` over `from…to`, or with no control.
    private static func step<T>(_ control: RasterJobControl?, _ from: Double, _ to: Double,
                                _ body: (RasterJobControl?) throws -> T) throws -> T {
        guard let control else { return try body(nil) }
        return try control.withChildOperation(from: from, to: to) { try body($0) }
    }

    /// The shared gates (main thread): the depth refusal, the model, the layer, a retained source.
    private static func admit(_ source: RasterDocument, layer: Int) throws -> (entry: ModelCatalogEntry, compiled: URL) {
        if let reason = NeuralFilterRun.refusal(source, name: name) { throw DoctoredCoreError(message: reason) }
        guard let model = ModelGate.ready(capability) else {
            throw DoctoredCoreError(message: ModelGate.notInstalled(capability))
        }
        guard source.isValidIndex(layer) else {
            throw DoctoredCoreError(message: "colorize layer \(layer) is out of range")
        }
        if source.hasRetainedSource(layer) { throw DoctoredCoreError(message: RetainedImagePolicy.rasterizeFirst) }
        return model
    }

    /// Starts the layer-output job on `layer` (main thread; output layer or new layer): the gates,
    /// the plan admitted by `DocumentJobs.start`, and a compute that runs the model and the apply.
    /// The job's details carry `planned_bytes` and `model`.
    static func start(document: ImageDocument, layer: Int, settings: ColorizeSettings,
                      record: [ActionStep], showProgress: Bool) throws -> DocumentJob {
        guard let source = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        let model = try admit(source, layer: layer)
        let planned = try plan(source, layer: layer, output: settings.output, entry: model.entry)
        let budget = try NeuralFilterRun.budget(source)
        let newLayer = settings.output == .newLayer
        let name = layerName(source, layer: layer)
        let compiled = model.compiled
        let profile = NeuralFilterRun.modelProfile(source)
        let placement = source.layerPlacement(layer)
        let job = try DocumentJobs.start(
            document: document, layer: layer, title: Self.name, scratch: planned, record: record,
            showProgress: showProgress, rasterizesTarget: !newLayer,
            compute: { source, control in
                try control.beginCompoundOperation()
                let ddcolor = try Self.model(compiled)
                if let precision = source.precision {
                    guard let pixels = precision.layerSource(layer) else {
                        throw DoctoredCoreError(message: "colorize needs a raster layer; layer \(layer) is not one")
                    }
                    // A Lab layer is colorized in its Display P3 view (#547 Lab phase 6b-iii).
                    var view = pixels
                    if source.isLab {
                        view = try control.withChildOperation(from: 0, to: 0.02) { child in
                            try NeuralFilterRun.modelInput(pixels, document: source, limit: budget.limit,
                                                           other: budget.other, control: child)
                        }
                    }
                    let input = view
                    guard case .wide(let colored)? = try colorize(
                        .wide(input), profile: profile, model: ddcolor, settings: settings,
                        limit: budget.limit, other: budget.other, control: control) else { return nil }
                    let applied = try control.withChildOperation(from: 0.9, to: 1) { child in
                        try precision.neuralLayerApply(layer, pixels: colored, newLayer: newLayer, name: name,
                                                       limit: budget.limit, other: budget.other, control: child)
                    }
                    return try applied.map { try RasterDocument(precision: $0) }
                }
                guard let pixels = source.layerImage(layer) else {
                    throw DoctoredCoreError(message: "colorize needs a raster layer; layer \(layer) is not one")
                }
                guard case .byte(let colored)? = try colorize(
                    .byte(pixels), profile: profile, model: ddcolor, settings: settings,
                    limit: budget.limit, other: budget.other, control: control) else { return nil }
                guard newLayer else { return source.withLayerPixels(layer, colored) }
                return source.addingImageLayer(above: layer, colored, name: name,
                                               offset: (x: placement?.x ?? 0, y: placement?.y ?? 0))
            })
        job.details["planned_bytes"] = planned
        job.details["model"] = "\(model.entry.id) @ \(model.entry.version)"
        return job
    }

    /// Colorize's New Document (main thread): the layer on its canvas colorized into a new
    /// document that always opens (the unchanged canvas when the output
    /// equals it, `changed` false), noted as a duplicate.
    static func runNewDocument(document: ImageDocument, layer: Int, settings: ColorizeSettings) throws
        -> (document: ImageDocument, changed: Bool, plannedBytes: UInt64) {
        guard let source = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        let model = try admit(source, layer: layer)
        let planned = try plan(source, layer: layer, output: .newDocument, entry: model.entry)
        let budget = try NeuralFilterRun.budget(source)
        _ = try RasterResources.shared.admit(scratch: planned)
        let control = try RasterJobControl()
        let compiled = model.compiled
        let profile = NeuralFilterRun.modelProfile(source)
        let made = try BackgroundWork.wait(title: name, control: control, revealAfter: DocumentJobs.panelDelay) {
            try WorkerTask(scratchEstimate: planned) { () -> (RasterDocument, Bool) in
                try control.beginCompoundOperation()
                var read: NeuralImage = try control.withChildOperation(from: 0, to: 0.04) { child in
                    if let precision = source.precision {
                        return .wide(try precision.layerCanvasImage(
                            layer, workingLimit: budget.limit, otherRetained: budget.other, control: child))
                    }
                    guard let image = source.layerCanvasImage(layer) else {
                        throw DoctoredCoreError(message: "The layer could not be read for Colorize.")
                    }
                    return .byte(image)
                }
                // A Lab canvas is colorized in its Display P3 view, which is also the new
                // document's picture when nothing changes (#547 Lab phase 6b-iii).
                if source.isLab, case .wide(let pixels) = read {
                    read = .wide(try control.withChildOperation(from: 0.04, to: 0.05) { child in
                        try NeuralFilterRun.modelInput(pixels, document: source, limit: budget.limit,
                                                       other: budget.other, control: child)
                    })
                }
                let canvas = read
                let ddcolor = try Self.model(compiled)
                let colored = try colorize(canvas, profile: profile, model: ddcolor, settings: settings,
                                           limit: budget.limit, other: budget.other, control: control)
                let image = colored ?? canvas
                let output = try control.withChildOperation(from: 0.9, to: 1) { child -> RasterDocument in
                    switch image {
                    case .byte(let pixels): return try source.superZoomDocument(pixels)
                    case .wide(let pixels):
                        guard let precision = source.precision else {
                            throw DoctoredCoreError(message: "Colorize lost its document.")
                        }
                        return try RasterDocument(precision: precision.superZoomDocument(
                            pixels, limit: budget.limit, other: budget.other, control: child))
                    }
                }
                return (output, colored != nil)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        let opened = ImageDocument.openSuperZoom(made.0, name: title(document))
        ActionRecorder.shared.noteDuplicatedDocument(opened, from: document)
        return (opened, made.1, planned)
    }
}
