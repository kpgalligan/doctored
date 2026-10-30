import Foundation
import ImageIO

/// Lensfun lens profiles (v1-lensfun): the camera and lens a document's EXIF
/// names, the choice between the automatic match, none and one lens, and the
/// bundled Lensfun database's answer (`RasterLensCore`, over `lens_db.rs`).
/// The answer's `profile` object is `lens_correction`'s `profile` parameter:
/// Filter ▸ Lens Correction's popup, `apply_filter`'s `profile` and the RAW
/// development's Lensfun stage all apply the same object.

/// The camera, lens, exposure and frame size Lensfun matching reads.
struct LensIdentity: Equatable {
    var make, model, lensMake, lensModel: String?
    var focalLength, fNumber, subjectDistance, focalLength35mm: Double?
    var pixelWidth, pixelHeight: Int?

    init() {}

    /// From a document's EXIF packet, read by the core (`dr_lens_exif_identity`).
    init(exifPacket: Data?) {
        guard let exifPacket, let object = RasterLensCore.exifIdentity(exifPacket) else { return }
        let text = { (key: String) in (object[key] as? String).flatMap { $0.isEmpty ? nil : $0 } }
        let number = { (key: String) in (object[key] as? NSNumber)?.doubleValue }
        make = text("make"); model = text("model")
        lensMake = text("lens_make"); lensModel = text("lens_model")
        focalLength = number("focal_length"); fNumber = number("f_number")
        subjectDistance = number("subject_distance"); focalLength35mm = number("focal_length_35mm")
        pixelWidth = number("pixel_width").map { Int($0) }
        pixelHeight = number("pixel_height").map { Int($0) }
    }

    /// From ImageIO's image properties (a camera RAW's `CIRAWFilter.properties`):
    /// the TIFF Make and Model, and the Exif lens, exposure and frame fields,
    /// with the ExifAux lens model when Exif names none.
    init(imageProperties properties: [AnyHashable: Any]) {
        let tiff = properties[kCGImagePropertyTIFFDictionary as String] as? [AnyHashable: Any]
        let exif = properties[kCGImagePropertyExifDictionary as String] as? [AnyHashable: Any]
        let aux = properties[kCGImagePropertyExifAuxDictionary as String] as? [AnyHashable: Any]
        let text = { (dictionary: [AnyHashable: Any]?, key: CFString) -> String? in
            let value = (dictionary?[key as String] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
            return value?.isEmpty == false ? value : nil
        }
        let number = { (key: CFString) -> Double? in
            (exif?[key as String] as? NSNumber).map(\.doubleValue).flatMap { $0.isFinite && $0 > 0 ? $0 : nil }
        }
        make = text(tiff, kCGImagePropertyTIFFMake)
        model = text(tiff, kCGImagePropertyTIFFModel)
        lensMake = text(exif, kCGImagePropertyExifLensMake)
        lensModel = text(exif, kCGImagePropertyExifLensModel) ?? text(aux, kCGImagePropertyExifAuxLensModel)
        focalLength = number(kCGImagePropertyExifFocalLength)
        fNumber = number(kCGImagePropertyExifFNumber)
        subjectDistance = number(kCGImagePropertyExifSubjectDistance)
        focalLength35mm = number(kCGImagePropertyExifFocalLenIn35mmFilm)
        pixelWidth = number(kCGImagePropertyExifPixelXDimension).map { Int($0) }
        pixelHeight = number(kCGImagePropertyExifPixelYDimension).map { Int($0) }
    }

    /// The `identity` object `dr_lens_match` reads; an unknown field is null.
    var json: [String: Any] {
        let value = { (v: Any?) -> Any in v ?? NSNull() }
        return ["make": value(make), "model": value(model), "lens_make": value(lensMake),
                "lens_model": value(lensModel), "focal_length": value(focalLength),
                "f_number": value(fNumber), "subject_distance": value(subjectDistance),
                "focal_length_35mm": value(focalLength35mm), "pixel_width": value(pixelWidth),
                "pixel_height": value(pixelHeight)]
    }
}

/// Which profile Lens Correction applies: the automatic match, none, or one
/// lens by its database id.
enum LensProfileChoice: Equatable {
    case auto, none, lens(String)

    /// `apply_filter`'s `profile` argument: absent or "none" is `.none`,
    /// "auto" the match, any other string a lens id.
    init(argument: Any?) throws {
        guard let argument else { self = .none; return }
        switch argument {
        case is NSNull: self = .none
        case let text as String where text == "none": self = .none
        case let text as String where text == "auto": self = .auto
        case let text as String where !text.isEmpty: self = .lens(text)
        default:
            throw DoctoredCoreError(message: "profile must be \"auto\", \"none\" or a lens id from list_lens_profiles.")
        }
    }

    /// What a recorded step carries: "auto", the id, or nothing for none.
    var argument: String? {
        switch self {
        case .auto: return "auto"
        case .none: return nil
        case .lens(let id): return id
        }
    }
}

/// One lens of the database's answer.
struct LensCandidate: Equatable {
    let id, maker, model: String
    let cropFactor: Double
    let distortion, tca, vignetting: Bool

