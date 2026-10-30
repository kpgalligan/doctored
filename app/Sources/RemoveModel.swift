import CoreML
import Foundation

// The Remove tool's Model mode (M3a): the stroke's hole is filled by a local inpainting
// model (MI-GAN 512 Places2, `ModelCatalog`'s "inpaint" entry) on a square crop around it at the
// model's native 512², and the core composites the fill back at 8 and 16 bits
// (`DoctoredCore+RemoveModel.swift`, `remove_model.rs`). Content-Aware stays the other Mode.
// Core ML runs only inside the job's worker, between the core's input and apply.

/// The Remove tool's engine.
enum RemoveMode: String, Codable, CaseIterable {
    /// The local inpainting model (the default).
    case model
    /// The Content-Aware Fill hole.
    case contentAware = "content_aware"
}

/// The Remove tool's own options beside its paint slot (`ToolOptionsStore.remove`).
struct RemoveToolOptions: Codable, Equatable {
    var mode: RemoveMode = .model
    /// "Remove after each stroke" (on by default): off, each stroke is held as an
    /// overlay until Apply (✓, Return) runs them all as one job, or Cancel (✕, Escape) drops them.
    var removeAfterEachStroke = true
}

/// The worker's crop, read on main by the job's completion.
private final class RemoveCropBox {
    private let lock = NSLock()
    private var value: [Int]?
    var crop: [Int]? {
        get { lock.lock(); defer { lock.unlock() }; return value }
        set { lock.lock(); value = newValue; lock.unlock() }
    }
}

enum RemoveModelRun {
    static let capability = "inpaint"
    /// MI-GAN's native side; the package's input shape is checked against it.
    static let side = 512
    /// The host's own term per canvas pixel beside the core's plan and the model's
    /// `inferenceBytes`: the stroke's u8 bitmap (1) and its u16 coverage (2). The record is on
    /// `ModelCatalog`'s comment.
    static let hostBytesPerPixel: UInt64 = 3

    /// Why the model cannot run on `doc`, nil when it can: a 32-bit or CMYK document. Both keep
    /// refusing by name, and Content-Aware runs there.
    static func refusal(_ doc: RasterDocument) -> String? {
        if doc.isFloat {
            return "Remove's model is not available in a 32-bit document. Choose Mode: "
                + "Content-Aware (mode \"content_aware\")."
        }
        if doc.isCmyk {
            return "Remove's model is not available in a CMYK document. Choose Mode: "
                + "Content-Aware (mode \"content_aware\")."
        }
        return nil
    }

    /// The job's admitted working set: the core's plan, the model's `inferenceBytes` and the
    /// host's own term.
    static func plan(entry: ModelCatalogEntry, doc: RasterDocument, layer: Int,
                     sample: RetouchSampling, newLayer: Bool) throws -> UInt64 {
        let core = try doc.removeModelPlan(layer: layer, sample: sample, side: side, newLayer: newLayer)
        let pixels = UInt64(doc.width) * UInt64(doc.height)
        return core + entry.inferenceBytes + hostBytesPerPixel * pixels
    }

    /// One prediction: `image` (`3·side²`, channel-major RGB 0…1) and `mask` (`side²`, 1 =
    /// remove) as the package's `image` and `mask` multi-arrays, its `filled` output read back
    /// as a plane. Worker thread only.
    static func predict(image: [Float], mask: [Float], compiled: URL) throws -> ModelPlane {
        let model = try CoreMLRuntime.load(compiled)
        let inputs = model.modelDescription.inputDescriptionsByName
        let expected: [NSNumber] = [1, 3, side, side].map { NSNumber(value: $0) }
        guard inputs["image"]?.multiArrayConstraint?.shape == expected,
              inputs["mask"]?.multiArrayConstraint?.shape == [1, 1, side, side].map({ NSNumber(value: $0) })
        else {
            throw DoctoredCoreError(message: "The Remove model does not take \(side)² image and mask planes.")
        }
        let provider = try MLDictionaryFeatureProvider(dictionary: [
            "image": MLFeatureValue(multiArray: try array(image, channels: 3)),
            "mask": MLFeatureValue(multiArray: try array(mask, channels: 1)),
        ])
        let result = try model.prediction(from: provider)
        guard let feature = result.featureValue(for: "filled") else {
            throw DoctoredCoreError(message: "The Remove model returned no fill.")
        }
        let plane = try CoreMLRuntime.plane(feature)
        guard plane.channels == 3, plane.width == side, plane.height == side else {
            throw DoctoredCoreError(message: "The Remove model's fill is not a \(side)² RGB plane.")
        }
        return plane
    }

