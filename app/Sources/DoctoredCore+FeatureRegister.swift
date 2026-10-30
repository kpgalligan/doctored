import Foundation

/// Auto-Align's feature registration (`ffi_feature_register.rs`):
/// the core's corners, descriptors and RANSAC homography between two grey
/// frames, which `AutoAlign.register` runs beside Vision's registration
/// (AutoAlignFeatures.swift). Reads only its arguments; safe on a worker.
enum FeatureRegistration {
    /// The most inliers one call reports: the core's feature cap, which the
    /// header states (`dr_feature_register`; master `feature_points.rs`'s
    /// `MAX_FEATURES`).
    static let maxPairs = 2000

    /// The homography (row-major, m[8] = 1, continuous pixels, y down) taking
    /// a point of `floating` to its place in `reference`, both `width ×
    /// height` grey bytes, and its RANSAC inliers, four values each (floating
    /// x, y, reference x, y; the same convention), and the lens test's
    /// departure on them, pixels (nil when no lens model fits); nil when the core finds no
    /// estimate or a buffer is not `width × height`. Throws the core's
    /// refusal of a size outside 16…4096.
    static func register(floating: [UInt8], reference: [UInt8], width: Int, height: Int) throws
        -> (map: [Double], pairs: [Double], departure: Double?)?
    {
        guard width > 0, height > 0, width <= Int(UInt32.max), height <= Int(UInt32.max),
              floating.count == width * height, reference.count == width * height else {
            return nil
        }
        var map = [Double](repeating: 0, count: 9)
        var pairs = [Double](repeating: 0, count: 4 * maxPairs)
        var inliers: UInt32 = 0
        var departure = Double.nan
        var error: UnsafeMutablePointer<CChar>?
        let found = floating.withUnsafeBufferPointer { first in
            reference.withUnsafeBufferPointer { second in
                map.withUnsafeMutableBufferPointer { out in
                    pairs.withUnsafeMutableBufferPointer { matched in
                        dr_feature_register(first.baseAddress, second.baseAddress, UInt32(width),
                                            UInt32(height), out.baseAddress, matched.baseAddress,
                                            UInt32(maxPairs), &inliers, &departure, &error)
                    }
                }
            }
        }
        if let error {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Feature registration failed."))
        }
        guard found else { return nil }
        return (map, Array(pairs.prefix(4 * min(Int(inliers), maxPairs))),
                departure.isFinite ? departure : nil)
    }

    /// The core's peak working bytes for one `register` at that size; 0
    /// outside 16…4096.
    static func scratchBytes(width: Int, height: Int) -> UInt64 {
        guard width > 0, height > 0, width <= Int(UInt32.max), height <= Int(UInt32.max) else {
            return 0
        }
        return dr_feature_register_scratch_bytes(UInt32(width), UInt32(height))
    }
}
