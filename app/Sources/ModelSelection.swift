import AppKit
import CoreML

// Model selections: Select ▸ Sky, Select ▸ Person Parts… (`PersonParts.swift`) and
// the Object Selection tool's Object Finder (`ObjectFinder.swift`). Each runs a segmentation
// model from the store (`ModelGate`) on the captured analysis composite — `capture.source`, at
// 16 and 32 bits the display-encoded copy — and its low-resolution output becomes canvas
// u16 coverage in ONE core kernel (`RasterModelMaskCore`, `model_mask.rs`), which Select and
// Mask's edge stage (`SelectionCapture.refinedEdge`) then refines against the picture. Smart
// Radius leaves flat display codes untouched, so the edge stage runs at every depth here. Core ML
// runs only inside `ModelMaskRun.coverage`'s admitted worker job; the refinement is its own
// self-admitted job afterwards on the main thread, as `ObjectSelection.shape` runs it.

/// One model output read as float planes: `channels` planes of `width × height` samples,
/// CHANNEL-MAJOR (`c·w·h + y·w + x`), row 0 at the top — the layout `dr_model_mask_coverage`
/// takes.
struct ModelPlane {
    let width: Int
    let height: Int
    let channels: Int
    let samples: [Float]
}

/// Which catalogue model serves a capability, and the sentences around a missing one.
enum ModelGate {
    /// The first catalogue entry for `capability` that is ready in the store (catalogue order is
    /// the preference) and its first compiled package; nil when none is.
    static func ready(_ capability: String) -> (entry: ModelCatalogEntry, compiled: URL)? {
        for entry in ModelCatalog.entries(capability: capability) {
            if let compiled = ModelStore.shared.compiledModelURL(entry.id) { return (entry, compiled) }
        }
        return nil
    }

    /// The refusal when no model for `capability` is ready: what to download, and where.
    static func notInstalled(_ capability: String) -> String {
        switch capability {
        case "sky": return "the sky model is not installed; download it in Settings ▸ Models"
        case "person_parts": return "the person parts model is not installed; download it in Settings ▸ Models"
        case "object_prompt":
            return "the Object Finder model is not installed; download SAM 2.1 in Settings ▸ Models"
        case "inpaint": return "the Remove model is not installed; download MI-GAN in Settings ▸ Models"
        case "super_resolution":
            return "the Super Zoom model is not installed; download Real-ESRGAN in Settings ▸ Models"
        case "jpeg_artifacts":
            return "the JPEG Artifacts Removal model is not installed; download FBCNN in Settings ▸ Models"
        case "colorize": return "the Colorize model is not installed; download DDColor in Settings ▸ Models"
        default: return "the \(capability) model is not installed; download it in Settings ▸ Models"
        }
    }

    /// The menu's answer to a missing model: the sentence, with OK and Open Settings….
    static func offerSettings(_ message: String) {
        let alert = NSAlert()
        alert.messageText = message
        alert.addButton(withTitle: "OK")
        alert.addButton(withTitle: "Open Settings…")
        if alert.runModal() == .alertSecondButtonReturn { SettingsWindowController.run() }
    }

    /// An Action step's note: the command and the model that made the selection, so a replay
    /// on another machine can say which model it needs.
    static func note(_ menuPath: String, _ entry: ModelCatalogEntry) -> String {
        "\(menuPath) — \(entry.id) @ \(entry.version)"
    }
}

/// One model run over one canvas rectangle: `rect` (whole pixels inside the canvas) is cropped
/// from the composite and squashed to the model's input; `request` is the core's request without
/// its frame (`values`, `channels`), which `ModelMaskRun` adds from `rect`; `clip` is nil or
/// canvas-sized bytes multiplied into the coverage (the person a part belongs to).
struct ModelMaskFrame {
    let rect: CGRect
    let request: [String: Any]
    let clip: [UInt8]?
}

enum ModelMaskRun {
    /// The capture's own term beside the model's `inferenceBytes`, per canvas pixel: the
    /// composite's `CGImage` (4), a frame's crop (at most 4), and three u16 planes — the core's
    /// output, this side's copy and the running union (6). The record is on
    /// `ModelCatalog.json`'s comment.
    static let captureBytesPerPixel: UInt64 = 14
    /// The largest plane, in samples, these models answer (MediaPipe's 6 × 256², SAM's 3 × 256²,
    /// the sky model's 384²), bounding the plane terms of `coverageScratch`.
    static let maximumPlaneSamples: UInt64 = 1 << 20

