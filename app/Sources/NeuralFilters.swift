import Accelerate
import CoreML
import Foundation

// Filter ▸ Neural Filters' shared runner (M4): Super Zoom (SuperZoom.swift) and JPEG
// Artifacts Removal (JpegArtifactsRemoval.swift) run an image model tile by tile inside the
// core's stage (`neural_stage.rs`, `DoctoredCore+Neural.swift`), which cuts the tiles, stitches the
// cores and runs the classical Noise Reduction and Sharpen. Core ML runs only on the worker that
// called the stage, through `NeuralTileModel`, loaded once per run or per sheet.

/// One loaded tile model: a `[1, 3, side, side]` `image` input (plus a `[1, 1]` `qf` input when
/// `qualityFactor` is set: FBCNN's 1 − QF/100) and one `[1, outputChannels, outputSide,
/// outputSide]` output (3, RGB, for the tiled filters; 2, Lab a* and b*, for Colorize's DDColor).
/// Worker threads only.
final class NeuralTileModel {
    private let model: MLModel
    let side: Int
    let outputSide: Int
    let qualityFactor: Int?
    let outputChannels: Int
    private let output: String
    private let quality: MLMultiArray?

    init(compiled: URL, side: Int, outputSide: Int, qualityFactor: Int?, outputChannels: Int = 3) throws {
        model = try CoreMLRuntime.load(compiled)
        self.side = side
        self.outputSide = outputSide
        self.qualityFactor = qualityFactor
        self.outputChannels = outputChannels
        let shape = { (values: [Int]) in values.map { NSNumber(value: $0) } }
        let inputs = model.modelDescription.inputDescriptionsByName
        guard inputs["image"]?.multiArrayConstraint?.shape == shape([1, 3, side, side]) else {
            throw DoctoredCoreError(message: "The model does not take a \(side)² RGB image plane.")
        }
        if let qualityFactor {
            guard inputs["qf"]?.multiArrayConstraint?.shape == shape([1, 1]) else {
                throw DoctoredCoreError(message: "The model does not take a quality factor.")
            }
            let array = try MLMultiArray(shape: shape([1, 1]), dataType: .float32)
            array[0] = NSNumber(value: Float(1 - Double(qualityFactor) / 100))
            quality = array
        } else {
            quality = nil
        }
        guard let name = model.modelDescription.outputDescriptionsByName.keys.sorted().first else {
            throw DoctoredCoreError(message: "The model has no output.")
        }
        output = name
    }

    /// One tile: `input` (`3·side²`, channel-major RGB 0…1) in, the model's plane written into
    /// `into` (`outputChannels·outputSide²`), shape-checked.
    func predict(_ input: UnsafeBufferPointer<Float>, into: UnsafeMutableBufferPointer<Float>) throws {
        guard input.count == 3 * side * side, into.count == outputChannels * outputSide * outputSide,
              let base = input.baseAddress else {
            throw DoctoredCoreError(message: "The neural tile has the wrong size.")
        }
        let shape = [1, 3, side, side].map { NSNumber(value: $0) }
        let strides = [3 * side * side, side * side, side, 1].map { NSNumber(value: $0) }
        // The core's buffer, read in place: Core ML only reads an input.
        let image = try MLMultiArray(dataPointer: UnsafeMutableRawPointer(mutating: base), shape: shape,
                                     dataType: .float32, strides: strides, deallocator: nil)
        var features: [String: Any] = ["image": MLFeatureValue(multiArray: image)]
        if let quality { features["qf"] = MLFeatureValue(multiArray: quality) }
        let result = try model.prediction(from: try MLDictionaryFeatureProvider(dictionary: features))
        guard let array = result.featureValue(for: output)?.multiArrayValue else {
            throw DoctoredCoreError(message: "The model returned no image.")
        }
        let expected = [1, outputChannels, outputSide, outputSide]
        guard array.shape.map(\.intValue) == expected else {
            throw DoctoredCoreError(message: "The model's output is not a \(outputChannels)-channel "
                                  + "\(outputSide)² plane.")
        }
        try Self.copy(array, into: into)
    }

    /// The output array into `into`: a contiguous Float32 array is copied, a contiguous Float16
    /// one converted by vImage, anything else read through `CoreMLRuntime.plane`.
    private static func copy(_ array: MLMultiArray, into: UnsafeMutableBufferPointer<Float>) throws {
        let shape = array.shape.map(\.intValue)
        let contiguous = array.strides.map(\.intValue)
            == [shape[1] * shape[2] * shape[3], shape[2] * shape[3], shape[3], 1]
        guard let target = into.baseAddress else { return }
        if contiguous, array.dataType == .float32 {
            array.withUnsafeBytes { raw in
                if let source = raw.baseAddress { memcpy(target, source, into.count * 4) }
            }
            return
        }
        if contiguous, array.dataType == .float16 {
            var status = kvImageNoError
            array.withUnsafeBytes { raw in
                guard let source = raw.baseAddress else { return }
                let rows = vImagePixelCount(shape[1] * shape[2]), width = vImagePixelCount(shape[3])
                var from = vImage_Buffer(data: UnsafeMutableRawPointer(mutating: source), height: rows,
                                         width: width, rowBytes: shape[3] * 2)
                var to = vImage_Buffer(data: target, height: rows, width: width, rowBytes: shape[3] * 4)
                status = vImageConvert_Planar16FtoPlanarF(&from, &to, vImage_Flags(kvImageNoFlags))
            }
            guard status == kvImageNoError else {
                throw DoctoredCoreError(message: "The model's output could not be read.")
            }
            return
        }
        let plane = try CoreMLRuntime.plane(MLFeatureValue(multiArray: array))
        guard plane.samples.count == into.count else {
            throw DoctoredCoreError(message: "The model's output has the wrong size.")
        }
        _ = plane.samples.withUnsafeBufferPointer { memcpy(target, $0.baseAddress, into.count * 4) }
    }

