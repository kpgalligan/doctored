import AppKit

// Color Range, selection transforms and refinement share the captured selection commands'
// stale-result checks and Phase 2 history.
//
// `SelectionCapture.similarShape` runs the core predicate at the document's depth (8-bit
// `dr_doc_select_similar` synchronously, 16-bit the typed `similar` kind through
// `NativeSelectionWork` with the u16 selection as input, tolerance ×257).
//
// `SelectionCapture` includes target/layer/selection identity.

/// Immutable selection inputs. Selection changes do not advance native document
/// revision, so the complete transient state is checked separately at commit.
struct SelectionCapture {
    let doc: RasterDocument
    let revision: UInt64
    let selection: SelectionHistoryState
    let source: RasterImage?
    let layer: Int
    let target: PaintTarget
    let selectedLayers: [Int]

    /// `edgeTrace`: the source is the Magnetic Lasso's trace image
    /// (`ImageDocument.edgeTraceComposite`) instead of the
    /// analysis composite; the two differ only at 32-bit float.
    init(document: ImageDocument, needsSource: Bool = true, edgeTrace: Bool = false) throws {
        guard let doc = document.doc, !document.quickMaskActive, !document.isLiveEditing, !document.hasClosed else {
            throw DoctoredCoreError(message: "Exit Quick Mask and open an image before making this selection.")
        }
        self.doc = doc
        layer = document.activeLayerIndex
        target = document.planeEditTarget
        selectedLayers = document.selectedLayerIndices
        revision = document.revision
        selection = document.selectionHistory
        // Selection tools retain the typed document as their source. This
        // explicit byte derivative is only the sheet/Vision analysis input.
        if !needsSource {
            source = nil
        } else if edgeTrace {
            source = try document.edgeTraceComposite(of: doc)
        } else {
            source = try document.analysisComposite(of: doc)
        }
    }

    func isCurrent(in document: ImageDocument) -> Bool {
        document.doc === doc && document.revision == revision && document.selectionHistory == selection
            && !document.quickMaskActive && !document.isLiveEditing && !document.hasClosed
            && document.activeLayerIndex == layer && document.planeEditTarget == target
            && document.selectedLayerIndices == selectedLayers
    }

    func requireCurrent(in document: ImageDocument) throws {
        guard isCurrent(in: document) else {
            throw DoctoredCoreError(message: "The image or selection changed. Reopen the selection command and try again.")
        }
    }

    /// The worker returns coverage only. AppKit paths, combination and history
    /// registration are deliberately performed after the final stale check.
    func run(in document: ImageDocument, title: String, scratchBytes: UInt64,
             compute: @escaping (RasterJobControl) throws -> [UInt8]) throws -> [UInt8] {
        try requireCurrent(in: document)
        _ = try RasterResources.shared.admit(scratch: scratchBytes)
        let token = try RasterJobControl()
        let outputBytes = UInt64(doc.width * doc.height)
        guard scratchBytes > outputBytes, token.setScratchLimit(scratchBytes - outputBytes) else {
            throw DoctoredCoreError(message: "The selection scratch estimate must include its output coverage.")
        }
        let task = try WorkerTask(scratchEstimate: scratchBytes) { try compute(token) }
        let mask = try BackgroundWork.wait(task, title: title, control: token)
        try requireCurrent(in: document)
        guard !token.isCancelled else { throw JobCancelled() }
        return mask
    }

    /// `antiAlias` false hardens an ellipse or polygon to 0/255 coverage
    /// before anything else (`CanvasSelection.Shape.aliased`).
    func commit(_ shape: CanvasSelection.Shape, in document: ImageDocument, name: String,
                mode: SelectionCombineMode = .replace, feather: Double = 0,
                record: [ActionStep] = .notACommand, antiAlias: Bool = true) throws {
        try requireCurrent(in: document)
        let shape = antiAlias ? shape : shape.aliased(canvasWidth: doc.width, canvasHeight: doc.height)
        guard feather.isFinite, (0...250).contains(feather) else {
            throw DoctoredCoreError(message: "Feather must be between 0 and 250 pixels.")
        }
        if case .mask16(let mask) = shape, mode == .replace, feather == 0,
           mask == selection.selection?.maskSamples16() { return }
        if case .mask(let mask) = shape, mode == .replace, feather == 0,
           selection.selection?.hasNativeCoverage != true,
           mask == selection.selection?.maskBytes() { return }
        let pixels = UInt64(doc.width * doc.height)
        let constructionBytes = pixels * 8 + CanvasSelection.contourWorkingBytes
        _ = try RasterResources.shared.admit(scratch: constructionBytes)
        var incoming = CanvasSelection(shape: shape, canvasWidth: doc.width, canvasHeight: doc.height)
        if feather > 0, let current = incoming, current.hasNativeCoverage || doc.isHighDepth {
            incoming = try current.nativeModified("feather", radius: feather)
            try requireCurrent(in: document)
        } else if feather > 0, let current = incoming {
            // Existing scalar feather math, on the bounded final queue. This
            // older kernel has no cooperative token, so progress has no Cancel.
            let bytes = current.maskBytes(), width = doc.width, height = doc.height
            let scratch = pixels * 12 + CanvasSelection.contourWorkingBytes
            _ = try RasterResources.shared.admit(scratch: scratch)
            let task = try WorkerTask(scratchEstimate: scratch) {
                var output = bytes
                guard RasterSelection.featherMask(&output, width: width, height: height, radius: feather) else {
                    throw DoctoredCoreError(message: "Selection feather could not be computed.")
                }
                return output
            }
            let softened = try BackgroundWork.wait(task, title: "Feather Selection")
            try requireCurrent(in: document)
            _ = try RasterResources.shared.admit(scratch: constructionBytes)
            incoming = CanvasSelection(shape: .mask(softened), canvasWidth: width, canvasHeight: height)
        }
        let result: CanvasSelection?
        if let incoming {
            result = CanvasSelection.combine(selection.selection, with: incoming, mode: mode)
        } else {
            result = mode == .replace || mode == .intersect ? nil : selection.selection
        }
        document.setCanvasSelection(result, actionName: name, record: record)
    }
}

