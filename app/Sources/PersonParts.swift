import AppKit

// Select ▸ Person Parts…: Hair, Facial Skin, Body Skin and Clothes from MediaPipe's
// multiclass selfie segmenter (`ModelCatalog`'s "person_parts" entry, converted by
// `scripts/convert_mediapipe_multiclass.py`), on the people Vision separates (`SubjectAnalysis`
// with `.people`, `PersonInstances.swift`). Each chosen person runs the model on their own box, so
// a small figure is seen at the model's full 256 px rather than as a speck of a whole-picture
// squash, and the person's own coverage clips the parts, so one person's hair never selects their
// neighbour's.

/// The parts the model labels. The raw value is the MCP spelling.
enum PersonPart: String, CaseIterable {
    case hair
    case faceSkin = "face_skin"
    case bodySkin = "body_skin"
    case clothes

    var title: String {
        switch self {
        case .hair: return "Hair"
        case .faceSkin: return "Facial Skin"
        case .bodySkin: return "Body Skin"
        case .clothes: return "Clothes"
        }
    }

    /// The model's class index: 0 background, 1 hair, 2 body-skin, 3 face-skin, 4 clothes,
    /// 5 others — the label map the conversion script asserts.
    var channel: Int {
        switch self {
        case .hair: return 1
        case .bodySkin: return 2
        case .faceSkin: return 3
        case .clothes: return 4
        }
    }
}

enum PersonPartsRun {
    static let noParts = "None of the chosen parts was found on the chosen people."
    static let menuPath = "Select ▸ Person Parts…"
    /// A person's box grows by this share of its longer side on every side before the model sees
    /// it: Vision's instance map is coarse, and hair and loose clothes reach past it.
    static let margin = 0.1

    /// The refined coverage of `parts` on the 1-based `people` (`inspect_subjects` model
    /// "people" numbering; nil = everyone), how many people Vision separated (0 when it could
    /// not and every person was read at once), and the model. With `people` nil a crowd Vision
    /// cannot separate falls back to one whole-canvas frame clipped by All People; with
    /// `people` given it refuses `PersonInstances.crowdMessage`. Throws
    /// `ModelGate.notInstalled("person_parts")`, Vision's "no people", and `noParts`. `analysis`
    /// is a people analysis of this capture already made (the sheet's), so Vision runs once.
    static func shape(_ capture: SelectionCapture, in document: ImageDocument, parts: [PersonPart],
                      people: [Int]?, analysis known: SubjectAnalysis? = nil, refine: Bool = true) throws
        -> (shape: CanvasSelection.Shape, personCount: Int, entry: ModelCatalogEntry) {
        guard let model = ModelGate.ready("person_parts") else {
            throw DoctoredCoreError(message: ModelGate.notInstalled("person_parts"))
        }
        guard !parts.isEmpty else { throw DoctoredCoreError(message: "Choose at least one part.") }
        let width = capture.doc.width, height = capture.doc.height
        let canvas = CGRect(x: 0, y: 0, width: width, height: height)
        let request: [String: Any] = ["values": "probability", "channels": parts.map(\.channel).sorted()]
        var frames: [ModelMaskFrame] = []
        var count = 0
        do {
            let analysis = try known ?? SubjectTargeting.analyze(capture, model: .people, in: document)
            count = analysis.instanceCount
            let chosen = people ?? Array(1...max(1, count))
            guard count > 0, !chosen.isEmpty, chosen.allSatisfy({ (1...count).contains($0) }) else {
                throw DoctoredCoreError(message: "people must be person numbers from 1 to \(count).")
            }
            let bounds = analysis.instanceBounds
            for person in Array(Set(chosen)).sorted() {
                let box = bounds[person - 1]
                let grow = margin * Double(max(box.width, box.height))
                let rect = box.insetBy(dx: -grow, dy: -grow).intersection(canvas).integral
                guard !rect.isNull, rect.width >= 1, rect.height >= 1 else { continue }
                let clip = try SubjectTargeting.mask(analysis, instances: [person], capture: capture, in: document)
                frames.append(ModelMaskFrame(rect: rect, request: request, clip: clip))
            }
        } catch let error as DoctoredCoreError where error.message == PersonInstances.crowdMessage && people == nil {
            let everyone = try SubjectTargeting.people(capture, in: document)
            frames = [ModelMaskFrame(rect: canvas, request: request, clip: everyone)]
            count = 0
        }
        guard !frames.isEmpty else { throw DoctoredCoreError(message: noParts) }
        let compiled = model.compiled
        let found = try ModelMaskRun.coverage(capture, in: document, title: "Select Person Parts",
                                              entry: model.entry, frames: frames, refine: refine) { image in
            try ModelMaskRun.predictImage(image, compiled: compiled, output: "classes")
        }
        guard found.covered > 0 else { throw DoctoredCoreError(message: noParts) }
        return (found.shape, count, model.entry)
    }
}
