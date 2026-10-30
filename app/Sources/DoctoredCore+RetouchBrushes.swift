import Foundation

// The retouch brushes' core calls (`core/src/ffi_retouch_brush.rs`): Blur, Sharpen and Sponge
// over a stroke overlay, the History Brush, and Smudge along a stroke's dabs, at 8 and 16 bits. The
// 16-bit Blur / Sharpen / Sponge is the `retouch_brush` op of `pixelEditPlan` / `editingPixels`.
// The typed calls run a 32-bit float document too (the core's `float_retouch_brush`): Blur,
// Sharpen, Sponge, Smudge and the History Brush in linear light with the range kept. The Art
// History Brush runs on the byte document and, typed, on an 8-bit CMYK document only.

/// `dr_doc_retouch_brush_layer`'s kind code.
enum RetouchBrushKind: UInt8 {
    case blur, sharpen, saturate, desaturate

    /// The kind's name in the typed (16-bit and float) `retouch_brush` request.
    var requestName: String {
        switch self {
        case .blur: return "blur"
        case .sharpen: return "sharpen"
        case .saturate: return "saturate"
        case .desaturate: return "desaturate"
        }
    }
}

extension RasterDocument {
    /// Blurs, sharpens, saturates or desaturates layer `idx` where the stroke
    /// overlay covers it. `data` is the canvas-sized premultiplied RGBA8
    /// overlay `dodgeBurnLayer` takes; only its alpha (the stroke's coverage)
    /// is read. `strength` is 0...1. `vibrance` (Sponge only) takes the
    /// Vibrance target; `sampleAllLayers` (Blur and Sharpen only) reads
    /// the sampled picture and writes the layer toward it, alpha included.
    /// nil when nothing would change; throws the core's sentence for an option
    /// on a kind it does not apply to.
    func retouchBrushLayer(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int,
        kind: RetouchBrushKind, strength: Double, vibrance: Bool = false,
        sampleAllLayers: Bool = false
    ) throws -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_retouch_brush_layer(
            ptr, idx, data, UInt32(w), UInt32(h), kind.rawValue, Float(strength), vibrance,
            sampleAllLayers, &error)
        if let error {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "The retouch brush failed."))
        }
        return wrap(result)
    }

    /// Paints layer `idx` back toward layer `sourceLayer` of `source` (an
    /// earlier state of this document) where the stroke overlay covers it, by
    /// coverage × `opacity` (0...1). nil when nothing would change, and for a
    /// source of another canvas size or a non-raster layer on either side.
    func historyBrushLayer(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int,
        source: RasterDocument, sourceLayer: Int, opacity: Double
    ) -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height, sourceLayer >= 0,
              let sourcePointer = source.ptr else { return nil }
        return wrap(
            dr_doc_history_brush_layer(
                ptr, idx, data, UInt32(w), UInt32(h), sourcePointer, sourceLayer,
                Float(opacity)))
    }

    /// Smudges layer `idx` along a stroke: `request` and `dabs` are
    /// `dr_brush_u16`'s (the dabs in canvas coordinates), `strength` is 0...1
    /// and `selection` is canvas-sized u8 coverage or nil. `sampleAllLayers`
    /// smears the sampled picture into the layer. nil when nothing
    /// would change; throws the core's sentence for a bad stroke, a stroke
    /// past Smudge's own 256-million work bound or a selection of another
    /// size.
    func smudgeLayer(
        _ idx: Int, request: String, dabs: [Double], strength: Double, selection: [UInt8]?,
        sampleAllLayers: Bool = false
    ) throws -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        try precisionCString(request, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer { input -> OpaquePointer? in
            let run = { (mask: UnsafePointer<UInt8>?, count: Int) in
                dr_doc_smudge_layer(
                    self.ptr, idx, request, input.baseAddress, dabs.count / 4, Float(strength),
                    mask, count, sampleAllLayers, &error)
            }
            guard let selection else { return run(nil, 0) }
            return selection.withUnsafeBufferPointer { run($0.baseAddress, $0.count) }
        }
        if let error {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Smudge failed."))
        }
        return wrap(result)
    }

    /// Paints layer `idx` with the Art History Brush from layer
    /// `sourceLayer` of `source` (an earlier state of this document):
    /// `request` is `ArtHistoryOptions.request`, `brushRequest` and `dabs`
    /// `dr_brush_u16`'s (the dabs in canvas coordinates), `selection`
    /// canvas-sized u8 coverage or nil. nil when nothing would change; throws
    /// the core's sentence for a bad request, a bad stroke, the work bound or
    /// a selection of another size.
    func artHistoryBrushLayer(
        _ idx: Int, source: RasterDocument, sourceLayer: Int, request: String,
        brushRequest: String, dabs: [Double], selection: [UInt8]?
    ) throws -> RasterDocument? {
        guard isValidIndex(idx), sourceLayer >= 0, let sourcePointer = source.ptr else {
            return nil
        }
        try precisionCString(request, maximumBytes: 4096)
        try precisionCString(brushRequest, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer { input -> OpaquePointer? in
            let run = { (mask: UnsafePointer<UInt8>?, count: Int) in
                dr_doc_art_history_brush_layer(
                    self.ptr, idx, sourcePointer, sourceLayer, request, brushRequest,
                    input.baseAddress, dabs.count / 4, mask, count, &error)
            }
            guard let selection else { return run(nil, 0) }
            return selection.withUnsafeBufferPointer { run($0.baseAddress, $0.count) }
        }
        if let error {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "The Art History Brush failed."))
        }
        return wrap(result)
    }
}

