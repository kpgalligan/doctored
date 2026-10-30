import AppKit
import Vision

// Auto Straighten runs `HorizonDetection.detect` (Vision `VNDetectHorizonRequest` on
// `ImageDocument.analysisComposite(of:)`, the byte analysis image `SelectionCapture` also reads) on
// the bounded worker.

/// Vision's horizon detector behind the Crop tool's Auto button and MCP
/// `detect_horizon` — another platform-model seam beside
/// `SubjectSelection` and `RedEye`: it turns a composite into one angle and
/// knows nothing about pixels or the crop box.
///
/// `VNDetectHorizonRequest`'s `angle` is used as the crop angle directly,
/// no negation: measured on synthetic horizons, a horizon descending to the
/// right by θ (canvas y-down) reports +θ, which is exactly the Straighten
/// angle that levels it. The detector answers only within about 1°–10° of
/// tilt (a level or steeper horizon reports nothing), quantized to about
/// 0.125°, and needs texture on both sides of the horizon — a flat or
/// noiseless image reports nothing. A detection costs about 5–25 ms.
/// The observation's `confidence` is never read.
enum HorizonDetection {
    /// The detected straighten in degrees; nil when Vision finds no horizon
    /// or answers a non-finite angle or one beyond ±45°. Runs synchronously;
    /// safe on a worker (it reads only `image`).
    static func detect(in image: CGImage) throws -> Double? {
        let request = VNDetectHorizonRequest()
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        try handler.perform([request])
        guard let observation = request.results?.first else { return nil }
        let degrees = Double(observation.angle) * 180 / .pi
        guard degrees.isFinite, abs(degrees) <= 45 else { return nil }
        return degrees
    }

    /// The document's horizon: its byte analysis composite
    /// (`analysisComposite(of:)` — the tone-mapped display for float) on the
    /// bounded worker, `SubjectTargeting.analyze`'s shape. Main thread.
    static func detect(_ document: ImageDocument, title: String) throws -> Double? {
        guard let doc = document.doc else {
            throw DoctoredCoreError(message: "Open an image before detecting its horizon.")
        }
        let composite = try document.analysisComposite(of: doc)
        guard let image = composite.makeCGImage(in: doc.colorSpace) else {
            throw DoctoredCoreError(message: "The image could not be flattened for horizon detection.")
        }
        let scratch = scratchBytes(width: doc.width, height: doc.height)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let task = try WorkerTask(scratchEstimate: scratch) { try detect(in: image) }
        return try BackgroundWork.wait(task, title: title)
    }

    /// Vision's working copies of a `width × height` image, plus its model.
    static func scratchBytes(width: Int, height: Int) -> UInt64 {
        UInt64(width * height) * 8 + 32 * 1024 * 1024
    }
}
