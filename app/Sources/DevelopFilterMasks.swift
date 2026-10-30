import AppKit
import CryptoKit

// Filters ▸ Develop Filter's masks: the host half of the core's `masks` request
// (`core/src/develop_filter_masks.rs` states the order, the mappings and the plan). The core runs
// the radial, linear, brush, full and range masks itself; a generated shape — subject, people or
// sky — is made here, on a ≤ 1024 px analysis copy of the target layer, and handed to the core as
// a `map` shape. The subject request is `SubjectSelection.mask(in:)` (Select ▸ Subject's call),
// the people request `SubjectTargeting.people(in:)` (Select ▸ All People's) and the sky request
// `SkySelection.coverage` (the M1 sky model's own matte, the RAW Sky local's call). Vision and
// Core ML run only inside one `BackgroundWork` job.
//
// A recorded step never carries a map or the document's profile (`recorded`): a replay or ⌃F
// runs the model again on the layer it reaches, which is Depth Blur's rule.

/// A mask whose coverage a model makes.
enum DevelopFilterGeneratedKind: String, CaseIterable {
    case subject, people, sky

    var title: String {
        switch self {
        case .subject: return "Subject"
        case .people: return "People"
        case .sky: return "Sky"
        }
    }

    /// The sentence a request that found nothing refuses with: the selection commands' own.
    var nothingFound: String {
        switch self {
        case .subject: return SubjectSelection.Failure.noSubject.message
        case .people: return SubjectTargeting.noPeopleMessage
        case .sky: return SkySelection.noSky
        }
    }

    /// The model that makes it, as the cache names it: the sky entry's id and version, or
    /// Vision's.
    fileprivate var modelKey: String? {
        guard self == .sky else { return "vision" }
        return ModelGate.ready("sky").map { "\($0.entry.id)@\($0.entry.version)" }
    }
}

enum DevelopFilterMasks {
    static let maximumMasks = 16, mapLongSide = 1024
    /// The core's refusals by name (`develop_filter_masks.rs`).
    static let floatRefusal = "develop_filter masks are not available on a 32-bit float layer"
    static let scalarRefusal = "develop_filter masks edit layer pixels only"

    /// Main thread. `params` with each generated shape (`{op: subject|people|sky}`) replaced by
    /// its `map` shape (from the cache, else one `BackgroundWork` job for every missing kind), and
    /// `range_profile` (the document's profile, base64) when a mask has a range. Refuses masks
    /// on a 32-bit float or CMYK document, a missing model and a model that found nothing. With
    /// no masks `params` is returned unchanged.
    static func resolved(_ params: [String: Any], document: ImageDocument, layer: Int) throws -> [String: Any] {
        guard let raw = params["masks"] as? [Any], !raw.isEmpty else { return params }
        guard let doc = document.doc else { throw DoctoredCoreError(message: "No document is open.") }
        if doc.isFloat { throw DoctoredCoreError(message: floatRefusal) }
        if doc.isCmyk { throw DoctoredCoreError(message: CmykAdjustFilter.refusal(op: "develop_filter", layer: false)) }
        let masks = try raw.map(normalizedRange)
        var result = params
        result["masks"] = masks
        if masks.contains(where: { ($0 as? [String: Any])?["range"].map { !($0 is NSNull) } == true }) {
            result["range_profile"] = rangeProfile(doc)
        }
        let kinds = Set(masks.compactMap { generatedKind($0) })
        guard !kinds.isEmpty else { return result }
        let maps = try generate(kinds, document: document, layer: layer)
        result["masks"] = masks.map { mask -> Any in
            guard let kind = generatedKind(mask), var object = mask as? [String: Any],
                  let map = maps[kind] else { return mask }
            object["shape"] = map
            return object
        }
        return result
    }

    /// The `map` shape of one kind on `layer` (the sheet's New Subject / People / Sky).
    static func mapShape(_ kind: DevelopFilterGeneratedKind, document: ImageDocument, layer: Int) throws
        -> [String: Any] {
        guard let shape = try generate([kind], document: document, layer: layer)[kind] else {
            throw DoctoredCoreError(message: kind.nothingFound)
        }
        return shape
    }

    /// One mask with its `range` as Develop reads one (`RawLocalRange(json:)`): `null` drops the
    /// key, and an object is rewritten with every field (falloffs 0 and invert false when
    /// omitted), refused by `RawLocalRange.refusal`. Anything that is not a mask object is the
    /// core's to refuse.
    private static func normalizedRange(_ mask: Any) throws -> Any {
        guard var object = mask as? [String: Any], let range = object["range"] else { return mask }
        if range is NSNull {
            object.removeValue(forKey: "range")
            return object
        }
        guard let fields = range as? [String: Any] else { throw DoctoredCoreError(message: RawLocalRange.refusal) }
        object["range"] = try RawLocalRange(json: fields).json
        return object
    }

