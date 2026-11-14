import AppKit

/// Selection-mask operations. Unlike images and documents, selection masks
/// cross the FFI as raw canvas-sized u8 coverage buffers (row 0 = top),
/// not handles.
enum RasterSelection {
    static func paintQuickMask(
        _ mask: inout [UInt8], width: Int, height: Int,
        overlay: UnsafePointer<UInt8>, scale: UInt8
    ) -> Bool {
        guard width > 0, height > 0, width <= 100_000_000 / height,
              mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_paint_quick_mask(buffer.baseAddress, overlay,
                                         UInt32(width), UInt32(height), scale)
        }
    }

    static func paintQuickMask(
        _ mask: inout [UInt8], width: Int, height: Int, runs: Data
    ) -> Bool {
        guard width > 0, height > 0, width <= 100_000_000 / height,
              mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            runs.withUnsafeBytes { bytes in
                dr_selection_paint_quick_mask_rle(buffer.baseAddress, UInt32(width), UInt32(height),
                                                  bytes.bindMemory(to: UInt8.self).baseAddress, runs.count)
            }
        }
    }

    /// Gaussian-feathers a coverage mask in place. Sampling clamps to the
    /// canvas edges, so a selection touching the border keeps full
    /// coverage there; radius <= 0 leaves the mask untouched. The core works
    /// over the selection's bounding rect grown by the kernel, not the
    /// canvas. false on a size mismatch, or a radius that is not finite or
    /// is above 250 (every caller offers at most 250).
    static func featherMask(
        _ mask: inout [UInt8], width: Int, height: Int, radius: Double
    ) -> Bool {
        guard width > 0, height > 0, mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_feather(
                buffer.baseAddress, UInt32(width), UInt32(height), Float(radius))
        }
    }

    // Morphology companions to featherMask, same conventions throughout:
    // in place, canvas edges are not contours, a parameter <= 0 leaves the
    // mask untouched, false on a size mismatch or a non-finite parameter.

    /// Grows (dilates) the coverage mask in place by `radius` px of true
    /// Euclidean distance from its 50% contour, re-antialiasing the new
    /// edge.
    static func growMask(
        _ mask: inout [UInt8], width: Int, height: Int, radius: Double
    ) -> Bool {
        guard width > 0, height > 0, mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_grow(
                buffer.baseAddress, UInt32(width), UInt32(height), Float(radius))
        }
    }

    /// Shrinks (erodes) the coverage mask in place by `radius` px — grow
    /// with the sign flipped, so edges move inward.
    static func shrinkMask(
        _ mask: inout [UInt8], width: Int, height: Int, radius: Double
    ) -> Bool {
        guard width > 0, height > 0, mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_shrink(
                buffer.baseAddress, UInt32(width), UInt32(height), Float(radius))
        }
    }

    /// Replaces the coverage mask in place with an anti-aliased band
    /// `widthPx` wide straddling its 50% contour.
    static func borderMask(
        _ mask: inout [UInt8], width: Int, height: Int, widthPx: Double
    ) -> Bool {
        guard width > 0, height > 0, mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_border(
                buffer.baseAddress, UInt32(width), UInt32(height), Float(widthPx))
        }
    }

    /// Replaces the coverage mask in place with the Edit ▸ Stroke… band
    /// `widthPx` wide at `location` (the canvas edge counts as a contour for
    /// Inside). False on a bad size, a non-finite width or an unknown
    /// location; `widthPx <= 0` answers true with an all-zero mask.
    static func strokeMask(
        _ mask: inout [UInt8], width: Int, height: Int, widthPx: Double,
        location: DrStrokeLocation
    ) -> Bool {
        guard width > 0, height > 0, mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_stroke(
                buffer.baseAddress, UInt32(width), UInt32(height), Float(widthPx), location)
        }
    }

    /// Smooths the coverage mask in place: featherMask's Gaussian blur
    /// followed by a smoothstep remap, so corners round and jagged edges
    /// reconcile while soft coverage stays soft. false above 250, as
    /// featherMask.
    static func smoothMask(
        _ mask: inout [UInt8], width: Int, height: Int, radius: Double
    ) -> Bool {
        guard width > 0, height > 0, mask.count == width * height else { return false }
        return mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_smooth(
                buffer.baseAddress, UInt32(width), UInt32(height), Float(radius))
        }
    }
}
