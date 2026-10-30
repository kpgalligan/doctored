import AppKit

// #1 phase 4: copy and paste across Grayscale and the other colour modes, host side (CMYK phase
// 4c's `CmykClipboard.swift` is the template). The core converts every PIXEL paste
// (`gray_clipboard`: into Gray by Image ▸ Mode ▸ Grayscale's luminance, a CMYK payload through
// ProPhoto RGB first; out of Gray by Mode ▸ RGB Color's conversion); this file takes the two host
// steps:
//
// - `GrayLayerClip.converted` — a LAYERS clip of another mode (or, under Convert, of another Gray
//   profile) brought into the destination's before the core's layers paste, which refuses a mixed
//   clip by name: the mode conversions themselves (`convertToGray` / `convertGrayToRGB`, a CMYK
//   side through 16-bit ProPhoto RGB), a blocker surfacing as the core's sentence.
// - `ExternalClipboardImage.grayPayload` — another app's gray TIFF on the pasteboard (or this
//   app's, `dr_clipboard_payload_gray_tiff`) read as a Gray payload, so it converts through the
//   core at every destination rather than through CoreGraphics.

/// A layers clip crossing into or out of Grayscale (module comment).
enum GrayLayerClip {
    /// `clip` in `doc`'s colour mode (and, between Gray documents under Convert, its Gray profile)
    /// and the conversion it crossed; nil when no host step is needed (one Gray profile, Don't
    /// Convert between Gray documents, or no Gray side at all). RGB → Gray promotes an 8-bit clip
    /// and narrows a float one for an 8 or 16-bit document; CMYK → Gray goes through 16-bit
    /// ProPhoto RGB; Gray → RGB converts into the document's profile (narrowed to the document's
    /// depth); Gray → CMYK goes through ProPhoto RGB and `CmykLayerClip.converted`; Gray A → Gray B
    /// through ProPhoto RGB, narrowed to 16 bits on the way for an 8 or 16-bit document. Don't
    /// Convert across modes refuses. The clip's type, shape and fill layers are re-rendered after
    /// the conversion from their authored colours (#1 phase 6b, `carried`), so they stay
    /// editable.
    static func converted(_ clip: RasterDocument, into doc: RasterDocument,
                          mode: PasteProfile) throws
        -> (clip: RasterDocument, conversion: ColorConversion?)? {
        guard clip.isGray || doc.isGray else { return nil }
        let proPhoto = RasterProfile.builtin(.proPhotoRGB)
        if clip.isGray, doc.isGray {
            guard mode == .convert, let source = clip.precision, let target = doc.precision,
                  PasteProfile.differ(source.grayProfileData, target.grayProfileData)
            else { return nil }
            let carry = try ArtworkCarry(clip, into: doc)
            let rgb = try RasterDocument(precision: toRGB(source, profile: proPhoto, into: doc))
            let gray = try toGray(try rgbSource(rgb, into: doc), into: doc)
            return (try carry.applied(to: gray), ColorConversion.layers(clip, into: doc, mode: mode))
        }
        guard mode == .convert else {
            throw DoctoredCoreError(message: ColorConversion.crossModePreserveGray)
        }
        let conversion = ColorConversion.layers(clip, into: doc, mode: mode)
        let carry = try ArtworkCarry(clip, into: doc)
        if doc.isGray {
            return (try carry.applied(to: toGray(try rgbSource(clip, into: doc), into: doc)),
                    conversion)
        }
        guard let source = clip.precision else {
            throw DoctoredCoreError(message: "The copied Grayscale layers have no typed pixels.")
        }
        if doc.isCmyk {
            let rgb = try RasterDocument(precision: toRGB(source, profile: proPhoto, into: doc))
            let crossed = try CmykLayerClip.converted(rgb, into: doc, mode: mode)
            return (try carry.applied(to: crossed?.clip ?? rgb), conversion)
        }
        let rgb = try toRGB(source, profile: doc.iccProfile ?? RasterProfile.builtin(.sRGB),
                            into: doc)
        return (try carry.applied(to: fitted(rgb, to: doc)), conversion)
    }

