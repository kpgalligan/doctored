import Foundation

/// Filter ▸ Neural Filters' tiled stage, info and documents: `dr_neural_stage_info`,
/// `dr_image_neural_stage` / `dr_precision_image_neural_stage`, the two Super Zoom documents and
/// the typed JPEG Artifacts Removal apply, and Colorize's plan, input and apply at both depths.
/// The only callers of those eleven exports; the runs are `NeuralFilterRun`, `SuperZoomRun`,
/// `JpegArtifactsRemovalRun` and `ColorizeRun`.

/// The core's figures for one stage over a source (`dr_neural_stage_info`).
struct NeuralStageInfo {
    let tiles: Int
    let outputWidth: Int
    let outputHeight: Int
    let inputLength: Int
    let planeLength: Int
    let plannedBytes: UInt64
}

/// The predict callback's context: the Swift model the C trampoline calls once per tile, and
/// the first error it threw (re-thrown by the wrapper, so a cancel stays a cancel). The core
/// calls it on the thread that called the stage.
final class NeuralPredictBox {
    let predict: (UInt64, UnsafeBufferPointer<Float>, UnsafeMutableBufferPointer<Float>) throws -> Void
    var error: Error?

    init(_ predict: @escaping (UInt64, UnsafeBufferPointer<Float>, UnsafeMutableBufferPointer<Float>) throws -> Void) {
        self.predict = predict
    }
}

/// The C trampoline: never throws across the boundary; a thrown error is kept in the box and
/// answered as false ("the model failed on tile N").
private let neuralTrampoline: DrNeuralPredict = { context, tile, input, inputCount, output, outputCount in
    guard let context, let input, let output else { return false }
    let box = Unmanaged<NeuralPredictBox>.fromOpaque(context).takeUnretainedValue()
    do {
        try box.predict(tile, UnsafeBufferPointer(start: input, count: inputCount),
                        UnsafeMutableBufferPointer(start: output, count: outputCount))
        return true
    } catch {
        if box.error == nil { box.error = error }
        return false
    }
}

enum RasterNeural {
    /// The stage request's JSON.
    static func json(_ stage: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: stage, options: [.sortedKeys])
        guard let text = String(data: data, encoding: .utf8) else {
            throw DoctoredCoreError(message: "The neural filter request could not be encoded.")
        }
        return text
    }

    /// The core's figures for `stage` over a `width × height` source at `bytesPerSample` (1 or 2).
    static func stageInfo(width: Int, height: Int, stage: [String: Any], bytesPerSample: Int) throws
        -> NeuralStageInfo {
        let request = try json(stage)
        var error: UnsafeMutablePointer<CChar>?
        guard let text = dr_neural_stage_info(UInt32(clamping: width), UInt32(clamping: height), request,
                                              UInt32(bytesPerSample), &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The neural filter was refused."))
        }
        defer { dr_string_free(text) }
        guard let object = try JSONSerialization.jsonObject(with: Data(String(cString: text).utf8))
                as? [String: Any],
              let tiles = (object["tiles"] as? NSNumber)?.intValue,
              let w = (object["output_width"] as? NSNumber)?.intValue,
              let h = (object["output_height"] as? NSNumber)?.intValue,
              let input = (object["input_len"] as? NSNumber)?.intValue,
              let plane = (object["plane_len"] as? NSNumber)?.intValue,
              let planned = (object["planned_bytes"] as? NSNumber)?.uint64Value else {
            throw DoctoredCoreError(message: "The neural filter's figures could not be read.")
        }
        return NeuralStageInfo(tiles: tiles, outputWidth: w, outputHeight: h, inputLength: input,
                               planeLength: plane, plannedBytes: planned)
    }

    /// Colorize's planned working bytes (`dr_neural_colorize_plan`) for a `width × height` layer
    /// at model side `side` and `bytesPerSample` (1 or 2).
    static func colorizePlan(width: Int, height: Int, side: Int, bytesPerSample: Int) throws -> UInt64 {
        var error: UnsafeMutablePointer<CChar>?
        let bytes = dr_neural_colorize_plan(UInt32(clamping: width), UInt32(clamping: height),
                                            UInt32(clamping: side), UInt32(bytesPerSample), &error)
        guard bytes > 0 else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Colorize was refused."))
        }
        return bytes
    }

    /// Runs `body` over `profile`'s bytes and length.
    fileprivate static func withProfile<T>(_ profile: Data,
                                           _ body: (UnsafePointer<UInt8>?, Int) throws -> T) rethrows -> T {
        try profile.withUnsafeBytes { raw in
            try body(raw.bindMemory(to: UInt8.self).baseAddress, raw.count)
        }
    }

    /// Runs `body` with the gray model input buffer (`3·side²`), answering it filled.
    fileprivate static func colorizeInput(side: Int, _ body: (UnsafeMutablePointer<Float>, Int) throws -> Bool)
        throws -> [Float] {
        var input = [Float](repeating: 0, count: 3 * side * side)
        let ok = try input.withUnsafeMutableBufferPointer { buffer -> Bool in
            guard let base = buffer.baseAddress else { return false }
            return try body(base, buffer.count)
        }
        guard ok else { throw DoctoredCoreError(message: "Colorize was refused.") }
        return input
    }

    /// Runs `body` with the callback and its context for `box` (NULL without one), re-throwing
    /// the model's own error over the core's sentence.
    fileprivate static func withPredict<T>(
        _ box: NeuralPredictBox?,
        _ body: (DrNeuralPredict?, UnsafeMutableRawPointer?) throws -> T
    ) throws -> T {
        guard let box else { return try body(nil, nil) }
        let context = Unmanaged.passRetained(box)
        defer { context.release() }
        do {
            let result = try body(neuralTrampoline, context.toOpaque())
            if let error = box.error { throw error }
            return result
        } catch {
            throw box.error ?? error
        }
    }
}