/// Parameters use the same document-profile Lab distances in the sheet and MCP.
/// `range` is Sampled Colors, a hue preset (`reds` … `magentas`, the
/// core's `hue` mode), Skin Tones (`skin_tones`, the core's `skin` mode with
/// `fuzziness` 0–200; tolerance and softness ignored), Out Of Gamut
/// (`out_of_gamut`, `ColorRangeGamut.swift`; tolerance and softness ignored) or
/// a tonal window. `invert` applies to every range;
/// `localized` (Localized Color Clusters, Sampled Colors only) pairs each
/// sample with the canvas pixel it was taken at (`points`) and weights it by
/// distance within `clusterRange` percent of the canvas's longer side.
struct ColorRangeParameters {
    /// The six hue presets, in the Select popup's order.
    static let hueRanges = ["reds", "yellows", "greens", "cyans", "blues", "magentas"]
    /// Skin Tones: `color_range_band::skin_coverage` at `fuzziness`.
    static let skinTones = "skin_tones"
    var range = "sampled"
    var samples: [[Int]] = []
    var samples16: [[Int]]? = nil
    var tolerance = 20.0
    var softness = 10.0
    var lower = 0.0
    var upper = 35.0
    var invert = false
    var localized = false
    var clusterRange = 100.0
    var points: [[Int]] = []
    /// Skin Tones' Fuzziness, 0–200 (default 40); recorded only for Skin Tones.
    var fuzziness = 40.0

    private var isHue: Bool { Self.hueRanges.contains(range) }
    /// Out Of Gamut: the gamut warning's rule, not a colour distance (`ColorRangeGamut`).
    var isOutOfGamut: Bool { range == ColorRangeGamut.range }
    var isSkinTones: Bool { range == Self.skinTones }

    func validated() throws -> ColorRangeParameters {
        guard ["sampled", "shadows", "midtones", "highlights", "tonal"].contains(range) || isHue
                || isOutOfGamut || isSkinTones,
              isHue || isOutOfGamut || isSkinTones
                || (softness.isFinite && (0...100).contains(softness)) else {
            throw DoctoredCoreError(message: "Choose a Color Range mode and finite softness from 0 to 100.")
        }
        if isSkinTones, !(fuzziness.isFinite && (0...200).contains(fuzziness)) {
            throw DoctoredCoreError(message: "Skin Tones needs a Fuzziness from 0 to 200.")
        }
        if range == "sampled" {
            guard tolerance.isFinite, (0...200).contains(tolerance),
                  (1...32).contains((samples16 ?? samples).count), (samples16 ?? samples).allSatisfy({
                    $0.count == 3 && $0.allSatisfy { (0...(samples16 == nil ? 255 : 65535)).contains($0) }
                  }) else {
                throw DoctoredCoreError(message: "Sample 1–32 integer RGB colors (0–255) and use Lab tolerance 0–200.")
            }
            if localized {
                guard clusterRange.isFinite, (0...100).contains(clusterRange),
                      points.count == (samples16 ?? samples).count,
                      points.allSatisfy({ $0.count == 2 && $0.allSatisfy { $0 >= 0 } }) else {
                    throw DoctoredCoreError(message: "Localized Color Clusters needs one canvas point per sample and a Range from 0 to 100 %.")
                }
            }
        } else if localized {
            throw DoctoredCoreError(message: "Localized Color Clusters applies only to Sampled Colors.")
        } else if range == "tonal" {
            guard lower.isFinite, upper.isFinite, (0...100).contains(lower),
                  (0...100).contains(upper), lower <= upper else {
                throw DoctoredCoreError(message: "Custom lightness requires ordered finite L* bounds from 0 to 100.")
            }
        }
        return self
    }

