import AppKit

// Paths: every `dr_*path*` and `dr_*vector_mask*` call. The document half reads and
// replaces the whole path list as the core's path-list JSON, and reads and writes a layer's vector
// mask, at either depth behind one call (`dr_doc_*` or `dr_precision_doc_*`), so nothing above
// this file knows which depth it is talking to. The shape
// half (`RasterPathCore`) takes one shape and no document: coverage, flattening, tracing, hit
// testing and the two anchor edits, each the core's own kernel (`core/src/path_geometry.rs`,
// `path_trace.rs`). Models are in `PathModel.swift`.

extension RasterDocument {
    // MARK: - Paths

    /// The most paths a document may hold (256), asked of the core.
    static var maxPaths: Int { dr_max_paths() }

    /// The document's paths, the work path first. Each read crosses the
    /// boundary and parses the list, so a caller that draws from it keeps the
    /// value until the document changes.
    var paths: PathSet {
        let raw = precision.map { dr_precision_doc_paths_json($0.ptr) } ?? dr_doc_paths_json(ptr)
        guard let raw else { return PathSet() }
        defer { dr_string_free(raw) }
        let json = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8))
        return (try? PathSet(json: json)) ?? PathSet()
    }

    /// The document holding exactly `set`: add, edit, rename, reorder and
    /// delete are one call. nil for the list the document already holds (an
    /// identical copy would register a phantom undo step). Throws the core's
    /// own sentence for a broken list rule or cap — a duplicate or empty
    /// name, a second work path, more than `maxPaths` paths, too many
    /// anchors — and for a shape it refuses.
    func withPaths(_ set: PathSet) throws -> RasterDocument? {
        let json = try RasterPathCore.text(set.jsonObject, finite: set.entries.allSatisfy(\.shape.isFinite))
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            let result = dr_precision_doc_with_paths_json(precision.ptr, json, &error)
            if let error {
                throw DoctoredCoreError(
                    message: takeErrorMessage(error, fallback: "Could not change the paths."))
            }
            return try wrapTyped(result)
        }
        let result = dr_doc_with_paths_json(ptr, json, &error)
        if let error {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not change the paths."))
        }
        return wrap(result)
    }

    // MARK: - Vector masks

    /// Layer `layer`'s vector mask — its shape in canvas coordinates, its two
    /// flags and whether a raster mask is kept beside it — or nil when the
    /// layer has none. The layer's ordinary mask is the two baked together
    /// (`core/src/doc_vector_mask.rs`).
    func vectorMask(_ layer: Int) -> VectorMaskInfo? {
        guard layer >= 0 else { return nil }
        let raw = precision.map { dr_precision_doc_vector_mask_json($0.ptr, layer) }
            ?? dr_doc_vector_mask_json(ptr, layer)
        guard let raw else { return nil }
        defer { dr_string_free(raw) }
        guard let json = try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8)),
              let object = json as? [String: Any], let shape = try? PathShape(json: object)
        else { return nil }
        var raster: LayerMaskProperties?
        if let part = object["raster"] as? [String: Any],
           let linked = part["linked"] as? Bool,
           let density = (part["density"] as? NSNumber)?.doubleValue,
           let feather = (part["feather"] as? NSNumber)?.doubleValue,
           let outside = (part["outside_coverage"] as? NSNumber)?.intValue,
           let x = (part["offset_x"] as? NSNumber)?.intValue,
           let y = (part["offset_y"] as? NSNumber)?.intValue,
           let width = (part["width"] as? NSNumber)?.intValue,
           let height = (part["height"] as? NSNumber)?.intValue {
            raster = LayerMaskProperties(
                linked: linked, density: density, feather: feather,
                outsideCoverage: UInt8(clamping: outside), offsetX: x, offsetY: y, width: width,
                height: height)
        }
        return VectorMaskInfo(
            shape: shape, enabled: object["enabled"] as? Bool ?? true,
            linked: object["linked"] as? Bool ?? true,
            hasRaster: object["has_raster"] as? Bool ?? false,
            rasterEnabled: object["raster_enabled"] as? Bool ?? false,
            rasterProperties: raster)
    }

    /// What a read-modify-write of layer `layer`'s MASK properties starts
    /// from: the mask's own properties and enabled flag — or, on a layer
    /// with a vector mask, those of the RASTER mask kept beside the shape,
    /// which is what the mask tools edit there. The layer's ordinary getters
    /// describe the baked mask (the shape's fill times the raster mask), and
    /// writing its offset, density, feather or enabled flag back would
    /// overwrite the raster mask's. nil with no mask, and for a vector mask
    /// with no raster mask beside it.
    func maskEditBaseline(_ layer: Int) -> (properties: LayerMaskProperties, enabled: Bool)? {
        if let vector = vectorMask(layer) {
            return vector.rasterProperties.map { ($0, vector.rasterEnabled) }
        }
        return layerMaskProperties(layer).map { ($0, layerMaskEnabled(layer)) }
    }

    /// The document with `shape` as layer `layer`'s vector mask: a new one
    /// (the layer's own mask, if any, kept beside it) or the same one
    /// reshaped. An empty shape is Reveal All. nil for the shape the mask
    /// already has, and at 8 bits for a missing layer or Lock All. Throws the
    /// core's sentence: a refused shape, the document's anchor cap, a lock at
    /// high depth, the working limit, a cancel.
    func settingVectorMask(
        _ layer: Int, shape: PathShape, control: RasterJobControl?
    ) throws -> RasterDocument? {
        guard layer >= 0 else { return nil }
        let json = try RasterPathCore.text(shape.jsonObject, finite: shape.isFinite)
        var error: UnsafeMutablePointer<CChar>?
        let fallback = "Could not set the vector mask."
        if let precision {
            let context = try PrecisionWorkContext()
            let result = dr_precision_doc_set_vector_mask(
                precision.ptr, layer, json, context.limit, context.otherRetained, control?.ptr,
                &error)
            if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback)) }
            return try wrapTyped(result)
        }
        let result = dr_doc_set_vector_mask(ptr, layer, json, control?.ptr, &error)
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback)) }
        return wrap(result)
    }

    /// The document with layer `layer`'s vector mask removed: the raster mask
    /// kept beside it is the layer's mask again, or the layer has none. nil
    /// for a layer with no vector mask, and at 8 bits under Lock All.
    func removingVectorMask(_ layer: Int) throws -> RasterDocument? {
        guard layer >= 0 else { return nil }
        if let precision {
            let context = try PrecisionWorkContext()
            var error: UnsafeMutablePointer<CChar>?
            let result = dr_precision_doc_remove_vector_mask(
                precision.ptr, layer, context.limit, context.otherRetained, &error)
            if let error {
                throw DoctoredCoreError(
                    message: takeErrorMessage(error, fallback: "Could not delete the vector mask."))
            }
            return try wrapTyped(result)
        }
        return wrap(dr_doc_remove_vector_mask(ptr, layer))
    }

    /// The document with layer `layer`'s vector mask enabled or disabled and
    /// linked or unlinked. nil for a layer with no vector mask and for the
    /// flags it already has.
    func settingVectorMaskProperties(
        _ layer: Int, enabled: Bool, linked: Bool, control: RasterJobControl?
    ) throws -> RasterDocument? {
        guard layer >= 0 else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let fallback = "Could not change the vector mask."
        if let precision {
            let context = try PrecisionWorkContext()
            let result = dr_precision_doc_set_vector_mask_properties(
                precision.ptr, layer, enabled, linked, context.limit, context.otherRetained,
                control?.ptr, &error)
            if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback)) }
            return try wrapTyped(result)
        }
        let result = dr_doc_set_vector_mask_properties(
            ptr, layer, enabled, linked, control?.ptr, &error)
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback)) }
        return wrap(result)
    }
}