    /// The core's predict callback over this model, polling `control` between tiles.
    func box(control: RasterJobControl?) -> NeuralPredictBox {
        NeuralPredictBox { [self] _, input, output in
            if control?.isCancelled == true { throw JobCancelled() }
            try autoreleasepool { try predict(input, into: output) }
        }
    }
}

/// A sheet's preview models, each loaded once on first use. Touched only on the sheet's
/// `PreviewRenderer` queue, which runs one render at a time.
final class NeuralPreviewModels {
    private var loaded: [URL: NeuralTileModel] = [:]

    func model(_ compiled: URL, side: Int, outputSide: Int, qualityFactor: Int?,
               outputChannels: Int = 3) throws -> NeuralTileModel {
        if let model = loaded[compiled], model.qualityFactor == qualityFactor,
           model.outputChannels == outputChannels { return model }
        let model = try NeuralTileModel(compiled: compiled, side: side, outputSide: outputSide,
                                        qualityFactor: qualityFactor, outputChannels: outputChannels)
        loaded[compiled] = model
        return model
    }
}

/// A neural filter's image at either depth.
enum NeuralImage {
    case byte(RasterImage)
    case wide(RasterPrecisionImage)

    var width: Int {
        switch self {
        case .byte(let image): return image.width
        case .wide(let image): return image.width
        }
    }
    var height: Int {
        switch self {
        case .byte(let image): return image.height
        case .wide(let image): return image.height
        }
    }
    var bytesPerSample: Int {
        switch self {
        case .byte: return 1
        case .wide: return 2
        }
    }
}

enum NeuralFilterRun {
    /// Real-ESRGAN's tile: 256² in, ×4 out, 16 px of context (core 224); the pad is a multiple of
    /// 8 as the core requires, and its receptive field's useful reach.
    static let superResolutionTile = 256
    static let superResolutionPad = 16
    static let superResolutionScale = 4
    /// FBCNN's tile: 512² at scale 1, 32 px of context (core 448), so every read origin sits on
    /// the JPEG block grid.
    static let jpegTile = 512
    static let jpegPad = 32
    /// DDColor's side: Colorize runs the model once at 512² on the layer's luminance.
    static let colorizeSide = 512
    /// The host's own term per OUTPUT pixel beside the core's plan and the models'
    /// `inferenceBytes`, at 8 and at 16 bits: the new document's copy of the zoomed pixels and
    /// its display preparation (Super Zoom), the publication's display (JPEG); the typed core plan
    /// already carries the 16-bit copies, so its host share is the smaller. Measured
    /// (`scripts/neural-filters-admission/measure.py`, Release, 6000×4000, 3 runs + twins,
    /// `MallocLargeCache=0`): at the one place-holder 8, zoom8 (×2 with Remove JPEG Artifacts)
    /// missed (P 1,568,133,120, max E 2,699,233,176, need 3,374,041,470), zoom16 missed
    /// (P 3,104,133,120, need 3,619,289,830) and jpeg8 missed (P 505,055,232, need 794,460,790)
    /// while jpeg16 and both slider cases passed. One value of 27 (the least clearing zoom8)
    /// cleared every 8-bit case in a second measure (zoom8 P 3,392,133,120 ≥ need 3,372,484,990)
    /// but planned sliders16 at 8.37 GB, past the 8 GiB working limit of the minimum Mac, for a
    /// measured 4.49 GB peak, so the term is per depth: 27 at 8 bits, and at 16 bits 14, the
    /// least whole value clearing zoom16 (need 3,619,289,830; P 3,680,133,120).
    static let hostBytesPerOutputPixel8: UInt64 = 27
    static let hostBytesPerOutputPixel16: UInt64 = 14

    /// The host term at `bytesPerSample` (1 or 2).
    static func hostBytesPerOutputPixel(bytesPerSample: Int) -> UInt64 {
        bytesPerSample == 2 ? hostBytesPerOutputPixel16 : hostBytesPerOutputPixel8
    }

    /// The Real-ESRGAN stage at `zoom` (2 or 4): the model when `enhance`, else the classical
    /// resample alone, with the classical sliders.
    static func superResolutionStage(zoom: Int, enhance: Bool, noiseReduction: Double,
                                     sharpen: Double) -> [String: Any] {
        ["tile": superResolutionTile, "pad": superResolutionPad, "scale": zoom,
         "model_scale": superResolutionScale, "model": enhance,
         "noise_reduction": noiseReduction, "sharpen": sharpen]
    }