    /// What one frame's coverage may hold beside the model: the core's figure, whose master copy
    /// is `model_mask::coverage_scratch` (4 bytes a plane sample for the summed plane, 2 a canvas
    /// pixel for the output), at the largest plane, plus the plane this side holds (4 a sample).
    static func coverageScratch(width: Int, height: Int) -> UInt64 {
        maximumPlaneSamples * 4 + UInt64(width) * UInt64(height) * 2 + maximumPlaneSamples * 4
    }

    /// The edge stage's radius for a frame read from `plane`: one model cell in canvas pixels
    /// (the frame's long side over the plane's), held to 2–64 px — the model cannot place an
    /// edge more finely than its cell, so the stage searches that far and no farther.
    static func refineRadius(frame: CGRect, plane: ModelPlane) -> Double {
        let cell = Double(max(frame.width, frame.height)) / Double(max(1, max(plane.width, plane.height)))
        return min(64, max(2, cell.rounded(.up)))
    }

    /// The union (per-sample max) of each frame's model coverage, refined by the edge stage.
    /// `predict` runs the model on one crop and answers its plane; it runs on the worker. The
    /// crops, predictions, coverages and union are one admitted, cancellable job titled `title`
    /// at `entry.inferenceBytes + captureBytesPerPixel·n + coverageScratch` plus the frames'
    /// clips; the refinement is its own job after it. `covered` counts samples at or above half
    /// coverage before the refinement, summed over the frames (exact when their clips are
    /// disjoint, as people's are); a run with nothing covered skips the refinement, and so does
    /// `refine` false (the MCP tools' `refine: false`: the model's own matte).
    static func coverage(_ capture: SelectionCapture, in document: ImageDocument, title: String,
                         entry: ModelCatalogEntry, frames: [ModelMaskFrame], refine: Bool = true,
                         predict: @escaping (CGImage) throws -> ModelPlane) throws
        -> (shape: CanvasSelection.Shape, covered: UInt64) {
        try capture.requireCurrent(in: document)
        let width = capture.doc.width, height = capture.doc.height
        let canvas = CGRect(x: 0, y: 0, width: width, height: height)
        guard let source = capture.source, let image = source.makeCGImage(in: capture.doc.colorSpace) else {
            throw DoctoredCoreError(message: SubjectSelection.Failure.noComposite.message)
        }
        let rects = frames.map { $0.rect.integral.intersection(canvas) }
        guard !frames.isEmpty, rects.allSatisfy({ !$0.isNull && $0.width >= 1 && $0.height >= 1 }) else {
            throw DoctoredCoreError(message: "The model's region lies outside the canvas.")
        }
        let pixels = UInt64(width) * UInt64(height)
        let clips = UInt64(frames.filter { $0.clip != nil }.count) * pixels
        let scratch = entry.inferenceBytes + captureBytesPerPixel * pixels
            + coverageScratch(width: width, height: height) + clips
        _ = try RasterResources.shared.admit(scratch: scratch)
        // Measured before the task registers its own estimate as transient.
        let context = try PrecisionWorkContext()
        let control = try RasterJobControl()
        let result = try BackgroundWork.wait(title: title, control: control,
                                             revealAfter: DocumentJobs.panelDelay) {
            try WorkerTask(scratchEstimate: scratch) { () -> ([UInt16], UInt64, Double) in
                var union: [UInt16]?
                var covered: UInt64 = 0
                var radius = 2.0
                for (frame, rect) in zip(frames, rects) {
                    guard !control.isCancelled else { throw JobCancelled() }
                    let plane: ModelPlane = try autoreleasepool {
                        guard let crop = image.cropping(to: rect) else {
                            throw DoctoredCoreError(message: "The image could not be prepared for the model.")
                        }
                        return try predict(crop)
                    }
                    guard !control.isCancelled else { throw JobCancelled() }
                    var request = frame.request
                    request["frame"] = [Int(rect.minX), Int(rect.minY), Int(rect.width), Int(rect.height)]
                    let found = try RasterModelMaskCore.coverage(
                        plane, request: request, clip: frame.clip, width: width, height: height,
                        limit: context.limit, other: context.otherRetained, control: nil)
                    covered += found.covered
                    radius = max(radius, refineRadius(frame: rect, plane: plane))
                    union = union.map { PlaneAlgebra.combine($0, found.coverage, mode: .add) } ?? found.coverage
                }
                return (union ?? [], covered, radius)
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        try capture.requireCurrent(in: document)
        guard result.1 > 0, refine else { return (.mask16(result.0), result.1) }
        let refined = try capture.refinedEdge(.mask16(result.0), edgeRadius: result.2, smartRadius: true,
                                              area: nil, in: document, title: title)
        return (refined, result.1)
    }

    /// The model at `compiled` on `image` (`CoreMLRuntime.imageInput`, scale-fill), read as a
    /// plane from `output`, else from the model's first output by name. The one-image predictor
    /// of Select ▸ Sky and Select ▸ Person Parts….
    static func predictImage(_ image: CGImage, compiled: URL, output: String) throws -> ModelPlane {
        let model = try CoreMLRuntime.load(compiled)
        let input = try CoreMLRuntime.imageInput(image, for: model)
        let provider = try MLDictionaryFeatureProvider(dictionary: [input.name: input.value])
        let result = try model.prediction(from: provider)
        let name = model.modelDescription.outputDescriptionsByName[output] != nil
            ? output : model.modelDescription.outputDescriptionsByName.keys.sorted().first
        guard let name, let feature = result.featureValue(for: name) else {
            throw DoctoredCoreError(message: "The model returned no output.")
        }
        return try CoreMLRuntime.plane(feature)
    }
}

/// Select ▸ Sky: the sky model over the whole canvas.
enum SkySelection {
    static let noSky = "No sky was found in this image."
    /// The least share of the canvas at or above half coverage that is a sky: 0.2 %, a strip of
    /// sky a few pixels high over a whole photo. Below it the model's answer is noise in a
    /// picture with no sky, and the command refuses by name rather than selecting specks.
    static let minimumShare = 0.002
    static let menuPath = "Select ▸ Sky"

    /// The refined sky coverage of the captured composite, the model that found it, and the
    /// share of the canvas the model covered at half or more before the refinement. Throws
    /// `ModelGate.notInstalled("sky")` and `noSky`.
    static func shape(_ capture: SelectionCapture, in document: ImageDocument, refine: Bool = true) throws
        -> (shape: CanvasSelection.Shape, entry: ModelCatalogEntry, share: Double) {
        guard let model = ModelGate.ready("sky") else {
            throw DoctoredCoreError(message: ModelGate.notInstalled("sky"))
        }
        let width = capture.doc.width, height = capture.doc.height
        let frame = ModelMaskFrame(rect: CGRect(x: 0, y: 0, width: width, height: height),
                                   request: ["values": "probability", "channels": [0]], clip: nil)
        let compiled = model.compiled
        let found = try ModelMaskRun.coverage(capture, in: document, title: "Select Sky", entry: model.entry,
                                              frames: [frame], refine: refine) { image in
            try ModelMaskRun.predictImage(image, compiled: compiled, output: "sky")
        }
        let share = Double(found.covered) / Double(width * height)
        guard share >= minimumShare else { throw DoctoredCoreError(message: noSky) }
        return (found.shape, model.entry, share)
    }
}

extension SkySelection {
    /// The sky model's own matte of all of `image` as bytes (0 outside…255 inside, row 0 top,
    /// `(v + 128) / 257` of the core's u16 coverage), with no edge refinement, as the RAW subject
    /// arm reads Vision's matte: the one sky-matte helper of a RAW Sky local
    /// (`RawLocalGenerated`) and the Develop Filter's Sky mask (`DevelopFilterMasks`). Worker
    /// thread. The caller admits `entry.inferenceBytes + ModelMaskRun.coverageScratch(w, h) +
    /// ModelMaskRun.captureBytesPerPixel · w · h` first. Throws `ModelGate.notInstalled("sky")`,
    /// and `noSky` below `minimumShare` of the image at half coverage.
    static func coverage(in image: CGImage, limit: UInt64, other: UInt64,
                         control: RasterJobControl?) throws -> [UInt8] {
        guard let model = ModelGate.ready("sky") else {
            throw DoctoredCoreError(message: ModelGate.notInstalled("sky"))
        }
        let width = image.width, height = image.height
        let plane = try autoreleasepool {
            try ModelMaskRun.predictImage(image, compiled: model.compiled, output: "sky")
        }
        let found = try RasterModelMaskCore.coverage(
            plane, request: ["values": "probability", "channels": [0], "frame": [0, 0, width, height]],
            clip: nil, width: width, height: height, limit: limit, other: other, control: control)
        guard Double(found.covered) / Double(width * height) >= minimumShare else {
            throw DoctoredCoreError(message: noSky)
        }
        return found.coverage.map { UInt8((UInt32($0) + 128) / 257) }
    }
}
