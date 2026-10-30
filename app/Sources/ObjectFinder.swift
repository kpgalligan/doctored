import AppKit
import CoreML

// The Object Selection tool's Object Finder: Segment Anything 2.1 (Apple's Core ML
// conversion, `ModelCatalog`'s "object_prompt" entries, Large before Tiny) answers a click or a
// box with the object's mask, and Shift- and ⌥-clicks refine it with include and exclude points,
// as SAM's prompts do. The image encoder runs once per document state on the whole canvas
// squashed to 1024², and its outputs are kept for the next prompt on the same image; the prompt
// encoder and mask decoder stay loaded. The decoder's 256² low-res mask that honours the prompt's
// points (the largest for a single click, else the best-scored; `decode`) is read as logits over
// the whole canvas by `ModelMaskRun`, upsampled bilinearly and thresholded at 0 as SAM's own
// post-processing does (its low-confidence background haze is negative logits), and the edge
// stage makes that hard mask's edge soft.
//
// A PROMPT stays live while the selection is the one its last step made (`livePrompt`): a refining
// click then replaces that step's result with the base selection combined with the new mask, by
// the prompt's own mode and feather, as one "Object Selection" step. Undo, or any other selection
// change, ends it.

/// One prompt point in canvas pixels: include (Shift-click, label 1) or exclude (⌥-click, 0).
struct ObjectFinderPoint: Equatable {
    let x: Double
    let y: Double
    let include: Bool
}

/// A live prompt: the selection it started from, how its mask combines with it, and the prompt
/// so far; `result` is the selection history its last step left.
final class ObjectFinderPrompt {
    let base: CanvasSelection?
    let mode: SelectionCombineMode
    let feather: Double
    let points: [ObjectFinderPoint]
    let box: CGRect?
    let result: SelectionHistoryState

    init(base: CanvasSelection?, mode: SelectionCombineMode, feather: Double, points: [ObjectFinderPoint],
         box: CGRect?, result: SelectionHistoryState) {
        self.base = base
        self.mode = mode
        self.feather = feather
        self.points = points
        self.box = box
        self.result = result
    }
}

enum ObjectFinder {
    /// The most prompt tokens SAM 2.1's Core ML decoder takes: its sparse embedding is enumerated
    /// at 2…15 tokens, and the prompt encoder always appends one padding point, so 14 — the
    /// points plus a box's two corners.
    static let maximumPoints = 14
    static let noObject = "No object was found at the prompt."
    static let noPrompt = "continue needs the selection the previous object_finder_select made."
    static let badInterface = "The Object Finder model has an unexpected interface."
    static let notePath = "Object Selection tool ▸ Object Finder"
    /// SAM's input side: the canvas is squashed to 1024² for the encoder, so prompt coordinates
    /// scale by 1024 over each canvas side.
    static let inputSide = 1024.0

    private static let prompts = NSMapTable<ImageDocument, ObjectFinderPrompt>.weakToStrongObjects()

    /// The image encoder's outputs for one document state (`doc` identity) and model version.
    private final class Embedding {
        weak var doc: RasterDocument?
        let model: String
        let features: [String: MLFeatureValue]

        init(doc: RasterDocument, model: String, features: [String: MLFeatureValue]) {
            self.doc = doc
            self.model = model
            self.features = features
        }
    }

    /// The prompt encoder and the mask decoder of one model version, kept loaded.
    private final class Decoders {
        let model: String
        let prompt: MLModel
        let decoder: MLModel

        init(model: String, prompt: MLModel, decoder: MLModel) {
            self.model = model
            self.prompt = prompt
            self.decoder = decoder
        }
    }

    /// Main thread only: read before a run and written after it, never from the worker.
    private static var embedding: Embedding?
    private static var decoders: Decoders?

    /// Whether `document` has a prompt that a refining click continues.
    static func livePrompt(_ document: ImageDocument) -> Bool { prompt(document) != nil }

    private static func prompt(_ document: ImageDocument) -> ObjectFinderPrompt? {
        guard let prompt = prompts.object(forKey: document), document.selectionHistory == prompt.result else {
            return nil
        }
        return prompt
    }