    /// A clip's type, shape and fill layers captured before a conversion and re-rendered on its
    /// result (#1 phase 6b, 6a-ii's carry: `CmykClipboard.swift`, `GrayProfileEdit.swift`): each
    /// colour from the clip's own colour end, through Mode ▸ Grayscale's CMYK stage for a CMYK
    /// clip into Gray. No captures, no step.
    private struct ArtworkCarry {
        let captures: [ArtworkCapture]
        let end: ArtworkColorEnd
        let options: CmykConversionOptions

        init(_ clip: RasterDocument, into doc: RasterDocument) throws {
            captures = try clip.artworkCaptures()
            end = clip.artworkColorEnd
            options = clip.isCmyk && doc.isGray
                ? CmykModeConversion.rgbStageOptions : CmykConversionOptions()
        }

        func applied(to result: RasterDocument) throws -> RasterDocument {
            captures.isEmpty ? result
                : try result.reinkingArtwork(captures, from: end, options: options)
        }
    }

    /// A 32-bit Gray clip at an 8 or 16-bit Gray document's depth, in its OWN Gray profile, for
    /// the paths `converted` leaves to the core (one profile, or Don't Convert): the core's layers
    /// paste refuses a float clip in an integer document, so it goes through ProPhoto RGB, is
    /// narrowed to 16 bits there and comes back to Gray at the document's depth. nil when the
    /// clip is not float Gray or the document is 32-bit or not Gray.
    static func depthFitted(_ clip: RasterDocument,
                            into doc: RasterDocument) throws -> RasterDocument? {
        guard clip.isGray, doc.isGray, doc.grayDepth != 32, let source = clip.precision,
              source.isFloat
        else { return nil }
        let carry = try ArtworkCarry(clip, into: doc)
        let proPhoto = RasterProfile.builtin(.proPhotoRGB)
        let rgb = try RasterDocument(precision: toRGB(source, profile: proPhoto, into: doc))
        return try carry.applied(to: toGray(try rgbSource(rgb, into: doc), into: doc,
                                            profile: source.grayProfileData))
    }

    /// The clip's typed RGB pixels for a Gray destination: an 8-bit clip promoted, a CMYK clip
    /// through 16-bit ProPhoto RGB (Mode ▸ Grayscale's CMYK stage), a float clip narrowed to 16
    /// bits for an 8 or 16-bit document.
    private static func rgbSource(_ clip: RasterDocument,
                                  into doc: RasterDocument) throws -> RasterPrecisionDocument {
        guard let precision = clip.precision else { return try clip.promotedStructure() }
        let budget = try PrecisionWorkContext(source: precision)
        if precision.isCmyk {
            return try precision.convertCMYKToRGB(
                profile: RasterProfile.builtin(.proPhotoRGB),
                options: CmykModeConversion.rgbStageOptions, workingLimit: budget.limit,
                otherRetained: budget.otherRetained, control: nil)
        }
        if precision.isFloat, doc.grayDepth != 32 {
            return try precision.narrowedStructureToU16(workingLimit: budget.limit,
                                                        otherRetained: budget.otherRetained)
        }
        return precision
    }

    /// `rgb` through Image ▸ Mode ▸ Grayscale into `profile` (else `doc`'s Gray profile) at
    /// `doc`'s depth (a 16-bit result into a 32-bit document, which the core's paste promotes).
    private static func toGray(_ rgb: RasterPrecisionDocument, into doc: RasterDocument,
                               profile: Data? = nil) throws -> RasterDocument {
        guard let target = doc.precision else {
            throw DoctoredCoreError(message: "The Grayscale document has no typed pixels.")
        }
        var options: [String: Any] = [:]
        if !rgb.isFloat { options["depth"] = doc.grayDepth == 8 ? 8 : 16 }
        do {
            let budget = try PrecisionWorkContext(source: rgb)
            let gray = try rgb.convertToGray(profile: profile ?? target.grayProfileData,
                                             options: options,
                                             workingLimit: budget.limit,
                                             otherRetained: budget.otherRetained, control: nil)
            return try RasterDocument(precision: gray)
        } catch {
            throw DoctoredCoreError(message: "The copied layers cannot be pasted into a Grayscale "
                + "document: " + ((error as? DoctoredCoreError)?.message ?? error.localizedDescription))
        }
    }