    /// A develop_filter apply_filter step with each map shape back to {op: generated} and no
    /// range_profile; others unchanged.
    static func recorded(_ step: ActionStep) -> ActionStep {
        guard step.tool == "apply_filter", step.arguments["filter"] as? String == "develop_filter" else {
            return step
        }
        var arguments = step.arguments
        let hadProfile = arguments.removeValue(forKey: "range_profile") != nil
        var changed = hadProfile
        if let masks = arguments["masks"] as? [Any] {
            arguments["masks"] = masks.map { mask -> Any in
                guard var object = mask as? [String: Any], let shape = object["shape"] as? [String: Any],
                      shape["op"] as? String == "map", let generated = shape["generated"] as? String else {
                    return mask
                }
                changed = true
                object["shape"] = ["op": generated]
                return object
            }
        }
        guard changed else { return step }
        return ActionStep(tool: step.tool, arguments: arguments, enabled: step.enabled, onError: step.onError,
                          geometry: step.geometry, note: step.note)
    }

    /// The document's profile as base64 ICC for a range: its own, else sRGB (an untagged
    /// document displays as sRGB).
    static func rangeProfile(_ doc: RasterDocument) -> String? {
        let data = doc.iccProfile ?? (CGColorSpace(name: CGColorSpace.sRGB)?.copyICCData() as Data?)
        return data?.base64EncodedString()
    }

    private static func generatedKind(_ mask: Any) -> DevelopFilterGeneratedKind? {
        guard let shape = (mask as? [String: Any])?["shape"] as? [String: Any],
              let op = shape["op"] as? String else { return nil }
        return DevelopFilterGeneratedKind(rawValue: op)
    }

    /// The analysis copy of `layer`: at most `mapLongSide` on its long side (an 8-bit layer
    /// resized, a 16-bit one the core's thumbnail), as straight RGBA8.
    private static func analysis(_ doc: RasterDocument, layer: Int) throws -> RasterImage {
        let empty = DoctoredCoreError(message: "develop_filter masks need visible pixels; layer \(layer) is empty")
        if doc.isHighDepth {
            guard let source = doc.precision?.layerSource(layer) else { throw empty }
            let context = try PrecisionWorkContext(image: source)
            return try source.thumbnailRGBA8(maxSide: mapLongSide, workingLimit: context.limit,
                                             otherRetained: context.otherRetained)
        }
        guard let image = doc.layerImage(layer), image.width > 0, image.height > 0 else { throw empty }
        let long = max(image.width, image.height)
        guard long > mapLongSide else { return image }
        let scale = Double(mapLongSide) / Double(long)
        let w = max(1, Int((Double(image.width) * scale).rounded()))
        let h = max(1, Int((Double(image.height) * scale).rounded()))
        guard let resized = image.resized(w: w, h: h, filter: DR_FILTER_BILINEAR) else { throw empty }
        return resized
    }