    /// The FBCNN stage at scale 1.
    static func jpegStage() -> [String: Any] {
        ["tile": jpegTile, "pad": jpegPad, "scale": 1, "model_scale": 1, "model": true]
    }

    /// Why `name` cannot run on `doc`, nil when it can: a 32-bit or CMYK document
    /// (Sky Replacement's precedent: no take, so both refuse by name).
    static func refusal(_ doc: RasterDocument, name: String) -> String? {
        if doc.isFloat { return "\(name) is not available in a 32-bit document" }
        if doc.isCmyk { return "\(name) is not available in a CMYK document" }
        return nil
    }

    /// The model's input on `document` (#547 Lab phase 6b-iii): a Lab document's image through
    /// its Display P3 view (`labDisplay`, the rule Remove's model reads), so the model sees colour
    /// and the core returns its result to L*a*b* codes by the Mode rule (`lab_neural`); on any
    /// other document `image` itself. `control` is single-use: pass a child of the run's control.
    static func modelInput(_ image: RasterPrecisionImage, document: RasterDocument, limit: UInt64,
                           other: UInt64, control: RasterJobControl?) throws -> RasterPrecisionImage {
        guard document.isLab else { return image }
        return try image.labDisplay(workingLimit: limit, otherRetained: other, control: control)
    }

    /// The profile a model's input and output are in on `doc`: the built-in Display P3 on a Lab
    /// document (its view's space; the twin is no matrix/TRC profile), else the document's own.
    static func modelProfile(_ doc: RasterDocument) -> Data {
        doc.isLab ? RasterProfile.builtin(.displayP3) : doc.iccProfile ?? Data()
    }

    /// The Lab leg's plan term on `doc` (`dr_lab_neural_scratch_bytes`): the view of an `input`
    /// image and the Lab image of an `output` result; 0 off Lab.
    static func labTerm(_ doc: RasterDocument, input: (Int, Int), output: (Int, Int)) -> UInt64 {
        guard doc.isLab else { return 0 }
        return RasterPrecisionDocument.labNeuralScratchBytes(
            inputWidth: input.0, inputHeight: input.1, outputWidth: output.0, outputHeight: output.1)
    }

    /// `plan` plus `term`, saturating (a failed `labTerm` is `UInt64.max`, which then refuses).
    static func plus(_ plan: UInt64, _ term: UInt64) -> UInt64 {
        let (sum, overflow) = plan.addingReportingOverflow(term)
        return overflow ? .max : sum
    }

    /// Argument `key` of `a` as a finite number (a string form too), nil when absent.
    static func number(_ a: [String: Any], _ key: String) throws -> Double? {
        guard let value = a[key] else { return nil }
        let parsed: Double?
        switch value {
        case let text as String: parsed = Double(text.trimmingCharacters(in: .whitespaces))
        case let number as NSNumber where CFGetTypeID(number) != CFBooleanGetTypeID(): parsed = number.doubleValue
        default: parsed = nil
        }
        guard let parsed, parsed.isFinite else { throw DoctoredCoreError(message: "\(key) must be a number.") }
        return parsed
    }

    /// Argument `key` of `a` as a boolean ("true"/"false" too), nil when absent.
    static func flag(_ a: [String: Any], _ key: String) throws -> Bool? {
        guard let value = a[key] else { return nil }
        if let text = value as? String, text == "true" || text == "false" { return text == "true" }
        guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else {
            throw DoctoredCoreError(message: "\(key) must be true or false.")
        }
        return number.boolValue
    }

    /// The working-memory context of `doc`'s jobs, measured on main.
    static func budget(_ doc: RasterDocument) throws -> (limit: UInt64, other: UInt64) {
        if let precision = doc.precision {
            let context = try PrecisionWorkContext(source: precision)
            return (context.limit, context.otherRetained)
        }
        return try doc.legacyPrecisionWorkContext()
    }

    /// The core's figures for `stage` over `image`.
    static func info(_ stage: [String: Any], width: Int, height: Int, bytesPerSample: Int) throws
        -> NeuralStageInfo {
        try RasterNeural.stageInfo(width: width, height: height, stage: stage, bytesPerSample: bytesPerSample)
    }

    /// One stage over `image` through `model` (nil: no model, the classical stage); the input
    /// itself when the stage changes nothing. Worker thread only.
    static func run(_ stage: [String: Any], model: NeuralTileModel?, image: NeuralImage, limit: UInt64,
                    other: UInt64, control: RasterJobControl?) throws -> NeuralImage {
        let box = model?.box(control: control)
        switch image {
        case .byte(let source):
            let out = try source.neuralStage(stage, model: box, limit: limit, other: other, control: control)
            return out.map(NeuralImage.byte) ?? image
        case .wide(let source):
            let out = try source.neuralStage(stage, model: box, limit: limit, other: other, control: control)
            return out.map(NeuralImage.wide) ?? image
        }
    }
}
