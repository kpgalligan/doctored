import AppKit

extension RasterImage {
    /// One plane of THIS image (`width * height` bytes, row 0 = top) — the
    /// ONE plane reader. Exact for the opaque grayscale images the
    /// plane-image getters return, which is what makes `.luma` the
    /// definition of "take the result's gray". nil for `DR_PLANE_MASK`.
    func plane(_ plane: DrPlane) -> [UInt8]? {
        guard width > 0, height > 0 else { return nil }
        var out = [UInt8](repeating: 0, count: width * height)
        let ok = out.withUnsafeMutableBufferPointer { buffer in
            dr_image_plane(ptr, plane, buffer.baseAddress, UInt32(width), UInt32(height))
        }
        return ok ? out : nil
    }

    /// Shorthand for `plane(DR_PLANE_LUMA)` — how an op's result comes back
    /// as a plane.
    func lumaPlane() -> [UInt8]? { plane(DR_PLANE_LUMA) }

    /// One plane of THIS image as an opaque grayscale image — the DISPLAY
    /// reader, run on the cached projection (`ImageDocument.projection`) so a
    /// panel row or a canvas redraw never re-flattens. `maxSide` 0 means full
    /// size.
    func planeImage(_ plane: DrPlane, maxSide: Int) -> RasterImage? {
        guard maxSide >= 0 else { return nil }
        return wrap(dr_image_plane_image(ptr, plane, UInt32(maxSide)))
    }
}

/// Plane arithmetic on caller-owned buffers — the ONE function behind Apply
/// Image and Calculations. Mirrors `RasterSelection`'s shape: planes cross
/// the FFI as raw canvas-sized u8 buffers, not handles.
enum RasterPlaneMath {
    /// `base` blended with `source` IN PLACE:
    /// `base = lerp(base, blend(base, source), opacity)` per pixel, through
    /// the same blend table the projection uses. `invertBase`/`invertSource`
    /// invert an operand FIRST, so the complement is both blended and lerped
    /// from. false on a length mismatch, a non-finite opacity, or one of the
    /// four HSL modes (`DrBlendMode.grayDegenerateModes`), which say nothing
    /// about a single gray plane — blend three planes as one colour with
    /// `blendRGB` for those.
    static func blend(
        _ base: inout [UInt8], with source: [UInt8], width: Int, height: Int,
        mode: DrBlendMode, opacity: Double, invertBase: Bool, invertSource: Bool
    ) -> Bool {
        guard width > 0, height > 0, base.count == width * height,
              source.count == width * height
        else { return false }
        return base.withUnsafeMutableBufferPointer { b in
            source.withUnsafeBufferPointer { s in
                dr_blend_planes(
                    b.baseAddress, s.baseAddress, UInt32(width), UInt32(height),
                    mode, Float(opacity), invertBase, invertSource)
            }
        }
    }

    /// Red, green and blue blended AS ONE COLOUR, in place on the three base
    /// planes — Apply Image with an RGB source onto an RGB target. Same rules
    /// as `blend`, and the one form in which the four HSL modes mean anything
    /// (blending the planes independently would hand each of them a gray
    /// triple, which is what made three of the four the identity). false on a
    /// length mismatch or a non-finite opacity.
    /// The three planes are separate `inout` parameters rather than one array
    /// of three: `base[0].withUnsafeMutableBufferPointer { base[1]… }` would
    /// be overlapping access to the same array, which Swift's exclusivity
    /// checking traps on.
    // Six buffers is what "blend a colour" costs at this boundary; bundling
    // them into a struct would only move the count, and the C declaration
    // spells the same six out.
    static func blendRGB(
        red: inout [UInt8], green: inout [UInt8], blue: inout [UInt8],
        withRed sourceRed: [UInt8], green sourceGreen: [UInt8], blue sourceBlue: [UInt8],
        width: Int, height: Int, mode: DrBlendMode, opacity: Double,
        invertBase: Bool, invertSource: Bool
    ) -> Bool {
        let count = width * height
        guard width > 0, height > 0,
              red.count == count, green.count == count, blue.count == count,
              sourceRed.count == count, sourceGreen.count == count, sourceBlue.count == count
        else { return false }
        // Six nested pointer scopes rather than a flattened interleave: the
        // core takes the three planes as they already are, so nothing is
        // copied on either side of the boundary.
        return red.withUnsafeMutableBufferPointer { r in
            green.withUnsafeMutableBufferPointer { g in
                blue.withUnsafeMutableBufferPointer { b in
                    sourceRed.withUnsafeBufferPointer { sr in
                        sourceGreen.withUnsafeBufferPointer { sg in
                            sourceBlue.withUnsafeBufferPointer { sb in
                                dr_blend_planes_rgb(
                                    r.baseAddress, g.baseAddress, b.baseAddress,
                                    sr.baseAddress, sg.baseAddress, sb.baseAddress,
                                    UInt32(width), UInt32(height), mode, Float(opacity),
                                    invertBase, invertSource)
                            }
                        }
                    }
                }
            }
        }
    }
}