    var coreParameters: [String: Any] {
        var fields: [String: Any]
        if isHue {
            fields = ["mode": "hue", "band": range]
        } else if isSkinTones {
            fields = ["mode": "skin", "fuzziness": fuzziness]
        } else if range == "sampled" {
            fields = ["mode": "sampled", "samples": samples, "tolerance": tolerance, "softness": softness]
            if localized {
                fields["points"] = points
                fields["cluster_range"] = clusterRange
            }
        } else {
            let bounds: (Double, Double)
            switch range {
            case "shadows": bounds = (0, 35)
            case "midtones": bounds = (35, 70)
            case "highlights": bounds = (70, 100)
            default: bounds = (lower, upper)
            }
            fields = ["mode": "tonal", "lower": bounds.0, "upper": bounds.1, "softness": softness]
        }
        if invert { fields["invert"] = true }
        return fields
    }

    var nativeCoreParameters: [String: Any] {
        var fields = coreParameters
        if range == "sampled" { fields["samples"] = samples16 ?? samples.map { $0.map { $0 * 257 } } }
        return fields
    }

    var arguments: [String: Any] {
        var values: [String: Any] = ["range": range]
        if !isHue && !isOutOfGamut && !isSkinTones { values["softness"] = softness }
        if isSkinTones { values["fuzziness"] = fuzziness }
        if range == "sampled" {
            values[samples16 == nil ? "samples" : "samples16"] = samples16 ?? samples
            values["tolerance"] = tolerance
            if localized {
                values["localized"] = true
                values["points"] = points
                values["cluster_range"] = clusterRange
            }
        } else if range == "tonal" {
            values["lower"] = lower
            values["upper"] = upper
        }
        if invert { values["invert"] = true }
        return values
    }

}

struct SelectionTransformParameters {
    var translateX = 0.0
    var translateY = 0.0
    var scaleX = 1.0
    var scaleY = 1.0
    var angle = 0.0
    var anchor: CGPoint
    var filter = "bilinear"

    func validated() throws -> SelectionTransformParameters {
        let values = [translateX, translateY, scaleX, scaleY, angle, Double(anchor.x), Double(anchor.y)]
        guard values.allSatisfy(\.isFinite), abs(translateX) <= 100_000,
              abs(translateY) <= 100_000, abs(anchor.x) <= 100_000, abs(anchor.y) <= 100_000,
              abs(scaleX) >= 0.001, abs(scaleY) >= 0.001,
              abs(scaleX) <= 1000, abs(scaleY) <= 1000, abs(angle) <= 36000,
              ["nearest", "bilinear", "bicubic", "lanczos3"].contains(filter) else {
            throw DoctoredCoreError(message: "Use finite translation/anchor within ±100,000 px, nonzero scale magnitude 0.001–1000, angle within ±36,000°, and a supported filter.")
        }
        return self
    }

    var matrix: CGAffineTransform {
        var transform = LayerTransform(pivot: anchor)
        transform.translation = CGVector(dx: translateX, dy: translateY)
        transform.angle = angle * .pi / 180
        transform.scaleX = scaleX
        transform.scaleY = scaleY
        return transform.matrix
    }

    var sampler: DrResizeFilter {
        switch filter {
        case "nearest": return DR_FILTER_NEAREST
        case "bicubic": return DR_FILTER_CATMULL_ROM
        case "lanczos3": return DR_FILTER_LANCZOS3
        default: return DR_FILTER_BILINEAR
        }
    }

    var arguments: [String: Any] {
        ["translate_x": translateX, "translate_y": translateY, "scale_x": scaleX,
         "scale_y": scaleY, "angle": angle, "anchor_x": anchor.x, "anchor_y": anchor.y,
         "filter": filter]
    }
}

/// Separate keys preserve the Codable schema of existing selection options.
final class QuickSelectionSettings {
    static let shared = QuickSelectionSettings()
    var tolerance: Double {
        get { (UserDefaults.standard.object(forKey: "QuickSelectionTolerance") as? Double) ?? 25 }
        set { UserDefaults.standard.set(newValue, forKey: "QuickSelectionTolerance") }
    }
    var edge: Double {
        get { (UserDefaults.standard.object(forKey: "QuickSelectionEdge") as? Double) ?? 12 }
        set { UserDefaults.standard.set(newValue, forKey: "QuickSelectionEdge") }
    }
    /// The brush diameter in canvas pixels, 1–512. The core's radius
    /// is `(size − 1) / 2`, so size 1 is the seed pixel alone.
    var size: Double {
        get { min(512, max(1, (UserDefaults.standard.object(forKey: "QuickSelectionSize") as? Double) ?? 30)) }
        set { UserDefaults.standard.set(min(512, max(1, newValue)), forKey: "QuickSelectionSize") }
    }
    /// Enhance Edge: refine the stroke's result against the picture's edges.
    var enhanceEdge: Bool {
        get { UserDefaults.standard.bool(forKey: "QuickSelectionEnhanceEdge") }
        set { UserDefaults.standard.set(newValue, forKey: "QuickSelectionEnhanceEdge") }
    }
}