extension RasterPrecisionDocument {
    /// `dr_precision_doc_smudge_plan`: `retained_bytes`, `scratch_bytes` and
    /// `input_samples`, as `pixelEditPlan` answers; `sampleAllLayers` adds the
    /// sampled picture's term.
    func smudgePlan(layer: Int, request: String, dabs: [Double], selection: Bool,
                    sampleAllLayers: Bool = false, workingLimit: UInt64,
                    otherRetained: UInt64) throws -> [String: Any] {
        guard layer >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        try precisionCString(request, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer {
            dr_precision_doc_smudge_plan(ptr, layer, request, $0.baseAddress, dabs.count / 4,
                selection, sampleAllLayers, workingLimit, otherRetained, &error)
        }
        return try precisionJSON(result, error)
    }

    /// `dr_precision_doc_smudge`: nil for an exact no-op, as `editingPixels`.
    func smudging(layer: Int, request: String, dabs: [Double], strength: Double,
                  selection: [UInt16]?, sampleAllLayers: Bool = false, workingLimit: UInt64,
                  otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument? {
        guard layer >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        try precisionCString(request, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer { input -> OpaquePointer? in
            let run = { (mask: UnsafePointer<UInt16>?, count: Int) in
                dr_precision_doc_smudge(
                    self.ptr, layer, request, input.baseAddress, dabs.count / 4, strength,
                    mask, count, sampleAllLayers, workingLimit, otherRetained, control?.ptr,
                    &error)
            }
            guard let selection else { return run(nil, 0) }
            return selection.withUnsafeBufferPointer { run($0.baseAddress, $0.count) }
        }
        return try editResult(result, error)
    }

    /// `dr_precision_doc_history_brush_plan`, as `pixelEditPlan` answers.
    /// `source` is a state at this document's own depth (16-bit or float).
    func historyBrushPlan(layer: Int, source: RasterPrecisionDocument, sourceLayer: Int,
                          selection: Bool, workingLimit: UInt64, otherRetained: UInt64)
        throws -> [String: Any] {
        guard layer >= 0, sourceLayer >= 0 else {
            throw DoctoredCoreError(message: "Invalid high-depth layer index.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_history_brush_plan(
            ptr, layer, source.ptr, sourceLayer, selection, workingLimit, otherRetained, &error)
        return try precisionJSON(result, error)
    }

    /// `dr_precision_doc_history_brush`: `samples` is the canvas-sized stroke
    /// coverage, then the selection plane when planned with one. nil for an
    /// exact no-op, as `editingPixels`.
    func historyBrushing(layer: Int, source: RasterPrecisionDocument, sourceLayer: Int,
                         opacity: Double, samples: [UInt16], workingLimit: UInt64,
                         otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument? {
        guard layer >= 0, sourceLayer >= 0 else {
            throw DoctoredCoreError(message: "Invalid high-depth layer index.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = samples.withUnsafeBufferPointer { input in
            dr_precision_doc_history_brush(
                ptr, layer, source.ptr, sourceLayer, opacity, input.baseAddress, input.count,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }

    /// `dr_precision_doc_art_history_brush_plan`, as `pixelEditPlan` answers:
    /// the core refuses, by name, every depth the tool is disabled at
    /// (16-bit, 32-bit and 16-bit CMYK), so only an 8-bit CMYK document plans.
    func artHistoryBrushPlan(layer: Int, source: RasterPrecisionDocument, sourceLayer: Int,
                             request: String, brushRequest: String, dabs: [Double],
                             selection: Bool, workingLimit: UInt64, otherRetained: UInt64)
        throws -> [String: Any] {
        guard layer >= 0, sourceLayer >= 0 else {
            throw DoctoredCoreError(message: "Invalid high-depth layer index.")
        }
        try precisionCString(request, maximumBytes: 4096)
        try precisionCString(brushRequest, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer {
            dr_precision_doc_art_history_brush_plan(
                ptr, layer, source.ptr, sourceLayer, request, brushRequest, $0.baseAddress,
                dabs.count / 4, selection, workingLimit, otherRetained, &error)
        }
        return try precisionJSON(result, error)
    }

    /// `dr_precision_doc_art_history_brush`: `selection` is the canvas-sized
    /// u16 selection plane or nil. nil for an exact no-op, as `editingPixels`.
    func artHistoryBrushing(layer: Int, source: RasterPrecisionDocument, sourceLayer: Int,
                            request: String, brushRequest: String, dabs: [Double],
                            selection: [UInt16]?, workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl? = nil) throws -> RasterPrecisionDocument? {
        guard layer >= 0, sourceLayer >= 0 else {
            throw DoctoredCoreError(message: "Invalid high-depth layer index.")
        }
        try precisionCString(request, maximumBytes: 4096)
        try precisionCString(brushRequest, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer { input -> OpaquePointer? in
            let run = { (mask: UnsafePointer<UInt16>?, count: Int) in
                dr_precision_doc_art_history_brush(
                    self.ptr, layer, source.ptr, sourceLayer, request, brushRequest,
                    input.baseAddress, dabs.count / 4, mask, count, workingLimit, otherRetained,
                    control?.ptr, &error)
            }
            guard let selection else { return run(nil, 0) }
            return selection.withUnsafeBufferPointer { run($0.baseAddress, $0.count) }
        }
        return try editResult(result, error)
    }
}
