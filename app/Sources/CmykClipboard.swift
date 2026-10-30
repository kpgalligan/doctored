import AppKit

// #1 phase 4c: copy and paste across colour modes and CMYK profiles, host side. The core
// converts every pixel (`cmyk_payload_convert`, relative colorimetric with black-point
// compensation through the profiles' own tables); this file is what the host says about it and
// the two host steps it takes:
//
// - `ColorConversion` — what a paste's pixels crossed (`PasteOutcome.colorConversion`,
//   `LayerPasteOutcome.colorConversion`): the history row's " (CMYK → RGB)" suffix and the MCP
//   reply's `color_conversion`. Engine "doctored" for the core's conversions,
//   "platform" for a Gray, Lab or CMYK image CoreGraphics converted while decoding it.
// - `ExternalClipboardImage.cmykPayload` — another app's CMYK TIFF on the pasteboard read as a
//   CMYK payload (`RasterClipboardPayload.fromCmykImage`), so a CMYK destination keeps its ink and
//   every other destination converts it through the core rather than through CoreGraphics.
// - `CmykLayerClip.converted` — a layers clip of the OTHER mode brought into the destination's
//   before the core's paste (which refuses a mixed clip by name): the mode conversions
//   themselves (`convertToCMYK` / `convertCMYKToRGB`), a blocker surfacing as the core's sentence.
//   Its type, shape and fill layers are re-rendered in the destination's colours
//   (`reinkingArtwork`, #1), and a CMYK clip of ANOTHER profile holding one is converted here too
//   (Convert to Profile's core edit, then the re-ink) so the core pastes it as same-profile ink.

/// What a paste's pixels crossed (module comment).
struct ColorConversion {
    let from: String
    let to: String
    let fromProfile: String
    let toProfile: String
    /// "doctored" (the core's conversion) or "platform" (CoreGraphics' decode).
    let engine: String
    /// The core's black-point compensation: true across CMYK and RGB, false for a crossing into or
    /// out of Grayscale (#1 phase 4: the luminance rule and Mode ▸ RGB Color take none) and into
    /// Lab Color (Lab phase 4c: Mode ▸ Lab Color's rule takes none; out of Lab it takes BPC).
    var blackPointCompensation = true

    /// The history row's suffix: " (CMYK → RGB)" when the colour model
    /// changed, "" for a conversion between two CMYK profiles.
    var historySuffix: String { from == to ? "" : " (\(from) → \(to))" }

    /// `{from, to, from_profile, to_profile, engine, intent,
    /// black_point_compensation}`; the rendering intent and BPC are the
    /// core's ("relative", `blackPointCompensation`) and null for a platform decode.
    var json: [String: Any] {
        let core = engine == "doctored"
        return [
            "from": from, "to": to, "from_profile": fromProfile, "to_profile": toProfile,
            "engine": engine, "intent": core ? "relative" : NSNull(),
            "black_point_compensation": core ? blackPointCompensation : NSNull(),
        ]
    }

    /// `json`, or null when nothing was converted.
    static func json(_ conversion: ColorConversion?) -> Any { conversion?.json ?? NSNull() }

    /// The core's refusal of Don't Convert across modes
    /// (`cmyk_payload_convert::CROSS_MODE_PRESERVE`), for the host's own
    /// cross-mode steps.
    static let crossModePreserve = "Don't Convert keeps numbers within one colour mode; pixels "
        + "pasted between RGB and CMYK are always converted"

    /// The core's refusal of Don't Convert into or out of Grayscale
    /// (`gray_clipboard::CROSS_MODE_PRESERVE`, #1 phase 4).
    static let crossModePreserveGray = "Don't Convert keeps numbers within one colour mode; pixels "
        + "pasted between Grayscale and another mode are always converted"

    /// The core's refusal of Don't Convert into or out of Lab Color
    /// (`lab_clipboard::CROSS_MODE_PRESERVE`, #1 Lab phase 4c).
    static let crossModePreserveLab = "Don't Convert keeps numbers within one colour mode; pixels "
        + "pasted between Lab Color and another mode are always converted"

