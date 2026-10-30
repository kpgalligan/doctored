import Foundation

/// A reviewed map always describes proposed oriented centres in saved oriented
/// coordinates. Decoder frame registration is captured independently of optics.
enum RawMigrationGeometry {
    static let identity = [1.0, 0, 0, 1, 0, 0]

    static func compose(_ a: [Double], _ b: [Double]) -> [Double] {
        [a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1],
         a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3],
         a[0] * b[4] + a[2] * b[5] + a[4],
         a[1] * b[4] + a[3] * b[5] + a[5]]
    }
    static func inverse(_ m: [Double]) throws -> [Double] {
        guard m.count == 6, m.allSatisfy(\.isFinite) else {
            throw DoctoredCoreError(message: "Invalid RAW migration coordinate matrix.")
        }
        let det = m[0] * m[3] - m[1] * m[2]
        guard det.isFinite, abs(det) >= 1e-12 else {
            throw DoctoredCoreError(message: "Singular RAW migration coordinate matrix.")
        }
        return [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det,
                (m[2] * m[5] - m[3] * m[4]) / det,
                (m[1] * m[4] - m[0] * m[5]) / det]
    }
    static func exif(_ orientation: UInt32, width: Double, height: Double) throws -> [Double] {
        switch orientation {
        case 1: return identity
        case 2: return [-1, 0, 0, 1, width, 0]
        case 3: return [-1, 0, 0, -1, width, height]
        case 4: return [1, 0, 0, -1, 0, height]
        case 5: return [0, 1, 1, 0, 0, 0]
        case 6: return [0, -1, 1, 0, 0, height]
        case 7: return [0, -1, -1, 0, width, height]
        case 8: return [0, 1, -1, 0, width, 0]
        default: throw DoctoredCoreError(message: "Unsupported RAW migration orientation.")
        }
    }
    static func dimensions(_ frame: RawDecoderFrame) throws -> (Int, Int) {
        try frame.validate()
        let width = Int(frame.extent[2]), height = Int(frame.extent[3])
        return frame.orientation >= 5 ? (height, width) : (width, height)
    }
    static func cameraMap(_ frame: RawDecoderFrame) throws -> [Double] {
        let e = try exif(frame.orientation, width: frame.extent[2], height: frame.extent[3])
        return compose([1, 0, 0, 1, frame.extent[0], frame.extent[1]], e)
    }
    static func frame(orientation: UInt32, width: Int, height: Int) -> RawDecoderFrame {
        let swapped = orientation >= 5
        return RawDecoderFrame(orientation: orientation,
            extent: [0, 0, Double(swapped ? height : width), Double(swapped ? width : height)])
    }
    static func resolve(saved: RawDevelopmentRecipe, savedSize: (Int, Int),
                        proposed: RawDecoderFrame, proposedSize: (Int, Int),
                        explicit: RawSourceMap?) throws -> (map: RawSourceMap, matrix: [Double], changed: Bool) {
        try proposed.validate()
        guard try dimensions(proposed) == proposedSize else {
            throw DoctoredCoreError(message: "The proposed decoder frame differs from its image dimensions.")
        }
        if let explicit {
            guard explicit.kind != .lens else {
                throw DoctoredCoreError(message: "Migration registration requires EXIF or affine coordinates.")
            }
            let dimensions = try explicit.destination(from: savedSize)
            guard dimensions == proposedSize else {
                throw DoctoredCoreError(message: "The migration map dimensions differ from the proposed source.")
            }
            let matrix = try explicit.affine(from: savedSize)
            return (explicit, matrix, savedSize != proposedSize || matrix != identity)
        }
        let old = saved.decoderFrame ?? frame(orientation: saved.orientation,
            width: savedSize.0, height: savedSize.1)
        guard try dimensions(old) == savedSize else {
            throw DoctoredCoreError(message: "The saved RAW decoder frame differs from its source.")
        }
        if saved.decoderFrame == nil {
            let oldCamera = old.extent
            let newCamera = proposed.extent
            guard oldCamera[2] == newCamera[2], oldCamera[3] == newCamera[3],
                  oldCamera[0] == newCamera[0], oldCamera[1] == newCamera[1] else {
                throw DoctoredCoreError(message: "This legacy RAW extent changed; supply a reviewed inverse migration map.")
            }
        }
        let m = compose(try inverse(cameraMap(old)), try cameraMap(proposed))
        guard m.allSatisfy({ $0.isFinite && abs($0) <= 1e9 }),
              m.allSatisfy({ $0.rounded() == $0 }) else {
            throw DoctoredCoreError(message: "Decoder frames need an explicit reviewed inverse map.")
        }
        let width = UInt32(proposedSize.0), height = UInt32(proposedSize.1)
        let map = RawSourceMap(width: width, height: height, inverse: m)
        return (map, m, savedSize != proposedSize || m != identity)
    }
}

extension RawSourceMap {
    func destination(from source: (Int, Int)) throws -> (Int, Int) {
        switch kind {
        case .exif:
            guard let orientation else { throw DoctoredCoreError(message: "Missing EXIF orientation.") }
            return orientation >= 5 ? (source.1, source.0) : source
        case .affine:
            guard let width, let height else { throw DoctoredCoreError(message: "Missing affine dimensions.") }
            return (Int(width), Int(height))
        case .lens: return source
        }
    }
    func affine(from source: (Int, Int)) throws -> [Double] {
        switch kind {
        case .exif:
            guard let orientation else { throw DoctoredCoreError(message: "Missing EXIF orientation.") }
            return try RawMigrationGeometry.exif(orientation,
                width: Double(source.0), height: Double(source.1))
        case .affine:
            guard let inverse, inverse.count == 6 else {
                throw DoctoredCoreError(message: "Missing affine migration coordinates.")
            }
            return inverse
        case .lens:
            throw DoctoredCoreError(message: "Lens geometry is not a decoder migration registration.")
        }
    }
}
