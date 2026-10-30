import Foundation

/// Edit ▸ Puppet Warp's mesh (`puppet_mesh.rs`): the triangle mesh the
/// core builds over a layer's opaque pixels (or the selection's float), with
/// its rest vertices and triangles read once. Immutable, as every core handle
/// is; a pose keeps its own reference to it in the core.
final class RasterPuppetMesh {
    fileprivate let ptr: OpaquePointer
    /// Rest vertices in canvas pixels.
    let vertices: [CGPoint]
    /// Vertex index triples.
    let triangles: [[Int]]

    fileprivate init(owning pointer: OpaquePointer) throws {
        ptr = pointer
        var error: UnsafeMutablePointer<CChar>?
        let value: [String: Any]
        do { value = try precisionJSON(dr_puppet_mesh_describe(pointer, &error), error) } catch {
            dr_puppet_mesh_free(pointer)
            throw error
        }
        vertices = (value["vertices"] as? [[Double]] ?? []).compactMap {
            $0.count == 2 ? CGPoint(x: $0[0], y: $0[1]) : nil
        }
        triangles = (value["triangles"] as? [[Int]] ?? []).filter { $0.count == 3 }
    }

    deinit { dr_puppet_mesh_free(ptr) }

    /// The pose of `pins` (`PuppetPin.json`) in `mode`. Throws the core's
    /// sentence for a pin outside the mesh, two pins on one vertex, or a bad
    /// field.
    func solving(mode: PuppetMode, pins: [PuppetPin]) throws -> RasterPuppetPose {
        try solving(fields: ["mode": mode.rawValue, "pins": pins.map(\.json)])
    }

    /// `solving` over the core's own `{"mode","pins"}` fields as given (the
    /// agent's values, which the core validates by name).
    func solving(fields: [String: Any]) throws -> RasterPuppetPose {
        let text = try PuppetWarp.json(fields)
        var error: UnsafeMutablePointer<CChar>?
        guard let pose = dr_puppet_mesh_solve(ptr, text, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Puppet Warp could not solve the pins."))
        }
        return try RasterPuppetPose(owning: pose)
    }
}

/// A solved Puppet Warp pose (`puppet_arap.rs`): the deformed vertices and
/// each pin's snapped vertex.
final class RasterPuppetPose {
    fileprivate let ptr: OpaquePointer
    /// Deformed vertices in canvas pixels, in the mesh's vertex order.
    let positions: [CGPoint]
    /// Each pin's vertex and that vertex's rest position, in pin order.
    let snapped: [(vertex: Int, from: CGPoint)]

    fileprivate init(owning pointer: OpaquePointer) throws {
        ptr = pointer
        var error: UnsafeMutablePointer<CChar>?
        let value: [String: Any]
        do { value = try precisionJSON(dr_puppet_pose_describe(pointer, &error), error) } catch {
            dr_puppet_pose_free(pointer)
            throw error
        }
        let flat = value["positions"] as? [Double] ?? []
        positions = stride(from: 0, to: flat.count - 1, by: 2).map {
            CGPoint(x: flat[$0], y: flat[$0 + 1])
        }
        snapped = (value["pins"] as? [[String: Any]] ?? []).compactMap { pin in
            guard let vertex = pin["vertex"] as? Int, let from = pin["from"] as? [Double],
                  from.count == 2 else { return nil }
            return (vertex, CGPoint(x: from[0], y: from[1]))
        }
    }

    deinit { dr_puppet_pose_free(ptr) }

