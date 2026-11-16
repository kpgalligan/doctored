import CoreGraphics

/// One dab a dynamics stamp resolved (`dr_brush_dynamics_dabs`): its centre, its
/// diameter, its deposit factor, which multiplies the tip's flow, and its tip shape
/// (angle 0 and roundness 1 when the request has no tip shape).
struct ResolvedDab: Equatable {
    var point: CGPoint
    var diameter: CGFloat
    var factor: CGFloat
    var angleDegrees: CGFloat
    var roundness: CGFloat
}

/// A parsed brush dynamics request (`dr_brush_dynamics_create`), immutable, so a
/// stroke's walk resolves every stamp without parsing again. The rule is the
/// core's (`brush_dynamics.rs`, `designs/brush-dynamics.md`).
final class RasterBrushDynamics {
    private let ptr: OpaquePointer

    init(request: String) throws {
        var err: UnsafeMutablePointer<CChar>?
        guard let ptr = request.withCString({ dr_brush_dynamics_create($0, &err) }) else {
            throw DoctoredCoreError(message: takeErrorMessage(err,
                fallback: "The brush dynamics could not be read."))
        }
        self.ptr = ptr
    }

    deinit { dr_brush_dynamics_free(ptr) }

    /// The nominal diameter the walk's spacing follows, for `size` and the pen
    /// sample; `size` when the core refuses the size.
    func nominalDiameter(size: CGFloat, pen: PenSample) -> CGFloat {
        var out = [Double](repeating: 0, count: 2)
        guard dr_brush_dynamics_nominal(ptr, Double(size), pen.pressure, pen.tiltX, pen.tiltY,
                                        &out) else { return size }
        return CGFloat(out[0])
    }

    /// Nominal stamp `index` at `point` resolved into its dabs; `direction` is the
    /// move from the stamp before (zero when there is none), which scatter reads, and
    /// `heading` the direction of travel the angle reads (zero until the stroke first
    /// moves). Empty when the core refuses the sample (a non-finite position or size).
    func dabs(index: UInt64, at point: CGPoint, size: CGFloat, pen: PenSample,
              direction: CGVector, heading: CGVector) -> [ResolvedDab] {
        let sample = [Double(point.x), Double(point.y), Double(size), pen.pressure, pen.tiltX,
                      pen.tiltY, Double(direction.dx), Double(direction.dy), pen.rotation,
                      Double(heading.dx), Double(heading.dy)]
        var out = [Double](repeating: 0, count: 6 * BrushDynamics.maximumCount)
        let n = dr_brush_dynamics_dabs(ptr, index, sample, sample.count, &out, out.count)
        return (0..<n).map { k in
            ResolvedDab(point: CGPoint(x: out[6 * k], y: out[6 * k + 1]),
                        diameter: CGFloat(out[6 * k + 2]), factor: CGFloat(out[6 * k + 3]),
                        angleDegrees: CGFloat(out[6 * k + 4]), roundness: CGFloat(out[6 * k + 5]))
        }
    }
}

/// A brush texture's core handle (`dr_brush_texture_create`): the straight RGBA8 tile's level
/// at its scale, anchored at the canvas origin, with its depth. Immutable; the textured
/// rasterizers read it (`RasterSelection.nativeBrush`, `nativeBrushOverlay`). The rule is the
/// core's (`brush_texture.rs`).
final class RasterBrushTexture {
    let ptr: OpaquePointer

    init(rgba: [UInt8], width: Int, height: Int, scale: Double, depth: Double) throws {
        guard width > 0, height > 0, rgba.count == width * height * 4,
              let w = UInt32(exactly: width), let h = UInt32(exactly: height) else {
            throw DoctoredCoreError(message: NativeBrushStroke.textureRefusal)
        }
        var err: UnsafeMutablePointer<CChar>?
        guard let ptr = rgba.withUnsafeBufferPointer({
            dr_brush_texture_create($0.baseAddress, w, h, scale, depth, &err)
        }) else {
            throw DoctoredCoreError(message: takeErrorMessage(err,
                fallback: "The brush texture could not be read."))
        }
        self.ptr = ptr
    }

    deinit { dr_brush_texture_free(ptr) }
}
