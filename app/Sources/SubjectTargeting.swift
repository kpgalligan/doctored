import AppKit
import Vision
import CoreVideo

/// The platform-model seam. Object identity comes from foreground instances;
/// individual people from Vision's person instances over the dedicated
/// person segmenter (`PersonInstances`), and All People is that segmenter's
/// one aggregate. Vision work has a responsive, noncancellable final-operation
/// progress panel. `capture.source` is `ImageDocument.analysisComposite(of:)`,
/// which says what Vision sees at float.
enum SubjectTargeting {
    static let noPeopleMessage = "No people were found in this image."
    static let noFaceMessage = "No face was found in this image."

    static func scratchBytes(_ capture: SelectionCapture) -> UInt64 {
        UInt64(capture.doc.width * capture.doc.height) * 12 + 32 * 1024 * 1024
    }

    static func analyze(_ capture: SelectionCapture, model: SubjectModel = .objects,
                        in document: ImageDocument) throws -> SubjectAnalysis {
        try capture.requireCurrent(in: document)
        guard let source = capture.source, let image = source.makeCGImage(in: capture.doc.colorSpace) else {
            throw DoctoredCoreError(message: SubjectSelection.Failure.noComposite.message)
        }
        let scratch = scratchBytes(capture)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let task = try WorkerTask(scratchEstimate: scratch) {
            do { return try SubjectAnalysis(image: image, model: model) }
            catch let error as SubjectSelection.Failure { throw DoctoredCoreError(message: error.message) }
        }
        let analysis = try BackgroundWork.wait(task, title: model.findTitle)
        try capture.requireCurrent(in: document)
        return analysis
    }

    static func mask(_ analysis: SubjectAnalysis, instances: [Int], capture: SelectionCapture,
                     in document: ImageDocument) throws -> [UInt8] {
        try capture.requireCurrent(in: document)
        guard !instances.isEmpty, instances.allSatisfy({ (1...analysis.instanceCount).contains($0) }) else {
            throw DoctoredCoreError(message: analysis.model.chooseRefusal)
        }
        let scratch = scratchBytes(capture)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let task = try WorkerTask(scratchEstimate: scratch) {
            guard let mask = analysis.mask(subjects: instances) else { throw DoctoredCoreError(message: SubjectSelection.Failure.unreadableMask.message) }
            return mask
        }
        let result = try BackgroundWork.wait(task, title: analysis.model.maskTitle)
        try capture.requireCurrent(in: document)
        return result
    }

    static func people(_ capture: SelectionCapture, in document: ImageDocument) throws -> [UInt8] {
        try capture.requireCurrent(in: document)
        guard let source = capture.source, let image = source.makeCGImage(in: capture.doc.colorSpace) else {
            throw DoctoredCoreError(message: SubjectSelection.Failure.noComposite.message)
        }
        let scratch = scratchBytes(capture)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let task = try WorkerTask(scratchEstimate: scratch) { try people(in: image) }
        let mask = try BackgroundWork.wait(task, title: "Select All People")
        try capture.requireCurrent(in: document)
        return mask
    }

    static func people(in image: CGImage) throws -> [UInt8] {
        let requests = personRequests()
        try VNImageRequestHandler(cgImage: image, options: [:])
            .perform([requests.aggregate, requests.humans, requests.faces])
        return try aggregateCoverage(requests, width: image.width, height: image.height)
    }

    /// All People's three requests, shared with `PersonInstances.analyze`,
    /// which performs them beside its own.
    static func personRequests() -> (aggregate: VNGeneratePersonSegmentationRequest,
                                     humans: VNDetectHumanRectanglesRequest, faces: VNDetectFaceRectanglesRequest) {
        let request = VNGeneratePersonSegmentationRequest()
        request.qualityLevel = .accurate
        request.outputPixelFormat = kCVPixelFormatType_OneComponent8
        let humans = VNDetectHumanRectanglesRequest()
        humans.upperBodyOnly = false
        let faces = VNDetectFaceRectanglesRequest()
        return (request, humans, faces)
    }

    /// The performed `personRequests()` as canvas-sized coverage, after the
    /// detection gate.
    static func aggregateCoverage(_ requests: (aggregate: VNGeneratePersonSegmentationRequest,
                                               humans: VNDetectHumanRectanglesRequest,
                                               faces: VNDetectFaceRectanglesRequest),
                                  width: Int, height: Int) throws -> [UInt8] {
        // The segmenter answers some mask for any image — a cat or a product
        // shot included — so a person must first be DETECTED: by a body or
        // by a face, since a close portrait may show no body at all.
        guard requests.humans.results?.isEmpty == false || requests.faces.results?.isEmpty == false else {
            throw DoctoredCoreError(message: noPeopleMessage)
        }
        guard let buffer = requests.aggregate.results?.first?.pixelBuffer,
              let mask = SubjectSelection.coverage(buffer, width: width, height: height) else {
            throw DoctoredCoreError(message: SubjectSelection.Failure.unreadableMask.message)
        }
        guard mask.contains(where: { $0 > 0 }) else {
            throw DoctoredCoreError(message: noPeopleMessage)
        }
        return mask
    }
}

extension SubjectTargeting {
    /// Select ▸ Face Parts…'s analysis: every face and its landmarks in the
    /// captured composite, on the final-operation worker.
    static func faces(_ capture: SelectionCapture, in document: ImageDocument) throws -> FaceAnalysis {
        try capture.requireCurrent(in: document)
        guard let source = capture.source, let image = source.makeCGImage(in: capture.doc.colorSpace) else {
            throw DoctoredCoreError(message: SubjectSelection.Failure.noComposite.message)
        }
        let scratch = scratchBytes(capture)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let task = try WorkerTask(scratchEstimate: scratch) { try FaceAnalysis(image: image) }
        let analysis = try BackgroundWork.wait(task, title: "Find Faces")
        try capture.requireCurrent(in: document)
        return analysis
    }

    /// The u16 coverage of `parts` of the 1-based `faces`, rasterized by the
    /// core (`RasterLandmarkCore`) on the final-operation worker.
    static func faceMask(_ analysis: FaceAnalysis, parts: [FacePart], faces: [Int],
                         capture: SelectionCapture, in document: ImageDocument) throws -> [UInt16] {
        try capture.requireCurrent(in: document)
        guard analysis.width == capture.doc.width, analysis.height == capture.doc.height else {
            throw DoctoredCoreError(message: "The image changed size. Reopen the selection command and try again.")
        }
        let json = try analysis.regionsJSON(parts: parts, faces: faces)
        let width = analysis.width, height = analysis.height
        let scratch = FaceParts.coverageScratch(width: width, height: height)
        _ = try RasterResources.shared.admit(scratch: scratch)
        // Measured before the task registers its own estimate as transient.
        let context = try PrecisionWorkContext()
        let task = try WorkerTask(scratchEstimate: scratch) {
            try RasterLandmarkCore.coverage(json, width: width, height: height, limit: context.limit,
                                            other: context.otherRetained, control: nil)
        }
        let mask = try BackgroundWork.wait(task, title: "Select Face Parts")
        try capture.requireCurrent(in: document)
        return mask
    }
}