    /// The map shapes of `kinds` on `layer`, from the cache or one admitted job.
    private static func generate(_ kinds: Set<DevelopFilterGeneratedKind>, document: ImageDocument,
                                 layer: Int) throws -> [DevelopFilterGeneratedKind: [String: Any]] {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "No document is open.") }
        let image = try analysis(doc, layer: layer)
        // The cache key names the picture, so a copy that cannot be hashed is never cached.
        guard let digest = image.withPlateBytes({ pixels in
            SHA256.hash(data: UnsafeRawBufferPointer(start: pixels, count: image.width * image.height * 4))
                .map { String(format: "%02x", $0) }.joined()
        }) else {
            throw DoctoredCoreError(message: "The layer could not be prepared for the mask models.")
        }
        var shapes: [DevelopFilterGeneratedKind: [String: Any]] = [:]
        var missing: [(DevelopFilterGeneratedKind, String)] = []
        for kind in DevelopFilterGeneratedKind.allCases where kinds.contains(kind) {
            guard let model = kind.modelKey else {
                throw DoctoredCoreError(message: ModelGate.notInstalled("sky"))
            }
            let key = "\(kind.rawValue)|\(model)|\(digest)"
            if let samples = DevelopFilterMaskCache.shared.coverage(key) {
                shapes[kind] = map(kind, samples, image)
            } else {
                missing.append((kind, key))
            }
        }
        guard !missing.isEmpty else { return shapes }
        guard let cg = image.makeCGImage(in: doc.colorSpace) else {
            throw DoctoredCoreError(message: "The layer could not be prepared for the mask models.")
        }
        let pixels = UInt64(image.width) * UInt64(image.height)
        var scratch = pixels * ModelMaskRun.captureBytesPerPixel * UInt64(missing.count)
        let sky = missing.contains { $0.0 == .sky } ? ModelGate.ready("sky") : nil
        if let sky {
            scratch += sky.entry.inferenceBytes
                + ModelMaskRun.coverageScratch(width: image.width, height: image.height)
        }
        _ = try RasterResources.shared.admit(scratch: scratch)
        let context = try PrecisionWorkContext()
        let control = try RasterJobControl()
        let found = try BackgroundWork.wait(title: "Making Develop Filter Masks", control: control,
                                            revealAfter: DocumentJobs.panelDelay) {
            try WorkerTask(scratchEstimate: scratch) { () -> [DevelopFilterGeneratedKind: [UInt8]] in
                var result: [DevelopFilterGeneratedKind: [UInt8]] = [:]
                for (kind, _) in missing {
                    guard !control.isCancelled else { throw JobCancelled() }
                    result[kind] = try autoreleasepool {
                        try coverage(kind, cg, limit: context.limit, other: context.otherRetained)
                    }
                }
                return result
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        for (kind, key) in missing {
            guard let samples = found[kind] else { throw DoctoredCoreError(message: kind.nothingFound) }
            DevelopFilterMaskCache.shared.store(key, samples)
            shapes[kind] = map(kind, samples, image)
        }
        return shapes
    }

    /// Worker thread. One kind's byte coverage of `image`, refused by its own sentence when it
    /// found nothing.
    private static func coverage(_ kind: DevelopFilterGeneratedKind, _ image: CGImage, limit: UInt64,
                                 other: UInt64) throws -> [UInt8] {
        let samples: [UInt8]
        switch kind {
        case .subject:
            do { samples = try SubjectSelection.mask(in: image).mask } catch let failure as SubjectSelection.Failure {
                throw DoctoredCoreError(message: failure.message)
            }
        case .people: samples = try SubjectTargeting.people(in: image)
        case .sky: samples = try SkySelection.coverage(in: image, limit: limit, other: other, control: nil)
        }
        guard samples.contains(where: { $0 != 0 }) else { throw DoctoredCoreError(message: kind.nothingFound) }
        return samples
    }

    private static func map(_ kind: DevelopFilterGeneratedKind, _ samples: [UInt8], _ image: RasterImage)
        -> [String: Any] {
        ["op": "map", "generated": kind.rawValue, "width": image.width, "height": image.height,
         "samples": Data(samples).base64EncodedString()]
    }
}

/// The last four generated coverages, keyed by kind, model and the analysis copy's SHA-256, so
/// a sheet's revisions and a repeated `apply_filter` do not run a model twice on one picture.
final class DevelopFilterMaskCache {
    static let shared = DevelopFilterMaskCache()
    private static let capacity = 4
    private let lock = NSLock()
    private var entries: [(key: String, samples: [UInt8])] = []

    func coverage(_ key: String) -> [UInt8]? {
        lock.lock()
        defer { lock.unlock() }
        guard let index = entries.firstIndex(where: { $0.key == key }) else { return nil }
        let entry = entries.remove(at: index)
        entries.append(entry)
        return entry.samples
    }

    func store(_ key: String, _ samples: [UInt8]) {
        lock.lock()
        defer { lock.unlock() }
        entries.removeAll { $0.key == key }
        entries.append((key, samples))
        if entries.count > Self.capacity { entries.removeFirst(entries.count - Self.capacity) }
    }
}

/// One sheet's resolved masks by revision. The panel publishes each edit as a new revision with
/// its immutable array; the sheet's `values` carry the revision, so the worker-side `compute`
/// reads the array it was asked for and never the panel's live state.
final class DevelopFilterMaskStore {
    private static let kept = 16
    private let lock = NSLock()
    private var revisions: [(revision: Int, masks: [[String: Any]])] = [(0, [])]
    let rangeProfile: String?

    init(rangeProfile: String?) { self.rangeProfile = rangeProfile }

    func publish(_ revision: Int, _ masks: [[String: Any]]) {
        lock.lock()
        defer { lock.unlock() }
        revisions.removeAll { $0.revision == revision }
        revisions.append((revision, masks))
        if revisions.count > Self.kept { revisions.removeFirst(revisions.count - Self.kept) }
    }

    func masks(_ revision: Int) -> [[String: Any]] {
        lock.lock()
        defer { lock.unlock() }
        return revisions.last { $0.revision == revision }?.masks ?? []
    }

    /// `params` with revision `revision`'s masks, and `range_profile` when one has a range. For the
    /// preview's `proxy` each mask drops `DevelopFilterMask.detailKeys`: their radii are fixed in
    /// target pixels, which a proxy would show wider (`DevelopFilterSheet.swift`).
    func params(_ params: [String: Any], revision: Int, proxy: Bool = false) -> [String: Any] {
        var masks = masks(revision)
        guard !masks.isEmpty else { return params }
        if proxy {
            masks = masks.map { mask in mask.filter { !DevelopFilterMask.detailKeys.contains($0.key) } }
        }
        var result = params
        result["masks"] = masks
        if masks.contains(where: { $0["range"].map { !($0 is NSNull) } == true }), let rangeProfile {
            result["range_profile"] = rangeProfile
        }
        return result
    }
}