    /// The workspace preview: `proxy`, a picture of the canvas rect `source`
    /// at any scale, through the pose, optionally times `coverage`
    /// (proxy-sized bytes). Answers the picture and the canvas rect it covers.
    func rendering(_ proxy: RasterImage, source: CGRect, coverage: [UInt8]?) throws
        -> (image: RasterImage, rect: CGRect)
    {
        guard source.width >= 1, source.height >= 1, source.width <= CGFloat(UInt32.max),
              source.height <= CGFloat(UInt32.max) else {
            throw DoctoredCoreError(message: "The Puppet Warp preview needs a layer with pixels.")
        }
        var rect = [Double](repeating: 0, count: 4)
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer? = rect.withUnsafeMutableBufferPointer { buffer in
            let out = buffer.baseAddress
            let render = { (pointer: UnsafePointer<UInt8>?, count: Int) in
                dr_puppet_pose_render(self.ptr, proxy.ptr, Int64(source.minX), Int64(source.minY),
                                      UInt32(source.width), UInt32(source.height), pointer, count,
                                      out, &error)
            }
            guard let coverage else { return render(nil, 0) }
            return coverage.withUnsafeBufferPointer { render($0.baseAddress, $0.count) }
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the Puppet Warp preview."))
        }
        return (RasterImage(owning: result), CGRect(x: rect[0], y: rect[1], width: rect[2], height: rect[3]))
    }
}

extension RasterDocument {
    /// The working-memory context Puppet Warp's and Perspective Warp's core
    /// calls take, read on main (`legacyPrecisionWorkContext` is main-only):
    /// the typed source's own accounting, or the byte one.
    func meshWarpBudget() throws -> (limit: UInt64, other: UInt64) {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            return (context.limit, context.otherRetained)
        }
        return try legacyPrecisionWorkContext()
    }

    /// The mesh of layer `layer`'s opaque pixels, or with `coverage`
    /// (canvas-sized u16) of its opaque AND selected ones (the float form).
    /// Admitted in the core; throws its sentence.
    func puppetMesh(layer: Int, coverage: [UInt16]?, options: PuppetWarpOptions) throws
        -> RasterPuppetMesh
    {
        try puppetMesh(layer: layer, coverage: coverage, fields: options.meshFields)
    }

    /// `puppetMesh` over the core's own `{"density","expansion"}` fields as
    /// given (the agent's values, which the core validates by name).
    func puppetMesh(layer: Int, coverage: [UInt16]?, fields: [String: Any]) throws
        -> RasterPuppetMesh
    {
        let budget = try meshWarpBudget()
        let text = try PuppetWarp.json(fields)
        var error: UnsafeMutablePointer<CChar>?
        let build = { (pointer: UnsafePointer<UInt16>?, count: Int) -> OpaquePointer? in
            if let precision = self.precision {
                return dr_precision_doc_puppet_mesh(precision.ptr, layer, pointer, count, text,
                                                    budget.limit, budget.other, &error)
            }
            return dr_doc_puppet_mesh(self.ptr, layer, pointer, count, text, budget.limit,
                                      budget.other, &error)
        }
        let result = coverage.map { samples in
            samples.withUnsafeBufferPointer { build($0.baseAddress, $0.count) }
        } ?? build(nil, 0)
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Puppet Warp could not mesh the layer."))
        }
        return try RasterPuppetMesh(owning: result)
    }

    /// `{"retained_bytes","scratch_bytes","rect","vertices"}` for a
    /// `puppet_warp` request; throws the core's refusal.
    func puppetWarpPlan(request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        let budget = try meshWarpBudget()
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_puppet_warp_plan(
                precision.ptr, request, budget.limit, budget.other, &error), error)
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for Puppet Warp.") }
        return try precisionJSON(dr_doc_puppet_warp_plan(ptr, request, budget.limit, budget.other,
                                                         &error), error)
    }

    /// The `puppet_warp` edit under `budget` (read on main,
    /// `meshWarpBudget`), so it may run on the worker. nil for an identity
    /// request; throws the core's refusal or `operation cancelled`.
    func editingPuppetWarp(request: String, budget: (limit: UInt64, other: UInt64),
                           control: RasterJobControl?) throws -> RasterDocument? {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            let result = dr_precision_doc_puppet_warp(precision.ptr, request, budget.limit,
                                                      budget.other, control?.ptr, &error)
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Puppet Warp was refused."))
            }
            return try result.map { try RasterDocument(precision: RasterPrecisionDocument(owning: $0)) }
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for Puppet Warp.") }
        let result = dr_doc_puppet_warp(ptr, request, budget.limit, budget.other, control?.ptr, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Puppet Warp was refused."))
        }
        return wrap(result)
    }
}