    /// A Gray clip through Image ▸ Mode ▸ RGB Color into `profile`.
    private static func toRGB(_ gray: RasterPrecisionDocument, profile: Data,
                              into doc: RasterDocument) throws -> RasterPrecisionDocument {
        do {
            let budget = try PrecisionWorkContext(source: gray)
            return try gray.convertGrayToRGB(profile: profile, options: [:],
                                             workingLimit: budget.limit,
                                             otherRetained: budget.otherRetained, control: nil)
        } catch {
            let mode = doc.isCmyk ? "a CMYK" : doc.isGray ? "a Grayscale" : "an RGB"
            throw DoctoredCoreError(message: "The copied layers cannot be pasted into \(mode) "
                + "document: " + ((error as? DoctoredCoreError)?.message ?? error.localizedDescription))
        }
    }

    /// `rgb` at `doc`'s depth: narrowed to 8 bits for an 8-bit document and to 16 for a 16-bit
    /// one (a 16-bit clip into a float document is the core paste's promotion). Also Lab → RGB's
    /// fit (`LabLayerClip`, #1 Lab phase 4c).
    static func fitted(_ rgb: RasterPrecisionDocument,
                               to doc: RasterDocument) throws -> RasterDocument {
        let budget = try PrecisionWorkContext(source: rgb)
        if !doc.isHighDepth {
            return try rgb.narrowedStructureToU8(dither: 0, workingLimit: budget.limit,
                                                 otherRetained: budget.otherRetained)
        }
        if rgb.isFloat, !doc.isFloat {
            return try RasterDocument(precision: rgb.narrowedStructureToU16(
                workingLimit: budget.limit, otherRetained: budget.otherRetained))
        }
        return try RasterDocument(precision: rgb)
    }
}

extension ExternalClipboardImage {
    /// The pasteboard's own TIFF bytes as a Gray payload, when they are a gray TIFF the core reads
    /// (`RasterClipboardPayload.fromGrayImage`); an untagged one takes `fallback` (a Gray
    /// destination's profile), else the working Gray. nil for every other image, and when the
    /// payload cannot be admitted (the CoreGraphics decode is then the paste).
    static func grayPayload(_ pasteboard: NSPasteboard, fallback: Data?) -> TypedClipboardPayload? {
        guard let tiff = pasteboard.data(forType: .tiff),
              let budget = try? PrecisionWorkContext(),
              let other = try? PrecisionDocumentDescriptor.sum(budget.otherRetained,
                                                                UInt64(tiff.count)),
              let made = try? RasterClipboardPayload.fromGrayImage(
                tiff, fallbackProfile: fallback, workingGray: ColorSettings.workingGray.rawValue,
                workingLimit: budget.limit, otherRetained: other),
              let info = try? made.payload.info()
        else { return nil }
        return TypedClipboardPayload(handle: made.payload, info: info)
    }
}

extension RasterClipboardPayload {
    /// The pasteboard TIFF of a Copy, Copy Merged or Cut of `doc`: a Grayscale document's
    /// one-channel gray TIFF (`grayTIFF`), nil for every other document (whose TIFF is the RGB
    /// rendering's). `held` is the typed bytes the caller holds beside it.
    func pasteboardTIFF(from doc: RasterDocument, held: Int) throws -> Data? {
        guard doc.isGray else { return nil }
        let budget = try PrecisionWorkContext(additionalRetainedBytes: UInt64(held))
        return try grayTIFF(workingLimit: budget.limit, otherRetained: budget.otherRetained)
    }
}