    /// The colour model a document's pixels are in.
    static func model(of doc: RasterDocument) -> String {
        doc.isLab ? "Lab" : doc.isGray ? "Gray" : doc.isCmyk ? "CMYK" : "RGB"
    }

    /// The profile bytes a document's pixels are in: its CMYK profile, its
    /// Gray profile, or its RGB profile (an untagged document counts as sRGB).
    static func profileData(of doc: RasterDocument) -> Data {
        if doc.isCmyk, let precision = doc.precision { return precision.cmykProfileData }
        if doc.isGray, let precision = doc.precision { return precision.grayProfileData }
        return doc.iccProfile ?? RasterProfile.builtin(.sRGB)
    }

    /// That profile's name.
    static func profileName(of doc: RasterDocument) -> String {
        if doc.isCmyk, let precision = doc.precision {
            return precision.cmykInfo()?["profile"] as? String ?? ""
        }
        if doc.isGray, let name = (doc.precisionInfo?["gray"] as? [String: Any])?["profile"]
            as? String {
            return name
        }
        return doc.profileName
    }

    /// A conversion the core runs into `doc`; a Grayscale side, and a Lab destination, take no
    /// BPC.
    private static func core(from: String, fromProfile: String,
                             into doc: RasterDocument) -> ColorConversion {
        let to = model(of: doc)
        return ColorConversion(from: from, to: to, fromProfile: fromProfile,
                               toProfile: profileName(of: doc), engine: "doctored",
                               blackPointCompensation: from != "Gray" && to != "Gray"
                                   && to != "Lab")
    }

    /// What a typed payload pasted into `doc` under `mode` crosses: the
    /// colour model (always converted), or another CMYK profile under
    /// Convert; nil when its pixels keep their model (an RGB → RGB paste's
    /// disclosure is `profile`, unchanged).
    static func paste(_ payload: TypedClipboardPayload, into doc: RasterDocument,
                      mode: PasteProfile) -> ColorConversion? {
        let from = payload.isLab ? "Lab" : payload.isCmyk ? "CMYK" : payload.isGray ? "Gray"
            : "RGB"
        if from != model(of: doc) {
            return core(from: from, fromProfile: payload.profileDescription, into: doc)
        }
        if payload.isCmyk || payload.isGray, mode == .convert,
           PasteProfile.differ(payload.profileData, profileData(of: doc)) {
            return core(from: from, fromProfile: payload.profileDescription, into: doc)
        }
        return nil
    }

    /// What another app's image pasted into `doc` crosses: CoreGraphics'
    /// decode of a Gray, Lab or CMYK image, else (a CMYK payload read off
    /// its TIFF aside, which `paste` describes) the core's RGB → CMYK into a
    /// CMYK document; nil for an RGB image into an RGB document.
    static func external(_ image: ExternalClipboardImage, into doc: RasterDocument,
                         mode: PasteProfile) -> ColorConversion? {
        if let cmyk = image.cmyk { return paste(cmyk, into: doc, mode: mode) }
        if let gray = image.gray { return paste(gray, into: doc, mode: mode) }
        if let model = image.platformModel {
            return ColorConversion(from: model, to: Self.model(of: doc), fromProfile: image.sourceName,
                                   toProfile: profileName(of: doc), engine: "platform")
        }
        guard doc.isCmyk || doc.isGray || doc.isLab else { return nil }
        return core(from: "RGB", fromProfile: image.untagged ? "sRGB IEC61966-2.1" : image.sourceName,
                    into: doc)
    }

    /// What a layers clip pasted into `doc` under `mode` crosses
    /// (`paste`'s rule over the clip's own model and profile).
    static func layers(_ clip: RasterDocument, into doc: RasterDocument,
                       mode: PasteProfile) -> ColorConversion? {
        let from = model(of: clip)
        if from != model(of: doc) {
            return core(from: from, fromProfile: profileName(of: clip), into: doc)
        }
        if clip.isCmyk || clip.isGray, mode == .convert,
           PasteProfile.differ(profileData(of: clip), profileData(of: doc)) {
            return core(from: from, fromProfile: profileName(of: clip), into: doc)
        }
        return nil
    }
}