/// The shape operations: one `PathShape` in, no document.
enum RasterPathCore {
    /// A JSON object as text for the core. `finite` false refuses first: a
    /// non-finite coordinate has no JSON form.
    fileprivate static func text(_ object: [String: Any], finite: Bool) throws -> String {
        guard finite else {
            throw DoctoredCoreError(message: "A path coordinate is not a finite number.")
        }
        let data = try JSONSerialization.data(withJSONObject: object)
        return String(decoding: data, as: UTF8.self)
    }

    private static func text(_ shape: PathShape) throws -> String {
        try text(shape.jsonObject, finite: shape.isFinite)
    }

    /// The shape the core answered as its shape JSON, or its error.
    private static func shape(
        _ result: UnsafeMutablePointer<CChar>?, _ error: UnsafeMutablePointer<CChar>?,
        fallback: String
    ) throws -> PathShape {
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: fallback))
        }
        defer { dr_string_free(result) }
        let json = try JSONSerialization.jsonObject(with: Data(String(cString: result).utf8))
        return try PathShape(json: json)
    }

    /// The shape's FILL as canvas-sized u16 coverage (65535 = wholly
    /// covered), row-major, through the core's one coverage rasterizer under
    /// the shape's fill rule. `antiAlias` false cuts each sample at one half.
    /// `clip` (nil, or `width * height` samples — a selection) is multiplied
    /// in. Throws the core's refusal: a shape it rejects, its edge and work
    /// caps, the working limit, a cancel.
    static func coverage(
        _ shape: PathShape, width: Int, height: Int, antiAlias: Bool, clip: [UInt16]?,
        control: RasterJobControl?
    ) throws -> [UInt16] {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              clip.map({ $0.count == width * height }) ?? true
        else { throw DoctoredCoreError(message: "Invalid path coverage dimensions.") }
        let json = try text(shape)
        let context = try PrecisionWorkContext()
        _ = try RasterResources.shared.admit(scratch: UInt64(width * height) * 2)
        var output = [UInt16](repeating: 0, count: width * height)
        var error: UnsafeMutablePointer<CChar>?
        let run = { (clip: UnsafeBufferPointer<UInt16>?) -> Bool in
            output.withUnsafeMutableBufferPointer { samples in
                dr_path_coverage(
                    json, 0, 0, UInt32(width), UInt32(height), antiAlias, clip?.baseAddress,
                    clip?.count ?? 0, samples.baseAddress, samples.count, context.limit,
                    context.otherRetained, control?.ptr, &error)
            }
        }
        let success = clip.map { $0.withUnsafeBufferPointer { run($0) } } ?? run(nil)
        guard success else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not rasterize the path."))
        }
        return output
    }

    /// Each subpath as a polyline within `tolerance` pixels of its curve. A
    /// polyline starts at the subpath's first anchor and passes through every
    /// anchor; a closed one covers the closing segment and does NOT repeat
    /// its first point.
    static func flattened(
        _ shape: PathShape, tolerance: Double
    ) throws -> [(points: [CGPoint], closed: Bool)] {
        let json = try text(shape)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_path_flatten(json, tolerance, &error) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not flatten the path."))
        }
        defer { dr_string_free(result) }
        let parsed = try JSONSerialization.jsonObject(with: Data(String(cString: result).utf8))
        guard let list = parsed as? [[String: Any]] else {
            throw DoctoredCoreError(message: "Invalid flattened path.")
        }
        return try list.map { entry in
            guard let closed = entry["closed"] as? Bool, let raw = entry["points"] as? [[NSNumber]],
                  raw.allSatisfy({ $0.count == 2 })
            else { throw DoctoredCoreError(message: "Invalid flattened path.") }
            return (raw.map { CGPoint(x: $0[0].doubleValue, y: $0[1].doubleValue) }, closed)
        }
    }

    /// Traces the region of `coverage` (canvas-sized u16, row-major) at or
    /// above one half into closed subpaths under the even-odd rule, each
    /// within `tolerance` pixels of the half-coverage contour. Coverage with
    /// nothing at or above one half answers an empty shape.
    static func traced(
        _ coverage: [UInt16], width: Int, height: Int, tolerance: Double,
        control: RasterJobControl?
    ) throws -> PathShape {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              coverage.count == width * height
        else { throw DoctoredCoreError(message: "Invalid path trace dimensions.") }
        let context = try PrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { samples in
            dr_path_trace(
                samples.baseAddress, samples.count, UInt32(width), UInt32(height), tolerance,
                context.limit, context.otherRetained, control?.ptr, &error)
        }
        return try shape(result, error, fallback: "Could not trace the path.")
    }

    /// What lies within `tolerance` pixels of `point`: an anchor before a
    /// handle before a segment. With `handles` false no handle is offered —
    /// the answer is an anchor, a segment or nothing — which is what a press
    /// that cannot grab a handle asks, so a handle tip lying near a segment
    /// does not hide it. `.none` for a miss or a shape the core refuses.
    static func hit(
        _ shape: PathShape, at point: CGPoint, tolerance: Double, handles: Bool
    ) -> PathHit {
        guard let json = try? text(shape),
              let result = dr_path_hit(json, Double(point.x), Double(point.y), tolerance, handles)
        else { return .none }
        defer { dr_string_free(result) }
        guard let parsed = try? JSONSerialization.jsonObject(
                with: Data(String(cString: result).utf8)) as? [String: Any],
              let kind = parsed["kind"] as? String
        else { return .none }
        let subpath = (parsed["subpath"] as? NSNumber)?.intValue ?? 0
        let index = (parsed["index"] as? NSNumber)?.intValue ?? 0
        switch kind {
        case "anchor": return .anchor(subpath: subpath, index: index)
        case "handle_in": return .handleIn(subpath: subpath, index: index)
        case "handle_out": return .handleOut(subpath: subpath, index: index)
        case "segment":
            return .segment(
                subpath: subpath, index: index, t: (parsed["t"] as? NSNumber)?.doubleValue ?? 0)
        default: return .none
        }
    }

    /// The shape with one more anchor on `segment` of `subpath` at parameter
    /// `t` (a segment hit's), its outline unchanged. Throws when there is no
    /// such segment, `t` is not strictly between 0 and 1, or the shape is
    /// full.
    static func insertingAnchor(
        _ shape: PathShape, subpath: Int, segment: Int, t: Double
    ) throws -> PathShape {
        guard subpath >= 0, segment >= 0 else {
            throw DoctoredCoreError(message: "The path has no such segment.")
        }
        let json = try text(shape)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_path_insert_anchor(json, subpath, segment, t, &error)
        return try self.shape(result, error, fallback: "Could not insert the anchor.")
    }

    /// The shape with `anchor` of `subpath` made a corner (`smooth` false) or
    /// a smooth anchor with automatic handles. The same shape comes back when
    /// the anchor is already that, or has no neighbour to aim a handle at.
    /// Throws for an anchor that does not exist.
    static func convertingAnchor(
        _ shape: PathShape, subpath: Int, anchor: Int, smooth: Bool
    ) throws -> PathShape {
        guard subpath >= 0, anchor >= 0 else {
            throw DoctoredCoreError(message: "The path has no such anchor.")
        }
        let json = try text(shape)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_path_convert_anchor(json, subpath, anchor, smooth, &error)
        // NULL with no message is the core's "unchanged".
        if result == nil, error == nil { return shape }
        return try self.shape(result, error, fallback: "Could not convert the anchor.")
    }
}