    /// Runs the prompt (`points` and/or `box`, canvas pixels) and commits the object's selection
    /// as one "Object Selection" step recording `record`. A new prompt combines with the current
    /// selection by `mode` after `feather`; `continuing` replaces the live prompt's last result
    /// with its base combined with the new mask by the prompt's own mode and feather (`mode` and
    /// `feather` are ignored). Answers the decoder's score for the mask used and the model.
    /// Throws `noPrompt`, `ModelGate.notInstalled("object_prompt")`, `badInterface`, `noObject`.
    static func select(_ capture: SelectionCapture, in document: ImageDocument, points: [ObjectFinderPoint],
                       box: CGRect?, mode: SelectionCombineMode, feather: Double, continuing: Bool,
                       record: [ActionStep], refine: Bool = true) throws -> (score: Double, entry: ModelCatalogEntry) {
        guard !points.isEmpty || box != nil else {
            throw DoctoredCoreError(message: "Give at least one point or a box.")
        }
        guard points.count + (box == nil ? 0 : 2) <= maximumPoints else {
            throw DoctoredCoreError(message: "The Object Finder takes at most \(maximumPoints) points, a box counting as two.")
        }
        let previous = continuing ? prompt(document) : nil
        if continuing && previous == nil { throw DoctoredCoreError(message: noPrompt) }
        guard let model = ModelGate.ready("object_prompt") else {
            throw DoctoredCoreError(message: ModelGate.notInstalled("object_prompt"))
        }
        let entry = model.entry
        let urls = entry.packages.compactMap { ModelStore.shared.compiledModelURL(entry.id, package: $0) }
        guard urls.count == 3 else { throw DoctoredCoreError(message: badInterface) }
        let width = Double(capture.doc.width), height = Double(capture.doc.height)
        var coordinates: [Float] = [], labels: [Float] = []
        for point in points {
            coordinates += [Float(point.x * inputSide / width), Float(point.y * inputSide / height)]
            labels.append(point.include ? 1 : 0)
        }
        if let box {
            coordinates += [Float(box.minX * inputSide / width), Float(box.minY * inputSide / height),
                            Float(box.maxX * inputSide / width), Float(box.maxY * inputSide / height)]
            labels += [2, 3]
        }
        let key = entry.id + "@" + entry.version
        let cached = embedding.flatMap { $0.doc === capture.doc && $0.model == key ? $0.features : nil }
        let loaded = decoders.flatMap { $0.model == key ? ($0.prompt, $0.decoder) : nil }
        // Written on the worker, read here after the job returns (the wait orders them).
        var encoded: [String: MLFeatureValue]?
        var kept: (MLModel, MLModel)?
        var score = 0.0
        let frame = ModelMaskFrame(rect: CGRect(x: 0, y: 0, width: width, height: height),
                                   request: ["values": "logit", "channels": [0]], clip: nil)
        let found = try ModelMaskRun.coverage(capture, in: document, title: "Object Selection", entry: entry,
                                              frames: [frame], refine: refine) { image in
            let features = try cached ?? encode(image, encoder: urls[0])
            encoded = features
            let models = try loaded ?? (CoreMLRuntime.load(urls[1]), CoreMLRuntime.load(urls[2]))
            kept = models
            let decoded = try decode(features, prompt: models.0, decoder: models.1,
                                     coordinates: coordinates, labels: labels)
            score = decoded.score
            return decoded.plane
        }
        if let encoded { embedding = Embedding(doc: capture.doc, model: key, features: encoded) }
        if let kept { decoders = Decoders(model: key, prompt: kept.0, decoder: kept.1) }
        guard found.covered > 0 else { throw DoctoredCoreError(message: noObject) }
        let next: ObjectFinderPrompt
        if let previous {
            try capture.requireCurrent(in: document)
            let pixels = UInt64(capture.doc.width * capture.doc.height)
            _ = try RasterResources.shared.admit(scratch: pixels * 8 + CanvasSelection.contourWorkingBytes)
            guard var incoming = CanvasSelection(shape: found.shape, canvasWidth: capture.doc.width,
                                                 canvasHeight: capture.doc.height) else {
                throw DoctoredCoreError(message: noObject)
            }
            if previous.feather > 0 {
                guard let softened = try incoming.nativeModified("feather", radius: previous.feather) else {
                    throw DoctoredCoreError(message: noObject)
                }
                incoming = softened
                try capture.requireCurrent(in: document)
            }
            let combined = CanvasSelection.combine(previous.base, with: incoming, mode: previous.mode)
            document.setCanvasSelection(combined, actionName: "Object Selection", record: record)
            next = ObjectFinderPrompt(base: previous.base, mode: previous.mode, feather: previous.feather,
                                      points: points, box: box, result: document.selectionHistory)
        } else {
            let base = capture.selection.selection
            try capture.commit(found.shape, in: document, name: "Object Selection", mode: mode,
                               feather: feather, record: record)
            next = ObjectFinderPrompt(base: base, mode: mode, feather: feather, points: points, box: box,
                                      result: document.selectionHistory)
        }
        prompts.setObject(next, forKey: document)
        return (score, entry)
    }

    /// The live prompt's points and box, for a refining click to extend.
    static func currentPrompt(_ document: ImageDocument) -> (points: [ObjectFinderPoint], box: CGRect?)? {
        prompt(document).map { ($0.points, $0.box) }
    }

    /// The image encoder (`encoder`, loaded for this run and released after it) on the canvas
    /// squashed to its input: `image_embedding`, `feats_s0` and `feats_s1`.
    private static func encode(_ image: CGImage, encoder: URL) throws -> [String: MLFeatureValue] {
        let model = try CoreMLRuntime.load(encoder)
        let input = try CoreMLRuntime.imageInput(image, for: model)
        let output = try model.prediction(from: MLDictionaryFeatureProvider(dictionary: [input.name: input.value]))
        var features: [String: MLFeatureValue] = [:]
        for name in ["image_embedding", "feats_s0", "feats_s1"] {
            guard let value = output.featureValue(for: name) else { throw DoctoredCoreError(message: badInterface) }
            features[name] = value
        }
        return features
    }