extension ExternalClipboardImage {
    /// The pasteboard's own TIFF bytes as a CMYK payload, when they are a
    /// CMYK TIFF the core reads (`RasterClipboardPayload.fromCmykImage`);
    /// an untagged one takes `fallback` (a CMYK destination's profile), else
    /// the working CMYK. nil for every other image, and when the payload
    /// cannot be admitted (the CoreGraphics decode is then the paste).
    static func cmykPayload(_ pasteboard: NSPasteboard, fallback: Data?) -> TypedClipboardPayload? {
        guard let tiff = pasteboard.data(forType: .tiff),
              let budget = try? PrecisionWorkContext(),
              let other = try? PrecisionDocumentDescriptor.sum(budget.otherRetained,
                                                                UInt64(tiff.count)),
              let made = try? RasterClipboardPayload.fromCmykImage(
                tiff, fallbackProfile: fallback, workingLimit: budget.limit, otherRetained: other),
              let info = try? made.payload.info()
        else { return nil }
        return TypedClipboardPayload(handle: made.payload, info: info)
    }
}

/// A layers clip of the other colour mode, brought into the destination's
/// (module comment).
enum CmykLayerClip {
    /// `clip` in `doc`'s colour mode and the conversion it crossed; nil when
    /// both are already in one mode (a CMYK → CMYK clip converts in the
    /// core's paste, under the chosen profile). RGB → CMYK narrows a float
    /// clip (or promotes an 8-bit one) to 16 bits, then converts into the
    /// document's CMYK profile at its declared depth; CMYK → RGB converts
    /// into the document's profile at 16 bits and narrows to 8 for an 8-bit
    /// document (a float document's paste promotes it). Don't Convert refuses.
    static func converted(_ clip: RasterDocument, into doc: RasterDocument,
                          mode: PasteProfile) throws
        -> (clip: RasterDocument, conversion: ColorConversion?)? {
        guard clip.isCmyk != doc.isCmyk else { return try reprofiled(clip, into: doc, mode: mode) }
        let conversion = ColorConversion.layers(clip, into: doc, mode: mode)
        guard mode == .convert else { throw DoctoredCoreError(message: ColorConversion.crossModePreserve) }
        // The clip's type, shape and fill layers, re-rendered after the conversion (#1).
        let captures = try clip.artworkCaptures()
        let sourceEnd = clip.artworkColorEnd
        if doc.isCmyk {
            guard let target = doc.precision else {
                throw DoctoredCoreError(message: "The CMYK document has no typed pixels.")
            }
            var source: RasterPrecisionDocument
            if let precision = clip.precision {
                if precision.isFloat {
                    let budget = try PrecisionWorkContext(source: precision)
                    source = try precision.narrowedStructureToU16(
                        workingLimit: budget.limit, otherRetained: budget.otherRetained)
                } else {
                    source = precision
                }
            } else {
                source = try clip.promotedStructure()
            }
            var options = CmykConversionOptions()
            options.depth = (target.cmykInfo()?["depth"] as? NSNumber)?.intValue
            do {
                let budget = try PrecisionWorkContext(source: source)
                let converted = try source.convertToCMYK(
                    profile: target.cmykProfileData, options: options, workingLimit: budget.limit,
                    otherRetained: budget.otherRetained, control: nil)
                return (try RasterDocument(precision: converted)
                    .reinkingArtwork(captures, from: sourceEnd, options: options), conversion)
            } catch {
                throw DoctoredCoreError(message: "The copied layers cannot be pasted into a CMYK "
                    + "document: " + ((error as? DoctoredCoreError)?.message ?? error.localizedDescription))
            }
        }
        guard let source = clip.precision else {
            throw DoctoredCoreError(message: "The copied CMYK layers have no typed pixels.")
        }
        let converted: RasterPrecisionDocument
        do {
            let budget = try PrecisionWorkContext(source: source)
            converted = try source.convertCMYKToRGB(
                profile: doc.iccProfile, options: CmykConversionOptions(), workingLimit: budget.limit,
                otherRetained: budget.otherRetained, control: nil)
        } catch {
            throw DoctoredCoreError(message: "The copied layers cannot be pasted into an RGB "
                + "document: " + ((error as? DoctoredCoreError)?.message ?? error.localizedDescription))
        }
        let reinked = try RasterDocument(precision: converted)
            .reinkingArtwork(captures, from: sourceEnd, options: CmykConversionOptions())
        guard !doc.isHighDepth else { return (reinked, conversion) }
        guard let typed = reinked.precision else { return (reinked, conversion) }
        let budget = try PrecisionWorkContext(source: typed)
        return (try typed.narrowedStructureToU8(dither: 0, workingLimit: budget.limit,
                                                otherRetained: budget.otherRetained), conversion)
    }