extension RasterImage {
    /// One 8-bit stage (`dr_image_neural_stage`): nil when it changes nothing.
    func neuralStage(_ stage: [String: Any], model: NeuralPredictBox?, limit: UInt64, other: UInt64,
                     control: RasterJobControl?) throws -> RasterImage? {
        let request = try RasterNeural.json(stage)
        return try RasterNeural.withPredict(model) { callback, context in
            var error: UnsafeMutablePointer<CChar>?
            guard let result = dr_image_neural_stage(ptr, request, callback, context, limit, other,
                                                     control?.ptr, &error) else {
                guard error != nil else { return nil }
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The neural filter was refused."))
            }
            return RasterImage(owning: result)
        }
    }
}

extension RasterImage {
    /// Colorize's model input from this 8-bit layer (`dr_image_neural_colorize_input`): the
    /// `3·side²` channel-major sRGB gray of each cell's L* in the document profile `profile`.
    func neuralColorizeInput(profile: Data, side: Int, limit: UInt64, other: UInt64,
                             control: RasterJobControl?) throws -> [Float] {
        try RasterNeural.colorizeInput(side: side) { buffer, count in
            try RasterNeural.withProfile(profile) { bytes, length in
                var error: UnsafeMutablePointer<CChar>?
                guard dr_image_neural_colorize_input(ptr, bytes, length, UInt32(clamping: side), buffer,
                                                     count, limit, other, control?.ptr, &error) else {
                    throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Colorize was refused."))
                }
                return true
            }
        }
    }

    /// Colorize applied to this 8-bit layer (`dr_image_neural_colorize_apply`): the model's
    /// `2·side²` a*, b* `chroma` adjusted by `adjustments` and carried onto the layer's own L*;
    /// nil when nothing changes.
    func neuralColorizeApply(profile: Data, side: Int, chroma: [Float], adjustments: [String: Any],
                             limit: UInt64, other: UInt64, control: RasterJobControl?) throws -> RasterImage? {
        let request = try RasterNeural.json(adjustments)
        return try RasterNeural.withProfile(profile) { bytes, length in
            try chroma.withUnsafeBufferPointer { plane in
                var error: UnsafeMutablePointer<CChar>?
                guard let result = dr_image_neural_colorize_apply(
                    ptr, bytes, length, UInt32(clamping: side), plane.baseAddress, plane.count, request,
                    limit, other, control?.ptr, &error) else {
                    guard error != nil else { return nil }
                    throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Colorize was refused."))
                }
                return RasterImage(owning: result)
            }
        }
    }
}

