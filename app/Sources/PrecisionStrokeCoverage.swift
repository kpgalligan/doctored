import AppKit

// Typed canvas strokes commit through `PrecisionStrokeWork` jobs (`PrecisionStrokeJob.swift`);
// continuous document-space ink and captured fractional retouch source matrices survive
// Actions recording.

/// Shared scalar-only brush rasterization. RGB is supplied independently to
/// Rust; neither the displayed canvas nor a colored byte overlay is sampled.
enum PrecisionStrokeCoverage {
    static func selection(width: Int, height: Int, selection: CanvasSelection?) throws -> PrecisionScalarBuffer {
        let bytes = UInt64(width) * UInt64(height)
        _ = try RasterResources.shared.admit(scratch: bytes * 5 + 4 * 1024 * 1024)
        let coverage = selection?.maskSamples16() ?? [UInt16](repeating: 65535, count: width * height)
        return try .init(samples: coverage, width: width, height: height,
            provenance: selection?.hasNativeCoverage == true ? .nativeU16 : .promotedU8)
    }
}