    /// The prompt encoder and the mask decoder over `features`: among the decoder's low-res masks
    /// (logits) that honour the prompt's points, the largest for a fresh single click and the
    /// best-scored otherwise, as one channel, and its score.
    private static func decode(_ features: [String: MLFeatureValue], prompt: MLModel, decoder: MLModel,
                               coordinates: [Float], labels: [Float]) throws -> (plane: ModelPlane, score: Double) {
        let count = labels.count
        let points = try MLMultiArray(shape: [1, NSNumber(value: count), 2], dataType: .float16)
        let kinds = try MLMultiArray(shape: [1, NSNumber(value: count)], dataType: .float16)
        for index in 0..<count {
            points[[0, NSNumber(value: index), 0]] = NSNumber(value: coordinates[2 * index])
            points[[0, NSNumber(value: index), 1]] = NSNumber(value: coordinates[2 * index + 1])
            kinds[[0, NSNumber(value: index)]] = NSNumber(value: labels[index])
        }
        let embedded = try prompt.prediction(from: MLDictionaryFeatureProvider(dictionary: [
            "points": MLFeatureValue(multiArray: points), "labels": MLFeatureValue(multiArray: kinds),
        ]))
        guard let sparse = embedded.featureValue(for: "sparse_embeddings"),
              let dense = embedded.featureValue(for: "dense_embeddings"),
              let image = features["image_embedding"], let s0 = features["feats_s0"],
              let s1 = features["feats_s1"] else { throw DoctoredCoreError(message: badInterface) }
        let decoded = try decoder.prediction(from: MLDictionaryFeatureProvider(dictionary: [
            "image_embedding": image, "sparse_embedding": sparse, "dense_embedding": dense,
            "feats_s0": s0, "feats_s1": s1,
        ]))
        guard let masks = decoded.featureValue(for: "low_res_masks"),
              let scores = decoded.featureValue(for: "scores")?.multiArrayValue else {
            throw DoctoredCoreError(message: badInterface)
        }
        let plane = try CoreMLRuntime.plane(masks)
        let values = (0..<scores.count).map { scores[$0].doubleValue }
        guard plane.channels == values.count, !values.isEmpty else { throw DoctoredCoreError(message: badInterface) }
        let cell = plane.width * plane.height
        // A mask that contradicts the prompt's own points — an include point outside it or an
        // exclude point inside it (its logit's sign at the point's cell) — answers a different
        // prompt, however well the decoder scores it: the conversion exports only the three
        // multimask outputs, which SAM ranks for an AMBIGUOUS single click, so a box with an
        // exclude point can score the mask that keeps the excluded part highest (measured on
        // product.jpg, 2026-10-14: 0.856 with the body cap kept, 0.775 without it). The best
        // score among the masks that honour every point wins; with none, the best score.
        // A fresh SINGLE include click (labels == [1], no box) is the exception: the three
        // multimask outputs are SAM's part / sub-object / whole-object granularities, and the
        // decoder scores the part highest — on product.jpg a click at the camera's centre scores
        // the body cap 0.882 at 10.4 % of the plane, the half 0.615 at 19.5 %, and the whole camera
        // only 0.247 at 53.0 % (scratch sam-explore-1, 2026-10-14). The Object Finder
        // selects the object under a click, so that prompt takes the honouring mask with the
        // LARGEST area (its count of positive logits), with no score floor, since the right mask
        // here scores 0.247. Refining points and boxes keep the best-score rule.
        let singleClick = labels == [1]
        let honours = { (channel: Int) -> Bool in
            labels.indices.allSatisfy { index in
                guard labels[index] == 0 || labels[index] == 1 else { return true }
                let x = min(plane.width - 1, max(0, Int(Double(coordinates[2 * index]) / inputSide * Double(plane.width))))
                let y = min(plane.height - 1, max(0, Int(Double(coordinates[2 * index + 1]) / inputSide * Double(plane.height))))
                let logit = plane.samples[channel * cell + y * plane.width + x]
                return labels[index] == 1 ? logit > 0 : logit < 0
            }
        }
        let candidates = values.indices.filter(honours)
        let pool = candidates.isEmpty ? Array(values.indices) : candidates
        let area = { (channel: Int) -> Int in
            plane.samples[channel * cell..<(channel + 1) * cell].reduce(0) { $1 > 0 ? $0 + 1 : $0 }
        }
        let chosen = singleClick && !candidates.isEmpty
            ? candidates.max(by: { area($0) < area($1) })
            : pool.max(by: { values[$0] < values[$1] })
        guard let best = chosen else { throw DoctoredCoreError(message: badInterface) }
        let samples = Array(plane.samples[best * cell..<(best + 1) * cell])
        return (ModelPlane(width: plane.width, height: plane.height, channels: 1, samples: samples), values[best])
    }
}