extension RasterPrecisionImage {
    /// Colorize's model input from this 16-bit layer (`dr_precision_image_neural_colorize_input`).
    func neuralColorizeInput(profile: Data, side: Int, limit: UInt64, other: UInt64,
                             control: RasterJobControl?) throws -> [Float] {
        try RasterNeural.colorizeInput(side: side) { buffer, count in
            try RasterNeural.withProfile(profile) { bytes, length in
                var error: UnsafeMutablePointer<CChar>?
                guard dr_precision_image_neural_colorize_input(ptr, bytes, length, UInt32(clamping: side),
                                                               buffer, count, limit, other, control?.ptr,
                                                               &error) else {
                    throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Colorize was refused."))
                }
                return true
            }
        }
    }

    /// Colorize applied to this 16-bit layer (`dr_precision_image_neural_colorize_apply`); nil
    /// when nothing changes.
    func neuralColorizeApply(profile: Data, side: Int, chroma: [Float], adjustments: [String: Any],
                             limit: UInt64, other: UInt64, control: RasterJobControl?) throws
        -> RasterPrecisionImage? {
        let request = try RasterNeural.json(adjustments)
        return try RasterNeural.withProfile(profile) { bytes, length in
            try chroma.withUnsafeBufferPointer { plane in
                var error: UnsafeMutablePointer<CChar>?
                guard let result = dr_precision_image_neural_colorize_apply(
                    ptr, bytes, length, UInt32(clamping: side), plane.baseAddress, plane.count, request,
                    limit, other, control?.ptr, &error) else {
                    guard error != nil else { return nil }
                    throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Colorize was refused."))
                }
                return RasterPrecisionImage(owning: result)
            }
        }
    }

    /// One 16-bit stage (`dr_precision_image_neural_stage`): nil when it changes nothing.
    func neuralStage(_ stage: [String: Any], model: NeuralPredictBox?, limit: UInt64, other: UInt64,
                     control: RasterJobControl?) throws -> RasterPrecisionImage? {
        let request = try RasterNeural.json(stage)
        return try RasterNeural.withPredict(model) { callback, context in
            var error: UnsafeMutablePointer<CChar>?
            guard let result = dr_precision_image_neural_stage(ptr, request, callback, context, limit,
                                                               other, control?.ptr, &error) else {
                guard error != nil else { return nil }
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The neural filter was refused."))
            }
            return RasterPrecisionImage(owning: result)
        }
    }
}

extension RasterDocument {
    /// Super Zoom's new 8-bit document (`dr_doc_super_zoom_document`): one "Background" layer
    /// holding `zoomed`, in this document's profile at its resolution.
    func superZoomDocument(_ zoomed: RasterImage) throws -> RasterDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_super_zoom_document(ptr, zoomed.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Super Zoom was refused."))
        }
        return RasterDocument(owning: result)
    }
}

extension RasterPrecisionDocument {
    /// Super Zoom's new 16-bit document (`dr_precision_doc_super_zoom_document`).
    func superZoomDocument(_ zoomed: RasterPrecisionImage, limit: UInt64, other: UInt64,
                           control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_super_zoom_document(ptr, zoomed.ptr, limit, other,
                                                                control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Super Zoom was refused."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// JPEG Artifacts Removal's 16-bit apply (`dr_precision_doc_neural_layer_apply`): `pixels`
    /// replace `layer` in place, or with `newLayer` land on a new layer `name` above it; nil when
    /// nothing changed.
    func neuralLayerApply(_ layer: Int, pixels: RasterPrecisionImage, newLayer: Bool, name: String,
                          limit: UInt64, other: UInt64, control: RasterJobControl?) throws
        -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_neural_layer_apply(ptr, layer, pixels.ptr, newLayer, name,
                                                               limit, other, control?.ptr, &error) else {
            guard error != nil else { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "JPEG Artifacts Removal was refused."))
        }
        return RasterPrecisionDocument(owning: result)
    }
}