    init?(_ object: Any?) {
        guard let object = object as? [String: Any], let id = object["id"] as? String else { return nil }
        self.id = id
        maker = object["maker"] as? String ?? ""
        model = object["model"] as? String ?? id
        cropFactor = (object["crop_factor"] as? NSNumber)?.doubleValue ?? 1
        distortion = object["distortion"] as? Bool ?? false
        tca = object["tca"] as? Bool ?? false
        vignetting = object["vignetting"] as? Bool ?? false
    }
}

/// The bundled database's answer for one identity and choice.
struct LensProfileMatch {
    let camera: String?
    let candidates, mountLenses: [LensCandidate]
    let selected: LensCandidate?
    /// The `lens_correction` `profile` object, nil when none applies.
    let profile: [String: Any]?
    /// `profile` as sorted-key JSON (what a RAW recipe keeps).
    let profileJSON: String?
    let reason: String?
    /// The database's version tag ("v0.3.4").
    let tag: String
    /// The core's whole answer (`list_lens_profiles` returns it).
    let answer: [String: Any]

    /// Asks the database. `.none` still lists the candidates but applies no
    /// profile; `dimensions` is the target plane's pixel size, which the
    /// automatic match compares with the EXIF frame.
    static func resolve(_ identity: LensIdentity, _ choice: LensProfileChoice, all: Bool = false,
                        dimensions: (width: Int, height: Int)? = nil) throws -> LensProfileMatch {
        var request: [String: Any] = ["identity": identity.json, "all": all]
        if case .lens(let id) = choice { request["lens"] = id } else { request["lens"] = "auto" }
        if let dimensions { request["dimensions"] = [dimensions.width, dimensions.height] }
        let answer = try RasterLensCore.match(request)
        let cameraObject = answer["camera"] as? [String: Any]
        let camera = cameraObject.flatMap { $0["model"] as? String }
        let lenses = { (key: String) in ((answer[key] as? [Any]) ?? []).compactMap(LensCandidate.init) }
        var profile = answer["profile"] as? [String: Any]
        var reason = answer["reason"] as? String
        if choice == .none { profile = nil; reason = nil }
        let json = try profile.map {
            String(decoding: try JSONSerialization.data(withJSONObject: $0, options: [.sortedKeys]), as: UTF8.self)
        }
        let tag = (answer["database"] as? [String: Any])?["tag"] as? String ?? "?"
        return LensProfileMatch(camera: camera, candidates: lenses("candidates"),
                                mountLenses: lenses("mount_lenses"), selected: LensCandidate(answer["selected"]),
                                profile: profile, profileJSON: json, reason: reason, tag: tag, answer: answer)
    }

    /// The one sentence the Lens Correction sheet and `apply_filter` show:
    /// the camera and lens with the database version, or why there is none.
    var summary: String {
        if let selected, profile != nil {
            let body = [camera, selected.model].compactMap { $0 }.joined(separator: " with ")
            let note = reason.map { "; \($0)" } ?? ""
            return "\(body) \u{2014} Lensfun \(tag)\(note)."
        }
        if let reason { return "No Lensfun profile: \(reason)." }
        return "No lens profile."
    }
}

extension RasterDocument {
    /// The lens identity the document's EXIF packet names (empty without one).
    var lensIdentity: LensIdentity { LensIdentity(exifPacket: metadata(DR_METADATA_EXIF)) }

    /// `lens_correction` parameters with a recorded `profile` string resolved to the core's
    /// object, as `apply_filter`'s `lensProfileArguments` resolves it: "none" removes it, "auto"
    /// is this document's EXIF match for layer `layer`'s pixel size (the canvas's for a sizeless
    /// layer), any other string a lens id. A profile object passes through for the core to
    /// validate. The 16/32-bit Lens Correction sheet records the string, so a replayed Action
    /// matches each photo again rather than reusing the first one's coefficients.
    func lensResolvedParameters(_ params: [String: Any], layer: Int) throws -> [String: Any] {
        guard let raw = params["profile"], !(raw is [String: Any]) else { return params }
        var result = params
        let choice = try LensProfileChoice(argument: raw)
        guard choice != .none else {
            result.removeValue(forKey: "profile")
            return result
        }
        var size: (width: Int, height: Int) = (width, height)
        if let info = layerInfo(layer), info.width > 0, info.height > 0 { size = (info.width, info.height) }
        let match = try LensProfileMatch.resolve(lensIdentity, choice, dimensions: size)
        guard let profile = match.profile else {
            throw DoctoredCoreError(message: "No Lensfun profile applies to this layer (\(match.reason ?? "no calibration applies")); nothing was applied.")
        }
        result["profile"] = profile
        return result
    }

    /// A RAW development's Lensfun stage (v1-lensfun): `lens_correction` with
    /// the recipe's resolved profile, through the selected edit at 16 bits and
    /// at 32 alike, right after the decoder render (on the base and on each local
    /// variant, before the locals mix and the manual vignette). Returns self for
    /// nil or a core no-op. With no `control` it runs under a fresh one, as
    /// `applyingRawPresence` does: an uncontrolled call is capped at the
    /// destructive work ceiling, a controlled one admitted by memory.
    func applyingRawLensfun(_ lensfun: RawLensfunProfile?, control: RasterJobControl? = nil) throws
        -> RasterDocument {
        guard let lensfun else { return self }
        guard let precision else {
            throw DoctoredCoreError(message: "A Lensfun RAW profile requires genuine high-depth storage.")
        }
        let profile = try JSONSerialization.jsonObject(with: Data(lensfun.profile.utf8))
        let request = try RawLocalJSON.string(["layer": 0, "op": "lens_correction",
                                               "params": ["profile": profile]])
        let budget = try PrecisionWorkContext(source: precision)
        let job = try control ?? RasterJobControl()
        let next = try precision.selectedEdit(request: request, coverage: [], workingLimit: budget.limit,
                                              otherRetained: budget.otherRetained, control: job)
        return try next.map { try RasterDocument(precision: $0) } ?? self
    }
}