    /// `samples` (channel-major) as a `[1, channels, side, side]` Float32 multi-array, written
    /// through the array's own strides.
    private static func array(_ samples: [Float], channels: Int) throws -> MLMultiArray {
        let shape = [1, channels, side, side].map { NSNumber(value: $0) }
        let array = try MLMultiArray(shape: shape, dataType: .float32)
        guard samples.count == channels * side * side else {
            throw DoctoredCoreError(message: "The Remove model's input has the wrong size.")
        }
        array.withUnsafeMutableBufferPointer(ofType: Float.self) { pointer, strides in
            let (cs, ys, xs) = (strides[1], strides[2], strides[3])
            for c in 0..<channels {
                for y in 0..<side {
                    for x in 0..<side {
                        pointer[c * cs + y * ys + x * xs] = samples[(c * side + y) * side + x]
                    }
                }
            }
        }
        return array
    }

    /// Starts the Model-mode job for the canvas-sized u16 `coverage`: the store's model
    /// (`ModelGate.notInstalled` when none is ready), the depth refusal, the plan admitted by
    /// `DocumentJobs.start`, and a compute that runs the core's input, the prediction and the
    /// core's apply, polling the cancel between the stages. `limit`/`other` are measured on main
    /// before the job. The job's details carry `planned_bytes`, `model` and, once the worker has
    /// run, `crop`; `RemoveStroke.start` adds `covered_pixels`.
    static func start(_ request: RemoveRequest, coverage: [UInt16], document: ImageDocument,
                      layer: Int, record: [ActionStep], showProgress: Bool) throws -> DocumentJob {
        guard let model = ModelGate.ready(capability) else {
            throw DoctoredCoreError(message: ModelGate.notInstalled(capability))
        }
        guard let source = document.doc else { throw DoctoredCoreError(message: "Document has no image.") }
        if let reason = refusal(source) { throw DoctoredCoreError(message: reason) }
        let newLayer = request.output == .newLayer
        let planned = try plan(entry: model.entry, doc: source, layer: layer,
                               sample: request.choice.mode, newLayer: newLayer)
        let budget: (limit: UInt64, other: UInt64)
        if let precision = source.precision {
            let context = try PrecisionWorkContext(source: precision)
            budget = (context.limit, context.otherRetained)
        } else {
            budget = try source.legacyPrecisionWorkContext()
        }
        let compiled = model.compiled
        let choice = request.choice
        let box = RemoveCropBox()
        let job = try DocumentJobs.start(
            document: document, layer: layer, title: "Remove", scratch: planned, record: record,
            showProgress: showProgress, rasterizesTarget: !newLayer,
            compute: { source, control in
                try control.beginCompoundOperation()
                let input = try control.withChildOperation(from: 0, to: 0.3) { child in
                    try source.removeModelInput(
                        layer: layer, choice: choice, coverage: coverage, side: side,
                        limit: budget.limit, other: budget.other, control: child)
                }
                box.crop = input.crop
                guard !control.isCancelled else { throw JobCancelled() }
                let plane = try autoreleasepool {
                    try predict(image: input.image, mask: input.mask, compiled: compiled)
                }
                guard !control.isCancelled else { throw JobCancelled() }
                return try control.withChildOperation(from: 0.8, to: 1) { child in
                    try source.removeModelApply(
                        layer: layer, coverage: coverage, side: side, plane: plane,
                        newLayer: newLayer, limit: budget.limit, other: budget.other, control: child)
                }
            })
        job.details["planned_bytes"] = planned
        job.details["model"] = "\(model.entry.id) @ \(model.entry.version)"
        job.completion = { finished in
            if let crop = box.crop { finished.details["crop"] = crop }
        }
        return job
    }
}
