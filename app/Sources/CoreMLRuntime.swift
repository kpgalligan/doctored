import CoreML
import CoreVideo
import Vision

// The Core ML runtime adapter: loads a compiled model from the store
// (`ModelStore`), feeds it a host-prepared image and reads its outputs as `ModelPlane`s. Worker
// thread only — `DepthBlurRun.capture` and `ModelMaskRun.coverage` call it inside their
// `BackgroundWork` jobs, admitted at the catalogue entry's `inferenceBytes`. `CoreMLRuntime` is
// the one loader and reader; `DepthEstimator` and the selection models are thin callers.

/// One depth map as the model emits it: `width × height` samples, row 0 at the top, larger =
/// nearer (relative inverse depth).
struct DepthMap {
    let width: Int
    let height: Int
    let samples: [Float]
}

enum CoreMLRuntime {
    /// The compiled model at `compiled`, on every compute unit: the F16 packages are built for
    /// the Neural Engine, and Core ML falls back to the GPU or the CPU by itself where a layer
    /// cannot run there, so no unit is excluded.
    static func load(_ compiled: URL) throws -> MLModel {
        let configuration = MLModelConfiguration()
        configuration.computeUnits = .all
        do { return try MLModel(contentsOf: compiled, configuration: configuration) } catch {
            throw DoctoredCoreError(message: "The model could not be loaded: \(error.localizedDescription)")
        }
    }

    /// The model's image input, named, holding `image` scaled to the input's own size and format
    /// (`MLFeatureValue(cgImage:constraint:options:)`, scale-fill: the whole image squashed, so a
    /// plane read back covers the whole of it). The packages carry their own normalisation, so
    /// the host hands them plain RGB.
    static func imageInput(_ image: CGImage, for model: MLModel) throws -> (name: String, value: MLFeatureValue) {
        guard let (name, input) = model.modelDescription.inputDescriptionsByName
            .first(where: { $0.value.type == .image }),
              let constraint = input.imageConstraint else {
            throw DoctoredCoreError(message: "The model takes no image input.")
        }
        let value = try MLFeatureValue(
            cgImage: image, constraint: constraint,
            options: [.cropAndScale: VNImageCropAndScaleOption.scaleFill.rawValue])
        return (name, value)
    }

    /// An output as planes: a one-component pixel buffer (OneComponent16Half / 32Float / 8, the
    /// raw value) is one channel; a multi-array (Float16 / Float32 / Double) shaped `[1,…,]C,H,W`
    /// or `[1,…,]H,W` is `C` channels or one.
    static func plane(_ value: MLFeatureValue) throws -> ModelPlane {
        if let buffer = value.imageBufferValue { return try read(buffer) }
        if let array = value.multiArrayValue { return try read(array) }
        throw DoctoredCoreError(message: "The model's output is neither an image nor an array.")
    }

    private static func read(_ buffer: CVPixelBuffer) throws -> ModelPlane {
        CVPixelBufferLockBaseAddress(buffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        let width = CVPixelBufferGetWidth(buffer), height = CVPixelBufferGetHeight(buffer)
        let stride = CVPixelBufferGetBytesPerRow(buffer)
        guard width > 0, height > 0, let base = CVPixelBufferGetBaseAddress(buffer) else {
            throw DoctoredCoreError(message: "The model returned an empty map.")
        }
        var samples = [Float](repeating: 0, count: width * height)
        let format = CVPixelBufferGetPixelFormatType(buffer)
        for y in 0..<height {
            let row = base.advanced(by: y * stride)
            for x in 0..<width {
                switch format {
                case kCVPixelFormatType_OneComponent16Half:
                    samples[y * width + x] = Float(row.assumingMemoryBound(to: Float16.self)[x])
                case kCVPixelFormatType_OneComponent32Float:
                    samples[y * width + x] = row.assumingMemoryBound(to: Float.self)[x]
                case kCVPixelFormatType_OneComponent8:
                    samples[y * width + x] = Float(row.assumingMemoryBound(to: UInt8.self)[x])
                default:
                    throw DoctoredCoreError(message: "The model's map has an unreadable pixel format.")
                }
            }
        }
        return ModelPlane(width: width, height: height, channels: 1, samples: samples)
    }

    private static func read(_ array: MLMultiArray) throws -> ModelPlane {
        let shape = array.shape.map(\.intValue)
        guard shape.count >= 2, let height = shape.dropLast().last, let width = shape.last,
              width > 0, height > 0 else {
            throw DoctoredCoreError(message: "The model's array is not a map.")
        }
        let channels = shape.count >= 3 ? shape[shape.count - 3] : 1
        guard channels > 0, shape.dropLast(3).allSatisfy({ $0 == 1 }) else {
            throw DoctoredCoreError(message: "The model's array is not one batch of maps.")
        }
        let strides = array.strides.map(\.intValue)
        let (rowStride, columnStride) = (strides[strides.count - 2], strides[strides.count - 1])
        let channelStride = shape.count >= 3 ? strides[strides.count - 3] : 0
        let dataType = array.dataType
        guard [.float16, .float32, .double].contains(dataType) else {
            throw DoctoredCoreError(message: "The model's array has an unreadable data type.")
        }
        let cell = width * height
        var samples = [Float](repeating: 0, count: channels * cell)
        array.withUnsafeBytes { raw in
            for c in 0..<channels {
                for y in 0..<height {
                    for x in 0..<width {
                        let offset = c * channelStride + y * rowStride + x * columnStride
                        let index = c * cell + y * width + x
                        switch dataType {
                        case .float16: samples[index] = Float(raw.load(fromByteOffset: offset * 2, as: Float16.self))
                        case .double: samples[index] = Float(raw.load(fromByteOffset: offset * 8, as: Double.self))
                        default: samples[index] = raw.load(fromByteOffset: offset * 4, as: Float.self)
                        }
                    }
                }
            }
        }
        return ModelPlane(width: width, height: height, channels: channels, samples: samples)
    }
}

enum DepthEstimator {
    /// The model at `compiledModel` on `image` (`CoreMLRuntime.imageInput`, scale-fill). The
    /// output is the model's first output by name, one map.
    static func estimate(_ image: CGImage, compiledModel: URL) throws -> DepthMap {
        let model = try CoreMLRuntime.load(compiledModel)
        let input = try CoreMLRuntime.imageInput(image, for: model)
        let provider = try MLDictionaryFeatureProvider(dictionary: [input.name: input.value])
        let output = try model.prediction(from: provider)
        guard let name = model.modelDescription.outputDescriptionsByName.keys.sorted().first,
              let feature = output.featureValue(for: name) else {
            throw DoctoredCoreError(message: "The depth model returned no output.")
        }
        let plane = try CoreMLRuntime.plane(feature)
        guard plane.channels == 1 else {
            throw DoctoredCoreError(message: "The depth model's array is not one map.")
        }
        return DepthMap(width: plane.width, height: plane.height, samples: plane.samples)
    }
}