    /// A CMYK clip of another profile holding a type, shape or fill layer, under Convert: the
    /// clip converted into `doc`'s profile on the host (Convert to Profile's core edit) and its
    /// described layers re-inked, so the core pastes it as same-profile ink and the layers stay
    /// editable. nil for any other clip: the core's paste converts or relabels it.
    private static func reprofiled(_ clip: RasterDocument, into doc: RasterDocument,
                                   mode: PasteProfile) throws
        -> (clip: RasterDocument, conversion: ColorConversion?)? {
        guard clip.isCmyk, doc.isCmyk, mode == .convert, let source = clip.precision,
              let target = doc.precision, !clip.artworkIndices.isEmpty,
              PasteProfile.differ(source.cmykProfileData, target.cmykProfileData) else { return nil }
        let captures = try clip.artworkCaptures()
        let options = CmykConversionOptions()
        let budget = try PrecisionWorkContext(source: source)
        guard let converted = try source.cmykProfileEdit(
            profile: target.cmykProfileData, request: CmykProfileEdit.request(.convert, options),
            workingLimit: budget.limit, otherRetained: budget.otherRetained, control: nil) else { return nil }
        let reinked = try RasterDocument(precision: converted)
            .reinkingArtwork(captures, from: clip.artworkColorEnd, options: options)
        return (reinked, ColorConversion.layers(clip, into: doc, mode: mode))
    }
}

extension ExternalClipboardImage {
    /// The image's straight RGBA8 in its OWN space and the RGB profile those
    /// numbers are in — an untagged image's are taken to be sRGB — for a
    /// CMYK destination, whose ink the core converts them into
    /// (`RasterClipboardPayload.fromRGBA8`). nil when CoreGraphics cannot
    /// decode it.
    func ownPixels() -> (image: RasterImage, profile: Data)? {
        let srgb = RasterProfile.builtin(.sRGB)
        if untagged {
            let space = ColorProfile.sRGB
            return RasterImage.pasted(image.copy(colorSpace: space) ?? image, in: space)
                .map { ($0, srgb) }
        }
        let ingest = Bitmap.ingestSpace(of: image)
        return RasterImage.pasted(image, in: ingest.space).map { ($0, ingest.profile ?? srgb) }
    }

    /// The payload a CMYK destination pastes this image as: its CMYK
    /// payload when it brought one, else its own pixels and profile as an
    /// RGB byte-origin payload (sRGB when its own profile is not one a
    /// payload carries, a LUT profile say). The core converts it.
    func cmykDestinationPayload(into doc: RasterDocument) throws -> RasterClipboardPayload {
        if let cmyk { return cmyk.handle }
        // #1 phase 4: a gray TIFF pastes as its Gray payload at every destination too.
        if let gray { return gray.handle }
        let budget = try PrecisionWorkContext()
        guard let own = ownPixels() else {
            throw DoctoredCoreError(message: "The clipboard's image could not be decoded.")
        }
        do {
            return try RasterClipboardPayload.fromRGBA8(
                own.image, profile: own.profile, offset: (0, 0), ppi: doc.resolution,
                workingLimit: budget.limit, otherRetained: budget.otherRetained)
        } catch {
            guard let pixels = RasterImage.pasted(image, in: ColorProfile.sRGB) else { throw error }
            return try RasterClipboardPayload.fromRGBA8(
                pixels, profile: RasterProfile.builtin(.sRGB), offset: (0, 0), ppi: doc.resolution,
                workingLimit: budget.limit, otherRetained: budget.otherRetained)
        }
    }
}
